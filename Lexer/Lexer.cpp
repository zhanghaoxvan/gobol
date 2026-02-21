/**
 * @file Lexer.cpp
 * @brief 词法分析器（Lexer）的核心实现文件
 *
 * 该文件实现了 Lexer 类的所有成员函数，包括字符读取、注释跳过、各类 Token
 * （标识符、关键字、数字、字符串、运算符等）的解析逻辑，是词法分析阶段的核心实现。
 * @author （可补充作者信息）
 * @date （可补充日期信息）
 */

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
        consume(); // 跳过 "/*" 中的 '*'
        while (!isSourceEnd()) {
            if (peek() == '*' && peekNext() == '/') {
                consume(); // 跳过 "*/" 中的 '*'
                consume(); // 跳过 "*/" 中的 '/'
                return true;
            }
            consume();
        }
        return false;
    }

    Lexer::Lexer(std::string_view source) : source(source), currentPosition(0), line(1), col(0) {
    }

    token::Token Lexer::getNextToken() {
        // 循环跳过非词法内容（空白字符和注释）
        while (!isSourceEnd()) {
            const char c = peek();
            // 跳过空白字符（空格/制表符/回车），但保留换行符用于行号跟踪
            if (std::isspace(static_cast<unsigned char>(c)) && c != '\n') {
                consume();
                continue;
            }
            // 检测并跳过单行注释 "//"
            if (c == '/' && peekNext() == '/') {
                skipLineComment();
                continue;
            }
            // 检测并跳过多行注释 "/* */"
            if (c == '/' && peekNext() == '*') {
                skipBlockComment();
                continue;
            }
            // 找到有效词法字符时退出循环
            break;
        }

        // 跳过非词法内容后若到达末尾，返回文件结束 Token
        if (isSourceEnd()) {
            return {token::TokenType::END_OF_FILE, ""};
        }

        // 获取当前有效字符（通过 peek() 安全读取）
        const char currentChar = peek();

        // 将换行符处理为独立的 END_OF_LINE Token（通过 consume() 更新行号/列号）
        if (currentChar == '\n') {
            consume();
            return {token::TokenType::END_OF_LINE, "\n"};
        }

        // 根据首字符分发到复合 Token 解析器
        // 解析标识符/关键字（以字母或下划线开头）
        if (std::isalpha(static_cast<unsigned char>(currentChar)) || currentChar == '_') {
            return parseIdentifier();
        }
        // 解析数字字面量（整数/浮点数，以数字开头）
        if (std::isdigit(static_cast<unsigned char>(currentChar))) {
            return parseNumber();
        }
        // 解析双引号包裹的字符串字面量（支持基础转义序列）
        if (currentChar == '"') {
            return parseString();
        }

        // 解析单字符运算符和分隔符
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
            return {token::TokenType::FORMAT_STRING, parseString().value};
        default:
            // 将无法识别的字符标记为 UNKNOWN（容错性解析）
            const std::string unknown(1, consume());
            return {token::TokenType::UNKNOWN, unknown};
        }
    }

    token::Token Lexer::parseIdentifier() {
        const size_t start = currentPosition;
        // 消费所有有效标识符字符
        while (!isSourceEnd() && (std::isalnum(peek()) || peek() == '_')) {
            consume();
        }
        // 从源代码中提取原始标识符字符串
        const std::string identifier(source.substr(start, currentPosition - start));
        // 检查标识符是否为语言关键字
        if (keywords.find(identifier) != keywords.end()) {
            return {token::TokenType::KEYWORD, identifier};
        }
        return {token::TokenType::IDENTIFIER, identifier};
    }

    token::Token Lexer::parseNumber() {
        const size_t start = currentPosition;
        bool hasDecimal = false; // 跟踪浮点数的单个小数点

        while (!isSourceEnd()) {
            const char c = peek();
            if (std::isdigit(c)) {
                consume();
            } else if (c == '.' && !hasDecimal) {
                // 验证小数点：必须后跟数字且不在文件末尾
                if (peekNext() == '\0' || !std::isdigit(peekNext())) {
                    break;
                }
                hasDecimal = true;
                consume();
            } else {
                // 遇到非数字/非法字符时停止解析
                break;
            }
        }

        // 兜底验证：确保至少解析了一个字符
        if (currentPosition == start) {
            const std::string wrong(1, consume());
            return {token::TokenType::UNKNOWN, wrong};
        }

        const std::string number(source.substr(start, currentPosition - start));
        return {token::TokenType::NUMBER, number};
    }

    token::Token Lexer::parseString() {
        consume(); // 跳过开头的双引号
        const size_t start = currentPosition;
        bool isClosed = false;

        while (!isSourceEnd()) {
            const char c = peek();
            if (c == '"') {
                isClosed = true;
                break;
            }
            // 处理基础转义序列（\"、\\）
            if (c == '\\' && peekNext() != '\0') {
                consume(); // 跳过反斜杠
            }
            consume();
        }

        // 提取字符串内容（排除首尾引号）
        const std::string str(source.substr(start, currentPosition - start));
        if (isClosed) {
            consume(); // 跳过结尾的双引号
            return {token::TokenType::STRING, str};
        }
        // 将未闭合的字符串标记为无效 Token
        return {token::TokenType::UNKNOWN, str};
    }

} // namespace lexer
