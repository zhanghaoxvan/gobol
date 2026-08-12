// gobol-lsp.rs — Gobol Language Server (LSP) for VS Code / any LSP client.
//
// Provides:
//   - Diagnostics (parse + semantic errors with positions)
//   - Hover (type info for identifiers)
//   - Goto Definition (jump to symbol definition)
//   - Completion (symbols + keywords)
//
// The analysis runs the existing compiler frontend (Lexer → AstBuilder →
// SemanticAnalyzer) and builds a token-based symbol index for position
// queries, since AST nodes don't currently carry source positions.

use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::io::{stdin, stdout};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use gobol::ast_builder::AstBuilder;
use gobol::error::ErrorFormatter;
use gobol::lexer::Lexer;
use gobol::semantic_analyzer::SemanticAnalyzer;
use gobol::token::{Token, TokenType};

// ==================== Symbol Index ====================

#[derive(Clone, Debug)]
enum SymKind {
    Function,
    Method,
    Struct,
    Variable,
    Parameter,
    Trait,
    Import,
}

impl SymKind {
    fn label(&self) -> &'static str {
        match self {
            SymKind::Function => "function",
            SymKind::Method => "method",
            SymKind::Struct => "struct",
            SymKind::Variable => "variable",
            SymKind::Parameter => "parameter",
            SymKind::Trait => "trait",
            SymKind::Import => "module",
        }
    }

    fn completion_kind(&self) -> CompletionItemKind {
        match self {
            SymKind::Function => CompletionItemKind::FUNCTION,
            SymKind::Method => CompletionItemKind::METHOD,
            SymKind::Struct => CompletionItemKind::CLASS,
            SymKind::Variable => CompletionItemKind::VARIABLE,
            SymKind::Parameter => CompletionItemKind::VARIABLE,
            SymKind::Trait => CompletionItemKind::INTERFACE,
            SymKind::Import => CompletionItemKind::MODULE,
        }
    }
}

#[derive(Clone, Debug)]
struct SymbolEntry {
    name: String,
    kind: SymKind,
    line: i32,  // 1-based (token line)
    col: i32,   // 0-based (token col)
    type_info: Option<String>,
    parent: Option<String>, // struct name for methods, module name for imports
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
    /// Prefers definitions (Function/Struct/Variable) over references.
    fn find_definition(&self, name: &str) -> Option<&SymbolEntry> {
        self.symbols.iter().find(|s| {
            s.name == name
                && matches!(
                    s.kind,
                    SymKind::Function | SymKind::Struct | SymKind::Variable | SymKind::Parameter
                )
        })
    }
}

// ==================== Symbol Index Builder ====================

/// Build a symbol index by scanning the token stream.
/// This is a lightweight approach that doesn't require AST position info.
fn build_symbol_index(tokens: &[Token]) -> Vec<SymbolEntry> {
    let mut symbols = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        // Check keyword tokens
        if tok.r#type == TokenType::Keyword {
            match tok.value.as_str() {
                "func" => {
                    if let Some(name_tok) = tokens.get(i + 1) {
                        if name_tok.r#type == TokenType::Identifier {
                            let type_info = find_return_type(tokens, i);
                            symbols.push(SymbolEntry {
                                name: name_tok.value.clone(),
                                kind: SymKind::Function,
                                line: name_tok.line,
                                col: name_tok.col,
                                type_info,
                                parent: None,
                            });
                            extract_parameters(tokens, i, &mut symbols);
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
                                type_info: None,
                                    parent: None,
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
                                type_info,
                            parent: None,
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
                                type_info: None,
                                    parent: None,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        // "trait" is not a keyword — it's an identifier
        if tok.r#type == TokenType::Identifier && tok.value == "trait" {
            if let Some(name_tok) = tokens.get(i + 1) {
                if name_tok.r#type == TokenType::Identifier {
                    symbols.push(SymbolEntry {
                        name: name_tok.value.clone(),
                        kind: SymKind::Trait,
                        line: name_tok.line,
                        col: name_tok.col,
                        type_info: None,
                                    parent: None,
                    });
                }
            }
        }
        // "impl" — extract methods defined in impl blocks
        if tok.r#type == TokenType::Keyword && tok.value == "impl" {
            // Skip past 'for Trait' if `impl Trait for Type`
            let mut j = i + 1;
            let mut struct_name: Option<String> = None;
            while j < tokens.len() && tokens[j].value != "{" {
                // "for" keyword: skip the trait name before it
                if tokens[j].value == "for" {
                    j += 1;
                    while j < tokens.len() && tokens[j].value != "{" {
                        j += 1;
                    }
                    break;
                }
                if tokens[j].r#type == TokenType::Identifier {
                    struct_name = Some(tokens[j].value.clone());
                }
                j += 1;
            }
            // Scan impl block body for func declarations
            if let Some(ref sname) = struct_name {
                if j < tokens.len() && tokens[j].value == "{" {
                    let mut depth = 1i32;
                    let mut k = j + 1;
                    while k < tokens.len() && depth > 0 {
                        let t = &tokens[k];
                        if t.value == "{" { depth += 1; }
                        if t.value == "}" { depth -= 1; }
                        if depth > 0 && t.r#type == TokenType::Keyword && t.value == "func" {
                            if let Some(name_tok) = tokens.get(k + 1) {
                                if name_tok.r#type == TokenType::Identifier {
                                    let type_info = find_return_type(tokens, k);
                                    symbols.push(SymbolEntry {
                                        name: name_tok.value.clone(),
                                        kind: SymKind::Method,
                                        line: name_tok.line,
                                        col: name_tok.col,
                                        type_info,
                                        parent: Some(sname.clone()),
                                    });
                                    extract_parameters(tokens, k, &mut symbols);
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
    symbols
}

/// Find the return type annotation after `func name(...): Type`.
fn find_return_type(tokens: &[Token], func_idx: usize) -> Option<String> {
    // Scan forward for ')' then ':'
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
    // j is at ')'. Look for ':' after it
    j += 1;
    if j < tokens.len() && tokens[j].value == ":" {
        j += 1;
        // Collect type tokens until '{' or ';' or EndOfLine
        let mut type_str = String::new();
        while j < tokens.len() {
            let t = &tokens[j];
            if t.value == "{" || t.value == ";" || t.r#type == TokenType::EndOfLine {
                break;
            }
            if !type_str.is_empty() && t.r#type != TokenType::Operator {
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

/// Find the type annotation after `var name: Type`.
fn find_var_type(tokens: &[Token], var_idx: usize) -> Option<String> {
    // var_idx is at 'var'/'val'. i+1 is name. i+2 should be ':'
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
                    if !type_str.is_empty() && t.r#type != TokenType::Operator {
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

/// Extract parameter declarations from `func name(param1: Type1, param2: Type2, ...)`.
fn extract_parameters(tokens: &[Token], func_idx: usize, symbols: &mut Vec<SymbolEntry>) {
    // Find '(' after func name
    let mut j = func_idx + 1;
    while j < tokens.len() && tokens[j].value != "(" {
        j += 1;
    }
    if j >= tokens.len() {
        return;
    }
    j += 1; // skip '('
    // Parse ident: type, ident: type, ...
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
            // Could be a parameter name
            // Check if next token is ':'
            if j + 1 < tokens.len() && tokens[j + 1].value == ":" {
                // Collect type
                let mut k = j + 2;
                let mut type_str = String::new();
                while k < tokens.len() {
                    let tk = &tokens[k];
                    if tk.value == "," || tk.value == ")" {
                        break;
                    }
                    if !type_str.is_empty() && tk.r#type != TokenType::Operator {
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
                    type_info: if type_str.trim().is_empty() {
                        None
                    } else {
                        Some(type_str.trim().to_string())
                    },
                    parent: None,
                });
                j = k;
                continue;
            }
        }
        j += 1;
    }
}

// ==================== Lib Paths ====================

/// Build library search paths (same logic as gobol.rs main).
fn build_lib_paths(file_path: &str) -> Vec<String> {
    let mut paths = Vec::new();

    if let Some(parent) = PathBuf::from(file_path).parent() {
        if let Some(p) = parent.join("lib").to_str() {
            paths.push(p.to_string());
        }
        if let Some(grandparent) = parent.parent() {
            if let Some(p) = grandparent.join("lib").to_str() {
                paths.push(p.to_string());
            }
        }
    }

    paths.push("std".to_string());

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            if let Some(p) = exe_dir
                .parent()
                .map(|d| d.join("std"))
                .and_then(|d| d.to_str().map(|s| s.to_string()))
            {
                paths.push(p);
            }
            if let Some(p) = exe_dir.join("std").to_str().map(|s| s.to_string()) {
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

    paths
}

// ==================== Analysis ====================

/// Run the full frontend pipeline and produce a DocState.
fn analyze_document(uri: &str, source: &str) -> DocState {
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
        let lib_paths = build_lib_paths(&file_path);
        let mut sem = SemanticAnalyzer::new();
        sem.set_main_file(&file_path);
        sem.set_lib_paths(lib_paths);
        sem.set_error_formatter(error_fmt);
        sem.analyze(prog.as_ref());
        errors.extend(sem.structured_errors.clone());
    }

    // 3. Build symbol index
    let symbols = build_symbol_index(&tokens);

    DocState {
        source: source.to_string(),
        tokens,
        symbols,
        errors,
    }
}

fn uri_to_path(uri: &str) -> String {
    // Convert file:// URI to filesystem path
    uri.strip_prefix("file://")
        .unwrap_or(uri)
        .to_string()
}

fn token_to_range(token: &Token) -> Range {
    let line = (token.line as u32).saturating_sub(1); // 1-based → 0-based
    let start_col = token.col as u32;
    let end_col = start_col + token.value.len() as u32;
    Range::new(
        Position::new(line, start_col),
        Position::new(line, end_col),
    )
}

fn error_to_diagnostic(line: i32, col: i32, msg: &str) -> Diagnostic {
    let lsp_line = if line > 0 {
        (line as u32).saturating_sub(1)
    } else {
        0
    };
    let lsp_col = col as u32;
    let range = Range::new(
        Position::new(lsp_line, lsp_col),
        Position::new(lsp_line, lsp_col + 1),
    );
    Diagnostic::new(
        range,
        Some(DiagnosticSeverity::ERROR),
        None,
        Some("gobol".to_string()),
        msg.to_string(),
        None,
        None,
    )
}

// ==================== LSP Backend ====================

struct GobolLsp {
    client: Client,
    documents: Arc<DashMap<String, DocState>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for GobolLsp {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        ..Default::default()
                    },
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "gobol-lsp".to_string(),
                version: Some("0.1.0".to_string()),
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

    // ---- Document lifecycle ----

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let text = params.text_document.text;
        self.reanalyze(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        // We use FULL sync — take the last full text change
        if let Some(change) = params.content_changes.into_iter().next() {
            self.reanalyze(&uri, &change.text).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        // Re-read from disk on save (in case of external edits)
        if let Some(text) = params.text {
            self.reanalyze(&uri, &text).await;
        } else if let Some(state) = self.documents.get(&uri) {
            let text = state.source.clone();
            drop(state);
            self.reanalyze(&uri, &text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        // Clear diagnostics on close
        let _ = self
            .client
            .publish_diagnostics(
                params.text_document.uri,
                Vec::new(),
                None,
            )
            .await;
        self.documents.remove(&uri);
    }

    // ---- Hover ----

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri.to_string();

        let state = match self.documents.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let token = match state.token_at(pos.position.line, pos.position.character) {
            Some(t) => t,
            None => return Ok(None),
        };

        // Find symbol info
        let mut hover_text = String::new();
        for sym in &state.symbols {
            if sym.name == token.value {
                hover_text = format!(
                    "**{}** `{}`\n\n```gobol\n{}{}{}\n```",
                    sym.kind.label(),
                    sym.name,
                    sym.kind.label(),
                    if sym.name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                        ""
                    } else {
                        " "
                    },
                    sym.name,
                );
                if let Some(ref ty) = sym.type_info {
                    hover_text.push_str(&format!("\n\nType: `{}`", ty));
                }
                break;
            }
        }

        if hover_text.is_empty() {
            // Fallback: just show the token value
            hover_text = format!("`{}`", token.value);
        }

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover_text,
            }),
            range: Some(token_to_range(token)),
        }))
    }

    // ---- Goto Definition ----

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let pos = params.text_document_position_params;
        let uri = pos.text_document.uri.to_string();

        let state = match self.documents.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let token = match state.token_at(pos.position.line, pos.position.character) {
            Some(t) => t,
            None => return Ok(None),
        };

        // Find the definition symbol
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

    // ---- Completion ----

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;

        // Check if we're in a `::` context (e.g., `io::` or `Point::`)
        let qualifier = self.find_qualifier_before_colon_colon(&uri, pos).await;

        if let Some(ref q) = qualifier {
            // Show completions scoped to the qualifier (module members or struct methods)
            return self.complete_qualified(&uri, q).await;
        }

        let keywords = &[
            "func", "var", "val", "struct", "impl", "trait", "if", "else",
            "for", "while", "return", "break", "continue", "import", "export",
            "as", "in", "match", "convert", "operator", "constructor", "new",
            "true", "false", "null", "self", "int", "float", "str", "bool",
        ];

        let mut items: Vec<CompletionItem> = keywords
            .iter()
            .map(|kw| CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            })
            .collect();

        // Add symbols from the document
        if let Some(state) = self.documents.get(&uri) {
            for sym in &state.symbols {
                let mut detail = sym.type_info.clone().unwrap_or_else(|| sym.kind.label().to_string());
                if let Some(ref parent) = sym.parent {
                    detail = format!("{}::{}({})", parent, sym.name, detail);
                } else {
                    detail = format!("{}: {}", sym.kind.label(), detail);
                }
                items.push(CompletionItem {
                    label: sym.name.clone(),
                    kind: Some(sym.kind.completion_kind()),
                    detail: Some(detail),
                    ..Default::default()
                });
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    // ---- Document Symbols (outline) ----

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri.to_string();

        let state = match self.documents.get(&uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let mut symbols: Vec<DocumentSymbol> = Vec::new();
        for sym in &state.symbols {
            if matches!(sym.kind, SymKind::Function | SymKind::Struct | SymKind::Trait) {
                let line = (sym.line as u32).saturating_sub(1);
                let col = sym.col as u32;
                let end_col = col + sym.name.len() as u32;
                let range = Range::new(
                    Position::new(line, col),
                    Position::new(line, end_col),
                );
                let kind = match sym.kind {
                    SymKind::Function => SymbolKind::FUNCTION,
                    SymKind::Struct => SymbolKind::CLASS,
                    SymKind::Trait => SymbolKind::INTERFACE,
                    _ => SymbolKind::VARIABLE,
                };
                symbols.push(DocumentSymbol {
                    name: sym.name.clone(),
                    detail: sym.type_info.clone(),
                    kind,
                    range,
                    selection_range: range,
                    children: None,
                    tags: None,
                    #[allow(deprecated)]
                    deprecated: None,
                });
            }
        }

        if symbols.is_empty() {
            Ok(None)
        } else {
            Ok(Some(DocumentSymbolResponse::Nested(symbols)))
        }
    }
}

impl GobolLsp {
    /// Check if the cursor is positioned after `ident::` and return the qualifier.
    async fn find_qualifier_before_colon_colon(
        &self,
        uri: &str,
        pos: Position,
    ) -> Option<String> {
        let state = self.documents.get(uri)?;
        // Check tokens before cursor for `identifier ::` pattern
        let target_line = (pos.line as i32) + 1;
        let target_col = pos.character as i32;
        for i in 1..state.tokens.len() {
            let t = &state.tokens[i];
            if t.line == target_line
                && t.col <= target_col
                && t.col + t.value.len() as i32 >= target_col
                && t.value == ":"
            {
                // Check if next token is also ':'
                if i + 1 < state.tokens.len() && state.tokens[i + 1].value == ":" {
                    // Found `::` at cursor. Look backwards for the qualifier identifier.
                    if i > 0 && state.tokens[i - 1].r#type == TokenType::Identifier {
                        return Some(state.tokens[i - 1].value.clone());
                    }
                }
            }
        }
        None
    }

    /// Provide completions for a qualified name (e.g., after `io::` or `Point::`).
    async fn complete_qualified(
        &self,
        uri: &str,
        qualifier: &str,
    ) -> Result<Option<CompletionResponse>> {
        let mut items: Vec<CompletionItem> = Vec::new();

        // First, try to load the qualifier as an imported module
        let module_symbols = self.index_imported_module(uri, qualifier);
        for (name, kind_label, ty) in &module_symbols {
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some(format!("{} → {}", qualifier, ty.as_deref().unwrap_or(kind_label))),
                ..Default::default()
            });
        }

        // Also look in the current document for methods of struct `qualifier`
        if let Some(state) = self.documents.get(uri) {
            for sym in &state.symbols {
                if let Some(ref parent) = sym.parent {
                    if parent == qualifier {
                        let detail = sym.type_info.clone()
                            .unwrap_or_else(|| sym.kind.label().to_string());
                        items.push(CompletionItem {
                            label: sym.name.clone(),
                            kind: Some(sym.kind.completion_kind()),
                            detail: Some(format!("{}::{}({})", parent, sym.name, detail)),
                            ..Default::default()
                        });
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

    /// Look up an imported module by name and extract its exported symbols.
    fn index_imported_module(&self, uri: &str, module_name: &str) -> Vec<(String, String, Option<String>)> {
        let mut result = Vec::new();
        let file_path = uri_to_path(uri);

        // Try to find the module file
        let lib_paths = build_lib_paths(&file_path);
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
                    let symbols = build_symbol_index(&tokens);
                    // Filter to function/method symbols (these are the "exports")
                    for sym in &symbols {
                        if matches!(sym.kind, SymKind::Function | SymKind::Method) {
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

    /// Re-analyze a document and publish diagnostics.
    async fn reanalyze(&self, uri: &str, text: &str) {
        // Run analysis (blocking — the compiler frontend is synchronous)
        let uri_owned = uri.to_string();
        let text_owned = text.to_string();
        let state = tokio::task::spawn_blocking(move || {
            analyze_document(&uri_owned, &text_owned)
        })
        .await
        .unwrap_or_else(|_| DocState {
            source: String::new(),
            tokens: Vec::new(),
            symbols: Vec::new(),
            errors: vec![(0, 0, "Internal error: analysis panicked".to_string())],
        });

        // Publish diagnostics
        let diagnostics: Vec<Diagnostic> = state
            .errors
            .iter()
            .map(|(line, col, msg)| error_to_diagnostic(*line, *col, msg))
            .collect();

        // Store state
        self.documents.insert(uri.to_string(), state);

        // Send diagnostics to client
        if let Ok(url) = url::Url::parse(uri) {
            self.client
                .publish_diagnostics(url, diagnostics, None)
                .await;
        }
    }
}

// ==================== Main ====================

fn main() {
    let stdin = stdin();
    let stdout = stdout();

    let (service, socket) =
        LspService::new(|client| GobolLsp {
            client,
            documents: Arc::new(DashMap::new()),
        });

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(Server::new(stdin, stdout, socket).serve(service));
}
