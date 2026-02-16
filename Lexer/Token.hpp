/**
 * @file token.hpp
 * 为Gobol词法分析器定义令牌（Token）类型及Token类
 */

#ifndef GOBOL_TOKEN_HPP
#define GOBOL_TOKEN_HPP

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
    class Token {
        TokenType type = TokenType::UNKNOWN; ///< 令牌类型
        std::string value;                   ///< 令牌的字符串值/字面量

    public:
        /**
         * @brief 默认构造函数
         * @details 将令牌类型初始化为WRONG_TOKEN，值初始化为空字符串
         */
        Token() = default;

        /**
         * @brief 带参数的构造函数
         * @param type 令牌类型（TokenType）
         * @param value 令牌的字符串字面量/值（移动语义以提升效率）
         */
        Token(TokenType type, std::string value);

        /**
         * @brief 获取令牌类型
         * @return TokenType的常量引用（不可修改）
         */
        [[nodiscard]] TokenType getType() const;

        /**
         * @brief 获取令牌的字符串值
         * @return std::string的常量引用（避免拷贝）
         */
        [[nodiscard]] const std::string &getValue() const;

        /**
         * @brief 为令牌设置新类型
         * @param newType 要赋值的新TokenType类型
         */
        void setType(TokenType newType);

        /**
         * @brief 为令牌设置新的字符串值
         * @param newValue 新的字符串值（移动语义以提升效率）
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