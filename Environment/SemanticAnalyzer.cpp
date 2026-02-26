#include "SemanticAnalyzer.hpp"

namespace analyzer {
    SemanticAnalyzer::SemanticAnalyzer()
        : hasError(false), currentFunction(""), currentFunctionReturnType(env::NONE), hasReturnStatement(false),
          loopDepth(0), currentModule("") {
    }

    SemanticAnalyzer::~SemanticAnalyzer() = default;

    bool SemanticAnalyzer::analyze(AST::Program *program) {
        if (!program) {
            error("Null program");
            return false;
        }

        // 注册内置模块
        env.declareModule("__builtins__");
        env.declareModule("io");

        // 声明 __builtins__ 模块的函数
        env.declareFunction("range", env::INT, "__builtins__");
        env.declareFunction("print", env::NONE, "__builtins__");
        env.declareFunction("len", env::INT, "__builtins__");

        // 声明 io 模块的函数
        env.declareFunction("print", env::NONE, "io");
        env.declareFunction("scan", env::STR, "io");
        env.declareFunction("read", env::STR, "io");

        // 开始分析
        program->accept(this);

#ifdef DEBUG
        printErrors();
#else
        if (hasError) {
            printErrors();
        }
#endif
        return !hasError;
    }

    void SemanticAnalyzer::printErrors() const {
        if (errors.empty()) {
            std::cout << "✅ Semantic analysis passed!" << std::endl;
        } else {
            std::cout << "❌ Semantic analysis failed with " << errors.size() << " errors:" << std::endl;
            for (const auto &err : errors) {
                std::cout << "  ⚠️  " << err << std::endl;
            }
        }
    }

    void SemanticAnalyzer::error(const std::string &msg) {
        errors.push_back(msg);
        hasError = true;
    }

    env::DataType SemanticAnalyzer::getDataTypeFromAST(AST::Type *type) {
        if (!type)
            return env::NONE;

        const std::string &name = type->getName();
        if (name == "int")
            return env::INT;
        if (name == "float")
            return env::FLOAT;
        if (name == "str")
            return env::STR;
        if (name == "bool")
            return env::BOOL;

        error("Unknown type: " + name);
        return env::UNKNOWN;
    }

    env::DataType SemanticAnalyzer::getCurrentType() {
        if (typeStack.empty())
            return env::UNKNOWN;
        return typeStack.top();
    }

    bool SemanticAnalyzer::checkTypeCompatibility(env::DataType target, env::DataType source,
                                                  const std::string &context) {
        if (env::Environment::isTypeCompatible(target, source)) {
            return true;
        }

        error("Type mismatch in " + context + ": expected " + env::dataTypeToString(target) + ", got " +
              env::dataTypeToString(source));
        return false;
    }

    // ==================== ASTVisitor 接口实现 ====================

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ASTNode) {
        // 默认不做任何事
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Statement) {
        // 默认不做任何事
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Expression) {
        // 默认不做任何事
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Program) {
        for (auto *stmt : node->getStatements()) {
            stmt->accept(this);
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ModuleStatement) {
        const std::string &moduleName = node->getModuleName();
#ifdef DEBUG
        std::cout << "  Module declaration: " << moduleName << std::endl;
#endif
        env.declareModule(moduleName);
        currentModule = moduleName;
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ImportStatement) {
        const std::string &moduleName = node->getModuleName();
#ifdef DEBUG
        std::cout << "  Import module: " << moduleName << std::endl;
#endif

        if (moduleName != "io" && moduleName != "__builtins__") {
            error("Unknown module: '" + moduleName + "'");
            return;
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Function) {
        const std::string &funcName = node->getName();
#ifdef DEBUG
        std::cout << "  Function: " << funcName << std::endl;
#endif

        // 获取返回类型
        env::DataType returnType = env::NONE;
        if (node->getReturnType()) {
            returnType = getDataTypeFromAST(node->getReturnType());
        }

        // 在 Environment 中声明函数（使用当前模块名）
        if (!env.declareFunction(funcName, returnType, currentModule)) {
            error("Failed to declare function '" + currentModule + "." + funcName + "'");
            return;
        }

        // 保存当前函数上下文
        std::string prevFunction = currentFunction;
        env::DataType prevReturnType = currentFunctionReturnType;
        bool prevHasReturn = hasReturnStatement;

        // 设置新函数上下文
        currentFunction = funcName;
        currentFunctionReturnType = returnType;
        hasReturnStatement = false;

        // 进入函数作用域
        env.enterScope();

        // 处理参数
        if (node->getParameters()) {
            for (auto *param : *node->getParameters()) {
                param->accept(this);
            }
        }

        // 处理函数体
        if (node->getBody()) {
            node->getBody()->accept(this);
        }

        // 检查返回值
        if (returnType != env::NONE && !hasReturnStatement) {
            error("Function '" + funcName + "' must return a value of type " + env::dataTypeToString(returnType));
        }

        // 退出函数作用域
        env.exitScope();

        // 恢复上下文
        currentFunction = prevFunction;
        currentFunctionReturnType = prevReturnType;
        hasReturnStatement = prevHasReturn;
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Parameter) {
        const std::string &paramName = node->getName();
        env::DataType paramType = getDataTypeFromAST(node->getType());

        env.declareVariable(paramName, paramType);
#ifdef DEBUG
        std::cout << "    Parameter: " << paramName << " : " << env::dataTypeToString(paramType) << std::endl;
#endif
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Block) {
        env.enterScope();
#ifdef DEBUG
        std::cout << "    Block (scope " << env.getCurrentScope() << ")" << std::endl;
#endif

        for (auto *stmt : node->getStatements()) {
            stmt->accept(this);
        }

        env.exitScope();
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Declaration) {
        const std::string &varName = node->getName();
        env::DataType declaredType = getDataTypeFromAST(node->getType());

        if (!env.declareVariable(varName, declaredType)) {
            error("Failed to declare variable '" + varName + "'");
            return;
        }

#ifdef DEBUG
        std::cout << "    Variable declaration: " << varName << " : " << env::dataTypeToString(declaredType)
                  << std::endl;
#endif

        if (node->getInitializer()) {
            node->getInitializer()->accept(this);
            env::DataType initType = getCurrentType();

            checkTypeCompatibility(declaredType, initType, "variable '" + varName + "' initialization");
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::IfStatement) {
#ifdef DEBUG
        std::cout << "    IfStatement" << std::endl;
#endif
        node->getCondition()->accept(this);
        env::DataType condType = getCurrentType();

        if (condType != env::BOOL && !env::Environment::isNumericType(condType)) {
            error("If condition must be boolean or numeric type");
        }

        if (node->getThenBranch()) {
            node->getThenBranch()->accept(this);
        }

        if (node->getElseBranch()) {
            node->getElseBranch()->accept(this);
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::WhileStatement) {
#ifdef DEBUG
        std::cout << "    WhileStatement" << std::endl;
#endif
        node->getCondition()->accept(this);
        env::DataType condType = getCurrentType();

        if (condType != env::BOOL && !env::Environment::isNumericType(condType)) {
            error("While condition must be boolean or numeric type");
        }

        loopDepth++;

        if (node->getBody()) {
            node->getBody()->accept(this);
        }

        loopDepth--;
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ForStatement) {
        const std::string &loopVar = node->getLoopVariable();

        env.enterScope();
#ifdef DEBUG
        std::cout << "    For loop (scope " << env.getCurrentScope() << ")" << std::endl;
#endif

        env.declareVariable(loopVar, env::INT);
#ifdef DEBUG
        std::cout << "      Loop variable: " << loopVar << " : int" << std::endl;
#endif

        if (node->getIterable()) {
            node->getIterable()->accept(this);
            env::DataType iterType = getCurrentType();

            if (iterType != env::INT) {
                error("For loop iterable must be range expression");
            }
        }

        loopDepth++;

        if (node->getBody()) {
            node->getBody()->accept(this);
        }

        loopDepth--;

        env.exitScope();
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ReturnStatement) {
        hasReturnStatement = true;

        if (currentFunction.empty()) {
            error("Return statement outside function");
            return;
        }

        if (!node->getValue()) {
            if (currentFunctionReturnType != env::NONE) {
                error("Function '" + currentFunction + "' expects return type " +
                      env::dataTypeToString(currentFunctionReturnType) + ", but got none");
            }
            return;
        }

        node->getValue()->accept(this);
        env::DataType returnType = getCurrentType();

        checkTypeCompatibility(currentFunctionReturnType, returnType, "function '" + currentFunction + "' return");
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::BreakStatement) {
        if (loopDepth == 0) {
            error("Break statement outside loop");
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ContinueStatement) {
        if (loopDepth == 0) {
            error("Continue statement outside loop");
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ExpressionStatement) {
        if (node->getExpression()) {
            node->getExpression()->accept(this);
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Identifier) {
        const std::string &name = node->getName();

        // 先查找当前模块的函数
        std::string fullName = currentModule + "." + name;
        auto *sym = env.lookupSymbol(fullName);

        // 没找到则在 __builtins__ 中查找
        if (!sym) {
            fullName = "__builtins__." + name;
            sym = env.lookupSymbol(fullName);
        }

        // 还没找到则查找普通变量
        if (!sym) {
            sym = env.lookupSymbol(name);
        }

        if (!sym) {
            error("Undeclared identifier: '" + name + "'");
            typeStack.push(env::UNKNOWN);
            return;
        }

        typeStack.push(sym->dataType);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::NumberLiteral) {
        double value = node->getValue();
        if (value == static_cast<int>(value)) {
            typeStack.push(env::INT);
        } else {
            typeStack.push(env::FLOAT);
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::StringLiteral) {
        typeStack.push(env::STR);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::BooleanLiteral) {
        typeStack.push(env::BOOL);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::FormatString) {
        for (const auto &var : node->getVariables()) {
            if (var.value) {
                var.value->accept(this);
            }
        }
        typeStack.push(env::STR);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::BinaryExpression) {
        node->getLeft()->accept(this);
        env::DataType leftType = getCurrentType();
        typeStack.pop();

        node->getRight()->accept(this);
        env::DataType rightType = getCurrentType();
        typeStack.pop();

        const std::string &op = node->getOperator();

        if (op == "=") {
            if (!dynamic_cast<AST::Identifier *>(node->getLeft())) {
                error("Left side of assignment must be a variable");
            }

            if (!env::Environment::isTypeCompatible(leftType, rightType)) {
                error("Cannot assign " + env::dataTypeToString(rightType) + " to " + env::dataTypeToString(leftType));
            }

            typeStack.push(leftType);
            return;
        }

        if (op == "+" || op == "-" || op == "*" || op == "/" || op == "%") {
            if (op == "+" && (leftType == env::STR || rightType == env::STR)) {
                typeStack.push(env::STR);
                return;
            }

            if (!env::Environment::isNumericType(leftType) || !env::Environment::isNumericType(rightType)) {
                error("Operator '" + op + "' requires numeric operands");
                typeStack.push(env::UNKNOWN);
                return;
            }

            if (leftType == env::FLOAT || rightType == env::FLOAT) {
                typeStack.push(env::FLOAT);
            } else {
                typeStack.push(env::INT);
            }
            return;
        }

        if (op == "==" || op == "!=" || op == "<" || op == ">" || op == "<=" || op == ">=") {
            if (!env::Environment::isTypeCompatible(leftType, rightType) &&
                !env::Environment::isTypeCompatible(rightType, leftType)) {
                error("Cannot compare " + env::dataTypeToString(leftType) + " and " + env::dataTypeToString(rightType));
            }

            typeStack.push(env::BOOL);
            return;
        }

        if (op == "&&" || op == "||") {
            if (leftType != env::BOOL || rightType != env::BOOL) {
                error("Logical operators require boolean operands");
            }
            typeStack.push(env::BOOL);
            return;
        }

        error("Unknown operator: " + op);
        typeStack.push(env::UNKNOWN);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::UnaryExpression) {
        node->getOperand()->accept(this);
        env::DataType operandType = getCurrentType();

        const std::string &op = node->getOperator();

        if (op == "-" || op == "+") {
            if (!env::Environment::isNumericType(operandType)) {
                error("Unary operator '" + op + "' requires numeric operand");
            }
            typeStack.push(operandType);
        } else if (op == "!") {
            if (operandType != env::BOOL) {
                error("Logical not '!' requires boolean operand");
            }
            typeStack.push(env::BOOL);
        } else {
            error("Unknown unary operator: " + op);
            typeStack.push(env::UNKNOWN);
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::FunctionCall) {
        std::string funcName;
        std::string moduleName = currentModule;

        if (auto *id = dynamic_cast<AST::Identifier *>(node->getCallee())) {
            funcName = id->getName();
        } else if (auto *member = dynamic_cast<AST::MemberAccess *>(node->getCallee())) {
            if (auto *obj = dynamic_cast<AST::Identifier *>(member->getObject())) {
                moduleName = obj->getName();
                funcName = member->getMember();
            }
        }

        // 构建完整函数名
        std::string fullName = moduleName + "." + funcName;
        auto *sym = env.lookupSymbol(fullName);

        if (!sym) {
            error("Undeclared function: '" + fullName + "'");
            typeStack.push(env::UNKNOWN);
            return;
        }

        // 处理参数
        if (node->getArguments()) {
            for (auto *arg : *node->getArguments()) {
                arg->accept(this);
                typeStack.pop();
            }
        }

        typeStack.push(sym->dataType);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::MemberAccess) {
        node->getObject()->accept(this);
        env::DataType objType = getCurrentType();
        typeStack.pop();

        if (auto *id = dynamic_cast<AST::Identifier *>(node->getObject())) {
            const std::string &moduleName = id->getName();
            const std::string &member = node->getMember();

            std::string fullName = moduleName + "." + member;
            auto *sym = env.lookupSymbol(fullName);

            if (!sym) {
                error("Module '" + moduleName + "' has no member '" + member + "'");
                typeStack.push(env::UNKNOWN);
                return;
            }

            typeStack.push(sym->dataType);
            return;
        }

        error("Member access left side must be an identifier");
        typeStack.push(env::UNKNOWN);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::RangeExpression) {
        for (auto *arg : node->getArguments()) {
            arg->accept(this);
            env::DataType argType = getCurrentType();
            typeStack.pop();

            if (!env::Environment::isNumericType(argType)) {
                error("Range arguments must be numeric");
            }
        }

        typeStack.push(env::INT);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::GroupedExpression) {
        node->getExpression()->accept(this);
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::Type) {
        // 类型节点本身不需要处理
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ArrayType) {
        if (node->getSize()) {
            node->getSize()->accept(this);
            env::DataType sizeType = getCurrentType();
            typeStack.pop();

            if (sizeType != env::INT) {
                error("Array size must be integer");
            }
        }
    }

    VISIT_ASTNODEO(SemanticAnalyzer, AST::ArrayIndex) {
        node->getArray()->accept(this);
        env::DataType arrayType = getCurrentType();
        typeStack.pop();

        node->getIndex()->accept(this);
        env::DataType indexType = getCurrentType();
        typeStack.pop();

        if (indexType != env::INT) {
            error("Array index must be integer");
        }

        typeStack.push(arrayType);
    }
} // namespace analyzer
