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
        }
    }

    pub fn set_error_formatter(&mut self, f: ErrorFormatter) {
        self.error_formatter = Some(f);
    }

    pub fn build(&mut self) -> Option<Box<Program>> {
        self.root = None;
        let program = self.parse_program();
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
        let token = self.current_token();
        if let Some(ref f) = self.error_formatter {
            let span = if token.value.is_empty() { 1 } else { token.value.len() };
            let formatted = f.format_error(token.line, token.col, span, "error", message, true);
            self.error_message.push(formatted);
        } else {
            self.error_message.push(format!("Builder Error: {}", message));
        }
    }

    // ==================== Program ====================

    fn parse_program(&mut self) -> Program {
        let mut program = Program::new();

        while !self.match_type(&TokenType::EndOfFile) && !self.error_occurred {
            self.consume_end_of_line();

            if self.match_type(&TokenType::EndOfFile) {
                break;
            }

            let stmt = self.parse_statement();
            if let Some(s) = stmt {
                program.add_statement(s);
            } else {
                self.advance();
            }
        }

        program
    }

    // ==================== Statement ====================

    fn parse_statement(&mut self) -> Option<Box<dyn Statement>> {
        if self.match_type(&TokenType::Keyword) {
            let keyword = self.current_token().value.clone();

            match keyword.as_str() {
                "import" => return self.parse_import(),
                "func" => return self.parse_function(),
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
                "struct" => return self.parse_struct_definition(),
                "impl" => return self.parse_impl_block(),
                "export" => return self.parse_export_statement(),
                "operator" => {
                    // Skip operator definition at top level
                    self.advance();
                    while !self.match_value("{") && !self.is_end_of_line() && !self.match_type(&TokenType::EndOfFile) {
                        self.advance();
                    }
                    if self.match_value("{") {
                        self.advance();
                        let mut depth = 1;
                        while depth > 0 && !self.error_occurred {
                            if self.match_value("{") { depth += 1; }
                            else if self.match_value("}") { depth -= 1; }
                            self.advance();
                        }
                    }
                    self.consume_end_of_line();
                    return Some(Box::new(ExportStatement::new(vec![])));
                }
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

        if !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected identifier after 'import'");
            return None;
        }

        let mut path = vec![self.current_token().value.clone()];
        self.advance();

        // Handle "import a.b.c"
        while self.match_value(".") {
            self.advance(); // consume '.'
            if !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected identifier after '.' in import path");
                return None;
            }
            path.push(self.current_token().value.clone());
            self.advance();
        }

        // Handle "import a as b"
        let alias = if self.match_type(&TokenType::Keyword) && self.current_token().value == "as" {
            self.advance(); // consume 'as'
            if !self.match_type(&TokenType::Identifier) {
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

    fn parse_export_statement(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'export'

        self.consume_value("(", "Expected '(' after 'export'");

        let mut names: Vec<String> = Vec::new();

        while !self.match_value(")") && !self.error_occurred {
            if !self.match_type(&TokenType::Identifier) {
                self.log_error("Expected identifier in export list");
                return None;
            }
            let mut name = self.current_token().value.clone();
            self.advance();

            // Handle dotted names: add.add, io.print, etc.
            while self.match_value(".") {
                self.advance();
                if !self.match_type(&TokenType::Identifier) {
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

    fn parse_struct_definition(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'struct'

        if !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected struct name");
            return None;
        }

        let name = self.current_token().value.clone();
        self.advance();

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

            fields.push(StructField { name: field_name, field_type });
            self.consume_end_of_line();

            if self.match_value(",") { self.advance(); }
            self.consume_end_of_line();
        }

        self.consume_value("}", "Expected '}' after struct body");
        self.consume_end_of_line();

        Some(Box::new(StructDefinition::new(name, fields, generic_params)))
    }

    fn parse_impl_block(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'impl'

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

        if !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected struct name after 'impl'");
            return None;
        }
        let struct_name = self.current_token().value.clone();
        self.advance();

        // Optionally <T> after struct name
        if self.match_value("<") {
            self.advance();
            while !self.match_value(">") && !self.error_occurred {
                self.advance();
            }
            if self.match_value(">") { self.advance(); }
        }

        self.consume_end_of_line();
        self.consume_value("{", "Expected '{' at start of impl block");
        self.consume_end_of_line();

        let mut items = Vec::new();
        while !self.match_value("}") && !self.error_occurred {
            self.consume_end_of_line();
            if self.match_value("}") { break; }

            if self.match_type(&TokenType::Keyword) {
                let kw = self.current_token().value.clone();
                match kw.as_str() {
                    "constructor" => {
                        if let Some(func) = self.parse_method("constructor") {
                            items.push(ImplItem::Constructor(Box::new(func)));
                        }
                    }
                    "func" => {
                        if let Some(func) = self.parse_method("func") {
                            items.push(ImplItem::Method(Box::new(func)));
                        }
                    }
                    "convert" => {
                        if let Some(func) = self.parse_method("convert") {
                            items.push(ImplItem::Convert(Box::new(func)));
                        }
                    }
                    "operator" => {
                        // Skip operator definition (including body)
                        self.advance(); // skip 'operator'
                        // Skip until '{' or end of line
                        while !self.match_value("{") && !self.is_end_of_line() && !self.match_type(&TokenType::EndOfFile) {
                            self.advance();
                        }
                        if self.match_value("{") {
                            self.advance(); // skip '{'
                            let mut depth = 1;
                            while depth > 0 && !self.error_occurred {
                                if self.match_value("{") { depth += 1; }
                                else if self.match_value("}") { depth -= 1; }
                                self.advance();
                            }
                        }
                        self.consume_end_of_line();
                    }
                    _ => { self.advance(); }
                }
            } else if self.match_type(&TokenType::Identifier) {
                // Method shorthand: name(params): type { body }
                if let Some(func) = self.parse_method("") {
                    items.push(ImplItem::Method(Box::new(func)));
                }
            } else {
                break;
            }
            self.consume_end_of_line();
        }

        self.consume_value("}", "Expected '}' after impl block");
        self.consume_end_of_line();

        Some(Box::new(ImplBlock::new(struct_name, generic_params, items)))
    }

    fn parse_method(&mut self, keyword: &str) -> Option<Function> {
        if !keyword.is_empty() {
            self.advance(); // consume keyword (constructor/func/convert)
        }

        let method_name = if keyword == "constructor" {
            "constructor".to_string()
        } else if keyword == "convert" {
            // Parse target type as name
            let target = self.parse_type()?;
            let name = target.get_name().to_string();
            format!("convert_{}", name)
        } else {
            let name = self.current_token().value.clone();
            self.advance();
            name
        };

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

        Some(Function::new(method_name, params, return_type, body))
    }

    fn parse_function(&mut self) -> Option<Box<dyn Statement>> {
        self.advance(); // consume 'func'

        if !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected function name");
            return None;
        }

        let func_name = self.current_token().value.clone();
        self.advance();

        // Handle <T> generic params on functions
        if self.match_value("<") {
            self.advance();
            let mut depth = 1;
            while depth > 0 && !self.error_occurred {
                if self.match_value("<") { depth += 1; }
                else if self.match_value(">") { depth -= 1; }
                if depth > 0 { self.advance(); }
            }
            if self.match_value(">") { self.advance(); }
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

        Some(Box::new(Function::new(
            func_name,
            params,
            return_type,
            body,
        )))
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
        if !self.match_type(&TokenType::Identifier)
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

    fn parse_type(&mut self) -> Option<Box<dyn Type>> {
        if !self.match_type(&TokenType::Keyword) && !self.match_type(&TokenType::Identifier) {
            self.log_error("Expected type name");
            return None;
        }

        let type_name = self.current_token().value.clone();
        self.advance();

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

    fn parse_expression_statement(&mut self) -> Option<Box<dyn Statement>> {
        let expr = self.parse_expression()?;
        // Check for explicit semicolon terminator
        let has_semi = self.is_semicolon();
        if has_semi {
            self.advance(); // consume ';'
        }
        self.consume_end_of_line();
        if has_semi {
            Some(Box::new(ExpressionStatement::new(Some(expr))))
        } else {
            // No semicolon → tail expression (value-producing)
            Some(Box::new(ExpressionStatement::new_tail(Some(expr))))
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
            self.advance(); // consume '..'
            let end = self.parse_expression()?;
            
            // 尝试优化：如果两端都是数字，编译时确定步长
            let step = if let (Some(start_val), Some(end_val)) = (as_number(&start), as_number(&end)) {
                let step_val = if start_val > end_val { -1.0 } else { 1.0 };
                Some(Box::new(NumberLiteral::new(step_val)) as Box<dyn Expression>)
            } else {
                // 运行时由 range 函数根据 start 和 end 的大小决定步长
                None
            };
            
            let range_func = Box::new(Identifier::new("range"));
            let mut args = vec![start, end];
            if let Some(s) = step {
                args.push(s);
            }
            
            return Some(Box::new(FunctionCall::new(Some(range_func), Some(args))));
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
        if self.match_value("!") || self.match_value("-") || self.match_value("+") {
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
                if !self.match_type(&TokenType::Identifier) && !self.match_type(&TokenType::Keyword) {
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
            } else {
                break;
            }
        }

        Some(expr)
    }

    fn parse_primary(&mut self) -> Option<Box<dyn Expression>> {
        // 标识符 (可能是变量名或结构体类型名)
        if self.match_type(&TokenType::Identifier) {
            let name = self.current_token().value.clone();
            self.advance();
            
            // 检查是否是结构体字面量: TypeName { ... }
            // 通过大写开头判断类型名（遵循Go命名约定）
            if self.match_value("{") && name.chars().next().map_or(false, |c| c.is_uppercase()) {
                return self.parse_struct_literal(Box::new(Identifier::new(name)));
            }
            
            return Some(Box::new(Identifier::new(name)));
        }

        // 数字字面量
        if self.match_type(&TokenType::Number) {
            let value: f64 = self.current_token().value.parse().unwrap_or(0.0);
            self.advance();
            return Some(Box::new(NumberLiteral::new(value)));
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
            let expr = self.parse_expression()?;
            // 处理元组（简单地跳过额外的表达式）
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

        let mut elements: Vec<Box<dyn Expression>> = Vec::new();

        while !self.match_value("]") && !self.error_occurred {
            if self.match_value(",") {
                self.advance();
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

            // Parse pattern
            let pattern = if self.match_value("_") {
                self.advance();
                MatchPattern::Wildcard
            } else if self.match_type(&TokenType::Number) {
                let val = self.current_token().value.clone();
                self.advance();
                if val.contains('.') {
                    MatchPattern::Literal(RtValueSimple::FloatStr(val))
                } else {
                    MatchPattern::Literal(RtValueSimple::Int(val.parse().unwrap_or(0)))
                }
            } else if self.match_type(&TokenType::String) {
                let val = self.current_token().value.clone();
                self.advance();
                MatchPattern::Literal(RtValueSimple::Str(val))
            } else if self.match_type(&TokenType::Keyword) && (self.current_token().value == "true" || self.current_token().value == "false") {
                let val = self.current_token().value == "true";
                self.advance();
                MatchPattern::Literal(RtValueSimple::Bool(val))
            } else if self.match_type(&TokenType::Identifier) {
                let name = self.current_token().value.clone();
                self.advance();
                MatchPattern::Variable(name)
            } else {
                self.log_error("Expected pattern in match arm");
                return None;
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
            } else {
                let expr = self.parse_expression()?;
                let mut block = Block::new();
                block.add_statement(Box::new(ExpressionStatement::new(Some(expr))));
                Some(Box::new(block))
            };

            arms.push(MatchArm { pattern, body });

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

        // Parse the type to allocate
        let _allocated_type = self.parse_type()?;

        // Check for [size] (array allocation)
        if self.match_value("[") {
            self.advance();
            let _size = self.parse_expression()?;
            if !self.match_value("]") {
                self.log_error("Expected ']' after array size");
                return None;
            }
            self.advance();
            // Skip ? if present
            if self.match_value("?") {
                self.advance();
            }
            // Return as a function-call-like expression (will expand later)
            return Some(Box::new(Identifier::new("__new_array")));
        }

        // Struct instantiation with no args: new Type
        // Return as identifier (will expand later)
        Some(Box::new(Identifier::new("__new_struct")))
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
}

// Helpers out of the AST builder

fn as_number(expr: &Box<dyn Expression>) -> Option<f64> {
    expr.as_any().downcast_ref::<NumberLiteral>().map(|n| n.get_value())
}
