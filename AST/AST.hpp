/**
 * @file AST.hpp
 * 定义所有关于AST的类，如ASTNode及其衍生类，ASTVisitor及其衍生类
 */

#ifndef AST_HPP
#define AST_HPP

#include <Lexer/Lexer.hpp>
#include <string>
#include <vector>

#define ACCEPT_VISITOR(className)                                                                                      \
    void accept(ASTVisitor *visitor) override {                                                                        \
        visitor->visit(this);                                                                                          \
    }

// NOLINTBEGIN

#define VISIT_ASTNODE(className) void visit(className *node) override;

#define __BASE_VISIT_ASTNODE(className) virtual void visit(className *node) = 0;

// NOLINTEND

namespace AST {

    class ASTNode;
    class Program;
    class Statement;
    class Expression;
    class Block;
    class Function;
    class Parameter;
    class Type;
    class IfStatement;
    class WhileStatement;
    class ForStatement;
    class ReturnStatement;
    class BreakStatement;
    class ContinueStatement;
    class Declaration;
    class ExpressionStatement;
    class ImportStatement;
    class BinaryExpression;
    class UnaryExpression;
    class FunctionCall;
    class MemberAccess;
    class ArrayIndex;
    class GroupedExpression;
    class Identifier;
    class NumberLiteral;
    class StringLiteral;
    class BooleanLiteral;
    class FormatString;
    class RangeExpression;

    /**
     * @class ASTVisitor
     * @brief AST访问者基类
     */
    class ASTVisitor {
    public:
        __BASE_VISIT_ASTNODE(ASTNode)
        __BASE_VISIT_ASTNODE(Program)
        __BASE_VISIT_ASTNODE(Statement)
        __BASE_VISIT_ASTNODE(Expression)
        __BASE_VISIT_ASTNODE(Block)
        __BASE_VISIT_ASTNODE(Function)
        __BASE_VISIT_ASTNODE(Parameter)
        __BASE_VISIT_ASTNODE(Type)
        __BASE_VISIT_ASTNODE(IfStatement)
        __BASE_VISIT_ASTNODE(WhileStatement)
        __BASE_VISIT_ASTNODE(ForStatement)
        __BASE_VISIT_ASTNODE(ReturnStatement)
        __BASE_VISIT_ASTNODE(BreakStatement)
        __BASE_VISIT_ASTNODE(ContinueStatement)
        __BASE_VISIT_ASTNODE(Declaration)
        __BASE_VISIT_ASTNODE(ExpressionStatement)
        __BASE_VISIT_ASTNODE(ImportStatement)
        __BASE_VISIT_ASTNODE(BinaryExpression)
        __BASE_VISIT_ASTNODE(UnaryExpression)
        __BASE_VISIT_ASTNODE(FunctionCall)
        __BASE_VISIT_ASTNODE(MemberAccess)
        __BASE_VISIT_ASTNODE(ArrayIndex)
        __BASE_VISIT_ASTNODE(GroupedExpression)
        __BASE_VISIT_ASTNODE(Identifier)
        __BASE_VISIT_ASTNODE(NumberLiteral)
        __BASE_VISIT_ASTNODE(StringLiteral)
        __BASE_VISIT_ASTNODE(BooleanLiteral)
        __BASE_VISIT_ASTNODE(FormatString)
        __BASE_VISIT_ASTNODE(RangeExpression)
    };

    /**
     * @class ASTNode
     * @brief AST节点的基类
     */
    class ASTNode {
    public:
        virtual ~ASTNode() = default;
        virtual void accept(ASTVisitor *visitor) = 0;
    };

    /**
     * @class Program
     * @brief 程序根节点
     */
    class Program : public ASTNode {
    private:
        ::std::vector<Statement *> statements;

    public:
        Program() = default;
        ~Program() override;

        void addStatement(Statement *stmt);
        const ::std::vector<Statement *> &getStatements() const {
            return statements;
        }

        ACCEPT_VISITOR(Program)
    };

    /**
     * @class Statement
     * @brief 语句基类
     */
    class Statement : public ASTNode {
    public:
        ~Statement() override = default;
        ACCEPT_VISITOR(Statement)
    };

    /**
     * @class Expression
     * @brief 表达式基类
     */
    class Expression : public ASTNode {
    public:
        ~Expression() override = default;
        ACCEPT_VISITOR(Expression)
    };

    /**
     * @class Block
     * @brief 代码块
     */
    class Block : public Statement {
    private:
        ::std::vector<Statement *> statements;

    public:
        Block() = default;
        ~Block() override;

        void addStatement(Statement *stmt);
        const ::std::vector<Statement *> &getStatements() const {
            return statements;
        }
        ACCEPT_VISITOR(BLOCK)
    };

    /**
     * @class Type
     * @brief 类型节点
     */
    class Type : public ASTNode {
    private:
        ::std::string name;

    public:
        explicit Type(::std::string name);
        ~Type() override = default;

        const ::std::string &getName() const {
            return name;
        }
        ACCEPT_VISITOR(Type)
    };

    /**
     * @class ArrayType
     * @brief 数组类型
     */
    class ArrayType : public Type {
    private:
        Expression *size;

    public:
        ArrayType(const ::std::string &elementType, Expression *size);
        ~ArrayType() override;

        Expression *getSize() const {
            return size;
        }
        ACCEPT_VISITOR(ArrayType)
    };

    /**
     * @class Parameter
     * @brief 函数参数
     */
    class Parameter : public ASTNode {
    private:
        ::std::string name;
        Type *type;

    public:
        Parameter(::std::string name, Type *type);
        ~Parameter() override;

        const ::std::string &getName() const {
            return name;
        }
        Type *getType() const {
            return type;
        }

        ACCEPT_VISITOR(Parameter)
    };

    /**
     * @class Function
     * @brief 函数定义
     */
    class Function : public Statement {
    private:
        ::std::string name;
        ::std::vector<Parameter *> *parameters;
        Type *returnType;
        Block *body;

    public:
        Function(::std::string name, ::std::vector<Parameter *> *parameters, Type *returnType, Block *body);
        ~Function() override;

        const ::std::string &getName() const {
            return name;
        }
        ::std::vector<Parameter *> *getParameters() const {
            return parameters;
        }
        Type *getReturnType() const {
            return returnType;
        }
        Block *getBody() const {
            return body;
        }
        ACCEPT_VISITOR(Function)
    };

    /**
     * @class ImportStatement
     * @brief 导入语句
     */
    class ImportStatement : public Statement {
    private:
        ::std::string moduleName;

    public:
        explicit ImportStatement(::std::string moduleName);
        ~ImportStatement() override = default;

        const ::std::string &getModuleName() const {
            return moduleName;
        }
        ACCEPT_VISITOR(ImportStatement)
    };

    /**
     * @class IfStatement
     * @brief if语句
     */
    class IfStatement : public Statement {
    private:
        Expression *condition;
        Statement *thenBranch;
        Statement *elseBranch;

    public:
        IfStatement(Expression *condition, Statement *thenBranch, Statement *elseBranch = nullptr);
        ~IfStatement() override;

        Expression *getCondition() const {
            return condition;
        }
        Statement *getThenBranch() const {
            return thenBranch;
        }
        Statement *getElseBranch() const {
            return elseBranch;
        }
        ACCEPT_VISITOR(IfStatement)
    };

    /**
     * @class WhileStatement
     * @brief while循环
     */
    class WhileStatement : public Statement {
    private:
        Expression *condition;
        Statement *body;

    public:
        WhileStatement(Expression *condition, Statement *body);
        ~WhileStatement() override;

        Expression *getCondition() const {
            return condition;
        }
        Statement *getBody() const {
            return body;
        }
        ACCEPT_VISITOR(WhileStatement)
    };

    /**
     * @class ForStatement
     * @brief for循环
     */
    class ForStatement : public Statement {
    private:
        ::std::string loopVariable;
        Expression *iterable;
        Block *body;

    public:
        ForStatement(::std::string loopVariable, Expression *iterable, Block *body);
        ~ForStatement() override;

        const ::std::string &getLoopVariable() const {
            return loopVariable;
        }
        Expression *getIterable() const {
            return iterable;
        }
        Block *getBody() const {
            return body;
        }
        ACCEPT_VISITOR(ForStatement)
    };

    /**
     * @class ReturnStatement
     * @brief return语句
     */
    class ReturnStatement : public Statement {
    private:
        Expression *value;

    public:
        explicit ReturnStatement(Expression *value = nullptr);
        ~ReturnStatement() override;

        Expression *getValue() const {
            return value;
        }
        ACCEPT_VISITOR(Expression)
    };

    /**
     * @class BreakStatement
     * @brief break语句
     */
    class BreakStatement : public Statement {
    public:
        BreakStatement() = default;
        ~BreakStatement() override = default;
        ACCEPT_VISITOR(BreakStatement)
    };

    /**
     * @class ContinueStatement
     * @brief continue语句
     */
    class ContinueStatement : public Statement {
    public:
        ContinueStatement() = default;
        ~ContinueStatement() override = default;
        ACCEPT_VISITOR(ContinueStatement)
    };

    /**
     * @class Declaration
     * @brief 变量声明
     */
    class Declaration : public Statement {
    private:
        ::std::string keyword; // var, val
        ::std::string name;
        Type *type;
        Expression *initializer;

    public:
        Declaration(::std::string keyword, ::std::string name, Type *type, Expression *initializer);
        ~Declaration() override;

        const ::std::string &getKeyword() const {
            return keyword;
        }
        const ::std::string &getName() const {
            return name;
        }
        Type *getType() const {
            return type;
        }
        Expression *getInitializer() const {
            return initializer;
        }
        ACCEPT_VISITOR(Declaration)
    };

    /**
     * @class ExpressionStatement
     * @brief 表达式语句
     */
    class ExpressionStatement : public Statement {
    private:
        Expression *expression;

    public:
        explicit ExpressionStatement(Expression *expression);
        ~ExpressionStatement() override;

        Expression *getExpression() const {
            return expression;
        }
        ACCEPT_VISITOR(ExpressionStatement)
    };

    /**
     * @class BinaryExpression
     * @brief 二元表达式
     */
    class BinaryExpression : public Expression {
    private:
        Expression *left;
        ::std::string op;
        Expression *right;

    public:
        BinaryExpression(Expression *left, ::std::string op, Expression *right);
        ~BinaryExpression() override;

        Expression *getLeft() const {
            return left;
        }
        const ::std::string &getOperator() const {
            return op;
        }
        Expression *getRight() const {
            return right;
        }
        ACCEPT_VISITOR(BinaryExpression)
    };

    /**
     * @class UnaryExpression
     * @brief 一元表达式
     */
    class UnaryExpression : public Expression {
    private:
        ::std::string op;
        Expression *operand;

    public:
        UnaryExpression(::std::string op, Expression *operand);
        ~UnaryExpression() override;

        const ::std::string &getOperator() const {
            return op;
        }
        Expression *getOperand() const {
            return operand;
        }
        ACCEPT_VISITOR(UnaryExpression)
    };

    /**
     * @class FunctionCall
     * @brief 函数调用
     */
    class FunctionCall : public Expression {
    private:
        Expression *callee;
        ::std::vector<Expression *> *arguments;

    public:
        FunctionCall(Expression *callee, ::std::vector<Expression *> *arguments);
        ~FunctionCall() override;

        Expression *getCallee() const {
            return callee;
        }
        ::std::vector<Expression *> *getArguments() const {
            return arguments;
        }
        ACCEPT_VISITOR(FunctionCall)
    };

    /**
     * @class MemberAccess
     * @brief 成员访问
     */
    class MemberAccess : public Expression {
    private:
        Expression *object;
        ::std::string member;

    public:
        MemberAccess(Expression *object, ::std::string member);
        ~MemberAccess() override;

        Expression *getObject() const {
            return object;
        }
        const ::std::string &getMember() const {
            return member;
        }
        ACCEPT_VISITOR(MemberAccess)
    };

    /**
     * @class ArrayIndex
     * @brief 数组索引
     */
    class ArrayIndex : public Expression {
    private:
        Expression *array;
        Expression *index;

    public:
        ArrayIndex(Expression *array, Expression *index);
        ~ArrayIndex() override;

        Expression *getArray() const {
            return array;
        }
        Expression *getIndex() const {
            return index;
        }
        ACCEPT_VISITOR(ArrayIndex)
    };

    /**
     * @class GroupedExpression
     * @brief 括号分组表达式
     */
    class GroupedExpression : public Expression {
    private:
        Expression *expression;

    public:
        explicit GroupedExpression(Expression *expression);
        ~GroupedExpression() override;

        Expression *getExpression() const {
            return expression;
        }
        ACCEPT_VISITOR(GroupedExpression)
    };

    /**
     * @class Identifier
     * @brief 标识符
     */
    class Identifier : public Expression {
    private:
        ::std::string name;

    public:
        explicit Identifier(::std::string name);
        ~Identifier() override = default;

        const ::std::string &getName() const {
            return name;
        }
        ACCEPT_VISITOR(Identifier)
    };

    /**
     * @class NumberLiteral
     * @brief 数字字面量
     */
    class NumberLiteral : public Expression {
    private:
        double value;

    public:
        explicit NumberLiteral(double value);
        ~NumberLiteral() override = default;

        double getValue() const {
            return value;
        }
        ACCEPT_VISITOR(NumberLiteral)
    };

    /**
     * @class StringLiteral
     * @brief 字符串字面量
     */
    class StringLiteral : public Expression {
    private:
        ::std::string value;

    public:
        explicit StringLiteral(::std::string value);
        ~StringLiteral() override = default;

        const ::std::string &getValue() const {
            return value;
        }
        ACCEPT_VISITOR(StringLiteral)
    };

    /**
     * @class BooleanLiteral
     * @brief 布尔字面量
     */
    class BooleanLiteral : public Expression {
    private:
        bool value;

    public:
        explicit BooleanLiteral(bool value);
        ~BooleanLiteral() override = default;

        bool getValue() const {
            return value;
        }
        ACCEPT_VISITOR(BooleanLiteral)
    };

    /**
     * @class FormatString
     * @brief 格式化字符串
     */
    class FormatString : public Expression {
    public:
        struct VariablePosition {
            int posInValue = 0;
            Expression *value;
        };

    private:
        ::std::string value;

        ::std::vector<VariablePosition> variables;

    public:
        explicit FormatString(::std::string value);
        ~FormatString() override = default;

        const ::std::string &getValue() const {
            return value;
        }
        const ::std::vector<VariablePosition> &getVariables() const {
            return variables;
        }
        ACCEPT_VISITOR(FormatString)
    };

    /**
     * @class RangeExpression
     * @brief range表达式
     */
    class RangeExpression : public Expression {
    private:
        ::std::vector<Expression *> arguments{};

    public:
        explicit RangeExpression(const ::std::vector<Expression *> &args);
        ~RangeExpression() override;

        const ::std::vector<Expression *> &getArguments() const {
            return arguments;
        }
        ACCEPT_VISITOR(RangeExpression)
    };

} // namespace AST

#endif // AST_HPP
