#ifndef SEMANTIC_ANALYZER_HPP
#define SEMANTIC_ANALYZER_HPP

#include <AST/AST.hpp>
#include <Environment/Environment.hpp>
#include <iostream>
#include <stack>
#include <string>
#include <vector>

namespace analyzer {

    /**
     * @class SemanticAnalyzer
     * @brief 语义分析器，使用 Environment 进行符号管理和类型检查
     *
     * 继承 ASTVisitor，遍历 AST 并进行：
     * - 符号声明与查找
     * - 作用域管理
     * - 类型检查
     * - 语义规则验证
     */
    class SemanticAnalyzer : public AST::ASTVisitor {
    private:
        env::Environment env;            // 符号表环境
        std::vector<std::string> errors; // 错误信息
        bool hasError;                   // 是否有错误

        // 当前函数上下文
        std::string currentFunction;
        env::DataType currentFunctionReturnType;
        bool hasReturnStatement;

        // 循环嵌套深度（用于 break/continue 检查）
        int loopDepth;

        // 当前模块名
        std::string currentModule;

        // 用于表达式类型推断的栈
        std::stack<env::DataType> typeStack;

    public:
        SemanticAnalyzer();
        virtual ~SemanticAnalyzer(); // 添加析构函数声明

        /**
         * @brief 执行语义分析
         * @param program AST根节点
         * @return 是否通过语义分析
         */
        bool analyze(AST::Program *program);

        /**
         * @brief 是否有错误
         * @return true 有错误
         */
        bool hasErrors() const {
            return hasError;
        }

        /**
         * @brief 获取错误信息
         * @return 错误信息列表
         */
        const std::vector<std::string> &getErrors() const {
            return errors;
        }

        /**
         * @brief 打印错误信息
         */
        void printErrors() const;

        // ==================== ASTVisitor 接口实现 ====================

        // 基类
        VISIT_ASTNODEI(AST::ASTNode)

        // 程序结构
        VISIT_ASTNODEI(AST::Program)
        VISIT_ASTNODEI(AST::ModuleStatement)
        VISIT_ASTNODEI(AST::ImportStatement)
        VISIT_ASTNODEI(AST::Function)
        VISIT_ASTNODEI(AST::Block)

        // 语句
        VISIT_ASTNODEI(AST::Declaration)
        VISIT_ASTNODEI(AST::IfStatement)
        VISIT_ASTNODEI(AST::WhileStatement)
        VISIT_ASTNODEI(AST::ForStatement)
        VISIT_ASTNODEI(AST::ReturnStatement)
        VISIT_ASTNODEI(AST::BreakStatement)
        VISIT_ASTNODEI(AST::ContinueStatement)
        VISIT_ASTNODEI(AST::ExpressionStatement)

        // 表达式
        VISIT_ASTNODEI(AST::BinaryExpression)
        VISIT_ASTNODEI(AST::UnaryExpression)
        VISIT_ASTNODEI(AST::FunctionCall)
        VISIT_ASTNODEI(AST::MemberAccess)
        VISIT_ASTNODEI(AST::Identifier)
        VISIT_ASTNODEI(AST::NumberLiteral)
        VISIT_ASTNODEI(AST::StringLiteral)
        VISIT_ASTNODEI(AST::BooleanLiteral)
        VISIT_ASTNODEI(AST::FormatString)
        VISIT_ASTNODEI(AST::RangeExpression)
        VISIT_ASTNODEI(AST::GroupedExpression)

        // 类型
        VISIT_ASTNODEI(AST::Type)
        VISIT_ASTNODEI(AST::ArrayType)
        VISIT_ASTNODEI(AST::ArrayIndex)

        // 其他
        VISIT_ASTNODEI(AST::Parameter)

        // 基类版本（保持空实现）
        VISIT_ASTNODEI(AST::Statement)
        VISIT_ASTNODEI(AST::Expression)

    private:
        // ==================== 辅助函数 ====================

        /**
         * @brief 记录错误
         * @param msg 错误信息
         */
        void error(const std::string &msg);

        /**
         * @brief 将 AST 类型转换为 Environment 数据类型
         * @param type AST类型节点
         * @return Environment数据类型
         */
        env::DataType getDataTypeFromAST(AST::Type *type);

        /**
         * @brief 获取当前表达式的类型（从栈顶）
         * @return 数据类型
         */
        env::DataType getCurrentType();

        /**
         * @brief 检查类型兼容性
         * @param target 目标类型
         * @param source 源类型
         * @param context 错误上下文
         * @return 是否兼容
         */
        bool checkTypeCompatibility(env::DataType target, env::DataType source, const std::string &context);
    };
} // namespace analyzer

#endif // SEMANTIC_ANALYZER_HPP
