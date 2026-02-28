#include "AST.hpp"

#include <iostream>
#include <utility>

namespace AST {

    // NOLINTBEGIN

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

    // ModuleStatement
    ModuleStatement::ModuleStatement(std::string moduleName) : moduleName(std::move(moduleName)) {
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
    ForStatement::ForStatement(std::string loopVariable, Expression *iterable, Block *body)
        : loopVariable(std::move(loopVariable)), iterable(iterable), body(body) {
    }

    ForStatement::~ForStatement() {
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
        if (arguments) {
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
    StringLiteral::StringLiteral(std::string value) : value(value) {
        std::string res;
        for (size_t i = 0; i < value.length(); i++) {
            if (value[i] == '\\' && i + 1 < value.length()) {
                switch (value[i + 1]) {
                case 'n':
                    res += '\n';
                    i++;
                    break;
                case 't':
                    res += '\t';
                    i++;
                    break;
                case '\\':
                    res += '\\';
                    i++;
                    break;
                case '"':
                    res += '"';
                    i++;
                    break;
                default:
                    res += value[i];
                }
            } else {
                res += value[i];
            }
        }
        this->value = res;
    }

    // BooleanLiteral
    BooleanLiteral::BooleanLiteral(bool value) : value(value) {
    }

    // FormatString
    Expression *FormatString::parseValue(const std::string &varName) {
        if (varName.empty())
            return nullptr;

        // 1. 尝试解析为字面量
        Expression *literal = tryParseLiteral(varName);
        if (literal)
            return literal;

        // 2. 解析为复杂表达式（标识符、成员访问、数组索引）
        return parseExpression(varName);
    }

    Expression *FormatString::tryParseLiteral(const std::string &str) {
        // 数字字面量
        bool isNumber = true;
        bool hasDot = false;
        for (char c : str) {
            if (c == '.') {
                if (hasDot) {
                    isNumber = false;
                    break;
                } // 多个小数点
                hasDot = true;
            } else if (!isdigit(c)) {
                isNumber = false;
                break;
            }
        }
        if (isNumber && !str.empty()) {
            try {
                double val = std::stod(str);
                return new NumberLiteral(val);
            } catch (...) {
            }
        }

        // 字符串字面量
        if (str.size() >= 2 && str.front() == '"' && str.back() == '"') {
            std::string content = str.substr(1, str.size() - 2);
            // 处理转义字符
            std::string unescaped;
            for (size_t i = 0; i < content.length(); i++) {
                if (content[i] == '\\' && i + 1 < content.length()) {
                    switch (content[i + 1]) {
                    case 'n':
                        unescaped += '\n';
                        i++;
                        break;
                    case 't':
                        unescaped += '\t';
                        i++;
                        break;
                    case '\\':
                        unescaped += '\\';
                        i++;
                        break;
                    case '"':
                        unescaped += '"';
                        i++;
                        break;
                    default:
                        unescaped += content[i];
                    }
                } else {
                    unescaped += content[i];
                }
            }
            return new StringLiteral(unescaped);
        }

        // 布尔字面量
        if (str == "true")
            return new BooleanLiteral(true);
        if (str == "false")
            return new BooleanLiteral(false);

        return nullptr;
    }

    Expression *FormatString::parseExpression(const std::string &expr) {
        // 从右向左解析，处理链式访问

        // 1. 先找最后一个 ']'（数组索引）
        size_t lastBracket = expr.rfind('[');
        if (lastBracket != std::string::npos) {
            size_t closingBracket = expr.find(']', lastBracket);
            if (closingBracket != std::string::npos && closingBracket == expr.length() - 1) {
                // 格式正确：...[...]
                std::string arrayPart = expr.substr(0, lastBracket);
                std::string indexPart = expr.substr(lastBracket + 1, closingBracket - lastBracket - 1);

                Expression *array = parseExpression(arrayPart); // 递归解析数组部分
                Expression *index = parseValue(indexPart);      // 解析索引表达式

                if (array && index) {
                    return new ArrayIndex(array, index);
                }
                delete array;
                delete index;
                return nullptr;
            }
        }

        // 2. 再找最后一个 '.'（成员访问）
        size_t lastDot = expr.rfind('.');
        if (lastDot != std::string::npos) {
            std::string objectPart = expr.substr(0, lastDot);
            std::string memberPart = expr.substr(lastDot + 1);

            // 成员名必须是有效的标识符
            bool validMember = true;
            for (char c : memberPart) {
                if (!isalnum(c) && c != '_') {
                    validMember = false;
                    break;
                }
            }

            if (validMember) {
                Expression *object = parseExpression(objectPart);
                if (object) {
                    return new MemberAccess(object, memberPart);
                }
                return nullptr;
            }
        }

        // 3. 检查是否是简单的标识符
        bool validIdentifier = true;
        if (expr.empty() || (!isalpha(expr[0]) && expr[0] != '_')) {
            validIdentifier = false;
        } else {
            for (char c : expr) {
                if (!isalnum(c) && c != '_') {
                    validIdentifier = false;
                    break;
                }
            }
        }

        if (validIdentifier) {
            return new Identifier(expr);
        }

        // 无法解析
        return nullptr;
    }

    FormatString::FormatString(::std::string value) : value(value) {
        std::string varName;
        bool inBrace = false;
        size_t startPos = 0;

        for (size_t i = 0; i < this->value.size(); ++i) {
            char c = this->value[i];

            if (c == '{' && !inBrace) {
                // 开始解析变量
                inBrace = true;
                varName.clear();
                startPos = i; // 记录 '{' 的位置
            } else if (c == '}' && inBrace) {
                // 结束解析变量
                inBrace = false;

                if (!varName.empty()) {
                    VariablePosition pos;
                    pos.posInValue = startPos; // 使用 '{' 的位置
                    // pos.value = new Identifier(varName);
                    pos.value = parseValue(varName);
                    if (pos.value) {
                        variables.push_back(pos);
                    } else {
                        throw std::runtime_error("Variable is not right!");
                    }
                }
            } else if (inBrace) {
                // 收集变量名
                varName += c;
            }
        }
        std::string res;
        for (size_t i = 0; i < value.length(); i++) {
            if (value[i] == '\\' && i + 1 < value.length()) {
                switch (value[i + 1]) {
                case 'n':
                    res += '\n';
                    i++;
                    break;
                case 't':
                    res += '\t';
                    i++;
                    break;
                case '\\':
                    res += '\\';
                    i++;
                    break;
                case '"':
                    res += '"';
                    i++;
                    break;
                default:
                    res += value[i];
                }
            } else {
                res += value[i];
            }
        }
        this->value = res;
    }

    // RangeExpression
    RangeExpression::RangeExpression(const std::vector<Expression *> &args) : arguments(args) {
    }

    RangeExpression::~RangeExpression() {
        for (auto arg : arguments) {
            delete arg;
        }
    }

    // NOLINTEND

} // namespace AST
