//
// Created by 35921 on 2026/1/14.
//

#ifndef GOBOL_TOKEN_HPP
#define GOBOL_TOKEN_HPP

#include <string>

/**
 * @file token.hpp
 * @brief Defines token types and Token class for the Gobol lexer
 * @author 35921
 * @date 2026/1/14
 * @namespace lexer::token
 * @brief Contains all token-related definitions for Gobol lexer
 */
namespace lexer::token {
    /**
     * @enum TokenType
     * @brief Enumerates all valid token types for the Gobol programming language
     * @note WRONG_TOKEN is the default type for uninitialized/invalid tokens
     */
    enum class TokenType {
        IDENTIFIER,    ///< Identifiers (variables, functions, classes, etc.)
        KEYWORD,       ///< The keyword of Gobol(int, if, for, etc.)
        NUMBER,        ///< Numeric literals (integers, e.g., 1, 114514)
        STRING,        ///< String literals (double-quoted, e.g., "Hello world!")
        FORMAT_STRING, ///< String literals to be formatted(e.g., @"Hello {114514}!")
        OPERATOR,      ///< Operator
        END_OF_LINE,   ///< End of line delimiter: \n
        END_OF_FILE,   ///< End of file marker: \0
        WRONG_TOKEN,   ///< Invalid/unknown token (error type)
    };
#ifdef DEBUG
    /**
     *
     * @param type the TokenType which will be converted
     * @return the converted string of type
     */
    std::string tokenTypeToString(TokenType type);
#endif

    /**
     * @struct Token
     * @brief Represents a single lexical token for the Gobol language
     * @details Stores the type and string value of a token, with full getter/setter methods
     *          Default initializes to WRONG_TOKEN with empty value
     */
    class Token {
        TokenType type = TokenType::WRONG_TOKEN; ///< Type of the token
        std::string value;                       ///< String value/literal of the token

    public:
        /**
         * @brief Default constructor
         * @details Initializes token type to WRONG_TOKEN and value to empty string
         */
        Token() = default;

        /**
         * @brief Parameterized constructor
         * @param type Type of the token (TokenType)
         * @param value String literal/value of the token (moved for efficiency)
         */
        Token(TokenType type, std::string value);

        /**
         * @brief Get the token type
         * @return Const reference to TokenType (immutable)
         */
        [[nodiscard]] TokenType getType() const;

        /**
         * @brief Get the token string value
         * @return Const reference to std::string (avoids copy)
         */
        [[nodiscard]] const std::string &getValue() const;

        /**
         * @brief Set a new type for the token
         * @param newType New TokenType to assign
         */
        void setType(TokenType newType);

        /**
         * @brief Set a new string value for the token
         * @param newValue New string value (moved for efficiency)
         */
        void setValue(std::string newValue);
    };

} // namespace lexer::token

inline bool operator==(const lexer::token::Token &lhs, const lexer::token::Token &rhs) {
    return lhs.getType() == rhs.getType() && lhs.getValue() == rhs.getValue();
}
inline bool operator!=(const lexer::token::Token &lhs, const lexer::token::Token &rhs) {
    return !(lhs == rhs);
}
#endif // GOBOL_TOKEN_HPP