/**
 * @file Token.hpp
 * @author 35921
 * @date 2026/2/4
 * @brief 词法分析器的Token（标记）相关定义
 *
 * 该文件定义了Token类型枚举、Token类以及将TokenType转换为字符串的工具函数，
 * 用于词法分析阶段对源代码进行标记化处理。
 */

#ifndef TOKEN_HPP
#define TOKEN_HPP

#include <string>

/**
 * @namespace lexer::token
 * @brief 词法分析器的Token相关命名空间
 *
 * 包含Token类型定义、Token类以及相关的辅助函数，封装词法分析的核心标记结构。
 */
namespace lexer::token {

    /**
     * @enum TokenType
     * @brief 标记类型枚举，定义所有支持的词法标记类型
     */
    enum class TokenType {
        IDENTIFIER,    ///< 标识符（变量名、函数名等）
        KEYWORD,       ///< 关键字（如if、else、for等）
        NUMBER,        ///< 数字字面量（整数、浮点数等）
        STRING,        ///< 普通字符串字面量
        FORMAT_STRING, ///< 格式化字符串字面量（如带占位符的字符串）
        OPERATOR,      ///< 运算符（+、-、*、/、=等）
        END_OF_LINE,   ///< 行结束标记
        END_OF_FILE,   ///< 文件结束标记
        UNKNOWN        ///< 未知标记（词法分析失败的情况）
    };

    /**
     * @brief 将TokenType枚举值转换为对应的字符串表示
     *
     * 用于调试、日志输出或错误提示，将枚举类型转为人类可读的字符串。
     * @param type 要转换的TokenType枚举值
     * @return std::string 对应的字符串（如IDENTIFIER、KEYWORD等）
     */
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

    /**
     * @class Token
     * @brief 词法标记类，封装标记的类型和值
     *
     * 每个Token对象代表源代码中的一个最小词法单元，包含其类型（如标识符、数字）
     * 和对应的文本值（如变量名"age"、数字"123"）。
     */
    class Token {
    private:
        TokenType type;    ///< 标记的类型
        std::string value; ///< 标记对应的文本值

    public:
        /**
         * @brief 构造函数，初始化Token的类型和值
         *
         * 使用std::move优化字符串的拷贝，避免不必要的内存开销。
         * @param type 标记的类型
         * @param value 标记对应的文本值
         */
        Token(const TokenType type, std::string value) : type(type), value(std::move(value)) {
        }

        /**
         * @brief 获取标记的类型
         * @return TokenType 标记的类型（只读）
         */
        [[nodiscard]] TokenType getType() const {
            return type;
        }

        /**
         * @brief 获取标记的文本值
         * @return const std::string& 标记的文本值（只读引用，避免拷贝）
         */
        [[nodiscard]] const std::string &getValue() const {
            return value;
        }

        /**
         * @brief 修改标记的类型
         * @param newType 新的标记类型
         */
        void setType(const TokenType newType) {
            type = newType;
        }

        /**
         * @brief 修改标记的文本值
         *
         * 使用std::move优化字符串的拷贝，提升性能。
         * @param newValue 新的文本值
         */
        void setValue(std::string newValue) {
            value = std::move(newValue);
        }
    };

} // namespace lexer::token

#endif // TOKEN_HPP
