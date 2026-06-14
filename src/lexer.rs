use crate::token::{Token, TokenType};
use std::collections::HashSet;

pub struct Lexer {
    source: String,
    current_position: usize,
    line: i32,
    col: i32,
    keywords: HashSet<String>,
}

impl Lexer {
    pub fn new(source: impl Into<String>) -> Self {
        let mut keywords = HashSet::new();
        let kw = [
            "if", "else", "for", "return", "int", "float", "str", "func", "var", "val",
            "module", "import", "in", "as", "true", "false", "while", "break", "continue",
            "null", "self", "export", "struct", "impl", "constructor", "new",
            "convert", "operator",
        ];
        for k in &kw {
            keywords.insert(k.to_string());
        }

        Lexer {
            source: source.into(),
            current_position: 0,
            line: 1,
            col: 0,
            keywords,
        }
    }

    #[allow(dead_code)]
    pub fn reset_position(&mut self) {
        self.current_position = 0;
        self.line = 1;
        self.col = 0;
    }

    fn is_source_end(&self) -> bool {
        self.current_position >= self.source.len()
    }

    fn peek(&self) -> char {
        if self.is_source_end() {
            '\0'
        } else {
            self.source.as_bytes()[self.current_position] as char
        }
    }

    fn peek_next(&self) -> char {
        if self.current_position + 1 < self.source.len() {
            self.source.as_bytes()[self.current_position + 1] as char
        } else {
            '\0'
        }
    }

    fn consume(&mut self) -> char {
        if self.is_source_end() {
            return '\0';
        }
        let c = self.source.as_bytes()[self.current_position] as char;
        self.current_position += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 0;
        } else {
            self.col += 1;
        }
        c
    }

    fn skip_line_comment(&mut self) {
        while !self.is_source_end() && self.peek() != '\n' {
            self.consume();
        }
    }

    fn skip_block_comment(&mut self) -> bool {
        self.consume(); // skip '*'
        while !self.is_source_end() {
            if self.peek() == '*' && self.peek_next() == '/' {
                self.consume(); // skip '*'
                self.consume(); // skip '/'
                return true;
            }
            self.consume();
        }
        false
    }

    fn skip_attribute(&mut self) {
        self.consume(); // skip '#'
        self.consume(); // skip '['
        let mut depth = 1;
        while !self.is_source_end() && depth > 0 {
            if self.peek() == '[' {
                depth += 1;
            } else if self.peek() == ']' {
                depth -= 1;
            }
            self.consume();
        }
    }

    fn parse_identifier(&mut self) -> Token {
        let start = self.current_position;
        while !self.is_source_end() && (self.peek().is_alphanumeric() || self.peek() == '_') {
            self.consume();
        }
        let identifier: String = self.source[start..self.current_position].to_string();
        if self.keywords.contains(&identifier) {
            Token::new(TokenType::Keyword, identifier)
        } else {
            Token::new(TokenType::Identifier, identifier)
        }
    }

    fn parse_number(&mut self) -> Token {
        let start = self.current_position;
        let mut has_decimal = false;

        while !self.is_source_end() {
            let c = self.peek();
            if c.is_ascii_digit() {
                self.consume();
            } else if c == '.' && !has_decimal {
                if self.peek_next() == '\0' || !self.peek_next().is_ascii_digit() {
                    break;
                }
                has_decimal = true;
                self.consume();
            } else {
                break;
            }
        }

        if self.current_position == start {
            let c = self.consume();
            return Token::new(TokenType::Unknown, c.to_string());
        }

        let number: String = self.source[start..self.current_position].to_string();
        Token::new(TokenType::Number, number)
    }

    fn parse_string(&mut self) -> Token {
        self.consume(); // skip opening '"'
        let start = self.current_position;
        let mut is_closed = false;

        while !self.is_source_end() {
            let c = self.peek();
            if c == '"' {
                is_closed = true;
                break;
            }
            if c == '\\' && self.peek_next() != '\0' {
                self.consume(); // skip backslash
            }
            self.consume();
        }

        let s: String = self.source[start..self.current_position].to_string();
        if is_closed {
            self.consume(); // skip closing '"'
            Token::new(TokenType::String, s)
        } else {
            Token::new(TokenType::Unknown, s)
        }
    }

    pub fn get_next_token(&mut self) -> Token {
        // Skip whitespace and comments
        while !self.is_source_end() {
            let c = self.peek();
            if c.is_ascii_whitespace() && c != '\n' {
                self.consume();
                continue;
            }
            if c == '/' && self.peek_next() == '/' {
                self.skip_line_comment();
                continue;
            }
            if c == '/' && self.peek_next() == '*' {
                self.skip_block_comment();
                continue;
            }
            if c == '#' && self.peek_next() == '[' {
                self.skip_attribute();
                continue;
            }
            break;
        }

        // Capture token start position
        let tok_line = self.line;
        let tok_col = self.col;

        if self.is_source_end() {
            return Token::with_pos(TokenType::EndOfFile, "", tok_line, tok_col);
        }

        let current_char = self.peek();

        if current_char == '\n' {
            self.consume();
            return Token::with_pos(TokenType::EndOfLine, "\n", tok_line, tok_col);
        }

        if current_char.is_alphabetic() || current_char == '_' {
            let mut tok = self.parse_identifier();
            tok.line = tok_line;
            tok.col = tok_col;
            return tok;
        }
        if current_char.is_ascii_digit() {
            let mut tok = self.parse_number();
            tok.line = tok_line;
            tok.col = tok_col;
            return tok;
        }
        if current_char == '"' {
            let mut tok = self.parse_string();
            tok.line = tok_line;
            tok.col = tok_col;
            return tok;
        }

        match current_char {
            '+' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "+=", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, "+", tok_line, tok_col)
            }
            '-' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "-=", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, "-", tok_line, tok_col)
            }
            '*' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "*=", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, "*", tok_line, tok_col)
            }
            '/' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "/=", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, "/", tok_line, tok_col)
            }
            '%' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "%=", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, "%", tok_line, tok_col)
            }
            '(' => {
                self.consume();
                Token::with_pos(TokenType::Operator, "(", tok_line, tok_col)
            }
            ')' => {
                self.consume();
                Token::with_pos(TokenType::Operator, ")", tok_line, tok_col)
            }
            '{' => {
                self.consume();
                Token::with_pos(TokenType::Operator, "{", tok_line, tok_col)
            }
            '}' => {
                self.consume();
                Token::with_pos(TokenType::Operator, "}", tok_line, tok_col)
            }
            '[' => {
                self.consume();
                Token::with_pos(TokenType::Operator, "[", tok_line, tok_col)
            }
            ']' => {
                self.consume();
                Token::with_pos(TokenType::Operator, "]", tok_line, tok_col)
            }
            '=' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "==", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, "=", tok_line, tok_col)
            }
            ':' => {
                self.consume();
                Token::with_pos(TokenType::Operator, ":", tok_line, tok_col)
            }
            '.' => {
                self.consume();
                if self.peek() == '.' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "..", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, ".", tok_line, tok_col)
            }
            ',' => {
                self.consume();
                Token::with_pos(TokenType::Operator, ",", tok_line, tok_col)
            }
            '>' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, ">=", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, ">", tok_line, tok_col)
            }
            '<' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "<=", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, "<", tok_line, tok_col)
            }
            '?' => {
                self.consume();
                Token::with_pos(TokenType::Operator, "?", tok_line, tok_col)
            }
            '&' => {
                self.consume();
                if self.peek() == '&' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "&&", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Unknown, "&", tok_line, tok_col)
            }
            '|' => {
                self.consume();
                if self.peek() == '|' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "||", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Unknown, "|", tok_line, tok_col)
            }
            '!' => {
                self.consume();
                if self.peek() == '=' {
                    self.consume();
                    return Token::with_pos(TokenType::Operator, "!=", tok_line, tok_col);
                }
                Token::with_pos(TokenType::Operator, "!", tok_line, tok_col)
            }
            '@' => {
                self.consume();
                if self.peek() != '"' {
                    self.consume();
                    return Token::with_pos(TokenType::Unknown, "@", tok_line, tok_col);
                }
                let mut tok = self.parse_string();
                tok.line = tok_line;
                tok.col = tok_col;
                Token::with_pos(TokenType::FormatString, tok.value, tok_line, tok_col)
            }
            _ => {
                let unknown = self.consume().to_string();
                Token::with_pos(TokenType::Unknown, unknown, tok_line, tok_col)
            }
        }
    }
}
