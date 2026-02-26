/**
 * @file Lexer.hpp
 * @brief 声明词法分析器（Lexer）的相关函数
 */

#ifndef LEXER_HPP
#define LEXER_HPP

#include <Lexer/Token.hpp>
#include <string_view>
#include <unordered_set>

/**
 * @namespace lexer
 * @brief 词法分析器（Lexer）相关逻辑归属此命名空间
 */
namespace lexer { // how many applications in your debian?

    /**
     * @class Lexer
     * @brief 词法分析器核心类，负责将源代码解析为词法令牌（Token）序列
     */
    class Lexer {
        /** 待解析的源代码字符串 */
        std::string source;
        /** 当前字符在源代码中的位置索引（从 0 开始） */
        size_t currentPosition;
        /** 用于错误定位的当前行号（从 1 开始） */
        int line;
        /** 用于错误定位的当前列号（从 0 开始） */
        int col;
        /** GOBOL 语言关键字集合，解析标识符时会匹配此集合 */
        const std::unordered_set<std::string> keywords = {"if",  "else", "for", "return", "int",    "float",
                                                          "str", "func", "var", "val",    "module", "import"};

        /**
         * @brief 检查解析器是否已到达源代码末尾
         * @return 若到达末尾返回 true，否则返回 false
         */
        [[nodiscard]] bool isSourceEnd() const;

        /**
         * @brief 查看当前字符（不移动解析指针）
         * @return 当前位置（currentPosition）的字符，若到达源代码末尾则返回 '\0'
         */
        [[nodiscard]] char peek() const;

        /**
         * @brief 查看下一个字符（向前预读 1 个字符，不移动解析指针）
         * @details 用于多字符模式匹配，例如运算符、注释分隔符等场景
         * @return 下一个位置（currentPosition+1）的字符，若到达源代码末尾则返回 '\0'
         */
        [[nodiscard]] char peekNext() const;

        /**
         * @brief 消费当前字符并更新解析状态
         * @details 推进 currentPosition、递增列号；若消费的字符是换行符（'\n'），则重置列号为 0 并递增行号。
         * @return 被消费的字符，若到达源代码末尾则返回 '\0'
         */
        char consume();

        /**
         * @brief 跳过单行注释（// ... 直至行尾）
         * @details 消费从 "//" 开始到换行符或源代码末尾的所有字符。
         *          注释内容不会生成任何词法令牌。
         */
        void skipLineComment();

        /**
         * @brief 跳过多行注释（/* ... *\/）
         * @details 消费从 "/*" 开始到 "*\/" 结束的所有字符。支持跨多行的注释，
         *          并会跳过注释块内的换行符。此过程不生成任何令牌。
         * @return 若注释正常以 "*\/" 闭合返回 true，若到达文件末尾（EOF）仍未闭合返回 false
         */
        bool skipBlockComment();

        /**
         * @brief 解析标识符或语言关键字
         * @details 标识符以字母或下划线开头，后接字母、数字或下划线。
         *          将解析出的字符串与关键字集合匹配：匹配成功则返回关键字令牌，否则返回标识符令牌。
         * @return 对应的 Token 对象（类型为 KEYWORD 或 IDENTIFIER）
         */
        token::Token parseIdentifier();

        /**
         * @brief 解析数值字面量（整数和浮点数）
         * @details 支持十进制整数和合法的浮点数格式。
         *          拒绝无效的数值格式（例如前导小数点、末尾小数点、多个小数点）。
         * @return 格式合法则返回 NUMBER 类型 Token，格式非法则返回 UNKNOWN 类型 Token
         */
        token::Token parseNumber();

        /**
         * @brief 解析双引号包裹的字符串字面量
         * @details 支持字符串内容中的转义序列（例如 \"、\\）。
         *          从开头的 " 开始解析，直至遇到闭合的 " 或源代码末尾。
         * @return 字符串正常闭合则返回 STRING 类型 Token，未闭合则返回 UNKNOWN 类型 Token
         */
        token::Token parseString();

    public:
        void resetPosition() {
            currentPosition = 0;
            line = 1, col = 0;
        }
        /**
         * @brief 构造函数：初始化词法分析器状态
         * @param source 待解析的源代码字符串（使用 std::string_view 实现零拷贝）
         * @note 显式构造函数，防止从字符串类型隐式转换
         */
        explicit Lexer(std::string_view source);

        /**
         * @brief 从源代码中获取下一个合法的词法令牌（核心公有接口）
         * @details 词法分析器的主入口，执行序列化的令牌解析。
         *          会迭代跳过非词法内容（除换行符外的空白字符、单行/多行注释），
         *          然后将下一个合法字符/字符序列解析为强类型 Token。实现了容错解析：
         *          无法识别的字符会标记为 UNKNOWN 类型 Token，且不会终止整个解析流程。
         *          所有解析状态的更新（currentPosition、line、col）均通过 consume() 方法统一管理，确保一致性。
         * @steps
         *  1. 跳过所有空白字符（空格/制表符/回车），保留换行符并将其解析为 END_OF_LINE 令牌
         *  2. 识别并跳过单行（//）和多行（/* *\/）注释，不生成任何令牌
         *  3. 跳过非词法内容后检查是否到达源代码末尾，若是则返回 END_OF_FILE 令牌
         *  4. 将换行符处理为独立的 END_OF_LINE 令牌，以保留行结构信息
         *  5. 根据首字符模式，分发至复合令牌解析逻辑（标识符/数字/字符串）
         *  6. 解析单字符运算符和分隔符，并立即更新解析状态
         *  7. 将无法识别的字符标记为 UNKNOWN 类型 Token，消费该字符以继续解析
         * @note 换行符会保留为 END_OF_LINE 令牌，以支持基于行的语法分析
         * @note 所有字符分类操作均使用 static_cast<unsigned char>，避免有符号字符的未定义行为
         * @note 容错设计确保即使遇到无效字符，解析流程也能继续执行
         * @note 无越界访问：所有字符读取均通过 peek()/peekNext() 完成，内置边界检查
         * @return 合法的 token::Token 对象（包含对应的 TokenType 和原始字符串值）；
         *         到达源代码末尾时返回 END_OF_FILE 类型令牌
         */
        token::Token getNextToken();
    };

} // namespace lexer

#endif // LEXER_HPP
