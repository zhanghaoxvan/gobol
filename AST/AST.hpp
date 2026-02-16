/**
 * @file AST.hpp
 * @brief GOBOL 语言抽象语法树（AST）核心头文件
 *
 * 该文件仅声明 AST 节点类型枚举、基础 ASTNode 结构体、各类具体节点及 AST 访问者接口，
 * 实现代码统一放在 AST.cpp 中，遵循“声明与实现分离”原则。
 * @author 35921
 * @date 2026/2/5
 */

#ifndef AST_HPP
#define AST_HPP

#include <Lexer/Token.hpp>
#include <vector>

namespace AST {

    // 前向声明访问者接口
    class ASTVisitor;

    // AST 节点类型枚举
    enum class ASTNodeType {
        PROGRAM, ///< 程序根节点（整个 AST 的入口）

        IMPORT_DECL,   ///< 导入声明节点（import 语句）
        FUNCTION_DECL, ///< 函数声明节点（函数定义）
        VAR_DECL,      ///< 变量声明节点（变量定义）
        MODULE_DECL,   ///< 模块声明节点（module 定义）

        RETURN_STMT, ///< 返回语句节点（return 语句）
        EXPR_STMT,   ///< 表达式语句节点（单独的表达式作为语句）
        BLOCK_STMT,  ///< 代码块节点（{} 包裹的语句块）
        IF_STMT,     ///< 条件语句节点（if 语句）
        WHILE_STMT,  ///< 循环语句节点（while 语句）
        FOR_STMT,    ///< 循环语句节点（for 语句）

        IDENTIFIER,    ///< 标识符节点（变量名、函数名等）
        KEYWORD,       ///< 关键字节点（语言保留字）
        NUMBER,        ///< 数字字面量节点（整数/浮点数）
        STRING,        ///< 普通字符串字面量节点
        FORMAT_STRING, ///< 格式化字符串字面量节点
        OPERATOR,      ///< 运算符节点（+、-、*、/ 等）
        END_OF_LINE,   ///< 行结束节点
        END_OF_FILE,   ///< 文件结束节点

        UNKNOWN ///< 未知节点（语法分析失败的节点）
    };

    // AST 基础节点结构体（仅声明）
    struct ASTNode {
        ASTNode *parent;
        std::vector<ASTNode *> children;
        lexer::token::Token token;
        ASTNodeType type;

        // 构造函数声明
        ASTNode(ASTNode *parent, lexer::token::Token token);
        ASTNode(ASTNode *parent, lexer::token::Token token, ASTNodeType type);

        // 虚析构函数声明
        virtual ~ASTNode();

        // 禁用拷贝语义（仅声明）
        ASTNode(const ASTNode &) = delete;
        ASTNode &operator=(const ASTNode &) = delete;

        // 启用移动语义（仅声明）
        ASTNode(ASTNode &&) = default;
        ASTNode &operator=(ASTNode &&) = default;

        // 纯虚方法声明
        virtual void accept(ASTVisitor &visitor) = 0;
    };

    // 各类具体节点的声明（仅保留构造函数和 accept 方法声明）
    struct ImportDeclNode : ASTNode {
        ImportDeclNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct FunctionDeclNode : ASTNode {
        FunctionDeclNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct VarDeclNode : ASTNode {
        VarDeclNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct ReturnStmtNode : ASTNode {
        ReturnStmtNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct ExprStmtNode : ASTNode {
        ExprStmtNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct BlockStmtNode : ASTNode {
        BlockStmtNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct IfStmtNode : ASTNode {
        IfStmtNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct WhileStmtNode : ASTNode {
        WhileStmtNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct ForStmtNode : ASTNode {
        ForStmtNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct IdentifierNode : ASTNode {
        IdentifierNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct KeywordNode : ASTNode {
        KeywordNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct NumberNode : ASTNode {
        NumberNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct StringNode final : ASTNode {
        StringNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct FormatStringNode final : ASTNode {
        FormatStringNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct OperatorNode final : ASTNode {
        OperatorNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct EndOfLineNode final : ASTNode {
        EndOfLineNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct EndOfFileNode final : ASTNode {
        EndOfFileNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    struct UnknownNode final : ASTNode {
        UnknownNode(ASTNode *parent, lexer::token::Token token);
        void accept(ASTVisitor &visitor) override;
    };

    // AST 访问者接口声明
    class ASTVisitor {
    public:
        virtual ~ASTVisitor() = default;

        virtual void visit(ImportDeclNode &node) = 0;
        virtual void visit(FunctionDeclNode &node) = 0;
        virtual void visit(VarDeclNode &node) = 0;
        virtual void visit(ReturnStmtNode &node) = 0;
        virtual void visit(ExprStmtNode &node) = 0;
        virtual void visit(BlockStmtNode &node) = 0;
        virtual void visit(IfStmtNode &node) = 0;
        virtual void visit(WhileStmtNode &node) = 0;
        virtual void visit(ForStmtNode &node) = 0;
        virtual void visit(IdentifierNode &node) = 0;
        virtual void visit(KeywordNode &node) = 0;
        virtual void visit(NumberNode &node) = 0;
        virtual void visit(StringNode &node) = 0;
        virtual void visit(FormatStringNode &node) = 0;
        virtual void visit(OperatorNode &node) = 0;
        virtual void visit(EndOfLineNode &node) = 0;
        virtual void visit(EndOfFileNode &node) = 0;
        virtual void visit(UnknownNode &node) = 0;
    };

} // namespace AST

#endif // AST_HPP
