//
// Created by 35921 on 2026/1/14.
//

#include "Lexer.hpp"
#include <cctype>
#include <string>

namespace lexer {

    bool Lexer::isSourceEnd() const {
        return currentPosition >= source.length();
    }

    char Lexer::peek() const {
        return isSourceEnd() ? '\0' : source[currentPosition];
    }

    char Lexer::peekNext() const {
        return currentPosition + 1 < source.length() ? source[currentPosition + 1] : '\0';
    }

    char Lexer::consume() {
        if (isSourceEnd()) {
            return '\0';
        }
        const char c = source[currentPosition];
        currentPosition++;
        if (c == '\n') {
            line++;
            col = 0;
        } else {
            col++;
        }
        return c;
    }

    void Lexer::skipLineComment() {
        while (!isSourceEnd() && peek() != '\n') {
            consume();
        }
    }

    bool Lexer::skipBlockComment() {
        consume(); // Skip the '*' in "/*"
        while (!isSourceEnd()) {
            if (peek() == '*' && peekNext() == '/') {
                consume(); // Skip the '*' in "*/"
                consume(); // Skip the '/' in "*/"
                return true;
            }
            consume();
        }
        return false;
    }

    Lexer::Lexer(std::string_view source) : source(source), currentPosition(0), line(1), col(0) {
    }

    token::Token Lexer::getNextToken() {
        // Loop to skip non-lexical content (whitespace and comments)
        while (!isSourceEnd()) {
            const char c = peek();
            // Skip whitespace (space/tab/cr) but preserve newlines for line tracking
            if (std::isspace(static_cast<unsigned char>(c)) && c != '\n') {
                consume();
                continue;
            }
            // Detect and skip single-line comment "//"
            if (c == '/' && peekNext() == '/') {
                skipLineComment();
                continue;
            }
            // Detect and skip multi-line comment "/* */"
            if (c == '/' && peekNext() == '*') {
                skipBlockComment();
                continue;
            }
            // Exit loop when a valid lexical character is found
            break;
        }

        // Return EOF if source is exhausted after skipping non-lexical content
        if (isSourceEnd()) {
            return {token::TokenType::END_OF_FILE, ""};
        }

        // Get current valid character (safe read via peek())
        const char currentChar = peek();

        // Process newline as independent END_OF_LINE token (updates line/col via consume())
        if (currentChar == '\n') {
            consume();
            return {token::TokenType::END_OF_LINE, "\n"};
        }

        // Dispatch to composite token parsers based on leading character
        // Parse identifiers/keywords (starts with letter or underscore)
        if (std::isalpha(static_cast<unsigned char>(currentChar)) || currentChar == '_') {
            return parseIdentifier();
        }
        // Parse numeric literals (integer/floating-point, starts with digit)
        if (std::isdigit(static_cast<unsigned char>(currentChar))) {
            return parseNumber();
        }
        // Parse double-quoted string literals (supports basic escape sequences)
        if (currentChar == '"') {
            return parseString();
        }

        // Parse single-character operators and delimiters
        switch (currentChar) {
        case '+':
            consume();
            if (peek() == '=') {
                consume();
                return {token::TokenType::OPERATOR, "+="};
            }
            return {token::TokenType::OPERATOR, "+"};
        case '-':
            consume();
            if (peek() == '=') {
                consume();
                return {token::TokenType::OPERATOR, "-="};
            }
            return {token::TokenType::OPERATOR, "-"};
        case '*':
            consume();
            if (peek() == '=') {
                consume();
                return {token::TokenType::OPERATOR, "*="};
            }
            return {token::TokenType::OPERATOR, "*"};
        case '/':
            consume();
            if (peek() == '=') {
                consume();
                return {token::TokenType::OPERATOR, "/="};
            }
            return {token::TokenType::OPERATOR, "/"};
        case '(':
            consume();
            return {token::TokenType::OPERATOR, "("};
        case ')':
            consume();
            return {token::TokenType::OPERATOR, ")"};
        case '{':
            consume();
            return {token::TokenType::OPERATOR, "{"};
        case '}':
            consume();
            return {token::TokenType::OPERATOR, "}"};
        case '[':
            consume();
            return {token::TokenType::OPERATOR, "["};
        case ']':
            consume();
            return {token::TokenType::OPERATOR, "]"};
        case '=':
            consume();
            return {token::TokenType::OPERATOR, "="};
        case ':':
            consume();
            return {token::TokenType::OPERATOR, ":"};
        case '.':
            consume();
            return {token::TokenType::OPERATOR, "."};
        case ',':
            consume();
            return {token::TokenType::OPERATOR, ","};
        case '@':
            consume();
            if (peek() != '"') {
                return {token::TokenType::UNKNOWN, "@"};
            }
            return {token::TokenType::FORMAT_STRING, parseString().getValue()};
        default:
            // Mark unrecognized characters as UNKNOWN (fault-tolerant parsing)
            const std::string unknown(1, consume());
            return {token::TokenType::UNKNOWN, unknown};
        }
    }

    token::Token Lexer::parseIdentifier() {
        const size_t start = currentPosition;
        // Consume all valid identifier characters
        while (!isSourceEnd() && (std::isalnum(peek()) || peek() == '_')) {
            consume();
        }
        // Extract the raw identifier string from source
        const std::string identifier(source.substr(start, currentPosition - start));
        // Check if the identifier is a language keyword
        if (keywords.contains(identifier)) {
            return {token::TokenType::KEYWORD, identifier};
        }
        return {token::TokenType::IDENTIFIER, identifier};
    }

    token::Token Lexer::parseNumber() {
        const size_t start = currentPosition;
        bool hasDecimal = false; // Track single decimal point for floating-point numbers

        while (!isSourceEnd()) {
            const char c = peek();
            if (std::isdigit(c)) {
                consume();
            } else if (c == '.' && !hasDecimal) {
                // Validate decimal point: must be followed by a digit and not at EOF
                if (peekNext() == '\0' || !std::isdigit(peekNext())) {
                    break;
                }
                hasDecimal = true;
                consume();
            } else {
                // Stop parsing at non-numeric/illegal characters
                break;
            }
        }

        // Fallback validation: ensure at least one character was parsed
        if (currentPosition == start) {
            const std::string wrong(1, consume());
            return {token::TokenType::UNKNOWN, wrong};
        }

        const std::string number(source.substr(start, currentPosition - start));
        return {token::TokenType::NUMBER, number};
    }

    token::Token Lexer::parseString() {
        consume(); // Skip the opening double quote
        const size_t start = currentPosition;
        bool isClosed = false;

        while (!isSourceEnd()) {
            const char c = peek();
            if (c == '"') {
                isClosed = true;
                break;
            }
            // Handle basic escape sequences (\", \\)
            if (c == '\\' && peekNext() != '\0') {
                consume(); // Skip the backslash
            }
            consume();
        }

        // Extract string content (excludes enclosing quotes)
        const std::string str(source.substr(start, currentPosition - start));
        if (isClosed) {
            consume(); // Skip the closing double quote
            return {token::TokenType::STRING, str};
        }
        // Mark unclosed string as invalid token
        return {token::TokenType::UNKNOWN, str};
    }

} // namespace lexer