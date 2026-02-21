/**
 * @file token.hpp
 * 为Gobol词法分析器定义令牌（Token）类型及Token类
 */

#ifndef TOKEN_HPP
#define TOKEN_HPP

#include <string>

/**
 * @namespace lexer::token
 * @brief Gobol的Token声明与实现均存放于此命名空间
 */
namespace lexer::token {
    /**
     * @enum TokenType
     * @brief 枚举Gobol编程语言中所有合法的令牌类型
     * @note WRONG_TOKEN（错误令牌）是未初始化/无效令牌的默认类型
     */
    enum class TokenType {
        PROGRAM,       ///< 根节点（除了AST::ProgramNode外没有什么东西用它）
        IDENTIFIER,    ///< 标识符（变量、函数、类等）
        KEYWORD,       ///< Gobol关键字（int、if、for等）
        NUMBER,        ///< 数值字面量（整数，例如 1、114514）
        STRING,        ///< 字符串字面量（双引号包裹，例如 "Hello world!"）
        FORMAT_STRING, ///< 待格式化的字符串字面量（例如 @"Hello {114514}!"）
        OPERATOR,      ///< 运算符
        END_OF_LINE,   ///< 行结束分隔符：\n
        END_OF_FILE,   ///< 文件结束标记：\0
        UNKNOWN,       ///< 无效/未知令牌（错误类型）
    };
#ifdef DEBUG
    /**
     * @param type 待转换的TokenType类型
     * @return 转换后的类型字符串
     */
    std::string tokenTypeToString(TokenType type);
#endif

    /**
     * @struct Token （注：代码中实际是class，按代码修正）
     * @brief 表示Gobol语言的单个词法令牌
     * @details 存储令牌的类型和字符串值，提供完整的获取/设置方法
     *          默认初始化为类型WRONG_TOKEN、值为空字符串的令牌
     */
    struct Token {
        TokenType type = TokenType::UNKNOWN; ///< 令牌类型
        std::string value;                   ///< 令牌的字符串值/字面量
    };

} // namespace lexer::token

inline bool operator==(const lexer::token::Token &lhs, const lexer::token::Token &rhs) {
    return lhs.type == rhs.type && lhs.value == rhs.value;
}
inline bool operator!=(const lexer::token::Token &lhs, const lexer::token::Token &rhs) {
    return !(lhs == rhs);
}
#endif // TOKEN_HPP
