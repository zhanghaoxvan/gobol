//
// Created by 35921 on 2026/1/16.
//

#ifndef AST_BUILDER_HPP
#define AST_BUILDER_HPP

#include "AST.hpp"
#include <Lexer/Lexer.hpp>
#include <string>
#include <vector>

/**
 * @namespace AST
 * @brief 抽象语法树（AST）相关命名空间
 *
 * 包含 AST 节点类型定义、基础节点结构、具体节点实现和访问者接口，封装 GOBOL 语言的语法树结构。
 */
namespace AST {
    /**
     * @class ASTBuilder
     * @brief 构建AST的主类
     */
    class ASTBuilder {
    private:
        std::vector<lexer::token::Token> tokens;
        ASTNode *root;
        size_t currentPosition;
        bool errorOccurred;
        std::string errorMessage;

        /**
         * @brief 获取当前token
         * @return 当前token的引用
         */
        const lexer::token::Token &currentToken() const;

        /**
         * @brief 获取下一个token
         * @return 下一个token的引用
         */
        const lexer::token::Token &peekNextToken() const;

        /**
         * @brief 前进到下一个token
         */
        void advance();

        /**
         * @brief 检查当前token类型是否匹配
         * @param type 期望的token类型
         * @return 是否匹配
         */
        bool match(lexer::token::TokenType type) const;

        /**
         * @brief 检查当前token值是否匹配
         * @param value 期望的token值
         * @return 是否匹配
         */
        bool matchValue(const std::string &value) const;

        /**
         * @brief 消费当前token并前进
         * @param type 期望的token类型
         * @param errorMessage 错误信息
         * @return 消费的token
         */
        lexer::token::Token consume(lexer::token::TokenType type, const std::string &errorMessage);

        /**
         * @brief 消费当前token并前进（根据值）
         * @param value 期望的token值
         * @param errorMessage 错误信息
         * @return 消费的token
         */
        lexer::token::Token consumeValue(const std::string &value, const std::string &errorMessage);

        /**
         * @brief 记录错误
         * @param message 错误信息
         */
        void logError(const std::string &message);

        /**
         * @brief 检查是否为行结束
         * @return 是否行结束
         */
        bool isEndOfLine() const;

        /**
         * @brief 消费行结束符
         */
        void consumeEndOfLine();

        /**
         * @brief 解析程序入口
         * @return 程序AST节点
         */
        Program *parseProgram();

        /**
         * @brief 解析语句
         * @return 语句AST节点
         */
        Statement *parseStatement();

        /**
         * @brief 解析导入语句
         * @return 导入语句节点
         */
        Statement *parseImport();

        /**
         * @brief 解析函数定义
         * @return 函数节点
         */
        Function *parseFunction();

        /**
         * @brief 解析参数列表
         * @return 参数列表
         */
        std::vector<Parameter *> *parseParameterList();

        /**
         * @brief 解析单个参数
         * @return 参数节点
         */
        Parameter *parseParameter();

        /**
         * @brief 解析类型
         * @return 类型节点
         */
        Type *parseType();

        /**
         * @brief 解析代码块
         * @return 代码块节点
         */
        Block *parseBlock();

        /**
         * @brief 解析变量声明
         * @return 变量声明节点
         */
        Statement *parseDeclaration();

        /**
         * @brief 解析表达式语句
         * @return 表达式语句节点
         */
        Statement *parseExpressionStatement();

        /**
         * @brief 解析return语句
         * @return return语句节点
         */
        Statement *parseReturnStatement();

        /**
         * @brief 解析for...in循环
         * @return for...in语句节点
         */
        Statement *parseForInStatement();

        /**
         * @brief 解析if语句
         * @return if语句节点
         */
        Statement *parseIfStatement();

        /**
         * @brief 解析while语句
         * @return while语句节点
         */
        Statement *parseWhileStatement();

        /**
         * @brief 解析for语句
         * @return for语句节点
         */
        Statement *parseForStatement();

        /**
         * @brief 解析break语句
         * @return break语句节点
         */
        Statement *parseBreakStatement();

        /**
         * @brief 解析continue语句
         * @return continue语句节点
         */
        Statement *parseContinueStatement();

        /**
         * @brief 解析range表达式
         * @return range表达式节点
         */
        Expression *parseRange();

        /**
         * @brief 解析格式化字符串
         * @param formatStr 格式化字符串内容
         * @return 格式化字符串节点
         */
        static Expression *parseFormatString(const std::string &formatStr);

        /**
         * @brief 解析表达式
         * @return 表达式节点
         */
        Expression *parseExpression();

        /**
         * @brief 解析赋值表达式
         * @return 表达式节点
         */
        Expression *parseAssignment();

        /**
         * @brief 解析逻辑或表达式
         * @return 表达式节点
         */
        Expression *parseLogicalOr();

        /**
         * @brief 解析逻辑与表达式
         * @return 表达式节点
         */
        Expression *parseLogicalAnd();

        /**
         * @brief 解析相等性表达式
         * @return 表达式节点
         */
        Expression *parseEquality();

        /**
         * @brief 解析比较表达式
         * @return 表达式节点
         */
        Expression *parseComparison();

        /**
         * @brief 解析加减表达式
         * @return 表达式节点
         */
        Expression *parseAdditive();

        /**
         * @brief 解析乘除表达式
         * @return 表达式节点
         */
        Expression *parseMultiplicative();

        /**
         * @brief 解析一元表达式
         * @return 表达式节点
         */
        Expression *parseUnary();

        /**
         * @brief 解析后缀表达式
         * @return 表达式节点
         */
        Expression *parsePostfix();

        /**
         * @brief 解析主表达式
         * @return 表达式节点
         */
        Expression *parsePrimary();

        /**
         * @brief 解析函数调用
         * @param callee 被调用的表达式
         * @return 函数调用节点
         */
        Expression *parseFunctionCall(Expression *callee);

        /**
         * @brief 解析参数列表（用于函数调用）
         * @return 表达式列表
         */
        std::vector<Expression *> *parseArgumentList();

    public:
        /**
         * @brief 构造函数
         * @param lexer 词法分析器
         */
        explicit ASTBuilder(lexer::Lexer lexer);

        /**
         * @brief 析构函数
         */
        ~ASTBuilder();

        // 禁止拷贝
        ASTBuilder(const ASTBuilder &) = delete;
        ASTBuilder &operator=(const ASTBuilder &) = delete;

        /**
         * @brief 构建AST
         * @return 构建的AST根节点指针
         */
        ASTNode *build();

        /**
         * @brief 获取构建的AST根节点
         * @return AST根节点指针
         */
        ASTNode *getRoot() const {
            return root;
        }

        /**
         * @brief 重置构建器状态
         */
        void reset();

        /**
         * @brief 检查是否有错误发生
         * @return 是否有错误
         */
        bool hasError() const {
            return errorOccurred;
        }

        /**
         * @brief 获取错误信息
         * @return 错误信息
         */
        const std::string &getErrorMessage() const {
            return errorMessage;
        }
    };

} // namespace AST

#endif // AST_BUILDER_HPP
