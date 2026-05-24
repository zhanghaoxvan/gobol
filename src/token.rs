use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TokenType {
    Identifier,
    Keyword,
    Number,
    String,
    FormatString,
    Operator,
    EndOfLine,
    EndOfFile,
    Unknown,
}

impl fmt::Display for TokenType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TokenType::Identifier => "IDENTIFIER",
            TokenType::Keyword => "KEYWORD",
            TokenType::Number => "NUMBER",
            TokenType::String => "STRING",
            TokenType::FormatString => "FORMAT_STRING",
            TokenType::Operator => "OPERATOR",
            TokenType::EndOfLine => "END_OF_LINE",
            TokenType::EndOfFile => "END_OF_FILE",
            TokenType::Unknown => "UNKNOWN",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub r#type: TokenType,
    pub value: String,
}

impl Token {
    pub fn new(r#type: TokenType, value: impl Into<String>) -> Self {
        Token {
            r#type,
            value: value.into(),
        }
    }
}

#[cfg(debug_assertions)]
pub fn token_type_to_string(token_type: &TokenType) -> String {
    token_type.to_string()
}
