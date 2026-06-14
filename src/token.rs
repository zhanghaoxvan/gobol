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
            TokenType::Identifier => "identifier",
            TokenType::Keyword => "keyword",
            TokenType::Number => "number",
            TokenType::String => "string",
            TokenType::FormatString => "format_string",
            TokenType::Operator => "operator",
            TokenType::EndOfLine => "end_of_line",
            TokenType::EndOfFile => "end_of_file",
            TokenType::Unknown => "unknown",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub r#type: TokenType,
    pub value: String,
    pub line: i32,
    pub col: i32,
}

impl Token {
    pub fn new(r#type: TokenType, value: impl Into<String>) -> Self {
        Token {
            r#type,
            value: value.into(),
            line: 0,
            col: 0,
        }
    }

    pub fn with_pos(r#type: TokenType, value: impl Into<String>, line: i32, col: i32) -> Self {
        Token {
            r#type,
            value: value.into(),
            line,
            col,
        }
    }
}

#[allow(dead_code)]
#[deprecated(note = "TokenType can now be turned into string inline")]
fn token_type_to_str(r#type: &TokenType) -> &str {
    match r#type {
        TokenType::Identifier => "identifier",
        TokenType::Keyword => "keyword",
        TokenType::Number => "number",
        TokenType::String => "string",
        TokenType::FormatString => "format_string",
        TokenType::Operator => "operator",
        TokenType::EndOfLine => "end_of_line",
        TokenType::EndOfFile => "end_of_file",
        TokenType::Unknown => "unknown",
    }
}
