// gobol-lsp.rs — Gobol Language Server (LSP) for VS Code / any LSP client.
//
// Provides:
//   - Diagnostics (parse + semantic errors with positions)
//   - Hover (type info for identifiers)
//   - Goto Definition (jump to symbol definition)
//   - Completion (symbols + keywords + snippets + std modules)
//   - Document Symbols (hierarchical: methods under struct/trait)
//
// The analysis runs the existing compiler frontend (Lexer → AstBuilder →
// SemanticAnalyzer) and builds a token-based symbol index for position
// queries, since AST nodes don't currently carry source positions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{stdin, stdout};
use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use gobol::ast_builder::AstBuilder;
use gobol::error::ErrorFormatter;
use gobol::lexer::Lexer;
use gobol::semantic_analyzer::SemanticAnalyzer;
use gobol::token::{Token, TokenType};

// ==================== Semantic Tokens ====================
// rust-analyzer-style automatic semantic highlighting. The LSP
// `textDocument/semanticTokens/full` handler below maps the lexer token stream
// (+ the symbol index) onto the standard LSP token types, so `import xxx`'s
// `xxx` (and other identifiers) are colored automatically by any client that
// advertises semantic-token support — no manual highlight toggling needed.

/// Order is significant: the client maps bitset positions back to these names.
/// All names are from the standard LSP token-type set so VS Code / Neovim give
/// them sensible default colours out of the box.
static SEMANTIC_TYPES: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::ENUM,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::OPERATOR,
];

static SEMANTIC_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

// ==================== Symbol Index ====================

#[derive(Clone, Debug, PartialEq)]
enum SymKind {
    Function,
    Method,
    Struct,
    Enum,
    EnumVariant,
    Variable,
    Parameter,
    Trait,
    Import,
    ExternFn,
    StaticFunc,
    TypeAlias,
}

impl SymKind {
    fn label(&self) -> &'static str {
        match self {
            SymKind::Function => "function",
            SymKind::Method => "method",
            SymKind::Struct => "struct",
            SymKind::Enum => "enum",
            SymKind::EnumVariant => "enum variant",
            SymKind::Variable => "variable",
            SymKind::Parameter => "parameter",
            SymKind::Trait => "trait",
            SymKind::Import => "module",
            SymKind::ExternFn => "extern fn",
            SymKind::StaticFunc => "static function",
            SymKind::TypeAlias => "type alias",
        }
    }

    fn completion_kind(&self) -> CompletionItemKind {
        match self {
            SymKind::Function => CompletionItemKind::FUNCTION,
            SymKind::Method => CompletionItemKind::METHOD,
            SymKind::Struct => CompletionItemKind::CLASS,
            SymKind::Enum => CompletionItemKind::ENUM,
            SymKind::EnumVariant => CompletionItemKind::ENUM_MEMBER,
            SymKind::Variable => CompletionItemKind::VARIABLE,
            SymKind::Parameter => CompletionItemKind::VARIABLE,
            SymKind::Trait => CompletionItemKind::INTERFACE,
            SymKind::Import => CompletionItemKind::MODULE,
            SymKind::ExternFn => CompletionItemKind::FUNCTION,
            SymKind::StaticFunc => CompletionItemKind::FUNCTION,
            SymKind::TypeAlias => CompletionItemKind::TYPE_PARAMETER,
        }
    }

    fn symbol_kind(&self) -> SymbolKind {
        match self {
            SymKind::Function => SymbolKind::FUNCTION,
            SymKind::Method => SymbolKind::METHOD,
            SymKind::Struct => SymbolKind::CLASS,
            SymKind::Enum => SymbolKind::ENUM,
            SymKind::EnumVariant => SymbolKind::ENUM_MEMBER,
            SymKind::Variable => SymbolKind::VARIABLE,
            SymKind::Parameter => SymbolKind::VARIABLE,
            SymKind::Trait => SymbolKind::INTERFACE,
            SymKind::Import => SymbolKind::MODULE,
            SymKind::ExternFn => SymbolKind::FUNCTION,
            SymKind::StaticFunc => SymbolKind::FUNCTION,
            SymKind::TypeAlias => SymbolKind::TYPE_PARAMETER,
        }
    }
}

#[derive(Clone, Debug)]
struct SymbolEntry {
    name: String,
    kind: SymKind,
    line: i32,   // 1-based (token line)
    col: i32,    // 0-based (token col)
    len: i32,    // length in chars
    type_info: Option<String>,
    parent: Option<String>, // struct/trait/enum name for methods/variants
    doc_comment: Option<String>, // documentation comment above the declaration
}

/// Parsed signature of a callable, used by signature help and inlay hints.
#[derive(Clone, Debug, Default)]
struct FuncSignature {
    name: String,
    /// Parameter names in declaration order (e.g. `["x", "y"]`).
    param_names: Vec<String>,
    /// Declared types of each parameter, parallel to `param_names`.
    param_types: Vec<Option<String>>,
    /// Full parameter text list, `["x: int", "y: str"]`, for the printed label.
    param_labels: Vec<String>,
    /// Return type annotation, if present.
    return_type: Option<String>,
    /// Doc comment above the function declaration.
    doc: Option<String>,
}

// ==================== Document State ====================

#[derive(Clone)]
struct DocState {
    source: String,
    tokens: Vec<Token>,
    symbols: Vec<SymbolEntry>,
    errors: Vec<(i32, i32, String)>,
    /// Signatures of callables declared in this document, keyed by name.
    signatures: std::collections::HashMap<String, FuncSignature>,
}

impl DocState {
    /// Find the token at a given LSP position (0-based line, 0-based character).
    fn token_at(&self, line: u32, character: u32) -> Option<&Token> {
        let target_line = (line as i32) + 1; // token lines are 1-based
        let target_col = character as i32;
        self.tokens.iter().find(|t| {
            t.line == target_line
                && target_col >= t.col
                && target_col < t.col + t.value.len() as i32
        })
    }

    /// Find the symbol definition for a given identifier name.
    /// Prefers definitions (Function/Struct/Variable/Enum/etc.) over references.
    fn find_definition(&self, name: &str) -> Option<&SymbolEntry> {
        self.symbols.iter().find(|s| {
            s.name == name
                && matches!(
                    s.kind,
                    SymKind::Function
                        | SymKind::Method
                        | SymKind::Struct
                        | SymKind::Enum
                        | SymKind::EnumVariant
                        | SymKind::Variable
                        | SymKind::Parameter
                        | SymKind::Trait
                        | SymKind::TypeAlias
                        | SymKind::ExternFn
                        | SymKind::StaticFunc
                )
        })
    }

    /// If `name` matches a module imported by this document, return the full
    /// imported module name (which may be a `a::b` path). Used to decide
    /// whether a symbol that carries this module as its `parent` is a
    /// `from lib import ...` re-export that should resolve cross-file.
    fn module_imported(&self, name: &str) -> Option<String> {
        self.symbols
            .iter()
            .find(|s| s.kind == SymKind::Import && s.name == name)
            .map(|s| s.name.clone())
    }

    /// Compute document-highlight ranges for every identifier occurrence of
    /// `name` in this document. Used both by the standard
    /// `textDocument/documentHighlight` handler and by the cross-file
    /// `gobol/highlights` custom method (re-computed on the fly per file).
    fn highlights_for(&self, name: &str) -> Vec<DocumentHighlight> {
        self.tokens
            .iter()
            .filter(|t| t.r#type == TokenType::Identifier && t.value == name)
            .map(|t| DocumentHighlight {
                range: token_to_range(t),
                kind: Some(DocumentHighlightKind::TEXT),
            })
            .collect()
    }

    /// Compute LSP semantic tokens (delta-encoded) from the token stream and
    /// the symbol index. Emits one token per lexer token, classifying
    /// identifiers by declaration kind / context so `import xxx`, types,
    /// function calls, variables, etc. are highlighted automatically.
    fn semantic_tokens(&self) -> Vec<SemanticToken> {
        build_semantic_tokens(&self.tokens, &self.symbols)
    }

    #[allow(dead_code)]
    fn source_line(&self, line_1based: i32) -> &str {
        if line_1based <= 0 {
            return "";
        }
        self.source
            .lines()
            .nth((line_1based - 1) as usize)
            .unwrap_or("")
    }

    /// Compute the brace nesting depth at a given (1-based line, 0-based col).
    /// Only visible identifiers shut inside the same-or-outer scope chain are
    /// offered for completion.
    fn brace_depth_at(&self, line_1: i32, col: i32) -> i32 {
        let mut depth = 0i32;
        for t in &self.tokens {
            // token's start position is before (line,col)
            let before = t.line < line_1 || (t.line == line_1 && t.col <= col);
            if !before {
                break;
            }
            if t.value == "{" {
                depth += 1;
            } else if t.value == "}" {
                depth -= 1;
            }
        }
        depth.max(0)
    }

    /// Whether an identifier used at the cursor is a visible local (variable or
    /// parameter) — i.e. declared earlier in source, at the same-or-outer brace
    /// depth, and inside the same function.
    fn local_visible_at(&self, sym: &SymbolEntry, line: u32, character: u32) -> bool {
        let target_line = (line as i32) + 1;
        let target_col = character as i32;
        let sym_line = sym.line;
        let sym_col = sym.col;

        // 1. Parameters are visible anywhere after the function signature's
        //    opening brace; i.e. anywhere inside their function body. We treat
        //    them as visible if the cursor is after the parameter declaration.
        // 2. Variables must be declared strictly before the cursor.
        let declared_before = sym_line < target_line
            || (sym_line == target_line && sym_col < target_col);
        if !declared_before {
            return false;
        }

        // Scope nesting: a local is visible if its brace depth is <= the
        // cursor's brace depth (outer scope or same scope).  A deeper nested
        // local (declared in an inner block that the cursor isn't inside yet)
        // is not visible.
        let sym_depth = self.brace_depth_at(sym_line, sym_col);
        let cursor_depth = self.brace_depth_at(target_line, target_col);
        sym_depth <= cursor_depth
    }

    /// Find the brace depth where the cursor currently sits, and the local
    /// variables/parameters visible there. Returns owned clones.
    fn visible_locals(&self, line: u32, character: u32) -> Vec<SymbolEntry> {
        let target_line = (line as i32) + 1;
        let target_col = character as i32;
        let cursor_depth = self.brace_depth_at(target_line, target_col);
        self.symbols
            .iter()
            .filter(|s| {
                matches!(s.kind, SymKind::Variable | SymKind::Parameter)
                    && self.local_visible_at(s, line, character)
            })
            .filter(|s| {
                // Extra guard: parameter/variable must not be shadowing-hidden
                // by being in a function that starts after the cursor.
                let sym_depth = self.brace_depth_at(s.line, s.col);
                sym_depth <= cursor_depth
            })
            .cloned()
            .collect()
    }
}

// ==================== Semantic Token Builder ====================

/// Collect the absolute (line, col) positions of module-name identifiers in
/// `import ...` / `from ... import ...` statements (excluding aliases and the
/// imported members of a `from` import). Used to mark those identifiers as
/// `NAMESPACE` so they get a distinct colour from plain variables.
fn import_module_token_positions(tokens: &[Token]) -> std::collections::HashSet<(i32, i32)> {
    let mut set = std::collections::HashSet::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        if t.r#type == TokenType::Keyword && (t.value == "import" || t.value == "from") {
            let is_from = t.value == "from";
            let mut j = i + 1;
            while j < tokens.len() {
                let nt = &tokens[j];
                if nt.r#type == TokenType::EndOfLine || nt.value == ";" {
                    break;
                }
                // `from X import ...`: stop before `import` so the imported
                // members are not treated as module names.
                if is_from && nt.value == "import" {
                    break;
                }
                // `import X as Alias` / `import X::Y`: `as` ends the module path,
                // while `::` continues it (nested module path segments).
                match &nt.r#type {
                    TokenType::Keyword => break,
                    TokenType::Operator => {
                        // `::` is a path separator inside a module path.
                        if nt.value != "::" {
                            break;
                        }
                    }
                    TokenType::Identifier => {
                        set.insert((nt.line, nt.col));
                    }
                    _ => break,
                }
                j += 1;
            }
        }
        i += 1;
    }
    set
}

/// Look up a symbol declared exactly at (line, col).
fn find_symbol_at<'a>(
    symbols: &'a [SymbolEntry],
    line: i32,
    col: i32,
) -> Option<&'a SymbolEntry> {
    symbols.iter().find(|s| s.line == line && s.col == col)
}

/// Map a `SymKind` to a semantic-token-type index (position in SEMANTIC_TYPES)
/// and the modifier bitset for its definition.
fn symbol_semantic(kind: &SymKind) -> (u32, u32) {
    match kind {
        SymKind::Function => (type_index(SemanticTokenType::FUNCTION), MOD_DECLARATION),
        SymKind::Method => (type_index(SemanticTokenType::METHOD), MOD_DECLARATION),
        SymKind::StaticFunc => (
            type_index(SemanticTokenType::FUNCTION),
            MOD_DECLARATION | MOD_STATIC,
        ),
        SymKind::Struct => (type_index(SemanticTokenType::STRUCT), MOD_DECLARATION),
        SymKind::Enum => (type_index(SemanticTokenType::ENUM), MOD_DECLARATION),
        SymKind::EnumVariant => (
            type_index(SemanticTokenType::ENUM_MEMBER),
            MOD_DECLARATION,
        ),
        SymKind::Variable => (type_index(SemanticTokenType::VARIABLE), MOD_DECLARATION),
        SymKind::Parameter => (type_index(SemanticTokenType::PARAMETER), MOD_DECLARATION),
        SymKind::Trait => (type_index(SemanticTokenType::TYPE), MOD_DECLARATION),
        SymKind::Import => (type_index(SemanticTokenType::NAMESPACE), 0),
        SymKind::ExternFn => (type_index(SemanticTokenType::FUNCTION), MOD_DECLARATION),
        SymKind::TypeAlias => (type_index(SemanticTokenType::TYPE), MOD_DECLARATION),
    }
}

// Modifier bits correspond to indices in SEMANTIC_MODIFIERS.
const MOD_DECLARATION: u32 = 1 << 0; // DECLARATION
const MOD_STATIC: u32 = 1 << 3; // STATIC

/// Index of a token type within SEMANTIC_TYPES.
fn type_index(ty: SemanticTokenType) -> u32 {
    SEMANTIC_TYPES
        .iter()
        .position(|t| *t == ty)
        .unwrap_or(0) as u32
}

fn is_capitalized(s: &str) -> bool {
    s.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false)
}

/// Extract `{name}` interpolation identifiers from a format-string literal
/// value (e.g. `"@{value: {x}}"` → `[(9, "x")]`). Respects `{{`/`}}` escapes
/// so brace pairs used as escaping do not produce dummy interpolation vars.
/// Byte offsets are relative to the start of `value`.
fn format_interpolation_spans(value: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            i += 2; // escaped {{ — skip
            continue;
        }
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            i += 2; // escaped }} — skip
            continue;
        }
        if bytes[i] == b'{' {
            // find matching close brace
            let start = i + 1;
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'}' {
                j += 1;
            }
            if j < bytes.len() {
                let inner = &value[start..j];
                let name = inner.trim();
                // Only simple identifier interpolation (no format spec like `{x:.2}`).
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':')
                {
                    // Take the segment before any `.`/`:` suffix to get the binding.
                    let ident = name
                        .split(|c| c == '.' || c == ':')
                        .next()
                        .unwrap_or(name)
                        .trim();
                    if !ident.is_empty()
                        && ident.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
                    {
                        out.push((start, ident.to_string()));
                    }
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Split the *literal* text of a string/format-string value into non-escape
/// `(start_offset, length)` spans (byte offsets within `value`). Escape
/// sequences (`\n`, `\t`, `\x41`, `\u{...}`, `\c`) are excluded so that the
/// semantic `string` token does not paint over them — letting the client's
/// syntax grammar (`constant.character.escape`) render them with their own
/// colour (VS Code semantic tokens otherwise override the TextMate escape
/// highlight, hiding `\n` & co).
///
/// When `skip_braces` is set (format strings), each `{ ... interp ... }` block
/// is also excluded (the interpolation identifier gets its own semantic token
/// in `build_semantic_tokens`).
fn string_literal_segments(value: &str, skip_braces: bool) -> Vec<(usize, usize)> {
    let bytes = value.as_bytes();
    let n = bytes.len();
    let mut segs: Vec<(usize, usize)> = Vec::new();
    let mut seg_start = 0usize;
    let mut i = 0usize;

    while i < n {
        let is_escape = bytes[i] == b'\\';
        let is_interp = skip_braces && bytes[i] == b'{';
        if !is_escape && !is_interp {
            i += 1;
            continue;
        }
        // Flush the literal text accumulated before this special span.
        if i > seg_start {
            segs.push((seg_start, i - seg_start));
        }
        // Advance past the escape / interpolation span.
        if is_escape {
            let mut len = 2; // `\` + one char
            if i + 1 < n {
                match bytes[i + 1] {
                    b'u' if i + 2 < n && bytes[i + 2] == b'{' => {
                        let mut j = i + 3;
                        while j < n && bytes[j] != b'}' {
                            j += 1;
                        }
                        len = j - i + 1;
                    }
                    b'x' => {
                        let mut j = i + 2;
                        let mut hex = 0;
                        while j < n && hex < 2 && bytes[j].is_ascii_hexdigit() {
                            j += 1;
                            hex += 1;
                        }
                        len = j - i;
                    }
                    _ => {}
                }
            }
            i += len;
        } else {
            // `{` interpolation block (or escaped `{{`).
            i += if i + 1 < n && bytes[i + 1] == b'{' {
                2 // `{{` literal brace pair — skip both
            } else {
                let mut j = i + 1;
                while j < n && bytes[j] != b'}' {
                    j += 1;
                }
                if j < n {
                    j - i + 1
                } else {
                    n - i
                }
            };
        }
        seg_start = i;
    }
    if n > seg_start {
        segs.push((seg_start, n - seg_start));
    }
    segs
}

/// Append one delta-encoded semantic token, maintaining the running prev
/// line/col bookkeeping for the relative encoding.
#[allow(clippy::too_many_arguments)]
fn push_sem(
    out: &mut Vec<SemanticToken>,
    prev_line: &mut u32,
    prev_start: &mut u32,
    line: u32,
    start: u32,
    length: u32,
    type_idx: u32,
    mods: u32,
) {
    let delta_line = line - *prev_line;
    let delta_start = if delta_line == 0 {
        start - *prev_start
    } else {
        start
    };
    out.push(SemanticToken {
        delta_line,
        delta_start,
        length,
        token_type: type_idx,
        token_modifiers_bitset: mods,
    });
    *prev_line = line;
    *prev_start = start;
}

/// Classify an identifier token's semantic type + modifiers based on the
/// symbol index and surrounding context.
fn classify_ident(
    tokens: &[Token],
    i: usize,
    symbols: &[SymbolEntry],
    ns_set: &std::collections::HashSet<(i32, i32)>,
) -> (u32, u32) {
    let t = &tokens[i];
    // 1. Position is a declaration (from the symbol index) — use its kind.
    if let Some(sym) = find_symbol_at(symbols, t.line, t.col) {
        return symbol_semantic(&sym.kind);
    }
    // 2. Module name in an import statement.
    if ns_set.contains(&(t.line, t.col)) {
        return (type_index(SemanticTokenType::NAMESPACE), 0);
    }
    // 3. Module qualifier before `::` (e.g. `io::println`, `Vec::new`).
    if let Some(next) = tokens.get(i + 1) {
        if next.r#type == TokenType::Operator && next.value == "::" {
            return (type_index(SemanticTokenType::NAMESPACE), 0);
        }
    }
    // 4. Capitalized identifier not matched above is a type reference.
    if is_capitalized(&t.value) {
        return (type_index(SemanticTokenType::TYPE), 0);
    }
    // 5. Identifier directly followed by `(` is a function call.
    if let Some(next) = tokens.get(i + 1) {
        if next.value == "(" {
            return (type_index(SemanticTokenType::FUNCTION), 0);
        }
    }
    // 6. Everything else is a variable reference.
    (type_index(SemanticTokenType::VARIABLE), 0)
}

/// Build delta-encoded semantic tokens from the lexer token stream.
fn build_semantic_tokens(tokens: &[Token], symbols: &[SymbolEntry]) -> Vec<SemanticToken> {
    let ns_set = import_module_token_positions(tokens);
    let mut out: Vec<SemanticToken> = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_start: u32 = 0;

    for i in 0..tokens.len() {
        let t = &tokens[i];
        if t.r#type == TokenType::EndOfFile || t.r#type == TokenType::EndOfLine {
            continue;
        }

        let (type_idx, mods): (u32, u32) = match &t.r#type {
            TokenType::Keyword => match t.value.as_str() {
                // Primitive types are lexed as keywords; color them as types.
                "int" | "float" | "str" | "bool" | "void" | "char" | "unit" => {
                    (type_index(SemanticTokenType::TYPE), 0)
                }
                _ => (type_index(SemanticTokenType::KEYWORD), 0),
            },
            TokenType::Number => (type_index(SemanticTokenType::NUMBER), 0),
            TokenType::String => {
                // Emit `string` tokens only for the non-escape text so the
                // client's TextMate `constant.character.escape` rendering of
                // `\n` etc. is not overridden by the semantic token.
                let line = (t.line as u32).saturating_sub(1);
                let content_base = t.col.max(0) as u32 + 1; // after opening quote
                for (off, len) in string_literal_segments(&t.value, false) {
                    push_sem(
                        &mut out,
                        &mut prev_line,
                        &mut prev_start,
                        line,
                        content_base + off as u32,
                        len.max(1) as u32,
                        type_index(SemanticTokenType::STRING),
                        0,
                    );
                }
                continue; // for-loop advances `i`
            }
            // Format string `@"...{var}..."`: colour literal (non-escape) text
            // as a string, and emit a `variable`/`type` token for each
            // interpolation identifier so `{var}` is typed as its binding.
            TokenType::FormatString => {
                let line = (t.line as u32).saturating_sub(1);
                let start = t.col.max(0) as u32;
                // String content begins after `@` and `"`.
                let content_base = start + 2;

                // Collect all sub-tokens for this literal (string segments and
                // interpolation ids) and emit them in ascending column order so
                // the delta encoding never has to go backwards.
                let mut parts: Vec<(u32, u32, u32, u32)> = Vec::new(); // (col,len,type,mods)
                for (off, len) in string_literal_segments(&t.value, true) {
                    parts.push((
                        content_base + off as u32,
                        len.max(1) as u32,
                        type_index(SemanticTokenType::STRING),
                        0,
                    ));
                }
                for (off, ident) in format_interpolation_spans(&t.value) {
                    let var_col = content_base + off as u32;
                    let (ity, imods) = if ident.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
                        (type_index(SemanticTokenType::TYPE), 0)
                    } else {
                        // Prefer declared variable type when known via symbol table.
                        let ty = symbols
                            .iter()
                            .find(|s| s.name == ident)
                            .map(|s| s.type_info.is_some())
                            .unwrap_or(false);
                        if ty {
                            (type_index(SemanticTokenType::TYPE), 0)
                        } else {
                            (type_index(SemanticTokenType::VARIABLE), 0)
                        }
                    };
                    parts.push((var_col, ident.len() as u32, ity, imods));
                }
                parts.sort_by_key(|p| p.0);
                for (col, len, ty, md) in parts {
                    push_sem(&mut out, &mut prev_line, &mut prev_start, line, col, len, ty, md);
                }
                continue; // for-loop advances `i`
            }
            TokenType::Operator => (type_index(SemanticTokenType::OPERATOR), 0),
            TokenType::Identifier => classify_ident(tokens, i, symbols, &ns_set),
            TokenType::Unknown | TokenType::EndOfLine | TokenType::EndOfFile => continue,
        };

        let line = (t.line as u32).saturating_sub(1); // LSP lines are 0-based
        let start = t.col.max(0) as u32;
        let length = (t.value.len() as u32).max(1);

        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            start - prev_start
        } else {
            start
        };

        out.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: type_idx,
            token_modifiers_bitset: mods,
        });

        prev_line = line;
        prev_start = start;
    }

    out
}

// ==================== Symbol Index Builder ====================

/// Extract documentation comments from the source text above a given line.
///
/// Scans upward from `line - 1` (0-based in the `lines` slice). Collects
/// consecutive `//`, `///`, `/* */`, `/** */` comment lines, skipping blank
/// lines and `#[...]` attribute lines in between. Stops at the first
/// non-comment, non-blank, non-attribute line.
///
/// Returns the cleaned comment text (markers stripped), or `None` if no
/// comment was found.
fn extract_doc_comment(source: &str, symbol_line_1based: i32) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    // symbol_line is 1-based; the line directly above it is at 0-based index
    // `symbol_line_1based - 2`.
    let start = symbol_line_1based.checked_sub(2)?; // 0-based index of line above symbol
    // `checked_sub` on i32 yields Some(-1) for line 1, so guard negatives too.
    if start < 0 || start >= lines.len() as i32 {
        return None;
    }

    // Phase 1: Skip blank lines and attributes directly above the symbol to
    // locate the bottom of the doc-comment block (the first comment line).
    // Such blanks/attributes are separators between the symbol and its doc
    // comment, not part of the comment itself.
    let mut bottom = start as usize;
    loop {
        let trimmed = lines[bottom].trim();
        if trimmed.is_empty() || trimmed.starts_with("#[") {
            if bottom == 0 {
                return None;
            }
            bottom -= 1;
            continue;
        }
        break;
    }

    // The located line must be a comment; otherwise there is no doc comment.
    // A multi-line block comment's closing line (e.g. ` * text */`) does not
    // start with `/*`, so also accept lines ending with `*/`.
    let bottom_trimmed = lines[bottom].trim();
    if !(bottom_trimmed.starts_with("//")
        || bottom_trimmed.starts_with("/*")
        || bottom_trimmed.ends_with("*/"))
    {
        return None;
    }

    // Phase 2: Collect comment lines upward, preserving blank lines that are
    // *inside* the comment block (used as Markdown paragraph separators).
    // Stop at attributes or code lines.
    let mut comment_lines: Vec<String> = Vec::new(); // collected bottom-up
    let mut idx = bottom;
    while idx < lines.len() {
        let trimmed = lines[idx].trim();

        // Inner blank line — keep as a Markdown paragraph separator.
        if trimmed.is_empty() {
            comment_lines.push(String::new());
            if idx == 0 {
                break;
            }
            idx -= 1;
            continue;
        }

        // Attribute marks the end of the doc block.
        if trimmed.starts_with("#[") {
            break;
        }

        // Line comment: // or /// (content is preserved as-is, so Markdown
        // headings like `#`, `##`, `###` inside `///` comments work natively).
        if trimmed.starts_with("//") {
            let text = trimmed.trim_start_matches('/');
            let text = text.trim_start(); // remove leading spaces after ///
            comment_lines.push(text.to_string());
            if idx == 0 {
                break;
            }
            idx -= 1;
            continue;
        }

        // Block comment (single-line): /* ... */ or /** ... */
        if trimmed.starts_with("/*") && trimmed.ends_with("*/") {
            let inner = &trimmed[2..trimmed.len() - 2];
            let inner = inner.trim_start_matches('*').trim();
            comment_lines.push(inner.to_string());
            if idx == 0 {
                break;
            }
            idx -= 1;
            continue;
        }

        // Multi-line block comment ending on this line: ... */
        if trimmed.ends_with("*/") && !trimmed.starts_with("//") {
            // Collect lines upward until we find the opening /*.
            // Push in bottom-up order so the final reverse() yields top-down.
            let mut bi = idx;
            loop {
                let raw = lines[bi].trim();
                if bi == idx {
                    // Last line of the block (contains closing */)
                    let without_end = raw.trim_end_matches("*/").trim();
                    if without_end.starts_with("/*") {
                        // Single-line block comment handled above; skip
                        break;
                    }
                    let content = without_end.trim_start_matches('*').trim();
                    comment_lines.push(content.to_string());
                } else if raw.starts_with("/*") {
                    // First line of the block comment
                    let first = raw.trim_start_matches('/').trim_start_matches('*').trim();
                    comment_lines.push(first.to_string());
                    break;
                } else {
                    // Middle line: strip leading * if present
                    let mid = raw.trim_start_matches('*').trim();
                    comment_lines.push(mid.to_string());
                }
                if bi == 0 {
                    break;
                }
                bi -= 1;
            }
            if idx == 0 {
                break;
            }
            idx -= 1;
            continue;
        }

        // Code line — stop scanning.
        break;
    }

    // Reverse to top-down order, then trim leading/trailing blank lines while
    // keeping inner blank lines (Markdown paragraph separators).
    comment_lines.reverse();
    while comment_lines.first().map(|s| s.is_empty()).unwrap_or(false) {
        comment_lines.remove(0);
    }
    while comment_lines.last().map(|s| s.is_empty()).unwrap_or(false) {
        comment_lines.pop();
    }

    if comment_lines.is_empty() {
        return None;
    }
    let result = comment_lines.join("\n");
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Build a symbol index by scanning the token stream.
/// `source` is the original source text, used to extract doc comments.
fn build_symbol_index(tokens: &[Token], source: &str) -> Vec<SymbolEntry> {
    let mut symbols = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.r#type == TokenType::Keyword {
            match tok.value.as_str() {
                "func" | "static" => {
                    // `static func name(...)` or `func name(...)`
                    let mut j = i;
                    let is_static = if tokens[j].value == "static" {
                        // check next token is `func`
                        if let Some(n) = tokens.get(j + 1) {
                            if n.value == "func" {
                                j += 1;
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if let Some(name_tok) = tokens.get(j + 1) {
                        if name_tok.r#type == TokenType::Identifier {
                            let type_info = find_return_type(tokens, j);
                            let kind = if is_static {
                                SymKind::StaticFunc
                            } else {
                                SymKind::Function
                            };
                            symbols.push(SymbolEntry {
                                name: name_tok.value.clone(),
                                kind,
                                line: name_tok.line,
                                col: name_tok.col,
                                len: name_tok.value.len() as i32,
                                type_info,
                                parent: None,
                                doc_comment: None,
                            });
                            extract_parameters(tokens, j, &mut symbols);
                        }
                    }
                }
                "struct" => {
                    if let Some(name_tok) = tokens.get(i + 1) {
                        if name_tok.r#type == TokenType::Identifier {
                            symbols.push(SymbolEntry {
                                name: name_tok.value.clone(),
                                kind: SymKind::Struct,
                                line: name_tok.line,
                                col: name_tok.col,
                                len: name_tok.value.len() as i32,
                                type_info: None,
                                parent: None,
                                doc_comment: None,
                            });
                        }
                    }
                }
                "enum" => {
                    if let Some(name_tok) = tokens.get(i + 1) {
                        if name_tok.r#type == TokenType::Identifier {
                            let enum_name = name_tok.value.clone();
                            symbols.push(SymbolEntry {
                                name: enum_name.clone(),
                                kind: SymKind::Enum,
                                line: name_tok.line,
                                col: name_tok.col,
                                len: name_tok.value.len() as i32,
                                type_info: None,
                                parent: None,
                                doc_comment: None,
                            });
                            // Scan enum body for variants
                            scan_enum_variants(tokens, i, &enum_name, &mut symbols);
                        }
                    }
                }
                "type" => {
                    if let Some(name_tok) = tokens.get(i + 1) {
                        if name_tok.r#type == TokenType::Identifier {
                            let type_info = find_type_alias_rhs(tokens, i);
                            symbols.push(SymbolEntry {
                                name: name_tok.value.clone(),
                                kind: SymKind::TypeAlias,
                                line: name_tok.line,
                                col: name_tok.col,
                                len: name_tok.value.len() as i32,
                                type_info,
                                parent: None,
                                doc_comment: None,
                            });
                        }
                    }
                }
                "var" | "val" => {
                    if let Some(name_tok) = tokens.get(i + 1) {
                        if name_tok.r#type == TokenType::Identifier {
                            let type_info = find_var_type(tokens, i);
                            symbols.push(SymbolEntry {
                                name: name_tok.value.clone(),
                                kind: SymKind::Variable,
                                line: name_tok.line,
                                col: name_tok.col,
                                len: name_tok.value.len() as i32,
                                type_info,
                                parent: None,
                                doc_comment: None,
                            });
                        }
                    }
                }
                "import" => {
                    if let Some(name_tok) = tokens.get(i + 1) {
                        if name_tok.r#type == TokenType::Identifier {
                            symbols.push(SymbolEntry {
                                name: name_tok.value.clone(),
                                kind: SymKind::Import,
                                line: name_tok.line,
                                col: name_tok.col,
                                len: name_tok.value.len() as i32,
                                type_info: None,
                                parent: None,
                                doc_comment: None,
                            });
                        }
                    }
                }
                "from" => {
                    // `from module import member1, member2, ...;`
                    // Index the module as Import, and each member as Function.
                    if let Some(name_tok) = tokens.get(i + 1) {
                        if name_tok.r#type == TokenType::Identifier {
                            symbols.push(SymbolEntry {
                                name: name_tok.value.clone(),
                                kind: SymKind::Import,
                                line: name_tok.line,
                                col: name_tok.col,
                                len: name_tok.value.len() as i32,
                                type_info: None,
                                parent: None,
                                doc_comment: None,
                            });
                            // Scan for `import` keyword, then collect member names
                            let mut j = i + 2;
                            while j < tokens.len()
                                && !(tokens[j].r#type == TokenType::Keyword
                                    && tokens[j].value == "import")
                            {
                                j += 1;
                            }
                            // j is at `import` keyword (or end)
                            j += 1; // skip `import`
                            while j < tokens.len() {
                                let mt = &tokens[j];
                                if mt.r#type == TokenType::Identifier {
                                    symbols.push(SymbolEntry {
                                        name: mt.value.clone(),
                                        kind: SymKind::Function,
                                        line: mt.line,
                                        col: mt.col,
                                        len: mt.value.len() as i32,
                                        type_info: None,
                                        parent: Some(name_tok.value.clone()),
                                        doc_comment: None,
                                    });
                                }
                                if mt.value == ";" || mt.line != name_tok.line {
                                    break;
                                }
                                j += 1;
                            }
                        }
                    }
                }
                "extern" => {
                    // `extern "C" { ... }` body functions
                    scan_extern_block(tokens, i, &mut symbols);
                }
                _ => {}
            }
        }
        // "trait" is an identifier in the lexer
        if tok.r#type == TokenType::Identifier && tok.value == "trait" {
            if let Some(name_tok) = tokens.get(i + 1) {
                if name_tok.r#type == TokenType::Identifier {
                    symbols.push(SymbolEntry {
                        name: name_tok.value.clone(),
                        kind: SymKind::Trait,
                        line: name_tok.line,
                        col: name_tok.col,
                        len: name_tok.value.len() as i32,
                        type_info: None,
                        parent: None,
                        doc_comment: None,
                    });
                }
            }
        }
        // "impl" — extract methods defined in impl blocks
        if tok.r#type == TokenType::Keyword && tok.value == "impl" {
            let mut j = i + 1;
            let mut struct_name: Option<String> = None;
            while j < tokens.len() && tokens[j].value != "{" {
                if tokens[j].value == "for" {
                    j += 1;
                    while j < tokens.len() && tokens[j].value != "{" {
                        if tokens[j].r#type == TokenType::Identifier {
                            struct_name = Some(tokens[j].value.clone());
                        }
                        j += 1;
                    }
                    break;
                }
                if tokens[j].r#type == TokenType::Identifier {
                    struct_name = Some(tokens[j].value.clone());
                }
                j += 1;
            }
            if let Some(ref sname) = struct_name {
                if j < tokens.len() && tokens[j].value == "{" {
                    let mut depth = 1i32;
                    let mut k = j + 1;
                    while k < tokens.len() && depth > 0 {
                        let t = &tokens[k];
                        if t.value == "{" {
                            depth += 1;
                        }
                        if t.value == "}" {
                            depth -= 1;
                        }
                        if depth > 0 {
                            // static func
                            if t.r#type == TokenType::Keyword && t.value == "static" {
                                if let Some(nt) = tokens.get(k + 1) {
                                    if nt.value == "func" {
                                        if let Some(name_tok) = tokens.get(k + 2) {
                                            if name_tok.r#type == TokenType::Identifier {
                                                let type_info = find_return_type(tokens, k + 1);
                                                symbols.push(SymbolEntry {
                                                    name: name_tok.value.clone(),
                                                    kind: SymKind::StaticFunc,
                                                    line: name_tok.line,
                                                    col: name_tok.col,
                                                    len: name_tok.value.len() as i32,
                                                    type_info,
                                                    parent: Some(sname.clone()),
                                                    doc_comment: None,
                                                });
                                                extract_parameters(tokens, k + 1, &mut symbols);
                                            }
                                        }
                                    }
                                }
                            }
                            // normal func method
                            if t.r#type == TokenType::Keyword && t.value == "func" {
                                if let Some(name_tok) = tokens.get(k + 1) {
                                    if name_tok.r#type == TokenType::Identifier {
                                        let type_info = find_return_type(tokens, k);
                                        symbols.push(SymbolEntry {
                                            name: name_tok.value.clone(),
                                            kind: SymKind::Method,
                                            line: name_tok.line,
                                            col: name_tok.col,
                                            len: name_tok.value.len() as i32,
                                            type_info,
                                            parent: Some(sname.clone()),
                                            doc_comment: None,
                                        });
                                        extract_parameters(tokens, k, &mut symbols);
                                    }
                                }
                            }
                        }
                        k += 1;
                    }
                }
            }
        }
        i += 1;
    }

    // Post-process: attach doc comments from the source text.
    for sym in &mut symbols {
        sym.doc_comment = extract_doc_comment(source, sym.line);
    }

    symbols
}

fn scan_enum_variants(
    tokens: &[Token],
    enum_idx: usize,
    enum_name: &str,
    symbols: &mut Vec<SymbolEntry>,
) {
    // find `{` after enum name
    let mut j = enum_idx + 1;
    while j < tokens.len() && tokens[j].value != "{" {
        j += 1;
    }
    if j >= tokens.len() {
        return;
    }
    let mut depth = 1i32;
    j += 1;
    while j < tokens.len() && depth > 0 {
        let t = &tokens[j];
        if t.value == "{" {
            depth += 1;
        } else if t.value == "}" {
            depth -= 1;
            if depth == 0 {
                break;
            }
        } else if depth == 1 {
            // Enum variants: VariantName( or VariantName, or VariantName{
            // Only at start positions (after `{` or `,`).
            let is_start = j == enum_idx + 2 || {
                let mut prev = j - 1;
                while prev > enum_idx + 1 {
                    let pt = &tokens[prev];
                    if pt.value == "," || pt.value == "{" {
                        break;
                    }
                    if pt.r#type != TokenType::EndOfLine && pt.value.trim() != "" {
                        // e.g., VariantName( — previous is `VariantName`; skip to simplify
                        // keep false for non-separator
                    }
                    prev -= 1;
                }
                tokens[prev].value == "," || tokens[prev].value == "{"
            };
            if t.r#type == TokenType::Identifier && is_caps_ident(&t.value) && is_start {
                symbols.push(SymbolEntry {
                    name: t.value.clone(),
                    kind: SymKind::EnumVariant,
                    line: t.line,
                    col: t.col,
                    len: t.value.len() as i32,
                    type_info: Some(enum_name.to_string()),
                    parent: Some(enum_name.to_string()),
                    doc_comment: None,
                });
            }
        }
        j += 1;
    }
}

fn is_caps_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() => true,
        _ => false,
    }
}

fn scan_extern_block(tokens: &[Token], extern_idx: usize, symbols: &mut Vec<SymbolEntry>) {
    // Skip `extern "C"` (or `extern "name"`)
    let mut j = extern_idx + 1;
    // Skip linkage string if present
    if let Some(t) = tokens.get(j) {
        if matches!(t.r#type, TokenType::String | TokenType::FormatString) || t.value.starts_with('"') {
            j += 1;
        }
    }
    if j >= tokens.len() || tokens[j].value != "{" {
        return;
    }
    let mut depth = 1i32;
    j += 1;
    while j < tokens.len() && depth > 0 {
        let t = &tokens[j];
        if t.value == "{" {
            depth += 1;
        } else if t.value == "}" {
            depth -= 1;
            if depth == 0 {
                break;
            }
        } else if depth == 1 && t.r#type == TokenType::Keyword && t.value == "func" {
            if let Some(name_tok) = tokens.get(j + 1) {
                if name_tok.r#type == TokenType::Identifier {
                    let type_info = find_return_type(tokens, j);
                    symbols.push(SymbolEntry {
                        name: name_tok.value.clone(),
                        kind: SymKind::ExternFn,
                        line: name_tok.line,
                        col: name_tok.col,
                        len: name_tok.value.len() as i32,
                        type_info,
                        parent: None,
                        doc_comment: None,
                    });
                    extract_parameters(tokens, j, symbols);
                }
            }
        }
        j += 1;
    }
}

/// Find the return type annotation after `func name(...): Type`.
fn find_return_type(tokens: &[Token], func_idx: usize) -> Option<String> {
    let mut depth = 0i32;
    let mut found_close = false;
    let mut j = func_idx + 1;
    while j < tokens.len() {
        let t = &tokens[j];
        if t.value == "(" {
            depth += 1;
        } else if t.value == ")" {
            depth -= 1;
            if depth == 0 {
                found_close = true;
                break;
            }
        }
        j += 1;
    }
    if !found_close {
        return None;
    }
    j += 1;
    if j < tokens.len() && tokens[j].value == ":" {
        j += 1;
        let mut type_str = String::new();
        let mut paren = 0i32;
        while j < tokens.len() {
            let t = &tokens[j];
            if t.value == "(" { paren += 1; }
            if t.value == ")" {
                if paren == 0 {
                    break;
                }
                paren -= 1;
            }
            if t.value == "{" || t.value == ";" || t.r#type == TokenType::EndOfLine {
                break;
            }
            if !type_str.is_empty() && t.r#type != TokenType::Operator && t.value != "[" && t.value != "]"
            {
                type_str.push(' ');
            }
            type_str.push_str(&t.value);
            j += 1;
        }
        let trimmed = type_str.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

fn find_type_alias_rhs(tokens: &[Token], type_idx: usize) -> Option<String> {
    // type Name = RHS ;
    let mut j = type_idx + 2; // skip `type` and name
    if j < tokens.len() && tokens[j].value == "=" {
        j += 1;
        let mut type_str = String::new();
        while j < tokens.len() {
            let t = &tokens[j];
            if t.value == ";" || t.value == "{" || t.r#type == TokenType::EndOfLine {
                break;
            }
            if !type_str.is_empty() && t.r#type != TokenType::Operator && t.value != "[" && t.value != "]"
            {
                type_str.push(' ');
            }
            type_str.push_str(&t.value);
            j += 1;
        }
        let trimmed = type_str.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    None
}

fn find_var_type(tokens: &[Token], var_idx: usize) -> Option<String> {
    if let Some(name_tok) = tokens.get(var_idx + 1) {
        if name_tok.r#type == TokenType::Identifier {
            let mut j = var_idx + 2;
            if j < tokens.len() && tokens[j].value == ":" {
                j += 1;
                let mut type_str = String::new();
                while j < tokens.len() {
                    let t = &tokens[j];
                    if t.value == "=" || t.value == ";" || t.r#type == TokenType::EndOfLine {
                        break;
                    }
                    if !type_str.is_empty() && t.r#type != TokenType::Operator && t.value != "[" && t.value != "]"
                    {
                        type_str.push(' ');
                    }
                    type_str.push_str(&t.value);
                    j += 1;
                }
                let trimmed = type_str.trim().to_string();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
        }
    }
    None
}

fn extract_parameters(tokens: &[Token], func_idx: usize, symbols: &mut Vec<SymbolEntry>) {
    let mut j = func_idx + 1;
    while j < tokens.len() && tokens[j].value != "(" {
        j += 1;
    }
    if j >= tokens.len() {
        return;
    }
    j += 1;
    let mut depth = 0i32;
    while j < tokens.len() {
        let t = &tokens[j];
        if t.value == "(" || t.value == "[" {
            depth += 1;
        } else if t.value == ")" || t.value == "]" {
            if depth == 0 {
                break;
            }
            depth -= 1;
        } else if depth == 0 && t.r#type == TokenType::Identifier {
            // Skip `self` / `self,`
            if t.value == "self" {
                j += 1;
                continue;
            }
            if j + 1 < tokens.len() && tokens[j + 1].value == ":" {
                let mut k = j + 2;
                let mut type_str = String::new();
                while k < tokens.len() {
                    let tk = &tokens[k];
                    if tk.value == "," || tk.value == ")" {
                        break;
                    }
                    if !type_str.is_empty()
                        && tk.r#type != TokenType::Operator
                        && tk.value != "["
                        && tk.value != "]"
                    {
                        type_str.push(' ');
                    }
                    type_str.push_str(&tk.value);
                    k += 1;
                }
                symbols.push(SymbolEntry {
                    name: t.value.clone(),
                    kind: SymKind::Parameter,
                    line: t.line,
                    col: t.col,
                    len: t.value.len() as i32,
                    type_info: if type_str.trim().is_empty() {
                        None
                    } else {
                        Some(type_str.trim().to_string())
                    },
                    parent: None,
                    doc_comment: None,
                });
                j = k;
                continue;
            }
        }
        j += 1;
    }
}

/// Parse the signatures of all callables declared in a token stream.
/// Returns a map keyed by function name. Handles `func name(...): Ret` and
/// `static func name(...): Ret`.
fn collect_signatures(tokens: &[Token], source: &str) -> std::collections::HashMap<String, FuncSignature> {
    use std::collections::HashMap;
    let mut sigs = HashMap::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.r#type != TokenType::Keyword || (tok.value != "func" && tok.value != "static") {
            i += 1;
            continue;
        }
        let mut func_idx = i;
        if tok.value == "static" {
            // `static func`
            if let Some(n) = tokens.get(i + 1) {
                if n.value == "func" {
                    func_idx = i + 1;
                } else {
                    i += 1;
                    continue;
                }
            } else {
                i += 1;
                continue;
            }
        }
        let Some(name_tok) = tokens.get(func_idx + 1) else {
            i += 1;
            continue;
        };
        if name_tok.r#type != TokenType::Identifier {
            i += 1;
            continue;
        }

        let mut sig = FuncSignature {
            name: name_tok.value.clone(),
            ..Default::default()
        };
        sig.doc = extract_doc_comment(source, tok.line);

        // Parse the parameter list `( ..., ... )`.
        let mut j = func_idx + 2;
        while j < tokens.len() && tokens[j].value != "(" {
            j += 1;
        }
        let mut depth = 0i32;
        j += 1;
        while j < tokens.len() {
            let t = &tokens[j];
            if t.value == "(" || t.value == "[" {
                depth += 1;
                j += 1;
                continue;
            }
            if t.value == ")" || t.value == "]" {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                j += 1;
                continue;
            }
            if depth == 0 && t.r#type == TokenType::Identifier && t.value != "self" {
                // Candidate: `name` or `name: type`
                let is_param = j + 1 < tokens.len() && tokens[j + 1].value == ":";
                if is_param {
                    let mut k = j + 2;
                    let mut ty = String::new();
                    while k < tokens.len() {
                        let tk = &tokens[k];
                        if tk.value == "," || tk.value == ")" || tk.r#type == TokenType::EndOfLine {
                            break;
                        }
                        if !ty.is_empty() && tk.r#type != TokenType::Operator && tk.value != "[" && tk.value != "]" {
                            ty.push(' ');
                        }
                        ty.push_str(&tk.value);
                        k += 1;
                    }
                    let name = t.value.clone();
                    let ty = if ty.trim().is_empty() { None } else { Some(ty.trim().to_string()) };
                    let label = match &ty {
                        Some(t) => format!("{}: {}", name, t),
                        None => name.clone(),
                    };
                    sig.param_names.push(name);
                    sig.param_types.push(ty);
                    sig.param_labels.push(label);
                    j = k;
                    continue;
                }
            }
            j += 1;
        }

        // Return type after the closing paren: `): Ret {` or `): Ret` EOL.
        let mut j2 = j + 1; // skip `)`
        if j2 < tokens.len() && tokens[j2].value == ":" {
            j2 += 1;
            let mut rt = String::new();
            let mut paren = 0i32;
            while j2 < tokens.len() {
                let t = &tokens[j2];
                if t.value == "(" { paren += 1; }
                if t.value == ")" {
                    if paren == 0 { break; }
                    paren -= 1;
                }
                if t.value == "{" || t.value == ";" || t.r#type == TokenType::EndOfLine { break; }
                if !rt.is_empty() && t.r#type != TokenType::Operator && t.value != "[" && t.value != "]" {
                    rt.push(' ');
                }
                rt.push_str(&t.value);
                j2 += 1;
            }
            let rt = rt.trim().to_string();
            if !rt.is_empty() {
                sig.return_type = Some(rt);
            }
        }

        sigs.insert(sig.name.clone(), sig);
        i = func_idx + 1; // advance past the `func` keyword to avoid reprocessing
    }
    sigs
}

/// If the cursor lies inside a call argument list, return the called
/// function's name (the identifier immediately before the enclosing `(`) and
/// the 0-based active parameter index (count of top-level commas so far).
/// Returns `None` when the cursor is not inside any call.
fn call_at(tokens: &[Token], line: u32, character: u32) -> Option<(String, u32)> {
    let tgt_line = (line as i32) + 1;
    let tgt_col = character as i32;

    // Enumerate every '(' .. ')' pair and pick the innermost one whose
    // argument span contains the cursor.
    let mut best: Option<(usize, usize)> = None;
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i].value != "(" {
            i += 1;
            continue;
        }
        // Match this '(' to its ')'.
        let mut depth = 0i32;
        let mut j = i;
        let mut steps = 0usize;
        while j < tokens.len() {
            if tokens[j].value == "(" {
                depth += 1;
            } else if tokens[j].value == ")" {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            j += 1;
            steps += 1;
            if steps > tokens.len() {
                break;
            }
        }
        if j >= tokens.len() {
            i += 1;
            continue;
        }
        let lp = &tokens[i];
        let rp = &tokens[j];
        // Cursor strictly between the parens (inclusive of the close paren
        // column is handled by <= rp.col; the open paren itself is a call site).
        let after_open = lp.line < tgt_line || (lp.line == tgt_line && lp.col <= tgt_col);
        let before_close = rp.line > tgt_line || (rp.line == tgt_line && tgt_col <= rp.col);
        if after_open && before_close {
            // Prefer the innermost (largest lp index) qualifying pair.
            if best.map(|(b, _)| i >= b).unwrap_or(true) {
                best = Some((i, j));
            }
        }
        i += 1;
    }

    let (lp_idx, rp_idx) = best?;

    // Function name = nearest identifier before '(' skipping `::`/`.` prefixes.
    let mut k = lp_idx;
    let mut callee: Option<String> = None;
    while k > 0 {
        let prev = &tokens[k - 1];
        match &prev.r#type {
            TokenType::Identifier => {
                callee = Some(prev.value.clone());
                break;
            }
            TokenType::Operator => {
                if prev.value == "::" || prev.value == "." {
                    k -= 1;
                    continue;
                }
                break;
            }
            _ => break,
        }
    }
    let callee = callee?;
    Some((
        callee,
        count_top_commas(tokens, lp_idx, rp_idx, tgt_line, tgt_col),
    ))
}

/// Count top-level commas between the enclosing parens that occur before the
/// cursor — the 0-based active parameter index.
fn count_top_commas(
    tokens: &[Token],
    lp_idx: usize,
    rp_idx: usize,
    tgt_line: i32,
    tgt_col: i32,
) -> u32 {
    let mut depth = 0i32;
    let mut count = 0u32;
    let mut i = lp_idx + 1;
    while i < rp_idx {
        let t = &tokens[i];
        if t.value == "(" || t.value == "[" {
            depth += 1;
        } else if t.value == ")" || t.value == "]" {
            depth -= 1;
        } else if depth == 0 && t.value == "," {
            // Only count commas that are before the cursor position.
            let before = t.line < tgt_line || (t.line == tgt_line && t.col <= tgt_col);
            if before {
                count += 1;
            }
        }
        i += 1;
    }
    count
}

/// Build signature-help response with the given active parameter index.
fn signature_help_for(sig: &FuncSignature, active_param: u32) -> SignatureHelp {
    let label = {
        let mut l = format!("{}(", sig.name);
        l.push_str(&sig.param_labels.join(", "));
        l.push(')');
        if let Some(rt) = &sig.return_type {
            l.push_str(&format!(": {}", rt));
        }
        l
    };

    let parameters: Option<Vec<ParameterInformation>> = Some(
        sig.param_labels
            .iter()
            .map(|pl| ParameterInformation {
                label: ParameterLabel::Simple(pl.clone()),
                documentation: None,
            })
            .collect(),
    );

    let info = SignatureInformation {
        label,
        documentation: sig.doc.as_ref().map(|d| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: d.clone(),
            })
        }),
        parameters,
        active_parameter: Some(active_param),
    };

    SignatureHelp {
        signatures: vec![info],
        active_signature: Some(0),
        active_parameter: Some(active_param),
    }
}

/// Compute folding ranges from matching `{`/`}` pairs (multi-line only).
fn mk_inlay_hint(position: Position, label: impl Into<InlayHintLabel>, kind: InlayHintKind) -> InlayHint {
    InlayHint {
        position,
        label: label.into(),
        kind: Some(kind),
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: None,
        data: None,
    }
}

fn brace_folding_ranges(tokens: &[Token]) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        if t.value == "{" {
            stack.push(i);
        } else if t.value == "}" {
            if let Some(open) = stack.pop() {
                let open_tok = &tokens[open];
                let start_line = (open_tok.line as u32).saturating_sub(1);
                let end_line = (t.line as u32).saturating_sub(1);
                if end_line > start_line {
                    ranges.push(FoldingRange {
                        start_line,
                        start_character: Some(open_tok.col as u32),
                        end_line,
                        end_character: Some(t.col as u32),
                        kind: None,
                        collapsed_text: Some("…".to_string()),
                    });
                }
            }
        }
        i += 1;
    }
    ranges
}

/// Best-effort inference of a simple expression's type, starting at
/// `start_idx` of `tokens`. Returns a string type or `None`.
fn infer_expr_type(
    tokens: &[Token],
    mut idx: usize,
    state: &DocState,
) -> Option<String> {
    // Skip a leading reference/unary operator.
    while idx < tokens.len() {
        if let TokenType::Operator = tokens[idx].r#type {
            if matches!(tokens[idx].value.as_str(), "&" | "!" | "*") {
                idx += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    let t = tokens.get(idx)?;
    match t.r#type {
        TokenType::String | TokenType::FormatString => return Some("str".to_string()),
        TokenType::Number => return Some("int".to_string()),
        _ => {}
    }
    match t.value.as_str() {
        "true" | "false" => return Some("bool".to_string()),
        "null" | "none" => return Some("null".to_string()),
        "self" => return Some("self".to_string()),
        _ => {}
    }
    if t.r#type == TokenType::Identifier {
        let next = tokens.get(idx + 1).map(|n| n.value.as_str());
        if next == Some("(") {
            // Constructor / call `Type(...)` or `f(...)`.
            return Some(t.value.clone());
        }
        if let Some(sym) = state.find_definition(&t.value) {
            return sym.type_info.clone();
        }
        if t.value.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
            return Some(t.value.clone());
        }
        return None;
    }
    None
}

/// Whether the `(` following the identifier at `ident_idx` is a *declaration*
/// parameter list rather than a call. This distinguishes `func add(a: int)`
/// (a declaration — no call argument hints) from `add(x, 1)` (a real call).
fn is_declaration_call_paren(tokens: &[Token], ident_idx: usize) -> bool {
    let prev1 = tokens.get(ident_idx.wrapping_sub(1)).map(|x| x.value.as_str());
    let prev2 = ident_idx >= 2
        && tokens.get(ident_idx - 2).map(|x| x.value.as_str()) == Some("func");
    matches!(
        prev1,
        Some("func") | Some("static") | Some("constructor") | Some("operator")
    ) || prev2
}

/// Build per-argument inlay hints ("name:") for a call at `ident_idx`.
/// Returns hints placed just before each top-level argument.
fn argument_position_hints(
    tokens: &[Token],
    ident_idx: usize,
    sig: &FuncSignature,
) -> Vec<InlayHint> {
    let mut hints = Vec::new();
    let mut j = ident_idx + 1;
    while j < tokens.len() && tokens[j].value != "(" {
        j += 1;
    }
    if j >= tokens.len() {
        return hints;
    }
    let mut depth = 0i32;
    let mut param_idx = 0usize;
    j += 1; // into args
    while j < tokens.len() {
        let t = &tokens[j];
        if t.value == "(" || t.value == "[" {
            depth += 1;
            j += 1;
            continue;
        }
        if t.value == ")" || t.value == "]" {
            if depth == 0 {
                break;
            }
            depth -= 1;
            j += 1;
            continue;
        }
        if depth == 0 {
            // Skip punctuation (commas) and non-expression starts (operators).
            if t.value == "," {
                // next argument begins after comma
            } else if t.value == ":" || t.value == "::" {
                // labeled argument `name: expr` — skip
            } else if !is_punctuation(&t.value) {
                if let Some(pname) = sig.param_names.get(param_idx) {
                    hints.push(mk_inlay_hint(
                        Position::new(
                            (t.line as u32).saturating_sub(1),
                            t.col.max(0) as u32,
                        ),
                        InlayHintLabel::String(format!("{}:", pname)),
                        InlayHintKind::PARAMETER,
                    ));
                }
                param_idx += 1;
            }
        }
        // Skip to end of current argument expression: stop at top-level comma/close.
        if depth == 0 {
            let is_arg_end = t.value == ","
                || t.value == ")";
            if is_arg_end {
                j += 1;
                continue;
            }
        }
        j += 1;
    }
    hints
}

fn is_punctuation(v: &str) -> bool {
    matches!(v, "(" | ")" | "[" | "]" | "," | ":" | "::" | "." | ";" | "{ " | "}")
}

/// Insert an `import <module>;` (or `from <module> import <name>;`) statement
/// at the top of the file if not already present.
fn build_import_action(uri: &url::Url, module: &str, state: &DocState) -> CodeAction {
    if state
        .symbols
        .iter()
        .any(|s| s.kind == SymKind::Import && s.name == module)
    {
        // Already imported; still offer for safety but title reflects nothing to do.
    }
    let text = format!("import {};\n", module);
    let edit = TextEdit {
        range: Range::new(insert_import_position(&state.source), insert_import_position(&state.source)),
        new_text: text,
    };
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    CodeAction {
        title: format!("Import `{}`", module),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn build_import_from_action(
    uri: &url::Url,
    module: &str,
    name: &str,
    state: &DocState,
) -> CodeAction {
    let text = format!("from {} import {};\n", module, name);
    let edit = TextEdit {
        range: Range::new(insert_import_position(&state.source), insert_import_position(&state.source)),
        new_text: text,
    };
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), vec![edit]);
    CodeAction {
        title: format!("Import `{}` from `{}`", name, module),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: None,
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn insert_import_position(source: &str) -> Position {
    // Insert at line 0; safe for typical .gbl files (assuming no shebang line
    // that must stay first). Could be refined to skip leading doc comments.
    let _ = source;
    Position::new(0, 0)
}

// ==================== Lib Paths ====================

/// Build library search paths.
/// `workspace_roots` — optional LSP workspace roots to resolve relative `std`.
fn build_lib_paths(file_path: &str, workspace_roots: &[PathBuf]) -> Vec<String> {
    let mut paths = Vec::new();

    if let Some(parent) = PathBuf::from(file_path).parent() {
        if let Some(p) = parent.join("lib").to_str() {
            paths.push(p.to_string());
        }
        if let Some(grandparent) = parent.parent() {
            if let Some(p) = grandparent.join("lib").to_str() {
                paths.push(p.to_string());
            }
            if let Some(p) = grandparent.join("std").to_str() {
                paths.push(p.to_string());
            }
        }
        if let Some(p) = parent.join("std").to_str() {
            paths.push(p.to_string());
        }
    }

    // LSP workspace roots: try `<root>/std` and `<root>/lib/std`
    for root in workspace_roots {
        if let Some(p) = root.join("std").to_str() {
            paths.push(p.to_string());
        }
        if let Some(p) = root.join("lib").join("std").to_str() {
            paths.push(p.to_string());
        }
    }

    paths.push("std".to_string());

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // target/debug/std, target/release/std
            if let Some(p) = exe_dir.join("std").to_str().map(|s| s.to_string()) {
                paths.push(p);
            }
            // target/std
            if let Some(p) = exe_dir
                .parent()
                .map(|d| d.join("std"))
                .and_then(|d| d.to_str().map(|s| s.to_string()))
            {
                paths.push(p);
            }
            // <repo>/std (two levels up: target/debug -> target -> repo root)
            if let Some(p) = exe_dir
                .parent()
                .and_then(|d| d.parent())
                .map(|d| d.join("std"))
                .and_then(|d| d.to_str().map(|s| s.to_string()))
            {
                paths.push(p);
            }
            // Installed layout with lib/: ~/.gobol/bin/gobol-lsp → ~/.gobol/lib
            // (std/mod.gbl lives at <install>/lib/std/mod.gbl). Fallback for
            // desktop-started editors that don't inherit GOBOL_INSTALL_DIR.
            if let Some(p) = exe_dir
                .parent()
                .map(|d| d.join("lib"))
                .and_then(|d| d.to_str().map(|s| s.to_string()))
            {
                paths.push(p);
            }
        }
    }

    if let Ok(install_dir) = std::env::var("GOBOL_INSTALL_DIR") {
        // lib_paths entries must point at the *parent* of the std/ directory so
        // that `import std;` resolves to <lib_path>/std/mod.gbl. The installed
        // layout is <install>/lib/std/mod.gbl (also <install>/std/mod.gbl for
        // legacy installs). See grape.rs find_std_path() for the same rule.
        let install = PathBuf::from(&install_dir);
        if let Some(p) = install.join("lib").to_str() {
            paths.push(p.to_string());
        }
        if let Some(p) = install.to_str() {
            paths.push(p.to_string());
        }
    }

    // Dedup while preserving order
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    paths
}

// ==================== Analysis ====================

fn analyze_document(
    uri: &str,
    source: &str,
    workspace_roots: &[PathBuf],
) -> DocState {
    let file_path = uri_to_path(uri);
    let error_fmt = ErrorFormatter::new(&file_path, source);

    // 1. Lex + Parse
    let lexer = Lexer::new(source);
    let mut builder = AstBuilder::new(lexer);
    builder.set_error_formatter(error_fmt.clone());
    let prog = builder.build();

    let tokens: Vec<Token> = builder.get_tokens().to_vec();
    let mut errors: Vec<(i32, i32, String)> = builder.structured_errors.clone();

    // 2. Semantic analysis (only if parse succeeded)
    if let Some(ref prog) = prog {
        let lib_paths = build_lib_paths(&file_path, workspace_roots);
        let mut sem = SemanticAnalyzer::new();
        sem.set_main_file(&file_path);
        sem.set_lib_paths(lib_paths);
        sem.set_error_formatter(error_fmt);
        sem.analyze(prog.as_ref());
        errors.extend(sem.structured_errors.clone());
    }

    // 3. Build symbol index
    let symbols = build_symbol_index(&tokens, source);
    let signatures = collect_signatures(&tokens, source);

    DocState {
        source: source.to_string(),
        tokens,
        symbols,
        errors,
        signatures,
    }
}

fn uri_to_path(uri: &str) -> String {
    uri.strip_prefix("file://")
        .unwrap_or(uri)
        .to_string()
}

/// Convert a filesystem path back into a `file://` URI (the inverse of
/// `uri_to_path`). Used to point cross-file `gotoDefinition`/hover results at
/// an imported module's source file.
fn path_to_uri(path: &str) -> url::Url {
    url::Url::parse(&format!("file://{}", path))
        .unwrap_or_else(|_| url::Url::parse("file:///").unwrap())
}

fn token_to_range(token: &Token) -> Range {
    let line = (token.line as u32).saturating_sub(1);
    let start_col = token.col as u32;
    let end_col = start_col + token.value.len() as u32;
    Range::new(
        Position::new(line, start_col),
        Position::new(line, end_col),
    )
}

/// Expand an error position (1-based line, 0-based col) to a range that covers
/// the identifier or the nearest token on that line. Falls back to col..col+1.
fn error_range(
    source: &str,
    tokens: &[Token],
    line: i32,
    col: i32,
) -> Range {
    let lsp_line = if line > 0 { (line as u32).saturating_sub(1) } else { 0 };
    let start_col = col.max(0) as u32;

    // Try exact token match on that (1-based) line
    if line > 0 {
        if let Some(tok) = tokens.iter().find(|t| {
            t.line == line
                && start_col >= t.col as u32
                && start_col < (t.col + t.value.len() as i32) as u32
        }) {
            return token_to_range(tok);
        }
        // Otherwise: the first token whose col is >= start_col on that line
        let candidates: Vec<_> = tokens.iter().filter(|t| t.line == line).collect();
        if let Some(tok) = candidates.iter().find(|t| t.col >= col).or_else(|| candidates.last()) {
            return token_to_range(tok);
        }
    }

    // Fallback: expand to end of word via source line scan
    let src_line = source
        .lines()
        .nth(if line > 0 { (line - 1) as usize } else { 0 })
        .unwrap_or("");
    let bytes = src_line.as_bytes();
    let mut end = start_col + 1;
    while (end as usize) < bytes.len() {
        let c = bytes[end as usize] as char;
        if c.is_ascii_alphanumeric() || c == '_' {
            end += 1;
        } else {
            break;
        }
    }
    Range::new(
        Position::new(lsp_line, start_col),
        Position::new(lsp_line, end.min(bytes.len() as u32)),
    )
}

fn error_to_diagnostic(
    source: &str,
    tokens: &[Token],
    line: i32,
    col: i32,
    msg: &str,
) -> Diagnostic {
    let range = error_range(source, tokens, line, col);
    // Determine severity from msg prefix heuristics
    let severity = if msg.contains("warning") || msg.contains("Warning") {
        DiagnosticSeverity::WARNING
    } else {
        DiagnosticSeverity::ERROR
    };
    Diagnostic::new(
        range,
        Some(severity),
        None,
        Some("gobol".to_string()),
        msg.to_string(),
        None,
        None,
    )
}

// ==================== Snippet Completions ====================

fn keyword_snippets() -> Vec<CompletionItem> {
    let mk = |label: &str, detail: &str, insert: &str, kind| CompletionItem {
        label: label.to_string(),
        kind: Some(kind),
        detail: Some(detail.to_string()),
        insert_text: Some(insert.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    };
    vec![
        mk(
            "func",
            "function declaration (snippet)",
            "func ${1:name}(${2:params}): ${3:ReturnType} {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "struct",
            "struct declaration (snippet)",
            "struct ${1:Name} {\n    ${2:field}: ${3:Type},\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "enum",
            "enum declaration (snippet)",
            "enum ${1:Name} {\n    ${2:Variant},\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "impl",
            "impl block (snippet)",
            "impl ${1:Type} {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "impl Trait for",
            "impl trait for type (snippet)",
            "impl ${1:Trait} for ${2:Type} {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "trait",
            "trait declaration (snippet)",
            "trait ${1:Name} {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "if-else",
            "if-else statement (snippet)",
            "if ${1:condition} {\n    ${2:body}\n} else {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "for",
            "for-in range loop (snippet)",
            "for ${1:i} in ${2:start}..${3:end} {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "while",
            "while loop (snippet)",
            "while ${1:condition} {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "match",
            "match expression (snippet)",
            "match ${1:expr} {\n    ${2:pattern} => $0,\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "extern C",
            "extern \"C\" block (snippet)",
            "extern \"C\" {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
        mk(
            "constructor",
            "constructor (snippet)",
            "constructor ${1:TypeName}(${2:params}) {\n    $0\n}",
            CompletionItemKind::SNIPPET,
        ),
    ]
}

// ==================== LSP Backend ====================

struct GobolLsp {
    client: Client,
    documents: Arc<RwLock<HashMap<String, DocState>>>,
    workspace_roots: Arc<std::sync::RwLock<Vec<PathBuf>>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for GobolLsp {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Capture workspace roots from initialization
        if let Ok(mut roots) = self.workspace_roots.write() {
            roots.clear();
            if let Some(folders) = params.workspace_folders {
                for f in folders {
                    let p = uri_to_path(f.uri.as_str());
                    roots.push(PathBuf::from(p));
                }
            } else if let Some(root_uri) = params.root_uri {
                let p = uri_to_path(root_uri.as_str());
                roots.push(PathBuf::from(p));
            }
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                            include_text: Some(true),
                        })),
                        ..Default::default()
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: Some(vec![",".to_string()]),
                    ..Default::default()
                }),
                rename_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(
                    CodeActionProviderCapability::Options(CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR,
                        ]),
                        ..Default::default()
                    }),
                ),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        ":".to_string(),
                        ",".to_string(),
                        " ".to_string(),
                    ]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                // Automatic, rust-analyzer-style semantic highlighting.
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: SEMANTIC_TYPES.to_vec(),
                                token_modifiers: SEMANTIC_MODIFIERS.to_vec(),
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            ..Default::default()
                        },
                    ),
                ),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "gobol-lsp".to_string(),
                version: Some("0.2.0".to_string()),
            }),
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "gobol-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let text = params.text_document.text;
        self.reanalyze(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        // With FULL sync, take the last change which carries full document text
        if let Some(change) = params.content_changes.into_iter().last() {
            self.reanalyze(&uri, &change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        if let Some(text) = params.text {
            self.reanalyze(&uri, &text).await;
        } else {
            let state = self.documents.read().await;
            if let Some(doc_state) = state.get(&uri) {
                let text = doc_state.source.clone();
                drop(state);
                self.reanalyze(&uri, &text).await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let _ = self
            .client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
        let mut documents = self.documents.write().await;
        documents.remove(&uri);
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri.to_string();

        let state_guard = self.documents.read().await;
        let state = match state_guard.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let token = match state.token_at(pos.position.line, pos.position.character) {
            Some(t) => t,
            None => return Ok(None),
        };

        let mut hover_text = String::new();
        let mut sym_not_found = true;
        for sym in &state.symbols {
            if sym.name == token.value {
                sym_not_found = false;
                let kind_label = sym.kind.label();
                let type_str = sym.type_info.clone().unwrap_or_else(|| "-".to_string());
                let parent_str = sym
                    .parent
                    .as_ref()
                    .map(|p| format!(" (on `{}`)", p))
                    .unwrap_or_default();

                let signature = match sym.kind {
                    SymKind::Function | SymKind::Method | SymKind::ExternFn
                    | SymKind::StaticFunc => {
                        if let Some(ty) = sym.type_info.as_ref() {
                            format!("func {}(): {}", sym.name, ty)
                        } else {
                            format!("func {}()", sym.name)
                        }
                    }
                    SymKind::Struct => format!("struct {}", sym.name),
                    SymKind::Enum => format!("enum {}", sym.name),
                    SymKind::EnumVariant => {
                        format!("{}::{} (variant of {})", sym.parent.as_deref().unwrap_or("?"), sym.name, sym.parent.as_deref().unwrap_or("?"))
                    }
                    SymKind::Trait => format!("trait {}", sym.name),
                    SymKind::TypeAlias => {
                        if let Some(ty) = sym.type_info.as_ref() {
                            format!("type {} = {}", sym.name, ty)
                        } else {
                            format!("type {}", sym.name)
                        }
                    }
                    SymKind::Variable | SymKind::Parameter => {
                        if let Some(ty) = sym.type_info.as_ref() {
                            format!("{}: {}", sym.name, ty)
                        } else {
                            sym.name.clone()
                        }
                    }
                    SymKind::Import => format!("import {}", sym.name),
                };

                hover_text = format!(
                    "**{}** `{}`{}\n\n```gobol\n{}\n```\n\nType: `{}`",
                    kind_label, sym.name, parent_str, signature, type_str
                );

                // Append doc comment if available
                if let Some(doc) = &sym.doc_comment {
                    hover_text.push_str(&format!("\n\n---\n\n{}", doc));
                } else if let Some(module) = sym
                    .parent
                    .as_ref()
                    .and_then(|p| state.module_imported(p))
                {
                    // This is a `from lib import ...` re-export whose source
                    // lives in the imported module: enrich the hover with the
                    // definition's doc comment from that module's file.
                    let uri_c = uri.clone();
                    let name_c = token.value.clone();
                    if let Some((_n, _mod2, _k, _ty, _u, _l, _c, doc)) = self
                        .resolve_imported_symbol(&uri_c, &name_c)
                        .await
                    {
                        if let Some(doc) = doc {
                            if !doc.trim().is_empty() {
                                hover_text.push_str(&format!(
                                    "\n\n---\n\n*from `{}`* —\n\n{}",
                                    module, doc
                                ));
                            }
                        }
                    }
                }

                break;
            }
        }

        if hover_text.is_empty() {
            hover_text = format!("`{}`", token.value);
        }

        // Cross-file fallback: the token may be a function/type defined in a
        // module this document imports (e.g. `lib::greet` or `from lib import
        // greet`).  Surface the imported definition's full signature, its
        // source module and (when available) its doc comment.
        if sym_not_found {
            let uri_q = uri.clone();
            let resolved = self.resolve_imported_symbol(&uri_q, &token.value).await;
            if let Some((_name, module, kind_label, ty, _file_uri, _line, _col, doc_comment)) =
                resolved
            {
                let type_part = ty.clone().unwrap_or_else(|| "-".to_string());
                let detail = match ty {
                    Some(_) => format!("{}::{}", module, token.value),
                    None => format!("{}::{} ({})", module, token.value, kind_label),
                };
                hover_text = format!(
                    "**{}** imported from `{}`\n\n```gobol\n{}\n```\n\nType: `{}`",
                    kind_label, detail, detail, type_part
                );
                if let Some(doc) = doc_comment {
                    if !doc.trim().is_empty() {
                        hover_text.push_str(&format!("\n\n---\n\n{}", doc));
                    }
                }
            }
        }

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text,
            }),
            range: Some(token_to_range(token)),
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri.to_string();

        let state_guard = self.documents.read().await;
        let state = match state_guard.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let token = match state.token_at(pos.position.line, pos.position.character) {
            Some(t) => t,
            None => return Ok(None),
        };
        if token.r#type != TokenType::Identifier {
            return Ok(None);
        }

        // 0. If the cursor is on an imported *module name* (`import lib;`,
        // `from lib import ...`, the module of `lib::greet`), jump to the
        // module source file itself. Detect this before the in-document symbol
        // lookup, since module names are indexed as SymKind::Import.
        if let Some(module) = state.module_imported(&token.value) {
            // But if the cursor is on the member side of `lib::greet`, the
            // `greet` symbol (not `lib`) is under it, so this only fires for
            // the module-name position. Jump to the module file.
            if let Some(loc) = self.goto_imported_module_file(&uri, &module).await {
                return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
            }
        }

        // 1. Definition in this document.
        if let Some(sym) = state.find_definition(&token.value) {
            // If the local symbol is a `from lib import greet` re-export (its
            // parent names an imported module), prefer the definition in that
            // module's source file so Ctrl+Click lands on the real code.
            if let Some(module) = sym
                .parent
                .as_ref()
                .and_then(|p| state.module_imported(p))
            {
                let loc = self
                    .goto_imported_symbol_in_module(&uri, &module, &token.value)
                    .await;
                if let Some(loc) = loc {
                    return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
                }
            }
            let line = (sym.line as u32).saturating_sub(1);
            let col = sym.col as u32;
            let end_col = col + sym.name.len() as u32;
            let location = Location {
                uri: pos.text_document.uri.clone(),
                range: Range::new(
                    Position::new(line, col),
                    Position::new(line, end_col),
                ),
            };
            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }

        // 2. Cross-file: the identifier may be defined in a module that this
        // document imports (`lib::greet` / `from lib import greet`). Jump to
        // the definition inside the imported module's source file.
        let uri_q = uri.clone();
        let name = token.value.clone();
        if let Some(loc) = self.goto_imported_symbol(&uri_q, &name).await {
            return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
        }

        Ok(None)
    }

    async fn references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        let pos = params.text_document_position;
        let uri = pos.text_document.uri.to_string();
        let state_guard = self.documents.read().await;
        let state = match state_guard.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };
        let token = match state.token_at(pos.position.line, pos.position.character) {
            Some(t) => t,
            None => return Ok(None),
        };
        let name = token.value.clone();
        // Gather token positions matching name (reference locations)
        let mut results = Vec::new();
        for t in &state.tokens {
            if t.r#type == TokenType::Identifier && t.value == name {
                let line = (t.line as u32).saturating_sub(1);
                let col = t.col as u32;
                let end_col = col + t.value.len() as u32;
                results.push(Location {
                    uri: pos.text_document.uri.clone(),
                    range: Range::new(
                        Position::new(line, col),
                        Position::new(line, end_col),
                    ),
                });
            }
        }
        if results.is_empty() {
            Ok(None)
        } else {
            Ok(Some(results))
        }
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri.to_string();
        let state_guard = self.documents.read().await;
        let state = match state_guard.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };
        let token = match state.token_at(pos.position.line, pos.position.character) {
            Some(t) => t,
            None => return Ok(None),
        };
        if token.r#type != TokenType::Identifier {
            return Ok(None);
        }
        let highlights = state.highlights_for(&token.value);
        if highlights.is_empty() {
            Ok(None)
        } else {
            Ok(Some(highlights))
        }
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri.to_string();
        let state_guard = self.documents.read().await;
        let state = match state_guard.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };
        Ok(Some(
            SemanticTokens {
                result_id: None,
                data: state.semantic_tokens(),
            }
            .into(),
        ))
    }

    async fn signature_help(
        &self,
        params: SignatureHelpParams,
    ) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri.to_string();
        let pos = params.text_document_position_params.position;

        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };

        // Find the call the cursor is inside and the active parameter index.
        let Some((callee, active_param)) =
            call_at(&state.tokens, pos.line, pos.character)
        else {
            return Ok(None);
        };

        let sig = self.signature_for_name(&uri, &callee, &state).await;
        Ok(sig.map(|s| signature_help_for(&s, active_param)))
    }

    async fn inlay_hint(
        &self,
        params: InlayHintParams,
    ) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri.to_string();
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        Ok(Some(self.inlay_hints_for(&uri, &state, params.range)))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = params.text_document.uri.to_string();
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let token = match state.token_at(params.position.line, params.position.character) {
            Some(t) if t.r#type == TokenType::Identifier => t,
            _ => return Ok(None),
        };
        let range = token_to_range(token);
        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
            range,
            placeholder: token.value.clone(),
        }))
    }

    async fn rename(
        &self,
        params: RenameParams,
    ) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let new_name = &params.new_name;

        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let token = match state.token_at(pos.line, pos.character) {
            Some(t) if t.r#type == TokenType::Identifier => t,
            _ => return Ok(None),
        };
        let old = token.value.clone();

        let mut changes: std::collections::HashMap<url::Url, Vec<TextEdit>> =
            std::collections::HashMap::new();

        // 1. All occurrences in this document.
        let doc_uri = params.text_document_position.text_document.uri.clone();
        let mut edits = Vec::new();
        for t in &state.tokens {
            if t.r#type == TokenType::Identifier && t.value == old {
                edits.push(TextEdit {
                    range: token_to_range(t),
                    new_text: new_name.clone(),
                });
            }
        }
        if !edits.is_empty() {
            changes.insert(doc_uri, edits);
        }

        // 2. If the rename target resolves to an imported module, apply the
        // rename at the definition site there too (cross-file import rename).
        if let Some((_name, _module, _k, _ty, file_uri, line, col, _doc)) = self
            .resolve_imported_symbol(&uri, &old)
            .await
        {
            let range = Range::new(
                Position::new((line as u32).saturating_sub(1), col as u32),
                Position::new(
                    (line as u32).saturating_sub(1),
                    col as u32 + old.len() as u32,
                ),
            );
            let entry = changes.entry(file_uri).or_default();
            entry.push(TextEdit {
                range,
                new_text: new_name.clone(),
            });
        }

        if changes.is_empty() {
            Ok(None)
        } else {
            Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }))
        }
    }

    async fn code_action(
        &self,
        params: CodeActionParams,
    ) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri.to_string();
        let range = params.range;
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        let mut actions: Vec<CodeActionOrCommand> = Vec::new();

        // Detect an unresolved qualified name directly under the cursor, e.g.
        // `io::println(...)` where `io` is not imported.
        if let Some(action) = self
            .code_action_resolve_qualified(&uri, &state, range)
        {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }

        // Detect an unresolved bare function call, e.g. `greet(...)` where
        // `greet` exists in a module but is not imported.
        if let Some(action) = self.code_action_import_bare(&uri, &state).await {
            actions.push(CodeActionOrCommand::CodeAction(action));
        }

        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri.to_string();
        let Some(state) = self.documents.read().await.get(&uri).cloned() else {
            return Ok(None);
        };
        Ok(Some(brace_folding_ranges(&state.tokens)))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;

        // Check for `::` qualifier (e.g., `io::` or `Point::`)
        let qualifier = self.find_qualifier_before_colon_colon(&uri, pos).await;
        if let Some(ref q) = qualifier {
            return self.complete_qualified(&uri, q).await;
        }

        // Check for `import <cursor>` context
        let import_ctx = self.is_import_context(&uri, pos).await;

        let keywords = &[
            "func", "var", "val", "struct", "enum", "impl", "trait", "if", "else",
            "for", "while", "return", "break", "continue", "import", "export",
            "as", "in", "match", "convert", "operator", "constructor", "new",
            "static", "type", "where", "loop",
            "true", "false", "null", "none", "nil", "self", "Self",
            "int", "float", "str", "bool", "void", "char", "unit",
        ];

        let mut items: Vec<CompletionItem> = Vec::new();

        // Add snippet completions FIRST (they carry an insert template and are
        // more useful than the bare keyword), then bare keywords, de-duplicating
        // labels so a snippet like "func" is not offered twice alongside the
        // plain "func" keyword (previously produced duplicate completion entries).
        let snippets = keyword_snippets();
        let snippet_labels: std::collections::HashSet<String> =
            snippets.iter().map(|s| s.label.clone()).collect();
        items.extend(snippets);
        for kw in keywords {
            if snippet_labels.contains(*kw) {
                // snippet supersedes the bare keyword label
                continue;
            }
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(format!("keyword `{}`", kw)),
                ..Default::default()
            });
        }

        // seen_labels tracks every completion label emitted so far; the
        // keyword/snippet entries, stdlib modules (import context) and the
        // document/cross-file symbols below all dedup against it.
        let mut seen_labels: std::collections::HashSet<String> =
            items.iter().map(|it| it.label.as_str().to_string()).collect();

        // Add stdlib modules when user is typing `import ...`
        if import_ctx {
            let std_modules = self.list_std_modules(&uri);
            for (name, kind) in &std_modules {
                if !seen_labels.insert(name.clone()) {
                    // module name collides with a keyword/snippet label (e.g.
                    // int/float/str/trait) — do not emit a duplicate entry.
                    continue;
                }
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some(format!("stdlib {}", kind)),
                    sort_text: Some(format!("0_{}", name)),
                    ..Default::default()
                });
            }
        }

        // Add symbols from the document. Globals (functions/types/traits/
        // imports/enum variants) are offered regardless of position; local
        // variables & parameters are offered only when visible in the current
        // scope (declared earlier at the same-or-outer brace level — a
        // rust-analyzer style scope-aware completion).
        {
            let state_guard = self.documents.read().await;
            if let Some(state) = state_guard.get(&uri) {
                let locals = state.visible_locals(pos.line, pos.character);
                // Emit visible locals first (they are the most contextually
                // relevant), then the globals.
                let mut ordered: Vec<&SymbolEntry> = Vec::new();
                let mut global: Vec<&SymbolEntry> = Vec::new();
                for sym in &state.symbols {
                    if matches!(sym.kind, SymKind::Variable | SymKind::Parameter) {
                        // locals handled via visible_locals
                        continue;
                    }
                    global.push(sym);
                }
                ordered.extend(locals.iter());
                ordered.extend(global);

                for sym in ordered {
                    if !seen_labels.insert(sym.name.clone()) {
                        continue;
                    }
                    let mut detail = sym.type_info.clone().unwrap_or_else(|| sym.kind.label().to_string());
                    if let Some(ref parent) = sym.parent {
                        detail = format!("{}::{} → {}", parent, sym.name, detail);
                    } else {
                        detail = format!("{}: {}", sym.kind.label(), detail);
                    }
                    items.push(CompletionItem {
                        label: sym.name.clone(),
                        kind: Some(sym.kind.completion_kind()),
                        detail: Some(detail),
                        documentation: sym.doc_comment.as_ref().map(|doc| {
                            tower_lsp::lsp_types::Documentation::MarkupContent(
                                tower_lsp::lsp_types::MarkupContent {
                                    kind: tower_lsp::lsp_types::MarkupKind::Markdown,
                                    value: doc.clone(),
                                },
                            )
                        }),
                        ..Default::default()
                    });
                }
            }
        }

        // Module members are NOT offered as bare names: since the compiler
        // enforces module scoping (`import mylib` does not let you call bare
        // `add()` — only `mylib::add()`), the editor must not suggest them
        // either. Members appear as completions only after `module::` via
        // `complete_qualified`.

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri.to_string();
        let state_guard = self.documents.read().await;
        let state = match state_guard.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        // Build hierarchical: struct/trait/enum → methods / variants
        let mut top_level: Vec<DocumentSymbol> = Vec::new();
        // Track container -> index in top_level
        let mut container_idx: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for sym in &state.symbols {
            let line = (sym.line as u32).saturating_sub(1);
            let col = sym.col as u32;
            let end_col = col + sym.len.max(1) as u32;
            let range = Range::new(
                Position::new(line, col),
                Position::new(line, end_col),
            );
            match sym.kind {
                SymKind::Struct | SymKind::Enum | SymKind::Trait => {
                    container_idx.insert(sym.name.clone(), top_level.len());
                    top_level.push(DocumentSymbol {
                        name: sym.name.clone(),
                        detail: Some(sym.kind.label().to_string()),
                        kind: sym.kind.symbol_kind(),
                        range,
                        selection_range: range,
                        children: Some(Vec::new()),
                        tags: None,
                        #[allow(deprecated)]
                        deprecated: None,
                    });
                }
                SymKind::Function
                | SymKind::StaticFunc
                | SymKind::TypeAlias
                | SymKind::ExternFn => {
                    if sym.parent.is_none() {
                        top_level.push(DocumentSymbol {
                            name: sym.name.clone(),
                            detail: sym.type_info.clone(),
                            kind: sym.kind.symbol_kind(),
                            range,
                            selection_range: range,
                            children: None,
                            tags: None,
                            #[allow(deprecated)]
                            deprecated: None,
                        });
                    }
                }
                _ => {}
            }
        }

        // Second pass: add methods/variants/statics as children of their containers
        for sym in &state.symbols {
            let parent = match sym.parent {
                Some(ref p) => p.clone(),
                None => continue,
            };
            let line = (sym.line as u32).saturating_sub(1);
            let col = sym.col as u32;
            let end_col = col + sym.len.max(1) as u32;
            let range = Range::new(
                Position::new(line, col),
                Position::new(line, end_col),
            );
            match sym.kind {
                SymKind::Method | SymKind::StaticFunc | SymKind::EnumVariant => {
                    if let Some(&idx) = container_idx.get(&parent) {
                        if let Some(children) = top_level[idx].children.as_mut() {
                            children.push(DocumentSymbol {
                                name: sym.name.clone(),
                                detail: sym.type_info.clone().or_else(|| {
                                    Some(sym.kind.label().to_string())
                                }),
                                kind: sym.kind.symbol_kind(),
                                range,
                                selection_range: range,
                                children: None,
                                tags: None,
                                #[allow(deprecated)]
                                deprecated: None,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        if top_level.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DocumentSymbolResponse::Nested(top_level)))
        }
    }
}

// ==================== GobolLsp ====================

/// Custom `gobol/highlights` request: given a document position, return the
/// ranges that highlight the same identifier across *all* open documents, so
/// clients can visualise cross-file occurrences of a symbol.
#[derive(serde::Serialize, serde::Deserialize)]
struct CrossFileHighlightRequest {
    #[serde(rename = "textDocument")]
    text_document: TextDocumentIdentifier,
    position: Position,
}

#[derive(serde::Serialize)]
struct CrossFileHighlightForFile {
    uri: String,
    highlights: Vec<DocumentHighlight>,
}

impl GobolLsp {
    /// Resolve a callable's signature for `name`, preferring a local
    /// declaration and falling back to an imported module definition.
    async fn signature_for_name(
        &self,
        uri: &str,
        name: &str,
        state: &DocState,
    ) -> Option<FuncSignature> {
        if let Some(sig) = state.signatures.get(name) {
            return Some(sig.clone());
        }
        // Cross-file: search the modules the document imports.
        let modules = self.imported_module_names(uri).await;
        for module in &modules {
            if let Some(sig) = self.imported_signature(uri, module, name) {
                return Some(sig);
            }
        }
        None
    }

    /// Parse the signature of `name` from an imported module's source file.
    fn imported_signature(&self, uri: &str, module: &str, name: &str) -> Option<FuncSignature> {
        let file_path = uri_to_path(uri);
        let roots = self
            .workspace_roots
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        let lib_paths = build_lib_paths(&file_path, &roots);
        let file_name = module.split("::").last().unwrap_or(module);

        for lib_path in &lib_paths {
            for mod_path in [
                PathBuf::from(lib_path).join(format!("{}.gbl", file_name)),
                PathBuf::from(lib_path).join(file_name).join("mod.gbl"),
            ] {
                if mod_path.exists() {
                    if let Ok(source) = std::fs::read_to_string(&mod_path) {
                        let mut lexer = Lexer::new(&source);
                        let mut toks = Vec::new();
                        loop {
                            let t = lexer.get_next_token();
                            if t.r#type == TokenType::EndOfFile {
                                break;
                            }
                            toks.push(t);
                        }
                        let sigs = collect_signatures(&toks, &source);
                        if let Some(sig) = sigs.get(name) {
                            return Some(sig.clone());
                        }
                    }
                    return None;
                }
            }
        }
        None
    }

    /// Compute inlay hints for a range: type hints after `var x =`, parameter
    /// name hints in call arguments, and return-type hints for `func f() { e }`
    /// single-expression bodies.
    fn inlay_hints_for(
        &self,
        uri: &str,
        state: &DocState,
        range: Range,
    ) -> Vec<InlayHint> {
        let tokens = &state.tokens;
        let mut hints = Vec::new();
        let start_line = range.start.line;
        let end_line = range.end.line;

        let mut i = 0;
        while i < tokens.len() {
            let t = &tokens[i];
            if (t.line as u32).saturating_sub(1) < start_line
                || (t.line as u32).saturating_sub(1) > end_line
            {
                i += 1;
                continue;
            }

            // `var name = expr` → type hint after `name`.
            if t.r#type == TokenType::Keyword && (t.value == "var" || t.value == "val") {
                if let Some(name_tok) = tokens.get(i + 1) {
                    if name_tok.r#type == TokenType::Identifier {
                        if let Some(eq) = tokens.get(i + 2) {
                            if eq.value == "=" {
                                // skip explicit type `var x: T =`
                                let has_explicit = self.var_has_explicit_type(tokens, i);
                                if !has_explicit {
                                    if let Some(ty) = infer_expr_type(tokens, i + 3, state) {
                                        hints.push(mk_inlay_hint(
                                            Position::new(
                                                (name_tok.line as u32).saturating_sub(1),
                                                (name_tok.col as u32) + name_tok.value.len() as u32,
                                            ),
                                            InlayHintLabel::String(format!(": {}", ty)),
                                            InlayHintKind::TYPE,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Call argument: `callee(arg, ...)` → parameter name hints.
            // Skip when this identifier is a *declaration* name (function /
            // method / constructor), whose `(` is a parameter list, not a call
            // — otherwise `func add(a: int)` would wrongly get `a:`/`b:`
            // parameter inlay hints.
            if t.r#type == TokenType::Identifier {
                let after = tokens.get(i + 1).map(|x| x.value.clone());
                if after.as_deref() == Some("(") && !is_declaration_call_paren(tokens, i) {
                    if let Some(sig) = self.signature_at_index(uri, state, i) {
                        let arg_edits = argument_position_hints(tokens, i, &sig);
                        hints.extend(arg_edits);
                    }
                }
            }

            i += 1;
        }
        hints
    }

    fn var_has_explicit_type(&self, tokens: &[Token], var_idx: usize) -> bool {
        if let Some(colon) = tokens.get(var_idx + 2) {
            return colon.value == ":";
        }
        false
    }

    fn signature_at_index(&self, uri: &str, state: &DocState, ident_idx: usize) -> Option<FuncSignature> {
        let t = &state.tokens[ident_idx];
        self.signature_for_name_blocking(uri, &t.value, state)
    }

    fn code_action_resolve_qualified(
        &self,
        uri: &str,
        state: &DocState,
        range: Range,
    ) -> Option<CodeAction> {
        let tokens = &state.tokens;
        let line = range.start.line;
        let col = range.start.character as i32;
        let target_line = (line as i32) + 1;

        // Look for the identifier at/near the cursor that is immediately
        // followed by `::` (a module qualifier) and not already imported.
        for i in 0..tokens.len() {
            let t = &tokens[i];
            if t.line != target_line {
                continue;
            }
            if t.r#type != TokenType::Identifier {
                continue;
            }
            let is_on_range = col >= t.col && col <= t.col + t.value.len() as i32;
            if !is_on_range {
                continue;
            }
            let qualifier = t.value.clone();
            if state.module_imported(&qualifier).is_some() {
                continue;
            }
            // Must be immediately followed by `::`.
            let next = tokens.get(i + 1);
            if !(next.map(|n| n.value.as_str() == "::").unwrap_or(false)) {
                continue;
            }
            // Provide "import <qualifier>;" action.
            let uri_u = url::Url::parse(&format!("file://{}", uri_to_path(uri)))
                .unwrap_or_else(|_| path_to_uri(uri));
            return Some(build_import_action(&uri_u, &qualifier, state));
        }
        None
    }

    async fn code_action_import_bare(
        &self,
        uri: &str,
        state: &DocState,
    ) -> Option<CodeAction> {
        // Find a bare call `name(` whose `name` is not defined locally or
        // imported, but is exported by a resolvable module.
        let tokens = &state.tokens;
        for i in 0..tokens.len() {
            let t = &tokens[i];
            if t.r#type != TokenType::Identifier {
                continue;
            }
            let is_call = tokens
                .get(i + 1)
                .map(|n| n.value.as_str() == "(")
                .unwrap_or(false);
            if !is_call {
                continue;
            }
            let name = t.value.clone();
            if state.find_definition(&name).is_some() || state.module_imported(&name).is_some() {
                continue;
            }
            // Is it exported by some importable module?
            let modules = self.imported_module_names(uri).await;
            for module in &modules {
                let defs = self.index_imported_module_detailed(uri, module);
                if defs.iter().any(|d| d.0 == name) {
                    let uri_u = url::Url::parse(&format!("file://{}", uri_to_path(uri)))
                        .unwrap_or_else(|_| path_to_uri(uri));
                    return Some(build_import_from_action(&uri_u, &module, &name, state));
                }
            }
        }
        None
    }

    // Blocking wrapper used inside non-async scanning when locking the doc is
    // already held (we clone the state so no lock is needed here).
    fn signature_for_name_blocking(
        &self,
        uri: &str,
        name: &str,
        state: &DocState,
    ) -> Option<FuncSignature> {
        if let Some(sig) = state.signatures.get(name) {
            return Some(sig.clone());
        }
        // Imported modules: use imported_module_names but we need sync — clone
        // isn't async here; reuse imported_module_names_async via block_on not
        // available. Instead scan symbols for Import names directly.
        for sym in &state.symbols {
            if sym.kind == SymKind::Import {
                if let Some(sig) = self.imported_signature(uri, &sym.name, name) {
                    return Some(sig);
                }
            }
        }
        None
    }

    /// Resolve a module name (e.g. `lib` in `import lib;` / `from lib import`)
    /// to the source *file* of that module so Ctrl+Click jumps into it.
    /// Returns `None` if no module file can be located.
    async fn goto_imported_module_file(
        &self,
        uri: &str,
        module_name: &str,
    ) -> Option<Location> {
        let file_path = uri_to_path(uri);
        let roots = self
            .workspace_roots
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        let lib_paths = build_lib_paths(&file_path, &roots);

        let module_parts: Vec<&str> = module_name.split("::").collect();
        let file_name = module_parts.last().unwrap_or(&module_name);

        for lib_path in &lib_paths {
            // `<lib>/std/mod.gbl` layout and `<lib>/io.gbl` layout.
            let candidates = [
                PathBuf::from(lib_path).join(file_name).join("mod.gbl"),
                PathBuf::from(lib_path).join(format!("{}.gbl", file_name)),
            ];
            for mod_path in candidates {
                if mod_path.exists() {
                    let mod_path_str = mod_path.to_string_lossy().into_owned();
                    return Some(Location {
                        uri: path_to_uri(&mod_path_str),
                        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                    });
                }
            }
        }
        None
    }

    /// Cross-file `gotoDefinition`: locate `name` among the modules imported by
    /// `uri` and build a `Location` pointing at the symbol's definition inside
    /// that module's source file.
    async fn goto_imported_symbol(&self, uri: &str, name: &str) -> Option<Location> {
        for module in self.imported_module_names(uri).await {
            if let Some(loc) = self.goto_imported_symbol_in_module(uri, &module, name).await {
                return Some(loc);
            }
        }
        None
    }

    /// Cross-file `gotoDefinition` restricted to a single imported `module`:
    /// look up `name` in that module and return a `Location` at its definition.
    async fn goto_imported_symbol_in_module(
        &self,
        uri: &str,
        module: &str,
        name: &str,
    ) -> Option<Location> {
        for sym in self.index_imported_module_detailed(uri, module) {
            if sym.0 == name {
                // Tuple layout: (name, kind, type, file_uri, line, col, doc).
                let col = sym.5.max(0) as u32;
                let end_col = col + name.len() as u32;
                let line0 = (sym.4 as u32).saturating_sub(1);
                return Some(Location {
                    uri: sym.3.clone(),
                    range: Range::new(
                        Position::new(line0, col),
                        Position::new(line0, end_col),
                    ),
                });
            }
        }
        None
    }

    async fn find_qualifier_before_colon_colon(
        &self,
        uri: &str,
        pos: Position,
    ) -> Option<String> {
        let state_guard = self.documents.read().await;
        let state = state_guard.get(uri)?;
        let target_line = (pos.line as i32) + 1;
        let target_col = pos.character as i32;
        // Find the `:` token at or just before cursor that pairs with the next `:`
        let tokens = &state.tokens;
        for i in 0..tokens.len() {
            let t = &tokens[i];
            if t.line == target_line && t.col <= target_col {
                // Consider cursor either in the middle of first `:`, just after `:`, or before second `:`
                // Case: tokens[i] = `:`, tokens[i+1] = `:`, with target_col in [tokens[i].col, tokens[i+1].col + 1)
                if t.value == ":" {
                    if i + 1 < tokens.len()
                        && tokens[i + 1].value == ":"
                        && target_col <= tokens[i + 1].col + 1
                    {
                        if i >= 1 && tokens[i - 1].r#type == TokenType::Identifier {
                            return Some(tokens[i - 1].value.clone());
                        }
                    }
                }
            }
        }
        // Fallback: scan the source line for `ident::` pattern ending <= target_col
        let line_str = state.source.lines().nth(pos.line as usize)?;
        let end = target_col.min(line_str.len() as i32) as usize;
        let slice = &line_str[..end];
        // Find last `::` in slice, then word before it
        if let Some(dbl_col) = slice.rfind("::") {
            let before = &slice[..dbl_col];
            let ident_start = before
                .char_indices()
                .rev()
                .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_')
                .map(|(i, _)| i + 1)
                .unwrap_or(0);
            let ident = &before[ident_start..];
            if !ident.is_empty() {
                return Some(ident.to_string());
            }
        }
        None
    }

    /// Returns true if the cursor appears inside an `import ____;` or
    /// `from ____ import ...;` statement (module name position).
    async fn is_import_context(&self, uri: &str, pos: Position) -> bool {
        let state_guard = self.documents.read().await;
        let state = match state_guard.get(uri) {
            Some(s) => s,
            None => return false,
        };
        let line_1 = (pos.line as i32) + 1;
        let col = pos.character as i32;
        for i in 0..state.tokens.len() {
            let t = &state.tokens[i];
            // Check both `import` and `from` keywords (both start a module import)
            if t.r#type == TokenType::Keyword
                && (t.value == "import" || t.value == "from")
                && t.line == line_1
            {
                // Cursor must be after the keyword and before `;` (or EOL)
                let mut end_idx = state.tokens.len();
                for j in i + 1..state.tokens.len() {
                    if state.tokens[j].line > line_1 {
                        end_idx = j;
                        break;
                    }
                    if state.tokens[j].value == ";" {
                        end_idx = j;
                        break;
                    }
                }
                let kw_end_col = t.col + t.value.len() as i32;
                if col >= kw_end_col {
                    if end_idx < state.tokens.len() && state.tokens[end_idx].line == line_1 {
                        if col <= state.tokens[end_idx].col {
                            return true;
                        }
                    } else {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Provide completions for a qualified name (e.g., after `io::` or `Point::`).
    async fn complete_qualified(
        &self,
        uri: &str,
        qualifier: &str,
    ) -> Result<Option<CompletionResponse>> {
        let mut items: Vec<CompletionItem> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

        // `std::...` → complete the standard sub-modules (io, math, fs, …).
        // Since export() was removed from std/mod.gbl, sub-modules are addressed
        // via `std::<name>`, and typing `std::` should surface them.
        if qualifier == "std" {
            let std_modules = self.list_std_modules(uri);
            for (name, _) in &std_modules {
                if name == "mod" || name == "builtins" {
                    continue;
                }
                if !seen.insert(name.clone()) {
                    continue;
                }
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some("std module".to_string()),
                    sort_text: Some(format!("0_{}", name)),
                    ..Default::default()
                });
            }
        }

        let module_symbols = self.index_imported_module(uri, qualifier);
        for (name, kind_label, ty) in &module_symbols {
            if !seen.insert(name.clone()) {
                continue;
            }
            let detail = if let Some(t) = ty {
                format!("{}::{} → {}", qualifier, name, t)
            } else {
                format!("{}::{} ({})", qualifier, name, kind_label)
            };
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(detail),
                ..Default::default()
            });
        }

        // Local methods / enum variants / static funcs of the qualifier
        {
            let state_guard = self.documents.read().await;
            if let Some(state) = state_guard.get(uri) {
                for sym in &state.symbols {
                    if let Some(ref parent) = sym.parent {
                        if parent == qualifier && seen.insert(sym.name.clone()) {
                            let detail = if let Some(ref ty) = sym.type_info {
                                format!("{}::{} → {}", parent, sym.name, ty)
                            } else {
                                format!("{}::{} ({})", parent, sym.name, sym.kind.label())
                            };
                            items.push(CompletionItem {
                                label: sym.name.clone(),
                                kind: Some(sym.kind.completion_kind()),
                                detail: Some(detail),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }

        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    /// List the standard sub-modules available under `std::`. This scans the
    /// std library directories for `<name>.gbl` files. It reuses the module
    /// discovery of `list_std_modules`, so each call recomputes lib paths from
    /// the current document.

    fn list_std_modules(&self, uri: &str) -> Vec<(String, String)> {
        let mut result = Vec::new();
        let file_path = uri_to_path(uri);
        let roots = self
            .workspace_roots
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        let lib_paths = build_lib_paths(&file_path, &roots);

        for lp in lib_paths {
            let dir = Path::new(&lp);
            if !dir.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("gbl") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            let label = stem.to_string();
                            if !result.iter().any(|(n, _)| n == &label) {
                                result.push((label, format!("module (from {})", lp)));
                            }
                        }
                    }
                }
            }
        }
        result
    }

    /// Collect the exported symbols of an imported module as tuples of
    /// `(name, kind_label, type_info)`. Used by completion.
    fn index_imported_module(
        &self,
        uri: &str,
        module_name: &str,
    ) -> Vec<(String, String, Option<String>)> {
        self.index_imported_module_detailed(uri, module_name)
            .into_iter()
            .map(|(name, kind_label, type_info, _uri, _line, _col, _doc)| {
                (name, kind_label, type_info)
            })
            .collect()
    }

    /// The module names imported by the document at `uri` (e.g. `io`,
    /// `math`, or the module of a `from lib import ...` statement).
    async fn imported_module_names(&self, uri: &str) -> Vec<String> {
        let imports: Vec<String> = {
            let state_guard = self.documents.read().await;
            state_guard
                .get(uri)
                .map(|state| {
                    state
                        .symbols
                        .iter()
                        .filter(|s| s.kind == SymKind::Import)
                        .map(|s| s.name.clone())
                        .collect()
                })
                .unwrap_or_default()
        };
        // Dedup, preserving order.
        let mut seen = std::collections::HashSet::new();
        imports.into_iter().filter(|n| seen.insert(n.clone())).collect()
    }

    /// Index the exported symbols of an imported module, returning rich detail
    /// per symbol: `(name, kind_label, type_info, file_uri, line, col,
    /// doc_comment)` where `file_uri` + `line`/`col` point at the symbol's
    /// definition in the module's source file (for cross-file
    /// gotoDefinition/hover), and `doc_comment` carries the doc comment above
    /// the definition. Supports both `<lib>/<mod>.gbl` and
    /// `<lib>/<mod>/mod.gbl` layouts.
    fn index_imported_module_detailed(
        &self,
        uri: &str,
        module_name: &str,
    ) -> Vec<(
        String,
        String,
        Option<String>,
        url::Url,
        i32,
        i32,
        Option<String>,
    )> {
        let mut result = Vec::new();
        let file_path = uri_to_path(uri);
        let roots = self
            .workspace_roots
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        let lib_paths = build_lib_paths(&file_path, &roots);

        let module_parts: Vec<&str> = module_name.split("::").collect();
        let file_name = module_parts.last().unwrap_or(&module_name);

        for lib_path in &lib_paths {
            let mut mod_paths = Vec::new();
            mod_paths.push(PathBuf::from(lib_path).join(format!("{}.gbl", file_name)));
            // `<lib>/<mod>/mod.gbl` layout (e.g. std/mod.gbl for `import std;`).
            mod_paths.push(PathBuf::from(lib_path).join(file_name).join("mod.gbl"));
            let mut found = false;
            for mod_path in mod_paths {
                if mod_path.exists() {
                    if let Ok(source) = std::fs::read_to_string(&mod_path) {
                        let mut lexer = Lexer::new(&source);
                        let mut mod_tokens: Vec<Token> = Vec::new();
                        loop {
                            let t = lexer.get_next_token();
                            if t.r#type == TokenType::EndOfFile {
                                break;
                            }
                            mod_tokens.push(t);
                        }
                        let symbols = build_symbol_index(&mod_tokens, &source);
                        let mod_uri = path_to_uri(&mod_path.to_string_lossy());
                        for sym in &symbols {
                            if matches!(
                                sym.kind,
                                SymKind::Function
                                    | SymKind::Method
                                    | SymKind::ExternFn
                                    | SymKind::StaticFunc
                                    | SymKind::EnumVariant
                                    | SymKind::Struct
                                    | SymKind::Enum
                                    | SymKind::Trait
                                    | SymKind::TypeAlias
                            ) {
                                result.push((
                                    sym.name.clone(),
                                    sym.kind.label().to_string(),
                                    sym.type_info.clone(),
                                    mod_uri.clone(),
                                    sym.line,
                                    sym.col,
                                    sym.doc_comment.clone(),
                                ));
                            }
                        }
                    }
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
        result
    }

    /// Look up `name` among the modules imported by the document at `uri`.
    /// Returns `(name, module, kind_label, type_info, file_uri, line, col,
    /// doc_comment)` if the symbol is found in an imported module (cross-file
    /// definition), else `None`.
    async fn resolve_imported_symbol(
        &self,
        uri: &str,
        name: &str,
    ) -> Option<(
        String,
        String,
        String,
        Option<String>,
        url::Url,
        i32,
        i32,
        Option<String>,
    )> {
        for module in self.imported_module_names(uri).await {
            for def in self.index_imported_module_detailed(uri, &module) {
                if def.0 == name {
                    return Some((
                        def.0.clone(),
                        module,
                        def.1,
                        def.2,
                        def.3,
                        def.4,
                        def.5,
                        def.6,
                    ));
                }
            }
        }
        None
    }

    /// Re-analyse a document: run the (relatively fast) semantic analysis and
    /// replace the stored document state + republish diagnostics.
    async fn reanalyze(&self, uri: &str, text: &str) {
        let uri_owned = uri.to_string();
        let text_owned = text.to_string();
        let roots = self
            .workspace_roots
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();
        let uri_for_clone = uri_owned.clone();
        let state = tokio::task::spawn_blocking(move || {
            analyze_document(&uri_for_clone, &text_owned, &roots)
        })
        .await
        .unwrap_or_else(|_| DocState {
            source: String::new(),
            tokens: Vec::new(),
            symbols: Vec::new(),
            errors: vec![(0, 0, "Internal error: analysis panicked".to_string())],
            signatures: std::collections::HashMap::new(),
        });

        let diagnostics: Vec<Diagnostic> = state
            .errors
            .iter()
            .map(|(line, col, msg)| {
                error_to_diagnostic(&state.source, &state.tokens, *line, *col, msg)
            })
            .collect();

        // Atomically replace the document state.
        let mut documents = self.documents.write().await;
        documents.insert(uri_owned.clone(), state);
        drop(documents);

        // Publish diagnostics without blocking the notification handler.
        if let Ok(url) = url::Url::parse(uri) {
            self.client
                .publish_diagnostics(url, diagnostics, None)
                .await;
        }
    }

    /// Handle the custom `gobol/highlights` request (registered in `main`).
    /// Resolves the identifier under the cursor in the source document, then
    /// returns per-file highlight ranges for that same identifier across every
    /// open document (i.e. cross-file auto-highlight).
    async fn highlights(
        &self,
        params: CrossFileHighlightRequest,
    ) -> Result<Vec<CrossFileHighlightForFile>> {
        let uri = params.text_document.uri.to_string();

        // Resolve the identifier under the cursor in the source document.
        let name = {
            let documents = self.documents.read().await;
            let source_state = match documents.get(&uri) {
                Some(s) => s,
                None => return Ok(Vec::new()),
            };
            let token = source_state.token_at(params.position.line, params.position.character);
            match token {
                Some(t) if t.r#type == TokenType::Identifier => t.value.clone(),
                _ => return Ok(Vec::new()),
            }
        };

        // Compute occurrences of that identifier in every open document.
        let mut results = Vec::new();
        let documents = self.documents.read().await;
        for (doc_uri, state) in documents.iter() {
            let highlights = state.highlights_for(&name);
            results.push(CrossFileHighlightForFile {
                uri: doc_uri.clone(),
                highlights,
            });
        }
        Ok(results)
    }
}

// ==================== Main ====================

fn main() {
    let stdin = stdin();
    let stdout = stdout();

    let (service, socket) = LspService::build(|client| GobolLsp {
        client,
        documents: Arc::new(RwLock::new(HashMap::new())),
        workspace_roots: Arc::new(std::sync::RwLock::new(Vec::new())),
    })
    .custom_method("gobol/highlights", GobolLsp::highlights)
    .finish();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(Server::new(stdin, stdout, socket).serve(service));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_doc_comment_line_comments() {
        let source = "// First line\n// Second line\nfunc foo() {}\n";
        // func is on line 3 (1-based)
        let doc = extract_doc_comment(source, 3);
        assert_eq!(doc.as_deref(), Some("First line\nSecond line"));
    }

    #[test]
    fn test_extract_doc_comment_triple_slash() {
        let source = "/// Brief description\n/// More details\nfunc bar() {}\n";
        let doc = extract_doc_comment(source, 3);
        assert_eq!(doc.as_deref(), Some("Brief description\nMore details"));
    }

    #[test]
    fn test_extract_doc_comment_with_attribute() {
        let source = "// Doc comment\n#[intrinsic(\"foo\")]\nfunc baz() {}\n";
        // func is on line 3, attribute on line 2, comment on line 1
        let doc = extract_doc_comment(source, 3);
        assert_eq!(doc.as_deref(), Some("Doc comment"));
    }

    #[test]
    fn test_extract_doc_comment_block_comment() {
        let source = "/* Block comment */\nfunc qux() {}\n";
        let doc = extract_doc_comment(source, 2);
        assert_eq!(doc.as_deref(), Some("Block comment"));
    }

    #[test]
    fn test_extract_doc_comment_multiline_block() {
        let source = "/* First line\n * Second line\n * Third line */\nfunc quux() {}\n";
        let doc = extract_doc_comment(source, 4);
        assert_eq!(doc.as_deref(), Some("First line\nSecond line\nThird line"));
    }

    #[test]
    fn test_extract_doc_comment_none() {
        let source = "func no_comment() {}\n";
        let doc = extract_doc_comment(source, 1);
        assert_eq!(doc, None);
    }

    #[test]
    fn test_extract_doc_comment_blank_line_gap() {
        let source = "// Comment above gap\n\nfunc with_gap() {}\n";
        // func is on line 3, blank on line 2, comment on line 1
        let doc = extract_doc_comment(source, 3);
        assert_eq!(doc.as_deref(), Some("Comment above gap"));
    }

    #[test]
    fn test_extract_doc_comment_stops_at_code() {
        let source = "struct Other {}\n// Real doc\nfunc target() {}\n";
        // func is on line 3, comment on line 2, struct on line 1
        let doc = extract_doc_comment(source, 3);
        assert_eq!(doc.as_deref(), Some("Real doc"));
    }

    #[test]
    fn test_extract_doc_comment_preserves_markdown() {
        // Markdown syntax in comments should be preserved verbatim so that
        // the LSP hover/completion renderer (MarkupKind::Markdown) can render it.
        // Inner blank lines (written as `//`) act as Markdown paragraph
        // separators and must be retained.
        let source = "\
// 计算斐波那契数列
//
// **参数：**
// - `n`: 第 n 项
//
// 示例：
// ```gobol
// fib(10) == 55
// ```
func fib(n: int): int { 0 }
";
        // func is on line 10 (1-based)
        let doc = extract_doc_comment(source, 10);
        let expected = "\
计算斐波那契数列

**参数：**
- `n`: 第 n 项

示例：
```gobol
fib(10) == 55
```";
        assert_eq!(doc.as_deref(), Some(expected));
    }

    #[test]
    fn test_extract_doc_comment_inner_real_blank_line() {
        // A real (non-`//`) blank line *inside* the comment block should be
        // preserved as a Markdown paragraph separator.
        let source = "// para one\n\n// para two\nfunc f() {}\n";
        // func is on line 4
        let doc = extract_doc_comment(source, 4);
        assert_eq!(doc.as_deref(), Some("para one\n\npara two"));
    }

    #[test]
    fn test_extract_doc_comment_block_inner_blank() {
        // Block comment with a `*`-only line in the middle: treat as paragraph
        // separator (Markdown blank line).
        let source = "/* first para\n *\n * second para */\nfunc g() {}\n";
        // func is on line 4
        let doc = extract_doc_comment(source, 4);
        assert_eq!(doc.as_deref(), Some("first para\n\nsecond para"));
    }

    #[test]
    fn test_extract_doc_comment_markdown_headings() {
        // `#`, `##`, `###` inside `///` doc comments are Markdown headings —
        // they must be preserved verbatim (not stripped) for rendering.
        let source = "/// # Title\n/// ## Section\n/// ### Subsection\n/// body\nfunc h() {}\n";
        // func is on line 5
        let doc = extract_doc_comment(source, 5);
        assert_eq!(doc.as_deref(), Some("# Title\n## Section\n### Subsection\nbody"));
    }

    #[test]
    fn test_extract_doc_comment_attribute_skipped() {
        // `#[...]` is an attribute, NOT a doc comment — must be skipped.
        let source = "// real doc\n#[intrinsic(\"foo\")]\nfunc k() {}\n";
        // func is on line 3, attribute on line 2, comment on line 1
        let doc = extract_doc_comment(source, 3);
        assert_eq!(doc.as_deref(), Some("real doc"));
    }

    // ---- Semantic token helpers ----

    fn lex(source: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();
        loop {
            let t = lexer.get_next_token();
            if t.r#type == TokenType::EndOfFile {
                break;
            }
            tokens.push(t);
        }
        tokens
    }

    /// Decode delta-encoded LSP semantic tokens back into absolute
    /// (line, col, type-name) triples for assertions.
    fn decode(data: &[SemanticToken]) -> Vec<(u32, u32, String)> {
        let mut out = Vec::new();
        let mut line = 0u32;
        let mut col = 0u32;
        for t in data {
            line += t.delta_line;
            if t.delta_line == 0 {
                col += t.delta_start;
            } else {
                col = t.delta_start;
            }
            let name = SEMANTIC_TYPES[t.token_type as usize]
                .as_str()
                .to_string();
            out.push((line, col, name));
        }
        out
    }

    #[test]
    fn test_semantic_tokens_import_module_is_namespace() {
        let source = "import std::io;\nimport math as m;\nfrom lib import helper, other;\n";
        let tokens = lex(source);
        let symbols = build_symbol_index(&tokens, source);
        let data = build_semantic_tokens(&tokens, &symbols);
        let decoded = decode(&data);

        // `import std::io;` → both `std` and `io` are namespaces.
        assert!(
            decoded.contains(&(0, 7, "namespace".to_string())),
            "std should be a namespace, got {:?}",
            decoded
        );
        assert!(
            decoded.contains(&(0, 12, "namespace".to_string())),
            "io should be a namespace, got {:?}",
            decoded
        );
        // `import math as m;` → module `math` is a namespace.
        assert!(
            decoded.contains(&(1, 7, "namespace".to_string())),
            "math should be a namespace, got {:?}",
            decoded
        );
        // `from lib import helper, other;` → `lib` is a namespace.
        assert!(
            decoded.contains(&(2, 5, "namespace".to_string())),
            "lib should be a namespace, got {:?}",
            decoded
        );
        assert!(
            decoded.contains(&(2, 16, "function".to_string())),
            "helper (a from-import member) should be a function, got {:?}",
            decoded
        );
    }

    #[test]
    fn test_semantic_tokens_primitive_types_and_keywords() {
        let source = "func add(a: int, b: int): int {\n    return a + b;\n}\n";
        let tokens = lex(source);
        let symbols = build_symbol_index(&tokens, source);
        let data = build_semantic_tokens(&tokens, &symbols);
        let decoded = decode(&data);

        // `func` keyword
        assert!(decoded.contains(&(0, 0, "keyword".to_string())));
        // `add` function declaration
        assert!(decoded.contains(&(0, 5, "function".to_string())));
        // `a: int` → int at col 12 (type)
        assert!(decoded.contains(&(0, 12, "type".to_string())));
        // `b: int` → int at col 20 (type)
        assert!(decoded.contains(&(0, 20, "type".to_string())));
        // `: int` return → int at col 26 (type)
        assert!(decoded.contains(&(0, 26, "type".to_string())));
        // `return` keyword
        assert!(decoded.contains(&(1, 4, "keyword".to_string())));
    }

    #[test]
    fn test_semantic_tokens_type_qualifier_and_variable() {
        let source = "var v = io::MAX;\nimport io;\n";
        let tokens = lex(source);
        let symbols = build_symbol_index(&tokens, source);
        let data = build_semantic_tokens(&tokens, &symbols);
        let decoded = decode(&data);

        // `var` keyword and `v` variable
        assert!(decoded.contains(&(0, 0, "keyword".to_string())));
        assert!(decoded.contains(&(0, 4, "variable".to_string())));
        // `io` before `::` is a module qualifier (namespace), line 0 col 8
        assert!(decoded.contains(&(0, 8, "namespace".to_string())));
        // `io` in an import statement (line 1 col 7) is a namespace
        assert!(decoded.contains(&(1, 7, "namespace".to_string())));
    }

    #[test]
    fn test_semantic_tokens_delta_encoding_is_absolute_consistent() {
        // Manually verify the first token encodes its own absolute position.
        let source = "import abc;\n";
        let tokens = lex(source);
        let symbols = build_symbol_index(&tokens, source);
        let data = build_semantic_tokens(&tokens, &symbols);
        // First emitted token is `import` at (0,0) keyword.
        let first = &data[0];
        assert_eq!(first.delta_line, 0);
        assert_eq!(first.delta_start, 0);
    }
}

#[cfg(test)]
mod unit_new {
    use super::*;

    fn lex(s: &str) -> Vec<Token> {
        let mut l = Lexer::new(s);
        let mut v = Vec::new();
        loop {
            let t = l.get_next_token();
            if t.r#type == TokenType::EndOfFile { break; }
            v.push(t);
        }
        v
    }

    #[test]
    fn format_interpolation_parses_bindings() {
        let v = format_interpolation_spans("value: {x}");
        assert_eq!(v, vec![(8, "x".to_string())]);
        // escaped braces are not bindings
        let v2 = format_interpolation_spans("{{literal}} and {y}");
        assert_eq!(v2, vec![(17, "y".to_string())]);
        // format spec suffix still resolves to the binding name
        let v3 = format_interpolation_spans("{n:04} and {p.x}");
        assert_eq!(v3.len(), 2);
        assert_eq!(v3[0], (1, "n".to_string()));
        assert_eq!(v3[1], (12, "p".to_string()));
    }

    #[test]
    fn string_escape_segments() {
        // `\n` split into literal "hi" + "world".
        let segs = string_literal_segments("hi\\nworld", false);
        assert_eq!(segs, vec![(0usize, 2usize), (4usize, 5usize)], "got {:?}", segs);
        // No escapes → single full segment.
        let segs2 = string_literal_segments("plain", false);
        assert_eq!(segs2, vec![(0usize, 5usize)]);
        // Escape at start/end handled.
        let segs3 = string_literal_segments("\\t x", false);
        assert_eq!(segs3, vec![(2usize, 2usize)]);
        // Format string: interpolation braces excluded, escapes too.
        let segs4 = string_literal_segments("a {name} b", true);
        // literal "a " (0-2) then " b" (after the {name} block ends at 8)
        assert_eq!(segs4, vec![(0usize, 2usize), (8usize, 2usize)], "got {:?}", segs4);
    }

    #[test]
    fn semantic_string_skips_escape_span() {
        // `"hi\nworld"` — the `\n` (content offset 2..4) must NOT get a string
        // semantic token, so the client's TextMate escape render shows through.
        let source = "var s: str = \"hi\\nworld\";\n";
        let tokens = lex(source);
        let symbols = build_symbol_index(&tokens, source);
        let data = build_semantic_tokens(&tokens, &symbols);

        let mut line = 0; let mut col = 0;
        let mut spans: Vec<(u32, u32, u32)> = Vec::new();
        for st in data {
            line += st.delta_line;
            if st.delta_line == 0 { col += st.delta_start; } else { col = st.delta_start; }
            spans.push((line, col, st.length));
        }
        // Only string tokens on line 1; none should cover the `\n` columns.
        // source: var s: str = "hi\nworld";  -> string content base col = after opening quote.
        // Opening quote col = 13 (count it below); content base = 14.
        // Assert: no string token spans across the escape (content offsets 2..4 → cols 16..18).
        let line1: Vec<_> = spans.iter().filter(|(l, _, _)| *l == 1).collect();
        for (_, c, len) in &line1 {
            let lo = *c;
            let hi = lo + *len;
            // escape occupies content cols 16..18 (hi=14,15; \n=16,17; world=18..)
            assert!(
                !(lo < 18 && hi > 16),
                "a string token spans the escape region {}-{}: {:?}",
                lo, hi, line1
            );
        }
    }

    #[test]
    fn semantic_format_string_marks_interpolation_variable() {
        // `@"hello {name}"` with a local var `name` → interpolation is TYPE
        // (declared variable) vs a plain undeclared `zzz` → VARIABLE.
        let source = "var name: str = \"\";\nvar s: str = @\"hi {name} {zzz}\";\n";
        let tokens = lex(source);
        let symbols = build_symbol_index(&tokens, source);
        let data = build_semantic_tokens(&tokens, &symbols);

        // Decode to (line, col, typename).
        let mut line = 0; let mut col = 0;
        let mut out = Vec::new();
        for st in data {
            line += st.delta_line;
            if st.delta_line == 0 { col += st.delta_start; } else { col = st.delta_start; }
            out.push((line, col, SEMANTIC_TYPES[st.token_type as usize].as_str().to_string()));
        }
        // line2: `var s: str = @"hi {name} {zzz}";`
        // name at content base col=?? content_base = @ col + 2. Find `name` token (TYPE).
        // Both name(declared) and zzz(undeclared) should appear; assert presence + types.
        let declared = out.iter().any(|(l, _c, t)| *l == 1 && t == "type");
        assert!(declared, "expected a `type` token on interpolation of a declared var; got {:?}", out);
        let has_var = out.iter().any(|(l, _, t)| *l == 1 && t == "variable");
        assert!(has_var, "expected a `variable` token for undeclared interpolation; got {:?}", out);
    }

    #[test]
    fn collect_signatures_basic() {
        let toks = lex("func add(a: int, b: str): bool {\n    true\n}\n");
        let sigs = collect_signatures(&toks, "");
        let s = sigs.get("add").expect("add");
        assert_eq!(s.param_names, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(s.param_types[0].as_deref(), Some("int"));
        assert_eq!(s.param_types[1].as_deref(), Some("str"));
        assert_eq!(s.return_type.as_deref(), Some("bool"));
    }

    #[test]
    fn collect_signatures_static_and_no_parens() {
        let toks = lex("static func make(): Point {\n    Point(0,0)\n}\n");
        let sigs = collect_signatures(&toks, "");
        assert!(sigs.contains_key("make"));
        assert_eq!(sigs["make"].return_type.as_deref(), Some("Point"));
    }

    #[test]
    fn call_at_basic() {
        let toks = lex("foo(1, 2)");
        // cursor inside second arg (after comma)
        let (name, active) = call_at(&toks, 0, 6).expect("call_at should find the call");
        assert_eq!(name, "foo");
        assert_eq!(active, 1);
    }

    #[test]
    fn call_at_qualified() {
        let toks = lex("calc::double(2)");
        // cursor inside the single argument `2` (after '(' at col 12)
        let (name, active) = call_at(&toks, 0, 14).unwrap();
        assert_eq!(name, "double");
        assert_eq!(active, 0);
    }

    #[test]
    fn signature_help_response_shape() {
        let sig = FuncSignature {
            name: "f".into(),
            param_names: vec!["a".into(), "b".into()],
            param_types: vec![Some("int".into()), Some("str".into())],
            param_labels: vec!["a: int".into(), "b: str".into()],
            return_type: Some("bool".into()),
            doc: Some("doc".into()),
        };
        let h = signature_help_for(&sig, 1);
        assert_eq!(h.signatures[0].label, "f(a: int, b: str): bool");
        assert_eq!(h.active_parameter, Some(1));
        assert_eq!(h.signatures[0].parameters.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn brace_folding_multi_line() {
        let toks = lex("func f() {\n    x\n}\n");
        let r = brace_folding_ranges(&toks);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].start_line, 0);
        assert_eq!(r[0].end_line, 2);
    }

    #[test]
    fn brace_folding_single_line_skipped() {
        let toks = lex("func f() { x }\n");
        let r = brace_folding_ranges(&toks);
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn import_action_inserts_import() {
        let uri = url::Url::parse("file:///tmp/x.gbl").unwrap();
        let state = DocState { source: "func main() {}\n".into(), tokens: vec![], symbols: vec![], errors: vec![], signatures: Default::default() };
        let a = build_import_action(&uri, "io", &state);
        assert!(a.title.contains("io"));
        assert!(a.edit.is_some());
    }

    #[test]
    fn scope_local_visibility() {
        let src = "func main() {\n    var a = 1;\n    if true {\n        var b = 2;\n    }\n    var c = a + b;\n}\n";
        let toks = lex(src);
        let syms = build_symbol_index(&toks, src);
        let state = DocState { source: src.into(), tokens: toks, symbols: syms, errors: vec![], signatures: Default::default() };

        // Inside `if` block after `var b`, both a (outer) and b visible.
        let in_block = state.visible_locals(3, 16);
        let names: Vec<&str> = in_block.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"), "outer `a` should be visible in inner block: {:?}", names);
        assert!(names.contains(&"b"), "inner `b` should be visible: {:?}", names);

        // On line 5 (`var c = a + b;`) after the if-block, `b` is NOT visible
        // (declared in a sibling inner block), while `a` still is.
        let after_block = state.visible_locals(5, 5);
        let names2: Vec<&str> = after_block.iter().map(|s| s.name.as_str()).collect();
        assert!(names2.contains(&"a"), "`a` visible after block: {:?}", names2);
        assert!(!names2.contains(&"b"), "`b` should not be visible after inner block: {:?}", names2);
    }

    #[test]
    fn declaration_vs_call_paren() {
        // `func add(a: int, b: int)` → add's `(` is a declaration param list.
        let decl = lex("func add(a: int, b: int): int { 0 }");
        // token index of `add` is 1 (func=0, add=1)
        assert!(is_declaration_call_paren(&decl, 1), "func decl name should be decl-paren");

        // static func make() → decl
        let sdecl = lex("static func make(): Point { Point(0) }");
        // static=0 func=1 make=2
        assert!(is_declaration_call_paren(&sdecl, 2), "static func name should be decl-paren");

        // Real call `add(x, 1)` → NOT decl
        let call = lex("add(x, 1)");
        assert!(!is_declaration_call_paren(&call, 0), "call should not be decl-paren");
    }

    #[test]
    fn argument_position_hints_labels_call_args() {
        // `func add(a: int, b: int): int` signature, then a call `add(x, y)`.
        let src_tok = lex("add(x, y)");
        let mut sig = FuncSignature::default();
        sig.name = "add".into();
        sig.param_names = vec!["a".into(), "b".into()];
        sig.param_types = vec![Some("int".into()), Some("int".into())];
        sig.param_labels = vec!["a: int".into(), "b: int".into()];
        let hints = argument_position_hints(&src_tok, 0, &sig);
        // Two hints: `a:` before x, `b:` before y.
        assert_eq!(hints.len(), 2, "expected 2 param hints, got {:?}", hints);
        match &hints[0].label {
            InlayHintLabel::String(s) => assert_eq!(s, "a:"),
            _ => panic!("expected a: label"),
        }
        match &hints[1].label {
            InlayHintLabel::String(s) => assert_eq!(s, "b:"),
            _ => panic!("expected b: label"),
        }
    }
}

#[cfg(test)]
mod e2e_doc {
    use super::*;
    fn lexall(s: &str) -> Vec<Token> {
        let mut l = Lexer::new(s);
        let mut v=Vec::new();
        loop { let t=l.get_next_token(); if t.r#type==TokenType::EndOfFile { break; } v.push(t); }
        v
    }
    #[test]
    fn analyze_and_signature_query() {
        let src = "func add(a: int, b: int): int { a + b }\nfunc main(): int { add(1, 2) }\n";
        let doc = analyze_document("file:///tmp/simple.gbl", src, &[]);
        assert!(!doc.tokens.is_empty());
        assert!(!doc.signatures.is_empty(), "signatures empty");
        assert!(doc.signatures.contains_key("add"));
        // call_at inside add(1,2) on line 1 (0-based): cursor in `2` (col 26)
        let toks = doc.tokens.clone();
        let (callee, active) = call_at(&toks, 1, 26).expect("call_at in main");
        assert_eq!(callee, "add");
        assert_eq!(active, 1);
        // signature_help shape
        let sig = &doc.signatures["add"];
        let h = signature_help_for(sig, active);
        assert_eq!(h.signatures[0].label, "add(a: int, b: int): int");
        assert_eq!(h.active_parameter, Some(1));
        // inlay type hints for `var x = 10` style
        let var_src = "func f() {\n    var x = 10;\n}\n";
        let doc2 = analyze_document("file:///tmp/var.gbl", var_src, &[]);
        // infer call expr type = int
        let toks2 = lexall("var x = 10;");
        assert_eq!(infer_expr_type(&toks2, 3, &doc2).as_deref(), Some("int")); // index3 is `10`
    }
}
