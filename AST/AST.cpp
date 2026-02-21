#include "AST.hpp"

#include <utility>

namespace AST {

    // Program
    Program::~Program() {
        for (auto stmt : statements) {
            delete stmt;
        }
    }

    void Program::addStatement(Statement *stmt) {
        if (stmt) {
            statements.push_back(stmt);
        }
    }

    // Block
    Block::~Block() {
        for (auto stmt : statements) {
            delete stmt;
        }
    }

    void Block::addStatement(Statement *stmt) {
        if (stmt) {
            statements.push_back(stmt);
        }
    }

    // Type
    Type::Type(std::string name) : name(std::move(name)) {
    }

    // ArrayType
    ArrayType::ArrayType(const std::string &elementType, Expression *size) : Type(elementType), size(size) {
    }

    ArrayType::~ArrayType() {
        delete size;
    }

    // Parameter
    Parameter::Parameter(std::string name, Type *type) : name(std::move(name)), type(type) {
    }

    Parameter::~Parameter() {
        delete type;
    }

    // Function
    Function::Function(std::string name, std::vector<Parameter *> *parameters, Type *returnType, Block *body)
        : name(std::move(name)), parameters(parameters), returnType(returnType), body(body) {
    }

    Function::~Function() {
        if (parameters) {
            for (auto param : *parameters) {
                delete param;
            }
            delete parameters;
        }
        delete returnType;
        delete body;
    }

    // ImportStatement
    ImportStatement::ImportStatement(std::string moduleName) : moduleName(std::move(moduleName)) {
    }

    // IfStatement
    IfStatement::IfStatement(Expression *condition, Statement *thenBranch, Statement *elseBranch)
        : condition(condition), thenBranch(thenBranch), elseBranch(elseBranch) {
    }

    IfStatement::~IfStatement() {
        delete condition;
        delete thenBranch;
        delete elseBranch;
    }

    // WhileStatement
    WhileStatement::WhileStatement(Expression *condition, Statement *body) : condition(condition), body(body) {
    }

    WhileStatement::~WhileStatement() {
        delete condition;
        delete body;
    }

    // ForStatement
    ForStatement::ForStatement(Statement *init, Expression *condition, Expression *increment, Statement *body)
        : init(init), condition(condition), increment(increment), body(body) {
    }

    ForStatement::~ForStatement() {
        delete init;
        delete condition;
        delete increment;
        delete body;
    }

    // ForInStatement
    ForInStatement::ForInStatement(std::string loopVariable, Expression *iterable, Block *body)
        : loopVariable(std::move(loopVariable)), iterable(iterable), body(body) {
    }

    ForInStatement::~ForInStatement() {
        delete iterable;
        delete body;
    }

    // ReturnStatement
    ReturnStatement::ReturnStatement(Expression *value) : value(value) {
    }

    ReturnStatement::~ReturnStatement() {
        delete value;
    }

    // Declaration
    Declaration::Declaration(std::string keyword, std::string name, Type *type, Expression *initializer)
        : keyword(std::move(keyword)), name(std::move(name)), type(type), initializer(initializer) {
    }

    Declaration::~Declaration() {
        delete type;
        delete initializer;
    }

    // ExpressionStatement
    ExpressionStatement::ExpressionStatement(Expression *expression) : expression(expression) {
    }

    ExpressionStatement::~ExpressionStatement() {
        delete expression;
    }

    // BinaryExpression
    BinaryExpression::BinaryExpression(Expression *left, std::string op, Expression *right)
        : left(left), op(std::move(op)), right(right) {
    }

    BinaryExpression::~BinaryExpression() {
        delete left;
        delete right;
    }

    // UnaryExpression
    UnaryExpression::UnaryExpression(std::string op, Expression *operand) : op(std::move(op)), operand(operand) {
    }

    UnaryExpression::~UnaryExpression() {
        delete operand;
    }

    // FunctionCall
    FunctionCall::FunctionCall(Expression *callee, std::vector<Expression *> *arguments)
        : callee(callee), arguments(arguments) {
    }

    FunctionCall::~FunctionCall() {
        delete callee;
        if (arguments) { // why the cursor ...?
            for (auto arg : *arguments) {
                delete arg;
            }
            delete arguments;
        }
    }

    // MemberAccess
    MemberAccess::MemberAccess(Expression *object, std::string member) : object(object), member(std::move(member)) {
    }

    MemberAccess::~MemberAccess() {
        delete object;
    }

    // ArrayIndex
    ArrayIndex::ArrayIndex(Expression *array, Expression *index) : array(array), index(index) {
    }

    ArrayIndex::~ArrayIndex() {
        delete array;
        delete index;
    }

    // GroupedExpression
    GroupedExpression::GroupedExpression(Expression *expression) : expression(expression) {
    }

    GroupedExpression::~GroupedExpression() {
        delete expression;
    }

    // Identifier
    Identifier::Identifier(std::string name) : name(std::move(name)) {
    }

    // NumberLiteral
    NumberLiteral::NumberLiteral(double value) : value(value) {
    }

    // StringLiteral
    StringLiteral::StringLiteral(std::string value) : value(std::move(value)) {
    }

    // BooleanLiteral
    BooleanLiteral::BooleanLiteral(bool value) : value(value) {
    }

    // FormatString
    FormatString::FormatString(std::string value) : value(std::move(value)) {
    }

    // RangeExpression
    RangeExpression::RangeExpression(const std::vector<Expression *> &args) : arguments(args) {
    }

    RangeExpression::~RangeExpression() {
        for (auto arg : arguments) {
            delete arg;
        }
    }

} // namespace AST
