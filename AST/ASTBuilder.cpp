#include "ASTBuilder.hpp"
#include <cstdlib>
#include <iostream>
#include <regex>

namespace AST {

    // 构造函数
    ASTBuilder::ASTBuilder(lexer::Lexer lexer) : root(nullptr), currentPosition(0), errorOccurred(false) {
        auto tk = lexer.getNextToken();
        while (tk.type != lexer::token::TokenType::END_OF_FILE) {
        }
    }

    // 析构函数
    ASTBuilder::~ASTBuilder() {
        delete root;
    }

    // 辅助函数
    const lexer::token::Token &ASTBuilder::currentToken() const {
        if (currentPosition >= tokens.size()) {
            static lexer::token::Token eofToken{lexer::token::TokenType::END_OF_FILE, ""};
            return eofToken;
        }
        return tokens[currentPosition];
    }

    const lexer::token::Token &ASTBuilder::peekNextToken() const {
        if (currentPosition + 1 >= tokens.size()) {
            static lexer::token::Token eofToken{lexer::token::TokenType::END_OF_FILE, ""};
            return eofToken;
        }
        return tokens[currentPosition + 1];
    }

    void ASTBuilder::advance() {
        if (currentPosition < tokens.size()) {
            currentPosition++;
        }
    }

    bool ASTBuilder::match(lexer::token::TokenType type) const {
        return currentToken().type == type;
    }

    bool ASTBuilder::matchValue(const std::string &value) const {
        return currentToken().value == value;
    }

    bool ASTBuilder::isEndOfLine() const {
        return match(lexer::token::TokenType::END_OF_LINE);
    }

    void ASTBuilder::consumeEndOfLine() {
        while (isEndOfLine()) {
            advance();
        }
    }

    lexer::token::Token ASTBuilder::consume(lexer::token::TokenType type, const std::string &errorMessage) {
        if (match(type)) {
            auto token = currentToken();
            advance();
            return token;
        }
        logError(errorMessage);
        return currentToken();
    }

    lexer::token::Token ASTBuilder::consumeValue(const std::string &value, const std::string &errorMessage) {
        if (matchValue(value)) {
            auto token = currentToken();
            advance();
            return token;
        }
        logError(errorMessage);
        return currentToken();
    }

    void ASTBuilder::logError(const std::string &message) {
        errorOccurred = true;
        errorMessage = message;
        std::cerr << "ASTBuilder Error: " << message << std::endl;
    }

    // 主解析函数
    ASTNode *ASTBuilder::build() {

        delete root;

        root = parseProgram();
        return root;
    }

    void ASTBuilder::reset() {
        if (root != nullptr) {
            delete root;
            root = nullptr;
        }
        currentPosition = 0;
        errorOccurred = false;
        errorMessage = "";
    }

    // 程序解析
    Program *ASTBuilder::parseProgram() {
        auto *program = new Program();

        while (!match(lexer::token::TokenType::END_OF_FILE) && !errorOccurred) {
            // 跳过空行
            consumeEndOfLine();

            if (match(lexer::token::TokenType::END_OF_FILE))
                break;

            Statement *stmt = parseStatement();
            if (stmt != nullptr) {
                program->addStatement(stmt);
            } else {
                // 如果无法解析，跳过当前token避免无限循环
                advance();
            }
        }

        return program;
    }

    // 语句解析
    Statement *ASTBuilder::parseStatement() {
        // 处理关键字
        if (match(lexer::token::TokenType::KEYWORD)) {
            std::string keyword = currentToken().value;

            if (keyword == "import") {
                return parseImport();
            } else if (keyword == "func") {
                return parseFunction();
            } else if (keyword == "var" || keyword == "let" || keyword == "const") {
                return parseDeclaration();
            } else if (keyword == "for") {
                // 检查是普通for还是for...in
                size_t savedPos = currentPosition;
                advance(); // 消费'for'

                // 检查下一个token是否是标识符且再下一个是'in'
                if (match(lexer::token::TokenType::IDENTIFIER) && peekNextToken().value == "in") {
                    currentPosition = savedPos; // 回退
                    return parseForInStatement();
                } else {
                    currentPosition = savedPos; // 回退
                    return parseForStatement();
                }
            } else if (keyword == "return") {
                return parseReturnStatement();
            } else if (keyword == "if") {
                return parseIfStatement();
            } else if (keyword == "while") {
                return parseWhileStatement();
            } else if (keyword == "break") {
                return parseBreakStatement();
            } else if (keyword == "continue") {
                return parseContinueStatement();
            }
        }

        // 如果不是关键字，可能是表达式语句
        if (match(lexer::token::TokenType::IDENTIFIER) || match(lexer::token::TokenType::NUMBER) ||
            match(lexer::token::TokenType::STRING) || match(lexer::token::TokenType::FORMAT_STRING)) {
            return parseExpressionStatement();
        }

        // 处理块结束
        if (match(lexer::token::TokenType::OPERATOR) && (currentToken().value == "}" || currentToken().value == ")")) {
            return nullptr;
        }

        logError("Unexpected token: " + currentToken().value);
        return nullptr;
    }

    // 导入语句解析
    Statement *ASTBuilder::parseImport() {
        advance(); // 消费 'import'

        if (!match(lexer::token::TokenType::IDENTIFIER)) {
            logError("Expected identifier after 'import'");
            return nullptr;
        }

        std::string moduleName = currentToken().value;
        advance();

        consumeEndOfLine();

        return new ImportStatement(moduleName);
    }

    // 函数解析
    Function *ASTBuilder::parseFunction() {
        advance(); // 消费 'func'

        // 函数名
        if (!match(lexer::token::TokenType::IDENTIFIER)) {
            logError("Expected function name");
            return nullptr;
        }

        std::string funcName = currentToken().value;
        advance();

        // 参数列表
        consumeValue("(", "Expected '(' after function name");

        std::vector<Parameter *> *params = parseParameterList();

        consumeValue(")", "Expected ')' after parameters");

        // 返回类型
        Type *returnType = nullptr;
        if (matchValue(":")) {
            advance(); // 消费 ':'
            returnType = parseType();
        }

        // 函数体
        consumeValue("{", "Expected '{' at start of function body");
        consumeEndOfLine();

        Block *body = parseBlock();

        consumeValue("}", "Expected '}' at end of function body");
        consumeEndOfLine();

        return new Function(funcName, params, returnType, body);
    }

    // 参数列表解析
    std::vector<Parameter *> *ASTBuilder::parseParameterList() {
        auto *params = new std::vector<Parameter *>();

        // 解析参数
        if (!matchValue(")")) {
            do {
                Parameter *param = parseParameter();
                if (param) {
                    params->push_back(param);
                }

                if (matchValue(",")) {
                    advance(); // 消费 ','
                } else {
                    break;
                }
            } while (!matchValue(")") && !errorOccurred);
        }

        return params;
    }

    // 单个参数解析
    Parameter *ASTBuilder::parseParameter() {
        if (!match(lexer::token::TokenType::IDENTIFIER)) {
            logError("Expected parameter name");
            return nullptr;
        }

        std::string paramName = currentToken().value;
        advance();

        // 参数类型
        Type *paramType = nullptr;
        if (matchValue(":")) {
            advance(); // 消费 ':'
            paramType = parseType();
        }

        return new Parameter(paramName, paramType);
    }

    // 类型解析
    Type *ASTBuilder::parseType() {
        if (!match(lexer::token::TokenType::KEYWORD) && !match(lexer::token::TokenType::IDENTIFIER)) {
            logError("Expected type name");
            return nullptr;
        }

        std::string typeName = currentToken().value;
        advance();

        return new Type(typeName);
    }

    // 代码块解析
    Block *ASTBuilder::parseBlock() {
        auto *block = new Block();

        while (!matchValue("}") && !match(lexer::token::TokenType::END_OF_FILE) && !errorOccurred) {
            consumeEndOfLine();

            if (matchValue("}"))
                break;

            Statement *stmt = parseStatement();
            if (stmt != nullptr) {
                block->addStatement(stmt);
            }

            consumeEndOfLine();
        }

        return block;
    }

    // 变量声明解析
    Statement *ASTBuilder::parseDeclaration() {
        std::string keyword = currentToken().value; // var, let, const
        advance();

        // 变量名
        if (!match(lexer::token::TokenType::IDENTIFIER)) {
            logError("Expected identifier in declaration");
            return nullptr;
        }

        std::string varName = currentToken().value;
        advance();

        // 类型注解
        Type *varType = nullptr;
        if (matchValue(":")) {
            advance(); // 消费 ':'
            varType = parseType();
        }

        // 初始化表达式
        Expression *initializer = nullptr;
        if (matchValue("=")) {
            advance(); // 消费 '='
            initializer = parseExpression();
        }

        consumeEndOfLine();

        return new Declaration(keyword, varName, varType, initializer);
    }

    // 表达式语句解析
    Statement *ASTBuilder::parseExpressionStatement() {
        Expression *expr = parseExpression();
        if (expr != nullptr) {
            consumeEndOfLine();
            return new ExpressionStatement(expr);
        }
        return nullptr;
    }

    // Return语句解析
    Statement *ASTBuilder::parseReturnStatement() {
        advance(); // 消费 'return'

        Expression *value = nullptr;
        if (!isEndOfLine() && !matchValue("}")) {
            value = parseExpression();
        }

        consumeEndOfLine();

        return new ReturnStatement(value);
    }

    // For...in循环解析
    Statement *ASTBuilder::parseForInStatement() {
        advance(); // 消费 'for'

        // 循环变量
        if (!match(lexer::token::TokenType::IDENTIFIER)) {
            logError("Expected identifier in for loop");
            return nullptr;
        }

        std::string loopVar = currentToken().value;
        advance();

        // 'in' 关键字
        if (!match(lexer::token::TokenType::IDENTIFIER) || currentToken().value != "in") {
            logError("Expected 'in' in for loop");
            return nullptr;
        }
        advance();

        // range表达式
        Expression *rangeExpr = parseRange();

        // 循环体
        consumeValue("{", "Expected '{' at start of loop body");
        consumeEndOfLine();

        Block *body = parseBlock();

        consumeValue("}", "Expected '}' at end of loop body");
        consumeEndOfLine();

        return new ForInStatement(loopVar, rangeExpr, body);
    }

    // Range表达式解析
    Expression *ASTBuilder::parseRange() {
        if (!match(lexer::token::TokenType::IDENTIFIER) || currentToken().value != "range") {
            logError("Expected 'range'");
            return nullptr;
        }
        advance();

        consumeValue("(", "Expected '(' after 'range'");

        std::vector<Expression *> args;

        // 解析参数
        while (!matchValue(")") && !errorOccurred) {
            Expression *arg = parseExpression();
            if (arg) {
                args.push_back(arg);
            }

            if (matchValue(",")) {
                advance();
            } else {
                break;
            }
        }

        consumeValue(")", "Expected ')' after range arguments");

        return new RangeExpression(args);
    }

    // 格式化字符串解析
    Expression *ASTBuilder::parseFormatString(const std::string &formatStr) {
        // 解析格式化字符串中的变量引用 {name}
        // 由FormatString处理细节
        return new FormatString(formatStr);
    }

    // 表达式解析 - 递归下降
    Expression *ASTBuilder::parseExpression() {
        return parseAssignment();
    }

    Expression *ASTBuilder::parseAssignment() {
        Expression *expr = parseLogicalOr();

        if (matchValue("=")) {
            advance();
            Expression *value = parseAssignment();
            expr = new BinaryExpression(expr, "=", value);
        }

        return expr;
    }

    Expression *ASTBuilder::parseLogicalOr() {
        Expression *expr = parseLogicalAnd();

        while (matchValue("||")) {
            std::string op = currentToken().value;
            advance();
            Expression *right = parseLogicalAnd();
            expr = new BinaryExpression(expr, op, right);
        }

        return expr;
    }

    Expression *ASTBuilder::parseLogicalAnd() {
        Expression *expr = parseEquality();

        while (matchValue("&&")) {
            std::string op = currentToken().value;
            advance();
            Expression *right = parseEquality();
            expr = new BinaryExpression(expr, op, right);
        }

        return expr;
    }

    Expression *ASTBuilder::parseEquality() {
        Expression *expr = parseComparison();

        while (matchValue("==") || matchValue("!=")) {
            std::string op = currentToken().value;
            advance();
            Expression *right = parseComparison();
            expr = new BinaryExpression(expr, op, right);
        }

        return expr;
    }

    Expression *ASTBuilder::parseComparison() {
        Expression *expr = parseAdditive();

        while (matchValue("<") || matchValue("<=") || matchValue(">") || matchValue(">=")) {
            std::string op = currentToken().value;
            advance();
            Expression *right = parseAdditive();
            expr = new BinaryExpression(expr, op, right);
        }

        return expr;
    }

    Expression *ASTBuilder::parseAdditive() {
        Expression *expr = parseMultiplicative();

        while (matchValue("+") || matchValue("-")) {
            std::string op = currentToken().value;
            advance();
            Expression *right = parseMultiplicative();
            expr = new BinaryExpression(expr, op, right);
        }

        return expr;
    }

    Expression *ASTBuilder::parseMultiplicative() { // 成功了！！
        Expression *expr = parseUnary();

        while (matchValue("*") || matchValue("/") || matchValue("%")) {
            std::string op = currentToken().value;
            advance();
            Expression *right = parseUnary();
            expr = new BinaryExpression(expr, op, right);
        }

        return expr;
    }

    Expression *ASTBuilder::parseUnary() {
        if (matchValue("!") || matchValue("-") || matchValue("+")) {
            std::string op = currentToken().value;
            advance();
            Expression *operand = parseUnary();
            return new UnaryExpression(op, operand);
        }

        return parsePostfix();
    }

    Expression *ASTBuilder::parsePostfix() {
        Expression *expr = parsePrimary();

        while (true) {
            if (matchValue(".")) {
                // 成员访问
                advance();
                if (!match(lexer::token::TokenType::IDENTIFIER)) {
                    logError("Expected identifier after '.'");
                    return expr;
                }
                std::string member = currentToken().value;
                advance();
                expr = new MemberAccess(expr, member);
            } else if (matchValue("(")) {
                // 函数调用
                expr = parseFunctionCall(expr);
            } else {
                break;
            }
        }

        return expr;
    }

    Expression *ASTBuilder::parsePrimary() {
        if (match(lexer::token::TokenType::IDENTIFIER)) {
            std::string name = currentToken().value;
            advance();
            return new Identifier(name);
        }

        if (match(lexer::token::TokenType::NUMBER)) {
            double value = std::stod(currentToken().value);
            advance();
            return new NumberLiteral(value);
        }

        if (match(lexer::token::TokenType::STRING)) {
            std::string value = currentToken().value;
            advance();
            return new StringLiteral(value);
        }

        if (match(lexer::token::TokenType::FORMAT_STRING)) {
            std::string value = currentToken().value;
            advance();
            return parseFormatString(value);
        }

        if (match(lexer::token::TokenType::KEYWORD)) {
            std::string value = currentToken().value;
            if (value == "true" || value == "false") {
                advance();
                return new BooleanLiteral(value == "true");
            }
        }

        if (matchValue("(")) {
            advance();
            Expression *expr = parseExpression();
            consumeValue(")", "Expected ')' after expression");
            return new GroupedExpression(expr);
        }

        logError("Unexpected token in expression: " + currentToken().value);
        return nullptr;
    }

    // 函数调用解析
    Expression *ASTBuilder::parseFunctionCall(Expression *callee) {
        consumeValue("(", "Expected '(' in function call");

        std::vector<Expression *> *args = parseArgumentList();

        consumeValue(")", "Expected ')' after arguments");

        return new FunctionCall(callee, args);
    }

    // 参数列表解析（用于函数调用）
    std::vector<Expression *> *ASTBuilder::parseArgumentList() {
        auto *args = new std::vector<Expression *>();

        if (!matchValue(")")) {
            do {
                Expression *arg = parseExpression();
                if (arg) {
                    args->push_back(arg);
                }

                if (matchValue(",")) {
                    advance();
                } else {
                    break;
                }
            } while (!matchValue(")") && !errorOccurred);
        }

        return args;
    }

    // 暂未实现的语句解析函数
    Statement *ASTBuilder::parseIfStatement() {
        logError("If statement not yet implemented");
        return nullptr;
    }

    Statement *ASTBuilder::parseWhileStatement() {
        logError("While statement not yet implemented");
        return nullptr;
    }

    Statement *ASTBuilder::parseForStatement() {
        logError("For statement not yet implemented");
        return nullptr;
    }

    Statement *ASTBuilder::parseBreakStatement() {
        logError("Break statement not yet implemented");
        return nullptr;
    }

    Statement *ASTBuilder::parseContinueStatement() {
        logError("Continue statement not yet implemented");
        return nullptr;
    }

} // namespace AST
