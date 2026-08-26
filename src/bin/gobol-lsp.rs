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

// ==================== Document State ====================

struct DocState {
    source: String,
    tokens: Vec<Token>,
    symbols: Vec<SymbolEntry>,
    errors: Vec<(i32, i32, String)>,
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
            TokenType::String | TokenType::FormatString => {
                (type_index(SemanticTokenType::STRING), 0)
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
        }
    }

    if let Ok(install_dir) = std::env::var("GOBOL_INSTALL_DIR") {
        let std_path = PathBuf::from(&install_dir).join("lib").join("std");
        if let Some(p) = std_path.to_str() {
            paths.push(p.to_string());
        }
        let alt = PathBuf::from(&install_dir).join("std");
        if let Some(p) = alt.to_str() {
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

    DocState {
        source: source.to_string(),
        tokens,
        symbols,
        errors,
    }
}

fn uri_to_path(uri: &str) -> String {
    uri.strip_prefix("file://")
        .unwrap_or(uri)
        .to_string()
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
                }

                break;
            }
        }

        if hover_text.is_empty() {
            hover_text = format!("`{}`", token.value);
        }

        // Cross-file fallback: the token may be a function/type defined in a
        // module this document imports (e.g. `lib::greet` or `from lib import
        // greet`).  Surface the imported definition's name, source module and
        // return type so hover still shows useful info across files.
        if sym_not_found {
            let uri_q = uri.clone();
            let resolved = self.resolve_imported_symbol(&uri_q, &token.value).await;
            if let Some((module, kind_label, ty)) = resolved {
                let type_part = ty.clone().unwrap_or_else(|| "-".to_string());
                let detail = match ty {
                    Some(_) => format!("{}::{}", module, token.value),
                    None => format!("{}::{} ({})", module, token.value, kind_label),
                };
                hover_text = format!(
                    "**{}** imported from `{}`\n\n`{}`\n\nType: `{}`",
                    kind_label, detail, detail, type_part
                );
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

        if let Some(sym) = state.find_definition(&token.value) {
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

        let mut items: Vec<CompletionItem> = keywords
            .iter()
            .map(|kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(format!("keyword `{}`", kw)),
                ..Default::default()
            })
            .collect();

        // Add snippet completions (lower sort so plain keywords first when user types `func`)
        let snippets = keyword_snippets();
        items.extend(snippets);

        // Add stdlib modules when user is typing `import ...`
        if import_ctx {
            let std_modules = self.list_std_modules(&uri);
            for (name, kind) in &std_modules {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some(format!("stdlib {}", kind)),
                    sort_text: Some(format!("0_{}", name)),
                    ..Default::default()
                });
            }
        }

        // Add symbols from the document
        let mut seen_labels: std::collections::HashSet<String> =
            items.iter().map(|it| it.label.as_str().to_string()).collect();
        {
            let state_guard = self.documents.read().await;
            if let Some(state) = state_guard.get(&uri) {
                for sym in &state.symbols {
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

        // Cross-file: surface functions / types defined in modules that this
        // document imports (`import lib;` / `from lib import ...;`).  This lets
        // the user type a bare `greet` after `import lib;` and get completion
        // for lib's `greet`, even though the definition lives in another file.
        {
            let state_guard = self.documents.read().await;
            let imports: Vec<String> = state_guard
                .get(&uri)
                .map(|state| {
                    state
                        .symbols
                        .iter()
                        .filter(|s| s.kind == SymKind::Import)
                        .map(|s| s.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            for module in imports {
                for (name, kind_label, ty) in self.index_imported_module(&uri, &module) {
                    if !seen_labels.insert(name.clone()) {
                        continue;
                    }
                    let detail = if let Some(t) = ty {
                        format!("{}::{} → {}", module, name, t)
                    } else {
                        format!("{}::{} ({})", module, name, kind_label)
                    };
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some(detail),
                        sort_text: Some(format!("1_{}", name)),
                        ..Default::default()
                    });
                }
            }
        }

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

        let module_symbols = self.index_imported_module(uri, qualifier);
        for (name, kind_label, ty) in &module_symbols {
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
                        if parent == qualifier {
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

    fn index_imported_module(
        &self,
        uri: &str,
        module_name: &str,
    ) -> Vec<(String, String, Option<String>)> {
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
            let mod_path = PathBuf::from(lib_path).join(format!("{}.gbl", file_name));
            if mod_path.exists() {
                if let Ok(source) = std::fs::read_to_string(&mod_path) {
                    let mut lexer = Lexer::new(&source);
                    let mut tokens: Vec<Token> = Vec::new();
                    loop {
                        let t = lexer.get_next_token();
                        if t.r#type == TokenType::EndOfFile {
                            break;
                        }
                        tokens.push(t);
                    }
                    let symbols = build_symbol_index(&tokens, &source);
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
                            ));
                        }
                    }
                }
                break;
            }
        }
        result
    }

    /// Look up `name` among the modules imported by the document at `uri`.
    /// Returns `(module, kind_label, type_info)` if the symbol is found in an
    /// imported module (cross-file definition), else `None`.
    async fn resolve_imported_symbol(
        &self,
        uri: &str,
        name: &str,
    ) -> Option<(String, String, Option<String>)> {
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
        for module in imports {
            for (sym_name, kind_label, ty) in self.index_imported_module(uri, &module) {
                if sym_name == name {
                    return Some((module, kind_label, ty));
                }
            }
        }
        None
    }

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
        });

        let diagnostics: Vec<Diagnostic> = state
            .errors
            .iter()
            .map(|(line, col, msg)| error_to_diagnostic(&state.source, &state.tokens, *line, *col, msg))
            .collect();

        // Atomically replace the document state — use write lock to ensure
        // exclusive access and proper replacement of old symbols.
        let mut documents = self.documents.write().await;
        documents.insert(uri_owned.clone(), state);

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
