/**
 * @file Token.hpp
 * @author 35921
 * @date 2026/2/4
 * @brief 词法分析器的Token（标记）相关实现
 *
 * 该文件实现了Token类以及将TokenType转换为字符串的工具函数，
 * 用于词法分析阶段对源代码进行标记化处理。
 */

#ifndef TOKEN_CPP
#define TOKEN_CPP

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
        case TokenType::UNKNOWN:
            return "UNKNOWN";
        }
    }

} // namespace lexer::token
#endif // TOKEN_CPP
