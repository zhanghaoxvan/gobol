#![allow(dead_code)]

use crate::ast::*;
use crate::error::ErrorFormatter;
use crate::lexer::Lexer;
use crate::token::{Token, TokenType};

pub struct AstBuilder {
    tokens: Vec<Token>,
    eof_token: Token,
    root: Option<Box<Program>>,
    current_position: usize,
    error_occurred: bool,
    error_message: Vec<String>,
    error_formatter: Option<ErrorFormatter>,
    /// Structured errors for LSP: (line, col, message)
    pub structured_errors: Vec<(i32, i32, String)>,
    /// File-level attributes (`#![attr]`) parsed at the top of the source.
    /// Merged into each top-level statement's own `#[...]` (closest-wins:
    /// a statement's local attribute with the same name overrides the
    /// file-level one).
    file_attributes: Vec<Attribute>,
    /// When enabled, the std prelude (`import std::xxx`) is auto-injected at
    /// the top of the built program. This is intended for the *entry* program
    /// only — standard-library modules (and any module loaded by the semantic
    /// analyser) declare their own imports explicitly, so they must not have
    /// the prelude injected (it would create recursive / self imports that
    /// defeat relative resolution).
    inject_prelude: bool,
}

impl AstBuilder {
    pub fn new(mut lexer: Lexer) -> Self {
        let mut tokens = Vec::new();
        let mut tk = lexer.get_next_token();
        while tk.r#type != TokenType::EndOfFile {
            tokens.push(tk);
            tk = lexer.get_next_token();
        }
        AstBuilder {
            tokens,
            eof_token: Token::new(TokenType::EndOfFile, ""),
            root: None,
            current_position: 0,
            error_occurred: false,
            error_message: Vec::new(),
            error_formatter: None,
            structured_errors: Vec::new(),
            file_attributes: Vec::new(),
            inject_prelude: false,
        }
    }

    pub fn set_error_formatter(&mut self, f: ErrorFormatter) {
        self.error_formatter = Some(f);
    }

    /// Enable/disable injection of the std prelude for this build.
    /// The entry program should opt in; std-library / loaded modules
    /// must not (see the field docs).
    pub fn set_inject_prelude(&mut self, enabled: bool) {
        self.inject_prelude = enabled;
    }

    pub fn build(&mut self) -> Option<Box<Program>> {
        self.root = None;
        let mut program = self.parse_program();
        if self.inject_prelude {
            self.inject_std_prelude(&mut program);
        }
        self.root = Some(Box::new(program));
        self.root.take()
    }

    pub fn get_root(&self) -> Option<&Program> {
        self.root.as_deref()
    }

    pub fn reset(&mut self) {
        self.root = None;
        self.current_position = 0;
        self.error_occurred = false;
        self.error_message.clear();
    }

    pub fn has_error(&self) -> bool {
        self.error_occurred
    }

    pub fn get_error_message(&self) -> &Vec<String> {
        &self.error_message
    }

    /// Return the token stream (for LSP symbol index).
    pub fn get_tokens(&self) -> &[Token] {
        &self.tokens
    }

    // ==================== Attribute parsing ====================

    /// Parse a sequence of `#[...]` node-level attributes.
    /// Handles: `#[name]`, `#[name("value")]`, `#[name(key = "value")]`, `#[name(key = "v", k2 = "v2")]`
    fn parse_attributes(&mut self) -> Vec<Attribute> {
        let mut attrs = Vec::new();
        while self.match_value("#[") {
            self.advance(); // consume '#['
            if let Some(attr) = self.parse_attribute_body() {
                attrs.push(attr);
                // Allow newlines between an attribute and the statement it
                // decorates (e.g. `#[expand]\nfunc ...`).
                self.consume_end_of_line();
            }
        }
        attrs
    }

    /// Parse a sequence of `#![...]` file-level attributes. These may only
    /// appear at the very top of a source file and apply to the whole
    /// module/program (e.g. `#![no_gc]`). They are stored on
    /// `Program.attributes` and propagated to all top-level statements.
    fn parse_file_attributes(&mut self) -> Vec<Attribute> {
        let mut attrs = Vec::new();
        while self.match_value("#![") {
            self.advance(); // consume '#!['
            if let Some(attr) = self.parse_attribute_body() {
                attrs.push(attr);
                self.consume_end_of_line();
            }
        }
        attrs
    }

    /// Parse the body of a single attribute (after `#[` or `#![` has been
    /// consumed): `name`, optional `(args)`, then the closing `]`.
    /// Returns `None` and logs an error if the name is missing.
    fn parse_attribute_body(&mut self) -> Option<Attribute> {
        if !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected attribute name after '#['");
            while !self.match_value("]") && !self.match_type(&TokenType::EndOfFile) {
                self.advance();
            }
            if self.match_value("]") {
                self.advance();
            }
            return None;
        }
        let name = self.current_token().value.clone();
        self.advance();

        let mut attr = Attribute::new(name);

        // Parse attribute arguments: (args) or (key = value, ...)
        if self.match_value("(") {
            self.advance(); // consume '('
            while !self.match_value(")") && !self.match_type(&TokenType::EndOfFile) {
                // Check for named arg: key = "value"
                if self.match_type(&TokenType::Identifier) {
                    let key = self.current_token().value.clone();
                    let next = self.peek_next_token();
                    if next.value == "=" {
                        self.advance(); // consume key
                        self.advance(); // consume '='
                        let val = self.parse_attribute_value();
                        attr.named.push((key, val));
                    } else {
                        // Positional string value
                        self.advance(); // consume key (treated as value)
                        let val = self.parse_attribute_value();
                        attr.value = Some(val);
                    }
                } else if self.match_type(&TokenType::String) {
                    let val = self.current_token().value.clone();
                    self.advance();
                    attr.value = Some(val);
                } else {
                    // Just consume whatever token is there
                    self.advance();
                }
                if self.match_value(",") {
                    self.advance();
                }
            }
            if self.match_value(")") {
                self.advance(); // consume ')'
            }
        }

        if !self.match_value("]") {
            self.log_error("Expected ']' to close attribute");
            None
        } else {
            self.advance(); // consume ']'
            Some(attr)
        }
    }

    /// Returns true when the current token can be used as a name binding
    /// (method/function/parameter name). This includes plain Identifiers as
    /// well as keywords that double as common identifiers in this language —
    /// most notably `new` (the `New` trait constructor) and `convert`.
    fn is_name_token(&self) -> bool {
        if self.match_type(&TokenType::Identifier) {
            return true;
        }
        if self.match_type(&TokenType::Keyword) {
            return matches!(
                self.current_token().value.as_str(),
                "new" | "convert" | "match"
            );
        }
        false
    }

    /// Parse an associated type declaration inside a trait body:
    ///   `type Name;`
    ///   `type Name = SomeType;`
    /// The declaration is consumed and discarded — associated types are not
    /// yet modelled in the AST, but the syntax must parse so that trait
    /// definitions like `Iterator { type Value; ... }` are accepted.
    fn parse_trait_assoc_type(&mut self) {
        // consume 'type'
        self.advance();
        if !self.is_name_token() {
            self.log_error("Expected associated type name after 'type'");
            return;
        }
        // consume name
        self.advance();
        // optional default binding: `= Type`
        if self.match_value("=") {
            self.advance();
            let _ = self.parse_type();
        }
        if self.match_value(";") {
            self.advance();
        }
        self.consume_end_of_line();
    }

    /// Parse an associated type binding inside an impl block:
    ///   `type Name = ConcreteType;`
    /// Like trait associated types, these are consumed and discarded — the
    /// impl block's methods already carry concrete signatures, so the binding
    /// is documentary at this stage.
    fn parse_impl_assoc_type(&mut self) {
        // consume 'type'
        self.advance();
        if !self.is_name_token() {
            self.log_error("Expected associated type name after 'type'");
            return;
        }
        // consume name
        self.advance();
        if self.match_value("=") {
            self.advance();
            let _ = self.parse_type();
        }
        if self.match_value(";") {
            self.advance();
        }
        self.consume_end_of_line();
    }

    /// Parse a possibly `::`-qualified name like "std::io::println" or "int".
    /// Returns the full qualified name string.
    fn parse_qualified_name(&mut self) -> Option<String> {
        if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
            return None;
        }
        let mut name = self.current_token().value.clone();
        self.advance();
        while self.match_value("::") {
            self.advance(); // consume '::'
            if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
                return None;
            }
            name.push_str("::");
            name.push_str(&self.current_token().value);
            self.advance();
        }
        Some(name)
    }

    fn parse_attribute_value(&mut self) -> String {
        if self.match_type(&TokenType::String) {
            let val = self.current_token().value.clone();
            self.advance();
            val
        } else if self.match_type(&TokenType::Identifier) {
            let val = self.current_token().value.clone();
            self.advance();
            val
        } else if self.match_type(&TokenType::Number) {
            let val = self.current_token().value.clone();
            self.advance();
            val
        } else {
            String::new()
        }
    }

    // ==================== Helpers ====================

    fn current_token(&self) -> &Token {
        if self.current_position >= self.tokens.len() {
            &self.eof_token
        } else {
            &self.tokens[self.current_position]
        }
    }

    fn peek_next_token(&self) -> &Token {
        if self.current_position + 1 >= self.tokens.len() {
            &self.eof_token
        } else {
            &self.tokens[self.current_position + 1]
        }
    }

    fn advance(&mut self) {
        if self.current_position < self.tokens.len() {
            self.current_position += 1;
        }
    }

    fn match_type(&self, tp: &TokenType) -> bool {
        &self.current_token().r#type == tp
    }

    fn match_value(&self, value: &str) -> bool {
        self.current_token().value == value
    }

    fn is_end_of_line(&self) -> bool {
        self.match_type(&TokenType::EndOfLine)
    }

    fn is_semicolon(&self) -> bool {
        self.match_value(";")
    }

    fn is_stmt_terminator(&self) -> bool {
        self.is_end_of_line() || self.is_semicolon()
    }

    fn save_state(&self) -> usize {
        self.current_position
    }

    fn restore_state(&mut self, checkpoint: usize) {
        self.current_position = checkpoint;
    }
    /// 执行一次"试探性解析"（Lookahead + Backtracking）。
    ///
    /// 保存当前解析位置，执行 `parse_fn`。无论成功与否，解析位置都会恢复到调用前的状态。
    /// 若 `parse_fn` 返回 `true`，则试探成功；否则失败。
    ///
    /// 这是解决 `<` 符号歧义（泛型 vs 比较运算符）的核心工具方法。
    fn lookahead<F>(&mut self, mut parse_fn: F) -> bool
    where
        F: FnMut(&mut Self) -> bool,
    {
        let checkpoint = self.save_state();
        let success = parse_fn(self);
        self.restore_state(checkpoint);
        success
    }

    fn consume_end_of_line(&mut self) {
        while self.is_end_of_line() || self.is_semicolon() {
            self.advance();
        }
    }

    fn consume(&mut self, tp: TokenType, error_msg: &str) -> Token {
        if self.match_type(&tp) {
            let token = self.current_token().clone();
            self.advance();
            token
        } else {
            self.log_error(error_msg);
            self.current_token().clone()
        }
    }

    fn consume_value(&mut self, value: &str, error_msg: &str) -> Token {
        if self.match_value(value) {
            let token = self.current_token().clone();
            self.advance();
            token
        } else {
            self.log_error(error_msg);
            self.current_token().clone()
        }
    }

    fn log_error(&mut self, message: &str) {
        self.error_occurred = true;
        let (line, col, val_len) = {
            let token = self.current_token();
            (token.line, token.col, token.value.len())
        };
        self.structured_errors.push((line, col, message.to_string()));
        if let Some(ref f) = self.error_formatter {
            let span = if val_len == 0 { 1 } else { val_len };
            let formatted = f.format_error(line, col, span, "error", message, true);
            self.error_message.push(formatted);
        } else {
            self.error_message.push(format!("Builder Error: {}", message));
        }
    }

    // ==================== Program ====================

    fn parse_program(&mut self) -> Program {
        let mut program = Program::new();

        // File-level attributes `#![attr]` that appear before any statement.
        // They are stored on Program.attributes AND on the builder so that
        // parse_statement can merge them into each top-level statement's own
        // `#[...]` (closest-wins: a local attribute overrides the file-level
        // one with the same name).
        let file_attrs = self.parse_file_attributes();
        self.file_attributes = file_attrs.clone();
        program.attributes = file_attrs;

        while !self.match_type(&TokenType::EndOfFile) && !self.error_occurred {
            self.consume_end_of_line();

            if self.match_type(&TokenType::EndOfFile) {
                break;
            }

            // Inner file-level attributes `#![attr]` may also appear between
            // top-level statements (like Rust's inner attributes). Append
            // them to the file-attribute set so subsequent statements inherit
            // them, and to Program.attributes.
            if self.match_value("#![") {
                let inner = self.parse_file_attributes();
                self.file_attributes.extend(inner.clone());
                program.attributes.extend(inner);
                continue;
            }

            let stmt = self.parse_statement();
            if let Some(s) = stmt {
                program.add_statement(s);
            } else {
                self.advance();
            }
        }

        // The merge into each statement already happened inside
        // parse_statement (via merge_with_file_attributes), so no separate
        // post-pass is needed here.

        program
    }

    /// Merge a statement's own node-level attributes with the file-level
    /// attributes. File-level attrs come first; the statement's own attrs are
    /// appended so that Attribute lookups (which scan left-to-right) find the
    /// closest (local) one first — implementing "子节点属性覆盖父节点属性".
    /// A local attribute with the same name entirely replaces the file-level
    /// one (no duplication).
    fn merge_with_file_attributes(&self, mut attrs: Vec<Attribute>) -> Vec<Attribute> {
        if self.file_attributes.is_empty() {
            return attrs;
        }
        let local_names: Vec<String> = attrs.iter().map(|a| a.name.clone()).collect();
        let mut merged = Vec::with_capacity(self.file_attributes.len() + attrs.len());
        for fa in &self.file_attributes {
            if !local_names.iter().any(|n| n == &fa.name) {
                merged.push(fa.clone());
            }
        }
        merged.append(&mut attrs);
        merged
    }

    // ==================== Statement ====================

    fn parse_statement(&mut self) -> Option<Box<dyn Statement>> {
        // Parse attributes first — they can precede struct, impl, func, trait.
        // Merge in any file-level `#![attr]` so they propagate to this node
        // (closest-wins: a local attribute overrides the file-level one).
        let raw_attrs = self.parse_attributes();
        let attrs = self.merge_with_file_attributes(raw_attrs);

        if self.match_type(&TokenType::Keyword) {
            let keyword = self.current_token().value.clone();

            match keyword.as_str() {
                "import" => return self.parse_import(),
                "from" => return self.parse_from_import(),
                "func" => return self.parse_function(attrs),
                "var" | "val" => return self.parse_declaration(),
                "for" => return self.parse_for_statement(),
                "return" => return self.parse_return_statement(),
                "module" => {
                    // module keyword is deprecated — skip the line
                    self.advance(); // consume 'module'
                    while !self.is_end_of_line() && !self.match_type(&TokenType::EndOfFile) {
                        self.advance();
                    }
                    self.consume_end_of_line();
                    return Some(Box::new(ExportStatement::new(vec![])));
                }
                "struct" => return self.parse_struct_definition(attrs),
                "enum" => return self.parse_enum_definition(attrs),
                "impl" => return self.parse_impl_block(attrs),
                "trait" => return self.parse_trait_definition(attrs),
                "extern" => return self.parse_extern_block(attrs),
                "export" => return self.parse_export_statement(),
                "if" => return self.parse_if_statement(),
                "match" => {
                    let match_expr = self.parse_match_expression()?;
                    // MatchExpression implements Statement too, go through as_any
                    return Some(Box::new(ExpressionStatement::new(Some(match_expr))));
                }
                "while" => return self.parse_while_statement(),
                "break" => return self.parse_break_statement(),
                "continue" => return self.parse_continue_statement(),
                _ => {}
            }
        }

        // Expression statements can start with: identifier, number, string, format string,
        // certain keywords (true, false, null, self, if, match, new),
        // and certain operators: (, !, -, +, [, {
        let is_expr_keyword = self.match_type(&TokenType::Keyword) && matches!(
            self.current_token().value.as_str(),
            "true" | "false" | "null" | "self" | "if" | "match" | "new"
        );
        let is_expr_operator = self.match_type(&TokenType::Operator) && matches!(
            self.current_token().value.as_str(),
            "(" | "!" | "-" | "+" | "[" | "{"
        );
        if self.match_type(&TokenType::Identifier)
            || self.match_type(&TokenType::Number)
            || self.match_type(&TokenType::String)
            || self.match_type(&TokenType::FormatString)
            || is_expr_keyword
            || is_expr_operator
        {
            return self.parse_expression_statement();
        }

        if self.match_type(&TokenType::Operator)
            && (self.current_token().value == "}" || self.current_token().value == ")")
        {
            return None;
        }

        self.log_error(&format!("Unexpected token: {}", self.current_token().value));
        None
    }

    fn parse_import(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'import'

        // `import` loads a module file. Multi-segment paths (e.g.
        // `import lib.math`) are allowed for nested modules.
        // To import specific members, use `from module import member;`.
        if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
            self.log_error("Expected module name after 'import'");
            return None;
        }

        let mut path = vec![self.current_token().value.clone()];
        self.advance();

        // Handle "import a.b.c" or "import a::b::c" (nested module paths)
        while self.match_value(".") || self.match_value("::") {
            self.advance(); // consume '.' or '::'
            if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
                self.log_error("Expected identifier after separator in import path");
                return None;
            }
            path.push(self.current_token().value.clone());
            self.advance();
        }

        // Handle "import module as alias"
        let alias = if self.match_type(&TokenType::Keyword) && self.current_token().value == "as" {
            self.advance(); // consume 'as'
            if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
                self.log_error("Expected identifier after 'as'");
                return None;
            }
            let alias = self.current_token().value.clone();
            self.advance();
            Some(alias)
        } else {
            None
        };

        self.consume_end_of_line();

        Some(Box::new(ImportStatement::new(path, alias)))
    }

    fn inject_std_prelude(&mut self, program: &mut Program) {
        let prelude_modules = vec![
            "assert",
            "builtins",
            "byte",
            "cmp",
            "debug",
            "float",
            "fs",
            "int",
            "io",
            "iterator",
            "math",
            "mem",
            "net",
            "ops",
            "option",
            "range",
            "ref",
            "result",
            "str",
            "thread",
            "vec",
        ];

        let mut imported: std::collections::HashSet<String> = std::collections::HashSet::new();
        for stmt in program.get_statements() {
            if let Some(import) = stmt.as_any().downcast_ref::<ImportStatement>() {
                imported.insert(import.get_module_name());
            }
        }

        // 从后往前插入，保持顺序
        for &name in prelude_modules.iter().rev() {
            if !imported.contains(name) {
                let import_stmt = ImportStatement::new(vec!["std".to_string(), name.to_string()], None);
                program.statements.insert(0, Box::new(import_stmt));
            }
        }
    }

    fn parse_from_import(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'from'

        // Module name (single segment; identifiers or keywords like `int`)
        if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
            self.log_error("Expected module name after 'from'");
            return None;
        }
        let module = self.current_token().value.clone();
        self.advance();

        // Reject `from io::sub import ...` — only a single module name is allowed.
        if self.match_value("::") || self.match_value(".") {
            self.log_error("'from' only accepts a single module name");
            while self.match_value("::") || self.match_value(".") {
                self.advance();
                if self.match_type(&TokenType::Identifier)
                    || self.match_type(&TokenType::Keyword)
                {
                    self.advance();
                }
            }
        }

        // Expect `import` keyword
        if !self.match_type(&TokenType::Keyword) || self.current_token().value != "import" {
            self.log_error("Expected 'import' after module name in 'from' statement");
            return None;
        }
        self.advance(); // consume 'import'

        // Check for wildcard import: `from module import *`
        if self.match_value("*") {
            self.advance(); // consume '*'
            self.consume_end_of_line();
            return Some(Box::new(FromImportStatement::new(module, Vec::new(), true)));
        }

        // Parse comma-separated member list with optional `as` aliases
        let mut members: Vec<(String, Option<String>)> = Vec::new();
        loop {
            if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
                self.log_error("Expected member name after 'import' in 'from' statement");
                return None;
            }
            let member_name = self.current_token().value.clone();
            self.advance();

            // Check for `as alias`
            let alias = if self.match_type(&TokenType::Keyword) && self.current_token().value == "as" {
                self.advance(); // consume 'as'
                if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
                    self.log_error("Expected alias name after 'as'");
                    return None;
                }
                let alias_name = self.current_token().value.clone();
                self.advance();
                Some(alias_name)
            } else {
                None
            };

            members.push((member_name, alias));

            if self.match_value(",") {
                self.advance(); // consume ','
                continue;
            }
            break;
        }

        if members.is_empty() {
            self.log_error("'from' import requires at least one member");
            return None;
        }

        self.consume_end_of_line();

        Some(Box::new(FromImportStatement::new(module, members, false)))
    }

    fn parse_export_statement(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'export'

        self.consume_value("(", "Expected '(' after 'export'");

        let mut names: Vec<String> = Vec::new();

        while !self.match_value(")") && !self.error_occurred {
            // Accept both Identifier and Keyword tokens here: export lists
            // commonly include type names (int, float, str, bool, vec, trait)
            // that the lexer classifies as keywords.
            let is_name = self.match_type(&TokenType::Identifier)
                || self.match_type(&TokenType::Keyword);
            if !is_name {
                self.log_error("Expected identifier in export list");
                return None;
            }
            let mut name = self.current_token().value.clone();
            self.advance();

            // Handle dotted names: add.add, io.print, etc.
            while self.match_value(".") {
                self.advance();
                let is_part = self.match_type(&TokenType::Identifier)
                    || self.match_type(&TokenType::Keyword);
                if !is_part {
                    self.log_error("Expected identifier after '.' in export name");
                    return None;
                }
                name.push('.');
                name.push_str(&self.current_token().value);
                self.advance();
            }

            names.push(name);

            if self.match_value(",") {
                self.advance();
            } else {
                break;
            }
        }

        self.consume_value(")", "Expected ')' after export list");
        self.consume_end_of_line();

        Some(Box::new(ExportStatement::new(names)))
    }

    /// try parse the generic args <T, U, ...>
    /// 如果成功，返回参数列表并消费 token；如果失败，不消费任何 token。
    fn try_parse_generic_args(&mut self) -> Option<Vec<String>> {
        let is_generic = self.lookahead(|parser| {
            if !parser.match_value("<") { return false; }
            parser.advance(); // consume '<'
            loop {
                if !parser.match_type(&TokenType::Identifier) { return false; }
                parser.advance();
                if parser.match_value(",") { 
                    parser.advance(); 
                } else { 
                    break; 
                }
            }
            if !parser.match_value(">") { return false; }
            parser.advance(); // consume '>'
            true
        });

        if is_generic {
            self.advance(); // consume '<'
            let mut params = Vec::new();
            loop {
                if !self.match_type(&TokenType::Identifier) { break; }
                params.push(self.current_token().value.clone());
                self.advance();
                if self.match_value(",") { self.advance(); } else { break; }
            }
            if self.match_value(">") { self.advance(); }
            Some(params)
        } else {
            None
        }
    }

    fn parse_struct_definition(&mut self, attrs: Vec<Attribute>) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'struct'

        let name = match self.parse_qualified_name() {
            Some(n) => n,
            None => {
                self.log_error("Expected struct name");
                return None;
            }
        };

        let mut generic_params = Vec::new();
        if self.match_value("<") {
            self.advance();
            loop {
                if !self.match_type(&TokenType::Identifier) { break; }
                generic_params.push(self.current_token().value.clone());
                self.advance();
                if self.match_value(",") { self.advance(); } else { break; }
            }
            if !self.match_value(">") { self.log_error("Expected '>'"); }
            else { self.advance(); }
        }

        self.consume_value("{", "Expected '{' at start of struct body");
        self.consume_end_of_line();

        let mut fields = Vec::new();
        while !self.match_value("}") && !self.error_occurred {
            self.consume_end_of_line();
            if self.match_value("}") { break; }

            if !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected field name");
                break;
            }
            let field_name = self.current_token().value.clone();
            self.advance();

            let field_type = if self.match_value(":") {
                self.advance();
                self.parse_type()
            } else {
                None
            };

            fields.push(StructField::new(field_name, field_type));
            self.consume_end_of_line();

            if self.match_value(",") { self.advance(); }
            self.consume_end_of_line();
        }

        self.consume_value("}", "Expected '}' after struct body");
        self.consume_end_of_line();

        Some(Box::new(StructDefinition::new(name, fields, generic_params).with_attributes(attrs)))
    }

        fn parse_impl_block(&mut self, attrs: Vec<Attribute>) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'impl'

        // 1. 解析 impl 级别的泛型: impl<T> ...
        let generic_params = self.try_parse_generic_args().unwrap_or_default();

        // 2. 解析 trait/type 名称
        let first_name = match self.parse_qualified_name() {
            Some(n) => n,
            None => {
                self.log_error("Expected type/trait name after 'impl'");
                return None;
            }
        };

        // 3. 解析 trait 的泛型: impl New<T> for ...
        let trait_generic_params = self.try_parse_generic_args();

        // 4. 检查 for 关键字，区分 impl Trait for Type 还是 impl Type
        let (struct_name, trait_name) = if self.match_type(&TokenType::Keyword) && self.current_token().value == "for" {
            self.advance(); // consume 'for'
            let type_name = match self.parse_qualified_name() {
                Some(n) => n,
                None => {
                    self.log_error("Expected type name after 'for'");
                    return None;
                }
            };
            
            // 5. 解析 struct 的泛型: impl Trait for Type<T>
            let _struct_generic_params = self.try_parse_generic_args();

            let full_trait_name = if let Some(params) = trait_generic_params {
                format!("{}<{}>", first_name, params.join(", "))
            } else {
                first_name
            };
            (type_name, Some(full_trait_name))
        } else {
            // impl Type { ... }
            let full_name = if let Some(params) = trait_generic_params {
                format!("{}<{}>", first_name, params.join(", "))
            } else {
                first_name
            };
            (full_name, None)
        };

        self.consume_end_of_line();
        self.consume_value("{", "Expected '{' at start of impl block");
        self.consume_end_of_line();

        let mut items = Vec::new();
        while !self.match_value("}") && !self.error_occurred {
            self.consume_end_of_line();
            if self.match_value("}") { break; }
            
            // Parse attributes on methods (e.g., #[intrinsic("i32_add")])
            let method_raw = self.parse_attributes();
            let method_attrs = self.merge_with_file_attributes(method_raw);

            // Associated type binding inside an impl block:
            //   `type Name = ConcreteType;`
            // Consumed and discarded — see parse_impl_assoc_type.
            if (self.match_type(&TokenType::Keyword)
                || self.match_type(&TokenType::Identifier))
                && self.current_token().value == "type"
            {
                self.parse_impl_assoc_type();
                continue;
            }

            if self.match_type(&TokenType::Keyword) {
                let kw = self.current_token().value.clone();
                match kw.as_str() {
                    "func" => {
                        if let Some(func) = self.parse_method("func", method_attrs.clone()) {
                            items.push(ImplItem::Method(Box::new(func)));
                        }
                    }
                    "convert" => {
                        if let Some(func) = self.parse_method("convert", method_attrs.clone()) {
                            items.push(ImplItem::Convert(Box::new(func)));
                        }
                    }
                    // `new` is a keyword but is a valid method name (New trait).
                    // Pass empty keyword so parse_method reads `new` itself as
                    // the method name (same path as the bare-identifier shorthand).
                    "new" => {
                        if let Some(func) = self.parse_method("", method_attrs.clone()) {
                            items.push(ImplItem::Method(Box::new(func)));
                        }
                    }
                    _ => { self.advance(); }
                }
            } else if self.match_type(&TokenType::Identifier) {
                // Method shorthand: name(params): type { body }
                if let Some(func) = self.parse_method("", method_attrs) {
                    items.push(ImplItem::Method(Box::new(func)));
                }
            } else {
                break;
            }
            self.consume_end_of_line();
        }
        
        self.consume_value("}", "Expected '}' after impl block");
        self.consume_end_of_line();

        let mut block = ImplBlock::new(struct_name, generic_params, items).with_attributes(attrs);
        if let Some(tn) = trait_name {
            block = block.with_trait(tn);
        }
        Some(Box::new(block))
    }

    fn parse_trait_definition(&mut self, attrs: Vec<Attribute>) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'trait'

        let name = match self.parse_qualified_name() {
            Some(n) => n,
            None => {
                self.log_error("Expected trait name");
                return None;
            }
        };

        let mut generic_params = Vec::new();
        if self.match_value("<") {
            self.advance();
            loop {
                if !self.match_type(&TokenType::Identifier) { break; }
                generic_params.push(self.current_token().value.clone());
                self.advance();
                if self.match_value(",") { self.advance(); } else { break; }
            }
            if !self.match_value(">") { self.log_error("Expected '>'"); }
            else { self.advance(); }
        }

        self.consume_end_of_line();
        self.consume_value("{", "Expected '{' at start of trait body");
        self.consume_end_of_line();

        let mut methods = Vec::new();
        while !self.match_value("}") && !self.error_occurred {
            self.consume_end_of_line();
            if self.match_value("}") { break; }

            // Parse attributes on trait methods (e.g., #[dynamic_args])
            let method_raw = self.parse_attributes();
            let method_attrs = self.merge_with_file_attributes(method_raw);

            // Parse method signature: func name(params): ret_type
            // Associated type declaration inside a trait body:
            //   type Name;
            //   type Name = SomeType;   (default binding)
            // These are recorded but not deeply validated by the semantic
            // analyser — they exist so trait/impl blocks that use associated
            // types (e.g. `Iterator::Value`) parse cleanly.
            if (self.match_type(&TokenType::Keyword)
                || self.match_type(&TokenType::Identifier))
                && self.current_token().value == "type"
            {
                self.parse_trait_assoc_type();
                continue;
            }

            // func keyword is optional in trait defs
            if self.match_type(&TokenType::Keyword) && self.current_token().value == "func" {
                self.advance(); // consume 'func'
            }

            // Method names may be keywords that double as identifiers in
            // this language — most notably `new` (the New trait constructor).
            if !self.is_name_token() {
                self.log_error("Expected method name in trait definition");
                break;
            }
            let method_name = self.current_token().value.clone();
            self.advance();

            self.consume_value("(", "Expected '(' for trait method parameters");

            let params = self.parse_parameter_list();

            self.consume_value(")", "Expected ')' after parameters");

            let mut return_type = None;
            if self.match_value(":") {
                self.advance();
                return_type = self.parse_type();
            }

            methods.push(TraitMethod {
                name: method_name,
                parameters: params.unwrap_or_default(),
                return_type,
                attributes: method_attrs,
            });

            self.consume_end_of_line();
        }

        self.consume_value("}", "Expected '}' after trait body");

        Some(Box::new(TraitDefinition::new(name, methods, generic_params).with_attributes(attrs)))
    }

    fn parse_enum_definition(&mut self, attrs: Vec<Attribute>) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'enum'

        let name = match self.parse_qualified_name() {
            Some(n) => n,
            None => {
                self.log_error("Expected enum name");
                return None;
            }
        };

        let mut generic_params = Vec::new();
        if self.match_value("<") {
            self.advance();
            loop {
                if !self.match_type(&TokenType::Identifier) { break; }
                generic_params.push(self.current_token().value.clone());
                self.advance();
                if self.match_value(",") { self.advance(); } else { break; }
            }
            if !self.match_value(">") { self.log_error("Expected '>' after generic params"); }
            else { self.advance(); }
        }

        self.consume_end_of_line();
        self.consume_value("{", "Expected '{' at start of enum body");
        self.consume_end_of_line();

        let mut variants = Vec::new();
        while !self.match_value("}") && !self.error_occurred {
            self.consume_end_of_line();
            if self.match_value("}") { break; }

            if !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected variant name in enum definition");
                break;
            }
            let variant_name = self.current_token().value.clone();
            self.advance();

            // Optional payload: VariantName(Type)
            let payload_type = if self.match_value("(") {
                self.advance();
                let ty = self.parse_type();
                self.consume_value(")", "Expected ')' after variant payload type");
                ty
            } else {
                None
            };

            variants.push(EnumVariant::new(variant_name, payload_type));

            if self.match_value(",") { self.advance(); }
            self.consume_end_of_line();
        }

        self.consume_value("}", "Expected '}' after enum body");

        Some(Box::new(EnumDefinition::new(name, variants, generic_params).with_attributes(attrs)))
    }

    fn parse_extern_block(&mut self, attrs: Vec<Attribute>) -> Option<Box<dyn Statement>> {
        self.advance();

        let library_name = if self.match_type(&TokenType::String) {
            let name = self.current_token().value.clone();
            self.advance();
            Some(name)
        } else {
            self.log_error("Expected library name string after 'extern'");
            None
        };

        self.consume_value("{", "Expected '{' after extern library name");
        self.consume_end_of_line();

        let mut functions = Vec::new();

        while !self.match_value("}") && !self.match_type(&TokenType::EndOfFile) {
            self.consume_end_of_line();
            if self.match_value("}") {
                break;
            }

            // Parse attributes on extern functions (e.g., #[intrinsic("c_printf")])
            let func_raw = self.parse_attributes();
            let func_attrs = self.merge_with_file_attributes(func_raw);
            if !(self.match_type(&TokenType::Keyword) && self.current_token().value == "func") {
                self.log_error("Expected 'func' keyword in extern block");
                break;
            }
            self.advance();

            if !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected function name after 'func'");
                break;
            }
            let func_name = self.current_token().value.clone();
            self.advance();

            self.consume_value("(", "Expected '(' for extern function parameters");

            let mut params: Vec<Box<Parameter>> = Vec::new();
            let mut is_variadic = false;

            if !self.match_value(")") {
                loop {
                    if self.match_value("...") {
                        is_variadic = true;
                        self.advance();
                        break;
                    }

                    let param = self.parse_parameter();
                    if let Some(p) = param {
                        params.push(Box::new(p));
                    }

                    if self.match_value(",") {
                        self.advance();
                        if self.match_value("...") {
                            is_variadic = true;
                            self.advance();
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            self.consume_value(")", "Expected ')' after extern function parameters");

            let mut return_type = None;
            if self.match_value(":") {
                self.advance();
                return_type = self.parse_type();
            }

            if self.match_value(";") {
                self.advance();
            }
            self.consume_end_of_line();

            functions.push(ExternFunc::new(func_name, params, return_type, is_variadic).with_attributes(func_attrs));
        }

        self.consume_value("}", "Expected '}' after extern block");
        self.consume_end_of_line();

        let block = ExternBlock::new(library_name, functions).with_attributes(attrs);
        Some(Box::new(block))
    }

    fn parse_method(&mut self, keyword: &str, attrs: Vec<Attribute>) -> Option<Function> {
        if !keyword.is_empty() {
            self.advance(); // consume keyword (func/convert)
        }

        let method_name = if keyword == "convert" {
            // Parse target type as name
            let target = self.parse_type()?;
            let name = target.get_name().to_string();
            format!("convert_{}", name)
        } else {
            let name = self.current_token().value.clone();
            self.advance();
            name
        };

        // Method-level generic params: `func map<U>(self, f: ...)` — the `<U>`
        // sits between the method name and its parameter list.
        let mut generic_params = Vec::new();
        if self.match_value("<") {
            self.advance();
            while !self.match_value(">") && !self.error_occurred {
                if self.is_name_token() {
                    generic_params.push(self.current_token().value.clone());
                    self.advance();
                    if self.match_value(",") { self.advance(); }
                } else if self.match_value("<") || self.match_value(">") {
                    if self.match_value(">") { break; }
                    self.advance();
                } else {
                    break;
                }
            }
            self.consume_value(">", "Expected '>' closing method generic params");
        }

        self.consume_value("(", "Expected '(' for method parameters");

        let params = self.parse_parameter_list();

        self.consume_value(")", "Expected ')' after parameters");

        let mut return_type = None;
        if self.match_value(":") {
            self.advance();
            if self.match_value("(") {
                // Tuple return type - skip for now
                self.advance();
                return_type = self.parse_type();
                while self.match_value(",") {
                    self.advance();
                    self.parse_type();
                }
                self.consume_value(")", "Expected ')' closing tuple type");
            } else {
                return_type = self.parse_type();
            }
        }

        // If { follows, parse body; otherwise just declaration
        let body = if self.match_value("{") {
            self.advance();
            self.consume_end_of_line();
            let b = self.parse_block();
            self.consume_value("}", "Expected '}' at end of method body");
            b
        } else {
            None
        };

        let func = Function::new(method_name, params, return_type, body)
            .with_attributes(attrs)
            .with_generic_params(generic_params);
        Some(func)
    }

    fn parse_function(&mut self, attrs: Vec<Attribute>) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'func'

        // Function names may be keywords that double as identifiers in this
        // language — most notably `new`. See is_name_token.
        if !self.is_name_token() {
            self.log_error("Expected function name");
            return None;
        }

        let func_name = self.current_token().value.clone();
        self.advance();

        // Handle <T> generic params on functions
        let mut generic_params = Vec::new();
        if self.match_value("<") {
            self.advance();
            while !self.match_value(">") && !self.error_occurred {
                if self.match_type(&TokenType::Identifier) {
                    generic_params.push(self.current_token().value.clone());
                    self.advance();
                    if self.match_value(",") { self.advance(); }
                } else if self.match_value("<") || self.match_value(">") {
                    // Nested generics: skip token but break if it's the closing >
                    if self.match_value(">") { break; }
                    self.advance();
                } else {
                    break;
                }
            }
            self.consume_value(">", "Expected '>' closing generic params");
        }

        self.consume_value("(", "Expected '(' after function name");

        let params = self.parse_parameter_list();

        self.consume_value(")", "Expected ')' after parameters");

        let mut return_type = None;
        if self.match_value(":") {
            self.advance();
            return_type = self.parse_type();
        }

        let body = if self.match_value("{") {
            self.advance();
            self.consume_end_of_line();
            let b = self.parse_block();
            self.consume_value("}", "Expected '}' at end of function body");
            self.consume_end_of_line();
            b
        } else {
            self.consume_end_of_line();
            None
        };

        let func = Function::new(func_name, params, return_type, body)
            .with_generic_params(generic_params)
            .with_attributes(attrs);
        Some(Box::new(func))
    }

    fn parse_parameter_list(&mut self) -> Option<Vec<Box<Parameter>>> {
        let mut params: Vec<Box<Parameter>> = Vec::new();

        if !self.match_value(")") {
            loop {
                let param = self.parse_parameter();
                if let Some(p) = param {
                    params.push(Box::new(p));
                }

                if self.match_value(",") {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        Some(params)
    }

    fn parse_parameter(&mut self) -> Option<Parameter> {
        if !self.is_name_token()
            && !(self.match_type(&TokenType::Keyword) && self.current_token().value == "self")
        {
            self.log_error("Expected parameter name");
            return None;
        }

        let param_name = self.current_token().value.clone();
        self.advance();

        let mut param_type = None;
        if self.match_value(":") {
            self.advance();
            param_type = self.parse_type();
        }

        Some(Parameter::new(param_name, param_type))
    }

    /// Parse a lambda expression: `lambda<GenericParams>(params): RetType { body }`
    fn parse_lambda(&mut self) -> Option<Box<dyn Expression>> {
        self.advance(); // consume 'lambda'

        // Generic params <T, U>
        let mut generic_params = Vec::new();
        if self.match_value("<") {
            self.advance();
            while !self.match_value(">") && !self.error_occurred {
                if self.match_type(&TokenType::Identifier) {
                    generic_params.push(self.current_token().value.clone());
                    self.advance();
                    if self.match_value(",") {
                        self.advance();
                    }
                } else {
                    break;
                }
            }
            self.consume_value(">", "Expected '>' closing lambda generic params");
        }

        // Parameters (params)
        self.consume_value("(", "Expected '(' after lambda");
        let params = self.parse_parameter_list();
        self.consume_value(")", "Expected ')' after lambda parameters");

        // Return type : RetType
        let mut return_type = None;
        if self.match_value(":") {
            self.advance();
            return_type = self.parse_type();
        }

        // Body { body }
        let body = if self.match_value("{") {
            self.advance();
            self.consume_end_of_line();
            let b = self.parse_block();
            self.consume_value("}", "Expected '}' at end of lambda body");
            self.consume_end_of_line();
            b
        } else {
            self.log_error("Expected '{' for lambda body");
            return None;
        };

        let body = match body {
            Some(b) => b,
            None => return None,
        };

        let lambda = Lambda::new(params, return_type, body).with_generic_params(generic_params);
        Some(Box::new(lambda))
    }

    fn parse_type(&mut self) -> Option<Box<dyn Type>> {
        // Function type: `func(T1, T2): R` or `func(): R` or `func(T1, T2)`.
        // Used as the type of callback parameters (e.g. `f: func(T): U`).
        if self.match_type(&TokenType::Keyword) && self.current_token().value == "func" {
            return self.parse_function_type();
        }

        // Byte pointer type: `*byte` (a pointer into a byte buffer). Lowered to
        // the same representation as `str` (a GC'd byte buffer) by the IR.
        if self.match_value("*") {
            self.advance(); // consume '*'
            if !self.match_type(&TokenType::Keyword) && !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected type name after '*'");
                return None;
            }
            let pointee = self.parse_type()?;
            return Some(Box::new(PointerType::new(pointee)));
        }

        // Byte array type: `[byte]` (an unsized array of bytes). Lowered to the
        // same representation as `str` by the IR.
        if self.match_value("[") {
            self.advance(); // consume '['
        if self.match_value("(") {
            self.advance();
            let mut args = Vec::new();
            while !self.match_value(")") && !self.error_occurred {
                args.push(self.parse_type()?);
                if self.match_value(",") {
                    self.advance();
                } else {
                    break;
                }
            }
            self.consume_value(")", "Expected ')' after tuple type");
            self.consume_value("]", "Expected ']' after tuple type");
            return Some(Box::new(GenericType::new("tuple", args)));
        }
        if !self.match_type(&TokenType::Keyword) && !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected type name after '['");
                return None;
            }
            let name = self.current_token().value.clone();
            self.advance();
            self.consume_value("]", "Expected ']' after array element type");
            return Some(Box::new(ArrayType::new_nested(
                Box::new(BasicType::new(name)),
                Box::new(NumberLiteral::new(0.0)),
            )));
        }

        if !self.match_type(&TokenType::Keyword) && !self.match_type(&TokenType::Identifier) {
            // Unit type: `()` — maps to None_ (the null/void type).
            if self.match_value("(") && self.peek_next_token().value == ")" {
                self.advance(); // '('
                self.advance(); // ')'
                return Some(Box::new(BasicType::new("()")));
            }
            self.log_error("Expected type name");
            return None;
        }

        let mut type_name = self.current_token().value.clone();
        self.advance();

        // Handle :: namespace paths in types: std::int
        while self.match_value("::") {
            self.advance(); // consume '::'
            if !self.match_type(&TokenType::Keyword) && !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected type name after '::'");
                return None;
            }
            type_name.push_str("::");
            type_name.push_str(&self.current_token().value);
            self.advance();
        }

        // Parse generic type args: vec<int> or map<str,int>
        let mut type_args: Vec<Box<dyn Type>> = Vec::new();
        if self.match_value("<") {
            self.advance();
            while !self.match_value(">") && !self.error_occurred {
                let arg = self.parse_type()?;
                type_args.push(arg);
                if self.match_value(",") {
                    self.advance();
                } else {
                    break;
                }
            }
            self.consume_value(">", "Expected '>' closing generic type");
        }

        let mut tp: Box<dyn Type> = if !type_args.is_empty() {
            Box::new(GenericType::new(type_name.clone(), type_args))
        } else {
            Box::new(BasicType::new(type_name))
        };

        while self.match_value("[") {
            self.advance();
            if self.match_value("]") {
                // Empty brackets: unsized array (e.g., int[])
                self.advance();
                tp = Box::new(ArrayType::new_nested(tp, Box::new(NumberLiteral::new(0.0))));
            } else {
                let size = self.parse_expression()?;
                if !self.match_value("]") {
                    self.log_error("Expected ']' after array size");
                    return None;
                }
                self.advance();
                tp = Box::new(ArrayType::new_nested(tp, size));
            }
        }

        if self.match_value("?") {
            self.advance();
            tp = Box::new(NullableType::new(tp));
        }

        Some(tp)
    }

    /// Parse a function type `func(T1, T2): R` (return type optional).
    /// The current token is the `func` keyword on entry.
    fn parse_function_type(&mut self) -> Option<Box<dyn Type>> {
        self.advance(); // consume 'func'

        self.consume_value("(", "Expected '(' after 'func' in function type");

        let mut param_types: Vec<Box<dyn Type>> = Vec::new();
        if !self.match_value(")") {
            loop {
                let arg = self.parse_type()?;
                param_types.push(arg);
                if self.match_value(",") {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        self.consume_value(")", "Expected ')' closing function type parameters");

        let return_type = if self.match_value(":") {
            self.advance();
            self.parse_type()
        } else {
            None
        };

        Some(Box::new(FunctionType::new(param_types, return_type)))
    }

    fn parse_block(&mut self) -> Option<Box<Block>> {
        let mut block = Block::new();

        while self.match_type(&TokenType::EndOfFile) {
            self.consume_end_of_line();
        }

        while !self.match_value("}")
            && !self.match_type(&TokenType::EndOfFile)
            && !self.error_occurred
        {
            self.consume_end_of_line();

            if self.match_value("}") {
                break;
            }

            let stmt = self.parse_statement();
            if let Some(s) = stmt {
                block.add_statement(s);
            }

            self.consume_end_of_line();
        }

        Some(Box::new(block))
    }

    fn parse_array_type(&mut self, element_type_name: &str) -> Option<Box<dyn Type>> {
        let first_size = self.parse_expression()?;

        if !self.match_value("]") {
            self.log_error("Expected ']' after array size");
            return None;
        }
        self.advance(); // consume ']'

        let mut current_type: Box<dyn Type> =
            Box::new(ArrayType::new_basic(element_type_name, first_size));

        while self.match_value("[") {
            self.advance(); // consume '['

            let next_size = self.parse_expression()?;

            if !self.match_value("]") {
                self.log_error("Expected ']' after array size");
                return None;
            }
            self.advance(); // consume ']'

            current_type = Box::new(ArrayType::new_nested(current_type, next_size));
        }

        Some(current_type)
    }

    fn parse_declaration(&mut self) -> Option<Box<dyn Statement>> {
        let keyword = self.current_token().value.clone();
        self.advance();

        if !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected identifier in declaration");
            return None;
        }

        let var_name = self.current_token().value.clone();
        self.advance();

        let mut var_type = None;
        if self.match_value(":") {
            self.advance();
            var_type = self.parse_type();
        }

        let mut initializer = None;
        if self.match_value("=") {
            self.advance();
            initializer = self.parse_expression();
        }

        self.consume_end_of_line();

        Some(Box::new(Declaration::new(keyword, var_name, var_type, initializer)))
    }

    /// Desugar `a[i] = v` → `a.index_mut(i).write(v)`.
    ///
    /// Runs in the AST builder (front-end) immediately after parsing, so
    /// both the semantic analyser and the Cranelift backend see the
    /// desugared form. This is logically equivalent to what SemanticAnalyzer
    /// would do in an AST-transform pass, but keeps the rewriting close to
    /// the parser where the rest of the syntactic sugar (e.g. `a..b` →
    /// `Range::new(a, b)`) already lives.
    fn desugar_index_assign(&mut self, expr: Box<dyn Expression>) -> Box<dyn Expression> {
        if let Some(bin) = expr.as_any().downcast_ref::<BinaryExpression>() {
            if bin.get_operator() == "=" {
                if let Some(left) = bin.get_left() {
                    if let Some(arr_idx) = left.as_any().downcast_ref::<ArrayIndex>() {
                        if let (Some(arr), Some(idx), Some(val)) =
                            (arr_idx.get_array(), arr_idx.get_index(), bin.get_right())
                        {
                            // Rebuild owned clones of the three expressions.
                            // They are trait objects; clone them by walking back
                            // through the parser is impossible, so instead we
                            // rebuild a simple chain using the raw AST nodes we
                            // can inspect. For that we need a "deep-ish clone"
                            // helper that re-boxes common shapes.
                            fn clone_type(t: &dyn Type) -> Box<dyn Type> {
                                use crate::ast::*;
                                let any = t.as_type_any();
                                if let Some(bt) = any.downcast_ref::<BasicType>() {
                                    return Box::new(BasicType::new(bt.get_name()));
                                }
                                if let Some(at) = any.downcast_ref::<ArrayType>() {
                                    let elem = clone_type(at.get_element_type());
                                    if let Some(size_expr) = at.get_size() {
                                        return Box::new(ArrayType::new_nested(
                                            elem,
                                            clone_expr(size_expr),
                                        ));
                                    }
                                    return Box::new(ArrayType::new_basic(
                                        &elem.get_name().to_string(),
                                        Box::new(NumberLiteral::new(0.0)),
                                    ));
                                }
                                if let Some(nt) = any.downcast_ref::<NullableType>() {
                                    return Box::new(NullableType::new(clone_type(nt.get_inner_type())));
                                }
                                if let Some(ft) = any.downcast_ref::<FunctionType>() {
                                    let params: Vec<Box<dyn Type>> = ft
                                        .get_param_types()
                                        .iter()
                                        .map(|t| clone_type(*t))
                                        .collect();
                                    let ret = ft.get_return_type().map(|t| clone_type(t));
                                    return Box::new(FunctionType::new(params, ret));
                                }
                                if let Some(gt) = any.downcast_ref::<GenericType>() {
                                    let args: Vec<Box<dyn Type>> = gt
                                        .get_type_args()
                                        .iter()
                                        .map(|a| clone_type(a.as_ref()))
                                        .collect();
                                    return Box::new(GenericType::new(gt.get_base_name(), args));
                                }
                                Box::new(BasicType::new(t.get_name()))
                            }
                            fn clone_expr(e: &dyn Expression) -> Box<dyn Expression> {
                                use crate::ast::*;
                                let any = e.as_any();
                                if let Some(id) = any.downcast_ref::<Identifier>() {
                                    return Box::new(Identifier::new(id.get_name()));
                                }
                                if let Some(n) = any.downcast_ref::<NumberLiteral>() {
                                    return Box::new(NumberLiteral::new(n.get_value()));
                                }
                                if let Some(s) = any.downcast_ref::<StringLiteral>() {
                                    return Box::new(StringLiteral::new(s.get_value()));
                                }
                                if let Some(b) = any.downcast_ref::<BooleanLiteral>() {
                                    return Box::new(BooleanLiteral::new(b.get_value()));
                                }
                                if let Some(_nl) = any.downcast_ref::<NullLiteral>() {
                                    return Box::new(NullLiteral::new());
                                }
                                if let Some(fs) = any.downcast_ref::<FormatString>() {
                                    // FormatString::new re-parses the value to
                                    // rebuild variable positions, preserving
                                    // brace-escapes such as {{ -> { and }} -> }.
                                    return Box::new(FormatString::new(fs.get_value()));
                                }
                                if let Some(be) = any.downcast_ref::<BinaryExpression>() {
                                    let l = be.get_left().map(clone_expr);
                                    let r = be.get_right().map(clone_expr);
                                    return Box::new(BinaryExpression::new(l, be.get_operator(), r));
                                }
                                if let Some(ue) = any.downcast_ref::<UnaryExpression>() {
                                    let op = ue.get_operator();
                                    let operand = ue.get_operand().map(clone_expr);
                                    return Box::new(UnaryExpression::new(op, operand));
                                }
                                if let Some(ce) = any.downcast_ref::<CastExpression>() {
                                    let inner = ce.get_expression().map(clone_expr);
                                    let tgt = clone_type(ce.get_target_type());
                                    return Box::new(CastExpression::new(inner, tgt));
                                }
                                if let Some(re) = any.downcast_ref::<RangeExpression>() {
                                    let args: Vec<Box<dyn Expression>> = re
                                        .get_arguments()
                                        .iter()
                                        .map(|a| clone_expr(a.as_ref()))
                                        .collect();
                                    return Box::new(RangeExpression::new(args));
                                }
                                if let Some(al) = any.downcast_ref::<ArrayLiteral>() {
                                    let elems: Vec<Box<dyn Expression>> = al
                                        .get_elements()
                                        .iter()
                                        .map(|a| clone_expr(a.as_ref()))
                                        .collect();
                                    return Box::new(ArrayLiteral::new(elems));
                                }
                                if let Some(sl) = any.downcast_ref::<StructLiteral>() {
                                    let fields: Vec<StructFieldInit> = sl
                                        .get_fields()
                                        .iter()
                                        .map(|f| match f {
                                            StructFieldInit::Named { name, value } => {
                                                StructFieldInit::Named {
                                                    name: name.clone(),
                                                    value: clone_expr(value.as_ref()),
                                                }
                                            }
                                            StructFieldInit::Positional(v) => {
                                                StructFieldInit::Positional(clone_expr(v.as_ref()))
                                            }
                                        })
                                        .collect();
                                    return Box::new(StructLiteral::new(sl.get_type_name(), fields));
                                }
                                if let Some(ma) = any.downcast_ref::<MemberAccess>() {
                                    let obj = ma.get_object().map(clone_expr);
                                    return Box::new(MemberAccess::new(obj, ma.get_member()));
                                }
                                if let Some(ai) = any.downcast_ref::<ArrayIndex>() {
                                    let arr = ai.get_array().map(clone_expr);
                                    let idx = ai.get_index().map(clone_expr);
                                    return Box::new(ArrayIndex::new(arr, idx));
                                }
                                if let Some(fc) = any.downcast_ref::<FunctionCall>() {
                                    let callee = fc.get_callee().map(clone_expr);
                                    let args: Option<Vec<Box<dyn Expression>>> =
                                        fc.get_arguments().map(|v| v.iter().map(|a| clone_expr(a.as_ref())).collect());
                                    return Box::new(FunctionCall::new(callee, args));
                                }
                                if let Some(pa) = any.downcast_ref::<PathAccess>() {
                                    return Box::new(PathAccess::new(
                                        pa.get_path().to_vec(),
                                        pa.get_member().to_string(),
                                    ));
                                }
                                Box::new(Identifier::new("_desugar_fallback"))
                            }
                            let arr_boxed = clone_expr(arr);
                            let idx_boxed = clone_expr(idx);
                            let val_boxed = clone_expr(val);
                            // `arr.index_mut(i)`
                            let index_mut_callee = Box::new(MemberAccess::new(
                                Some(arr_boxed),
                                "index_mut",
                            ));
                            let index_mut_call = Box::new(FunctionCall::new(
                                Some(index_mut_callee),
                                Some(vec![idx_boxed]),
                            ));
                            // `.write(v)`
                            let write_callee = Box::new(MemberAccess::new(
                                Some(index_mut_call),
                                "write",
                            ));
                            return Box::new(FunctionCall::new(
                                Some(write_callee),
                                Some(vec![val_boxed]),
                            ));
                        }
                    }
                }
            }
        }
        expr
    }

    fn parse_expression_statement(&mut self) -> Option<Box<dyn Statement>> {
        let expr = self.parse_expression()?;
        // Note: `arr[i] = v` array-index assignment is NOT desugared in the
        // AST builder. The IR builder preserves ArrayIndex-as-assignment-target
        // shape (`IRExpr::Assignment { target: IRExpr::ArrayIndex { .. } }`),
        // which lets Cranelift dispatch correctly:
        //   * For raw arrays (int[], str[]): gobol_array_elem_addr + gobol_mem_store
        //   * For structs with index_mut (e.g. vec<T>): method call chain
        //     index_mut(i).write(v) with proper struct-type dispatch.
        // Check for explicit semicolon terminator
        let has_semi = self.is_semicolon();
        if has_semi {
            self.advance(); // consume ';'
        }
        self.consume_end_of_line();
        // A bare expression (no semicolon) is a tail expression — the block's
        // implicit return value — ONLY when it is the last statement before
        // the closing '}'. Marking every non-semicolon expression as a tail
        // (return) emits a terminator per expression; after the first one the
        // current Cranelift block is "already filled", so the next statement
        // panics ("you cannot add an instruction to a block already filled").
        let is_last_in_block = self.match_value("}");
        if !has_semi && is_last_in_block {
            Some(Box::new(ExpressionStatement::new_tail(Some(expr))))
        } else {
            Some(Box::new(ExpressionStatement::new(Some(expr))))
        }
    }

    fn parse_return_statement(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'return'

        let mut value = None;
        if !self.is_end_of_line() && !self.match_value("}") {
            value = self.parse_expression();
        }

        self.consume_end_of_line();

        Some(Box::new(ReturnStatement::new(value)))
    }

    fn parse_for_statement(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'for'

        if !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected identifier in for loop");
            return None;
        }

        let mut loop_vars = vec![self.current_token().value.clone()];
        self.advance();

        // Support `for i, v in expr` syntax
        if self.match_value(",") {
            self.advance();
            if !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected second identifier after ',' in for loop");
                return None;
            }
            loop_vars.push(self.current_token().value.clone());
            self.advance();
        }

        if !(self.match_type(&TokenType::Keyword) && self.current_token().value == "in") {
            self.log_error("Expected 'in' in for loop");
            return None;
        }
        self.advance();

        let range_expr = self.parse_range_or_iterable()?;

        self.consume_value("{", "Expected '{' at start of loop body");
        self.consume_end_of_line();

        let body = self.parse_block();

        self.consume_value("}", "Expected '}' at end of loop body");
        self.consume_end_of_line();

        Some(Box::new(ForStatement::new_multi(loop_vars, Some(range_expr), body)))
    }

    fn parse_range_or_iterable(&mut self) -> Option<Box<dyn Expression>> {
        let start = self.parse_expression()?;

        if self.match_value("..") {
            self.advance(); // consume first '..'
            let end = self.parse_expression()?;

            // Check for explicit step: 0..10..2
            let step = if self.match_value("..") {
                self.advance(); // consume second '..'
                Some(self.parse_expression()?)
            } else {
                // Auto-detect step: if start > end, step = -1; else step = 1
                if let (Some(start_val), Some(end_val)) = (as_number(&start), as_number(&end)) {
                    let step_val = if start_val > end_val { -1.0 } else { 1.0 };
                    Some(Box::new(NumberLiteral::new(step_val)) as Box<dyn Expression>)
                } else {
                    // Runtime step detection by range::new
                    None
                }
            };

            // Desugar 0..10 → Range::new(0, 10[, step])
            let range_new = Box::new(PathAccess::new(vec!["Range".to_string()], "new"));
            let mut args = vec![start, end];
            if let Some(s) = step {
                args.push(s);
            }

            return Some(Box::new(FunctionCall::new(Some(range_new), Some(args))));
        }

        Some(start)
    }

    fn parse_format_string(&self, format_str: &str) -> Option<Box<dyn Expression>> {
        Some(Box::new(FormatString::new(format_str)))
    }

    // ==================== Expression parsing ====================

    fn parse_expression(&mut self) -> Option<Box<dyn Expression>> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_logical_or()?;

        if self.match_value("=")
            || self.match_value("+=")
            || self.match_value("-=")
            || self.match_value("*=")
            || self.match_value("/=")
        {
            let op = self.current_token().value.clone();
            self.advance();
            let value = self.parse_assignment();
            expr = Box::new(BinaryExpression::new(Some(expr), op, value));
        }

        Some(expr)
    }

    fn parse_logical_or(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_logical_and()?;

        while self.match_value("||") {
            let op = self.current_token().value.clone();
            self.advance();
            let right = self.parse_logical_and()?;
            expr = Box::new(BinaryExpression::new(Some(expr), op, Some(right)));
        }

        Some(expr)
    }

    fn parse_logical_and(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_equality()?;

        while self.match_value("&&") {
            let op = self.current_token().value.clone();
            self.advance();
            let right = self.parse_equality()?;
            expr = Box::new(BinaryExpression::new(Some(expr), op, Some(right)));
        }

        Some(expr)
    }

    fn parse_equality(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_comparison()?;

        while self.match_value("==") || self.match_value("!=") {
            let op = self.current_token().value.clone();
            self.advance();
            let right = self.parse_comparison()?;
            expr = Box::new(BinaryExpression::new(Some(expr), op, Some(right)));
        }

        Some(expr)
    }

    fn parse_comparison(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_additive()?;

        while self.match_value("<")
            || self.match_value("<=")
            || self.match_value(">")
            || self.match_value(">=")
        {
            let op = self.current_token().value.clone();
            self.advance();
            let right = self.parse_additive()?;
            expr = Box::new(BinaryExpression::new(Some(expr), op, Some(right)));
        }

        Some(expr)
    }

    fn parse_additive(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_multiplicative()?;

        while self.match_value("+") || self.match_value("-") {
            let op = self.current_token().value.clone();
            self.advance();
            let right = self.parse_multiplicative()?;
            expr = Box::new(BinaryExpression::new(Some(expr), op, Some(right)));
        }

        Some(expr)
    }

    fn parse_multiplicative(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_cast()?;

        while self.match_value("*") || self.match_value("/") || self.match_value("%") {
            let op = self.current_token().value.clone();
            self.advance();
            let right = self.parse_cast()?;
            expr = Box::new(BinaryExpression::new(Some(expr), op, Some(right)));
        }

        Some(expr)
    }

    fn parse_cast(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_unary()?;

        while self.match_type(&TokenType::Keyword) && self.current_token().value == "as" {
            self.advance(); // consume 'as'
            let target_type = self.parse_type()?;
            expr = Box::new(CastExpression::new(Some(expr), target_type));
        }

        Some(expr)
    }

    fn parse_unary(&mut self) -> Option<Box<dyn Expression>> {
        if self.match_value("!") || self.match_value("-") || self.match_value("+") || self.match_value("&") {
            let op = self.current_token().value.clone();
            self.advance();
            let operand = self.parse_unary()?;
            return Some(Box::new(UnaryExpression::new(op, Some(operand))));
        }

        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Option<Box<dyn Expression>> {
        let mut expr = self.parse_primary()?;

        loop {
            if self.match_value(".") {
                self.advance();
                if !self.match_type(&TokenType::Identifier)
                    && !self.match_type(&TokenType::Keyword)
                    && !self.match_type(&TokenType::Number)
                {
                    self.log_error("Expected identifier after '.'");
                    return Some(expr);
                }
                let member = self.current_token().value.clone();
                self.advance();
                expr = Box::new(MemberAccess::new(Some(expr), member));
            } else if self.match_value("[") {
                self.advance();
                let index = self.parse_expression()?;
                if !self.match_value("]") {
                    self.log_error("Expected ']' after array index");
                    return Some(expr);
                }
                self.advance();
                expr = Box::new(ArrayIndex::new(Some(expr), Some(index)));
            } else if self.match_value("(") {
                expr = self.parse_function_call(expr)?;
            } else if self.match_value("?") {
                // Try operator: expr? desugars to early-return on Err.
                self.advance();
                expr = Box::new(TryOperator::new(Some(expr)));
            } else {
                break;
            }
        }

        Some(expr)
    }

    fn parse_primary(&mut self) -> Option<Box<dyn Expression>> {
        // Lambda expression: lambda<GenericParams>(params): RetType { body }
        if self.match_type(&TokenType::Keyword) && self.current_token().value == "lambda" {
            return self.parse_lambda();
        }

        // 标识符 (可能是变量名或结构体类型名，也可能是 :: 命名空间路径)
        if self.match_type(&TokenType::Identifier) {
            let name = self.current_token().value.clone();
            self.advance();

            // Lookahead: 判断当前标识符后是否紧跟泛型参数 <T> 或 <T, U>
            // 如果是，则进入泛型解析路径；否则保留 < 给比较运算符处理
            if self.match_value("<") {
                let is_generic = self.lookahead(|parser| {
                    parser.advance(); // 跳过 '<'
                    // 至少一个类型参数（Identifier 或类型关键字如 int/float/str/bool）
                    if !parser.match_type(&TokenType::Identifier)
                        && !(parser.match_type(&TokenType::Keyword)
                            && matches!(
                                parser.current_token().value.as_str(),
                                "int" | "float" | "str" | "bool"
                            ))
                    {
                        return false;
                    }
                    parser.advance();
                    // 后续可有逗号分隔的更多类型参数
                    while parser.match_value(",") {
                        parser.advance();
                        if !parser.match_type(&TokenType::Identifier)
                            && !(parser.match_type(&TokenType::Keyword)
                                && matches!(
                                    parser.current_token().value.as_str(),
                                    "int" | "float" | "str" | "bool"
                                ))
                        {
                            return false;
                        }
                        parser.advance();
                    }
                    if !parser.match_value(">") {
                        return false;
                    }
                    parser.advance();
                    true
                });
                if is_generic {
                    return self.parse_generic_type_or_call(name);
                }
                // 不是泛型，< 留给后续比较运算符处理
            }

            // parse namespace: std::io::println
            if self.match_value("::") {
                self.advance(); // consume '::'
                return self.parse_path_access(name);
            }

            // parse structure: TypeName { ... }
            if self.match_value("{") && name.chars().next().map_or(false, |c| c.is_uppercase()) {
                return self.parse_struct_literal(Box::new(Identifier::new(name)));
            }

            return Some(Box::new(Identifier::new(name)));
        }

        // 数字字面量
        if self.match_type(&TokenType::Number) {
            let raw = self.current_token().value.clone();
            let value: f64 = raw.parse().unwrap_or(0.0);
            self.advance();
            // A literal is a float if the source text contains '.' or 'e'/'E'
            // (scientific notation). Otherwise treat as int.
            let is_float = raw.contains('.') || raw.contains('e') || raw.contains('E');
            if is_float {
                return Some(Box::new(NumberLiteral::new_float(value)));
            } else {
                return Some(Box::new(NumberLiteral::new(value)));
            }
        }

        // 字符串字面量
        if self.match_type(&TokenType::String) {
            let value = self.current_token().value.clone();
            self.advance();
            return Some(Box::new(StringLiteral::new(value)));
        }

        // 格式化字符串
        if self.match_type(&TokenType::FormatString) {
            let value = self.current_token().value.clone();
            self.advance();
            return self.parse_format_string(&value);
        }

        // 关键字字面量
        if self.match_type(&TokenType::Keyword) {
            let value = self.current_token().value.clone();
            match value.as_str() {
                "true" | "false" => {
                    self.advance();
                    return Some(Box::new(BooleanLiteral::new(value == "true")));
                }
                "null" => {
                    self.advance();
                    return Some(Box::new(NullLiteral::new()));
                }
                "self" => {
                    self.advance();
                    return Some(Box::new(Identifier::new("self")));
                }
                "if" => return self.parse_if_expression(),
                "match" => return self.parse_match_expression(),
                "new" => return self.parse_new_expression(),
                _ => {}
            }
        }

        // 数组字面量: [1, 2, 3]
        if self.match_value("[") {
            return self.parse_array_literal();
        }

        // 块表达式: { stmt1; stmt2; expr }
        if self.match_value("{") {
            self.advance(); // consume '{'
            self.consume_end_of_line();
            let block = self.parse_block();
            self.consume_value("}", "Expected '}' after block expression");
            return block.map(|b| b as Box<dyn Expression>);
        }

        // 括号表达式: (1 + 2)
        if self.match_value("(") {
            self.advance();
            let first = self.parse_expression()?;
            if self.match_value(",") {
                let mut elements = vec![first];
                while self.match_value(",") {
                    self.advance();
                    if self.match_value(")") {
                        break;
                    }
                    elements.push(self.parse_expression()?);
                }
                self.consume_value(")", "Expected ')' after tuple expression");
                return Some(Box::new(ArrayLiteral::new(elements)));
            }
            let expr = first;
            while self.match_value(",") {
                self.advance();
                self.parse_expression();
            }
            self.consume_value(")", "Expected ')' after expression");
            return Some(Box::new(GroupedExpression::new(Some(expr))));
        }

        self.log_error(&format!("Unexpected token in expression: {}", self.current_token().value));
        None
    }

    /// Parse a `::`-separated namespace path: `std::io::println`
    /// Called after the first identifier and `::` have been consumed.
    fn parse_path_access(&mut self, first: String) -> Option<Box<dyn Expression>> {
        let mut path = vec![first];
        loop {
            // Accept both Identifier and Keyword tokens in paths
            if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
                self.log_error("Expected identifier after '::'");
                return None;
            }
            let segment = self.current_token().value.clone();
            self.advance();

            if self.match_value("::") {
                self.advance(); // consume '::'
                path.push(segment);
            } else {
                return Some(Box::new(PathAccess::new(path, segment)));
            }
        }
    }

    fn parse_function_call(&mut self, callee: Box<dyn Expression>) -> Option<Box<dyn Expression>> {
        self.consume_value("(", "Expected '(' in function call");

        let args = self.parse_argument_list();

        self.consume_value(")", "Expected ')' after arguments");

        Some(Box::new(FunctionCall::new(Some(callee), args)))
    }

    fn parse_struct_literal(&mut self, type_expr: Box<dyn Expression>) -> Option<Box<dyn Expression>> {
        // Extract type name from the expression (must be an Identifier)
        let type_name = if let Some(id) = type_expr.as_any().downcast_ref::<Identifier>() {
            id.get_name().to_string()
        } else {
            self.log_error("Expected type name before '{'");
            return Some(type_expr);
        };

        self.advance(); // consume '{'

        let mut fields: Vec<StructFieldInit> = Vec::new();

        while !self.match_value("}") && !self.error_occurred {
            // Peek ahead: if we see `identifier :`, it's a named field
            if self.match_type(&TokenType::Identifier) && self.peek_next_token().value == ":" {
                let name = self.current_token().value.clone();
                self.advance(); // consume identifier
                self.advance(); // consume ':'
                let value = self.parse_expression()?;
                fields.push(StructFieldInit::Named { name, value });
            } else if !self.match_value("}") {
                // Positional field: bare expression (or spread identifier)
                let value = self.parse_expression()?;
                fields.push(StructFieldInit::Positional(value));
            }

            if self.match_value(",") {
                self.advance();
            } else {
                break;
            }
        }

        self.consume_value("}", "Expected '}' after struct literal");

        Some(Box::new(StructLiteral::new(type_name, fields)))
    }

    fn parse_argument_list(&mut self) -> Option<Vec<Box<dyn Expression>>> {
        let mut args: Vec<Box<dyn Expression>> = Vec::new();

        if !self.match_value(")") {
            loop {
                let arg = self.parse_expression();
                if let Some(a) = arg {
                    args.push(a);
                }

                if self.match_value(",") {
                    self.advance();
                } else {
                    break;
                }
            }
        }

        Some(args)
    }

    fn parse_array_literal(&mut self) -> Option<Box<dyn Expression>> {
        self.advance(); // consume '['
        self.consume_end_of_line();

        let mut elements: Vec<Box<dyn Expression>> = Vec::new();

        while !self.match_value("]") && !self.error_occurred {
            self.consume_end_of_line();
            if self.match_value("]") {
                break;
            }
            if self.match_value(",") {
                self.advance();
                self.consume_end_of_line();
                continue;
            }
            let elem = match self.parse_expression() {
                Some(e) => {
                    e
                }
                None => {
                    return None;
                }
            };
            elements.push(elem);

            if self.match_value(",") {
                self.advance();
                self.consume_end_of_line();
            } else {
                break;
            }
        }
        self.consume_value("]", "Expected ']' after array literal");
        Some(Box::new(ArrayLiteral::new(elements)))
    }

    fn parse_if_expression(&mut self) -> Option<Box<dyn Expression>> {
        self.advance(); // consume 'if'
        let condition = self.parse_expression()?;

        self.consume_value("{", "Expected '{' at start of if-expression branch");
        self.consume_end_of_line();

        let then_branch = self.parse_if_expr_branch();

        self.consume_value("}", "Expected '}' at end of if-expression branch");
        self.consume_end_of_line();

        let then_branch = then_branch?;

        let else_branch = if self.match_value("else") {
            self.advance();

            self.consume_value("{", "Expected '{' at start of else branch");
            self.consume_end_of_line();

            let else_block = self.parse_if_expr_branch();

            self.consume_value("}", "Expected '}' at end of else branch");
            self.consume_end_of_line();

            else_block.map(|b| b as Box<dyn Statement>)
        } else {
            None
        };

        Some(Box::new(IfStatement::new(
            Some(condition),
            Some(then_branch),
            else_branch,
        )))
    }

    fn parse_if_expr_branch(&mut self) -> Option<Box<dyn Statement>> {
        // Parse an expression and wrap it in a block
        let mut block = Block::new();
        self.consume_end_of_line();
        let stmt = self.parse_statement()?;
        block.add_statement(stmt);
        Some(Box::new(block))
    }

    /// Parse a match pattern (supports literals, wildcard, variables, and enum variants)
    /// 
    /// Examples:
    ///   - `_` → Wildcard
    ///   - `42` → Literal(Int(42))
    ///   - `"hello"` → Literal(Str("hello"))
    ///   - `x` → Variable("x")
    ///   - `None` → Variable("None") (语义分析时判断是否为枚举变体)
    ///   - `Some(x)` → EnumVariant { variant_name: "Some", payload: Some(Variable("x")) }
    ///   - `Ok(value)` → EnumVariant { variant_name: "Ok", payload: Some(Variable("value")) }
    ///   - `Err(e)` → EnumVariant { variant_name: "Err", payload: Some(Variable("e")) }
    ///   - `Some(Ok(x))` → EnumVariant { variant_name: "Some", payload: Some(EnumVariant { ... }) }
    fn parse_match_pattern(&mut self) -> Option<MatchPattern> {
        if self.match_value("_") {
            self.advance();
            return Some(MatchPattern::Wildcard);
        }

        if self.match_type(&TokenType::Number) {
            let val = self.current_token().value.clone();
            self.advance();
            if val.contains('.') {
                return Some(MatchPattern::Literal(RtValueSimple::FloatStr(val)));
            } else {
                return Some(MatchPattern::Literal(RtValueSimple::Int(val.parse().unwrap_or(0))));
            }
        }

        if self.match_type(&TokenType::String) {
            let val = self.current_token().value.clone();
            self.advance();
            return Some(MatchPattern::Literal(RtValueSimple::Str(val)));
        }

        if self.match_type(&TokenType::Keyword) && (self.current_token().value == "true" || self.current_token().value == "false") {
            let val = self.current_token().value == "true";
            self.advance();
            return Some(MatchPattern::Literal(RtValueSimple::Bool(val)));
        }

        if self.match_type(&TokenType::Identifier) {
            let name = self.current_token().value.clone();
            self.advance();

            // 枚举变体带负载: VariantName(payload)
            if self.match_value("(") {
                self.advance(); // consume '('

                // 解析负载模式（递归调用自己）
                let payload = self.parse_match_pattern()?;

                self.consume_value(")", "Expected ')' after enum variant payload");

                return Some(MatchPattern::EnumVariant {
                    enum_name: String::new(), // 语义分析时填充
                    variant_name: name,
                    variant_index: 0,
                    payload: Some(Box::new(payload)),
                });
            }

            // 普通变量绑定（语义分析时会判断是否为无负载枚举变体）
            return Some(MatchPattern::Variable(name));
        }

        self.log_error("Expected pattern in match arm");
        None
    }

    /// Parse a match expression
    fn parse_match_expression(&mut self) -> Option<Box<dyn Expression>> {
        self.advance(); // consume 'match'

        // Parse scrutinee expression
        let scrutinee = self.parse_expression()?;

        self.consume_value("{", "Expected '{' after match scrutinee");
        self.consume_end_of_line();

        let mut arms: Vec<MatchArm> = Vec::new();

        while !self.match_value("}") && !self.error_occurred {
            self.consume_end_of_line();
            if self.match_value("}") { break; }

            // ---- 使用提取出的 parse_match_pattern ----
            let pattern = match self.parse_match_pattern() {
                Some(p) => p,
                None => return None,
            };

            // Expect '=>' (tokenized as '=' then '>')
            self.consume_value("=", "Expected '=>' after match pattern");
            self.consume_value(">", "Expected '=>' after match pattern");

            // Parse arm body (single expression or block)
            let body: Option<Box<dyn Statement>> = if self.match_value("{") {
                self.advance();
                self.consume_end_of_line();
                let b = self.parse_block();
                self.consume_value("}", "Expected '}' after match arm block");
                b.map(|b| b as Box<dyn Statement>)
            } else if self.match_type(&TokenType::Keyword)
                && self.current_token().value == "return"
            {
                let stmt = self.parse_return_statement()?;
                let mut block = Block::new();
                block.add_statement(stmt);
                Some(Box::new(block))
            } else {
                let expr = self.parse_expression()?;
                let mut block = Block::new();
                block.add_statement(Box::new(ExpressionStatement::new(Some(expr))));
                Some(Box::new(block))
            };

            arms.push(MatchArm {
                pattern,
                body,
                attributes: Vec::new(),
            });

            // Optional comma between arms
            self.consume_end_of_line();
            if self.match_value(",") {
                self.advance();
            }
            self.consume_end_of_line();
        }

        self.consume_value("}", "Expected '}' after match body");

        Some(Box::new(MatchExpression::new(Some(scrutinee), arms)))
    }

    fn parse_new_expression(&mut self) -> Option<Box<dyn Expression>> {
        self.advance(); // consume 'new'

        // Parse the type name (must be an identifier; generic args parsed by call site)
        if !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected type name after 'new'");
            return None;
        }
        let type_name = self.current_token().value.clone();
        self.advance();

        // Parse generic type <T, U, ...>
        let mut generic_args = Vec::new();
        if self.match_value("<") {
            // consume '<'
            self.advance();
            while !self.match_value(">") && !self.is_end_of_line() {
                if !self.match_type(&TokenType::Identifier) {
                    self.log_error("Expected generic type argument");
                    return None;
                }
                let arg = self.current_token().value.clone();
                self.advance();
                generic_args.push(arg);
                if !self.match_value(",") {
                    break;
                }
                self.advance();
            }
            if !self.match_value(">") {
                self.log_error("Expected '>' to close generic arguments");
                return None;
            }
            self.advance();
        }

        // Array allocation: new T[size] or new T[size]?
        if self.match_value("[") {
            self.advance();
            let _size = self.parse_expression()?;
            if !self.match_value("]") {
                self.log_error("Expected ']' after array size");
                return None;
            }
            self.advance();
            // Skip ? if present (nullable array)
            if self.match_value("?") {
                self.advance();
            }
            // Array allocation marker (handled by cranelift backend)
            return Some(Box::new(Identifier::new("__new_array")));
        }

        // Struct construction: new Type(args) desugars to Type::new(args).
        // The `New<T>` trait (std/new.gbl) documents the contract; this
        // is resolved as a static method call on the type.
        let args = if self.match_value("(") {
            self.advance();
            let mut args = Vec::new();
            while !self.match_value(")") {
                args.push(self.parse_expression()?);
                if self.match_value(",") {
                    self.advance();
                } else {
                    break;
                }
            }
            self.consume_value(")", "Expected ')' to close new expression arguments");
            Some(args)
        } else {
            None
        };

        // new Type(args) → Type::new(args)
        let callee = Box::new(PathAccess::new(vec![type_name], "new"));
        Some(Box::new(FunctionCall::new(Some(callee), args)))
    }

    fn parse_if_statement(&mut self) -> Option<Box<dyn Statement>> {
        self.consume_value("if", "An If Statement's begin token must be token 'if'");
        let condition = self.parse_expression()?;

        self.consume_value("{", "Expect '{' at start of branch body");
        self.consume_end_of_line();

        let then_branch = self.parse_block();

        self.consume_value("}", "Expect '}' at end of branch body");
        self.consume_end_of_line();

        let then_branch = then_branch?;

        let else_branch = if self.match_value("else") {
            self.advance();

            if self.match_value("if") {
                let inner_if = self.parse_if_statement()?;
                Some(inner_if)
            } else {
                self.consume_value("{", "Expect '{' at start of branch body");
                self.consume_end_of_line();

                let else_block = self.parse_block();

                self.consume_value("}", "Expect '}' at end of branch body");
                self.consume_end_of_line();

                else_block.map(|b| b as Box<dyn Statement>)
            }
        } else {
            None
        };

        Some(Box::new(IfStatement::new(
            Some(condition),
            Some(then_branch),
            else_branch,
        )))
    }

    fn parse_while_statement(&mut self) -> Option<Box<dyn Statement>> {
        self.consume_value("while", "while statement must start with 'while' keyword");
        let condition = self.parse_expression()?;

        let body: Option<Box<dyn Statement>> = if self.match_value("{") {
            self.advance(); // consume '{'
            self.consume_end_of_line();
            let block = self.parse_block();
            self.consume_value("}", "Expected '}' at end of while body");
            self.consume_end_of_line();
            block.map(|b| b as Box<dyn Statement>)
        } else {
            self.parse_statement()
        };

        let body = body?;
        Some(Box::new(WhileStatement::new(Some(condition), Some(body))))
    }

    fn parse_break_statement(&mut self) -> Option<Box<dyn Statement>> {
        self.consume_value("break", "break statement must start with 'break' keyword");
        self.consume_end_of_line();
        Some(Box::new(BreakStatement::new()))
    }

    fn parse_continue_statement(&mut self) -> Option<Box<dyn Statement>> {
        self.consume_value("continue", "continue statement must start with 'continue' keyword");
        self.consume_end_of_line();
        Some(Box::new(ContinueStatement::new()))
    }

    /// 解析泛型类型或调用：`Vec<int>`、`Result<str, int>`、`Vec<int>::new()` 等。
    /// 当 `parse_primary` 通过 lookahead 确认当前标识符后跟的是泛型语法时，
    /// 调用此方法进行完整的泛型解析。
    fn parse_generic_type_or_call(&mut self, name: String) -> Option<Box<dyn Expression>> {
        // 解析泛型参数列表：<T, U>（类型参数可为标识符或类型关键字 int/float/str/bool）
        self.consume_value("<", "Expected '<' after generic type name");
        let mut type_args = Vec::new();
        while !self.match_value(">") && !self.error_occurred {
            if !self.match_type(&TokenType::Identifier)
                && !(self.match_type(&TokenType::Keyword)
                    && matches!(self.current_token().value.as_str(), "int" | "float" | "str" | "bool"))
            {
                self.log_error("Expected generic type argument");
                return None;
            }
            type_args.push(self.current_token().value.clone());
            self.advance();
            if self.match_value(",") { self.advance(); }
        }
        self.consume_value(">", "Expected '>' after generic arguments");

        // 处理泛型类型后的 `::method(args)` 调用链，如 `Ref<T>::new(addr)`
        if self.match_value("::") {
            self.advance(); // consume '::'
            // 解析方法名
            if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
                self.log_error("Expected method name after '::'");
                return None;
            }
            let method_name = self.current_token().value.clone();
            self.advance();

            // 如果后面有 `(`，解析参数列表并构建 FunctionCall
            if self.match_value("(") {
                self.advance();
                let args = self.parse_argument_list();
                self.consume_value(")", "Expected ')' after generic function call arguments");
                let type_expr = format!("{}<{}>", name, type_args.join(", "));
                let callee = Box::new(PathAccess::new(vec![type_expr], method_name));
                return Some(Box::new(FunctionCall::new(Some(callee), args)));
            }

            // 没有 `(`，返回 PathAccess（泛型类型的方法引用）
            let type_expr = format!("{}<{}>", name, type_args.join(", "));
            let callee = Box::new(PathAccess::new(vec![type_expr], method_name));
            return Some(callee);
        }

        // 如果后面跟着 `(`，则是泛型直接调用：Vec<int>(args)
        if self.match_value("(") {
            return self.parse_generic_function_call(name, type_args);
        }

        // 否则仅作为泛型类型标识：Vec<int>
        let full_name = format!("{}<{}>", name, type_args.join(", "));
        Some(Box::new(Identifier::new(full_name)))
    }

    /// 解析带泛型参数的函数调用：`Vec<int>::new(args)`
    fn parse_generic_function_call(&mut self, name: String, type_args: Vec<String>) -> Option<Box<dyn Expression>> {
        self.consume_value("(", "Expected '(' in generic function call");
        let args = self.parse_argument_list();
        self.consume_value(")", "Expected ')' after generic function call arguments");

        // 构建调用目标：Vec<int>::new
        let callee = Box::new(PathAccess::new(
            vec![format!("{}<{}>", name, type_args.join(", "))],
            "new"
        ));
        Some(Box::new(FunctionCall::new(Some(callee), args)))
    }
}

// Helpers out of the AST builder

fn as_number(expr: &Box<dyn Expression>) -> Option<f64> {
    expr.as_any().downcast_ref::<NumberLiteral>().map(|n| n.get_value())
}
