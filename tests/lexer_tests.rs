use gobol::lexer::Lexer;
use gobol::token::{TokenType};

#[test]
fn test_lexer_identifier() {
    let mut lexer = Lexer::new("hello");
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Identifier);
    assert_eq!(token.value, "hello");
}

#[test]
fn test_lexer_keyword() {
    let mut lexer = Lexer::new("func var val struct impl");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Keyword);
    assert_eq!(token.value, "func");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Keyword);
    assert_eq!(token.value, "var");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Keyword);
    assert_eq!(token.value, "val");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Keyword);
    assert_eq!(token.value, "struct");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Keyword);
    assert_eq!(token.value, "impl");
}

#[test]
fn test_lexer_number() {
    let mut lexer = Lexer::new("123 45.67");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Number);
    assert_eq!(token.value, "123");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Number);
    assert_eq!(token.value, "45.67");
}

#[test]
fn test_lexer_string() {
    let mut lexer = Lexer::new("\"hello\"");
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::String);
    assert_eq!(token.value, "hello");
}

#[test]
fn test_lexer_format_string() {
    let mut lexer = Lexer::new("@\"Hello {name}\"");
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::FormatString);
    assert_eq!(token.value, "Hello {name}");
}

#[test]
fn test_lexer_operators() {
    let mut lexer = Lexer::new("+ - * / = == != < > <= >= && || !");
    
    let expected = vec!["+", "-", "*", "/", "=", "==", "!=", "<", ">", "<=", ">=", "&&", "||", "!"];
    
    for op in expected {
        let token = lexer.get_next_token();
        assert_eq!(token.r#type, TokenType::Operator);
        assert_eq!(token.value, op);
    }
}

#[test]
fn test_lexer_range_operator() {
    let mut lexer = Lexer::new("0..10");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Number);
    assert_eq!(token.value, "0");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Operator);
    assert_eq!(token.value, "..");
    
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Number);
    assert_eq!(token.value, "10");
}

#[test]
fn test_lexer_compound_assignment() {
    let mut lexer = Lexer::new("+= -= *= /=");
    
    let expected = vec!["+=", "-=", "*=", "/="];
    
    for op in expected {
        let token = lexer.get_next_token();
        assert_eq!(token.r#type, TokenType::Operator);
        assert_eq!(token.value, op);
    }
}

#[test]
fn test_lexer_null() {
    let mut lexer = Lexer::new("null");
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Keyword);
    assert_eq!(token.value, "null");
}

#[test]
fn test_lexer_self_keyword() {
    let mut lexer = Lexer::new("self");
    let token = lexer.get_next_token();
    assert_eq!(token.r#type, TokenType::Keyword);
    assert_eq!(token.value, "self");
}
