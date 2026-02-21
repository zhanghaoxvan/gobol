//
// Created by 35921 on 2026/1/16.
//

#ifndef AST_HPP
#define AST_HPP

#include <Lexer/Lexer.hpp>
#include <string>
#include <vector>

namespace AST {

    // 前向声明
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
    class ForInStatement;
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
     * @class ASTNode
     * @brief AST节点的基类
     */
    class ASTNode {
    public:
        virtual ~ASTNode() = default;
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
    };

    /**
     * @class Statement
     * @brief 语句基类
     */
    class Statement : public ASTNode {
    public:
        ~Statement() override = default;
    };

    /**
     * @class Expression
     * @brief 表达式基类
     */
    class Expression : public ASTNode {
    public:
        ~Expression() override = default;
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
    };

    /**
     * @class ForStatement
     * @brief for循环
     */
    class ForStatement : public Statement {
    private:
        Statement *init;
        Expression *condition;
        Expression *increment;
        Statement *body;

    public:
        ForStatement(Statement *init, Expression *condition, Expression *increment, Statement *body);
        ~ForStatement() override;

        Statement *getInit() const {
            return init;
        }
        Expression *getCondition() const {
            return condition;
        }
        Expression *getIncrement() const {
            return increment;
        }
        Statement *getBody() const {
            return body;
        }
    };

    /**
     * @class ForInStatement
     * @brief for...in循环
     */
    class ForInStatement : public Statement {
    private:
        ::std::string loopVariable;
        Expression *iterable;
        Block *body;

    public:
        ForInStatement(::std::string loopVariable, Expression *iterable, Block *body);
        ~ForInStatement() override;

        const ::std::string &getLoopVariable() const {
            return loopVariable;
        }
        Expression *getIterable() const {
            return iterable;
        }
        Block *getBody() const {
            return body;
        }
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
    };

    /**
     * @class BreakStatement
     * @brief break语句
     */
    class BreakStatement : public Statement {
    public:
        BreakStatement() = default;
        ~BreakStatement() override = default;
    };

    /**
     * @class ContinueStatement
     * @brief continue语句
     */
    class ContinueStatement : public Statement {
    public:
        ContinueStatement() = default;
        ~ContinueStatement() override = default;
    };

    /**
     * @class Declaration
     * @brief 变量声明
     */
    class Declaration : public Statement {
    private:
        ::std::string keyword; // var, let, const
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
    };

    /**
     * @class RangeExpression
     * @brief range表达式
     */
    class RangeExpression : public Expression {
    private:
        ::std::vector<Expression *> arguments;

    public:
        explicit RangeExpression(const ::std::vector<Expression *> &args);
        ~RangeExpression() override;

        const ::std::vector<Expression *> &getArguments() const {
            return arguments;
        }
    };

} // namespace AST

#endif // AST_HPP
