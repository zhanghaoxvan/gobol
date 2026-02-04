//
// Created by 35921 on 2026/2/4.
//
#include "Token.hpp"
#include <string>
namespace lexer::token {

    std::string tokenTypeToString(const TokenType type) {
        switch (type) {
        case TokenType::IDENTIFIER:
            return "IDENTIFIER";
        case TokenType::KEYWORD:
            return "KEYWORD";
        case TokenType::NUMBER:
            return "NUMBER";
        case TokenType::STRING:
            return "STRING";
        case TokenType::FORMAT_STRING:
            return "FORMAT_STRING";
        case TokenType::OPERATOR:
            return "OPERATOR";
        case TokenType::END_OF_LINE:
            return "END_OF_LINE";
        case TokenType::END_OF_FILE:
            return "END_OF_FILE";
        default:
        case TokenType::WRONG_TOKEN:
            return "WRONG_TOKEN";
        }
    }

    Token::Token(TokenType type, std::string value) : type(type), value(std::move(value)) {
    }

    TokenType Token::getType() const {
        return type;
    }

    const std::string &Token::getValue() const {
        return value;
    }

    void Token::setType(TokenType newType) {
        type = newType;
    }

    void Token::setValue(std::string newValue) {
        value = std::move(newValue);
    }

} // namespace lexer::token