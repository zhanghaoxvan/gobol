#include "Interpreter.hpp"

namespace interpreter {

    // ==================== FunctionValue 实现 ====================
    FunctionValue::FunctionValue() : body(nullptr), closureLevel(0) {
    }

    FunctionValue::FunctionValue(std::string n, const std::vector<std::string> &p, AST::Block *b, int level)
        : name(std::move(n)), params(p), body(b), closureLevel(level) {
    }

    // ==================== RuntimeValue 实现 ====================
    RuntimeValue::RuntimeValue() : value(std::monostate{}), type(TypeKind::NONE) {
    }
    RuntimeValue::RuntimeValue(int v) : value(v), type(TypeKind::INT) {
    }
    RuntimeValue::RuntimeValue(double v) : value(v), type(TypeKind::FLOAT) {
    }
    RuntimeValue::RuntimeValue(bool v) : value(v), type(TypeKind::BOOL) {
    }
    RuntimeValue::RuntimeValue(const std::string &v) : value(v), type(TypeKind::STR) {
    }
    RuntimeValue::RuntimeValue(const FunctionValue &v) : value(v), type(TypeKind::FUNCTION) {
    }
    RuntimeValue::RuntimeValue(const RangeData &v) : value(v), type(TypeKind::RANGE) {
    }

    bool RuntimeValue::isInt() const {
        return type == TypeKind::INT;
    }
    bool RuntimeValue::isFloat() const {
        return type == TypeKind::FLOAT;
    }
    bool RuntimeValue::isBool() const {
        return type == TypeKind::BOOL;
    }
    bool RuntimeValue::isStr() const {
        return type == TypeKind::STR;
    }
    bool RuntimeValue::isNone() const {
        return type == TypeKind::NONE;
    }
    bool RuntimeValue::isFunction() const {
        return type == TypeKind::FUNCTION;
    }
    bool RuntimeValue::isRange() const {
        return type == TypeKind::RANGE;
    }

    int RuntimeValue::getInt() const {
        if (isInt())
            return std::get<int>(value);
        throw std::bad_variant_access();
    }

    double RuntimeValue::getFloat() const {
        if (isFloat())
            return std::get<double>(value);
        if (isInt())
            return static_cast<double>(std::get<int>(value));
        throw std::bad_variant_access();
    }

    bool RuntimeValue::getBool() const {
        if (isBool())
            return std::get<bool>(value);
        throw std::bad_variant_access();
    }

    const std::string &RuntimeValue::getStr() const {
        if (isStr())
            return std::get<std::string>(value);
        throw std::bad_variant_access();
    }

    const FunctionValue &RuntimeValue::getFunction() const {
        if (isFunction())
            return std::get<FunctionValue>(value);
        throw std::bad_variant_access();
    }

    const RuntimeValue::RangeData &RuntimeValue::getRange() const {
        if (isRange())
            return std::get<RangeData>(value);
        throw std::bad_variant_access();
    }

    std::string RuntimeValue::toString() const {
        switch (type) {
        case TypeKind::INT:
            return std::to_string(std::get<int>(value));
        case TypeKind::FLOAT:
            return std::to_string(std::get<double>(value));
        case TypeKind::BOOL:
            return std::get<bool>(value) ? "true" : "false";
        case TypeKind::STR:
            return "\"" + std::get<std::string>(value) + "\"";
        case TypeKind::NONE:
            return "none";
        case TypeKind::FUNCTION:
            return "<function " + std::get<FunctionValue>(value).name + ">";
        case TypeKind::RANGE: {
            auto &r = std::get<RangeData>(value);
            return "<range " + std::to_string(r.start) + ":" + std::to_string(r.end) + ":" + std::to_string(r.step) +
                   ">";
        }
        default:
            return "unknown";
        }
    }

    // ==================== Interpreter 实现 ====================
    Interpreter::Interpreter() : returnFlag(false), breakFlag(false), continueFlag(false), loopDepth(0) {
        environments.emplace_back(); // 全局作用域
        initBuiltins();
    }

    void Interpreter::enterScope() {
        environments.emplace_back();
    }

    void Interpreter::exitScope() {
        if (!environments.empty()) {
            environments.pop_back();
        }
    }

    bool Interpreter::declareVariable(const std::string &name, const RuntimeValue &value) {
        if (environments.empty()) {
            environments.emplace_back();
        }

        auto &currentScope = environments.back();
        if (currentScope.find(name) != currentScope.end()) {
            std::cerr << "Runtime Error: Variable '" << name << "' already declared" << std::endl;
            return false;
        }

        currentScope[name] = value;
        return true;
    }

    bool Interpreter::setVariable(const std::string &name, const RuntimeValue &value) {
        for (int i = environments.size() - 1; i >= 0; i--) {
            auto it = environments[i].find(name);
            if (it != environments[i].end()) {
                it->second = value;
                return true;
            }
        }
        std::cerr << "Runtime Error: Variable '" << name << "' not declared" << std::endl;
        return false;
    }

    RuntimeValue *Interpreter::getVariable(const std::string &name) {
        for (int i = environments.size() - 1; i >= 0; i--) {
            auto it = environments[i].find(name);
            if (it != environments[i].end()) {
                return &(it->second);
            }
        }
        return nullptr;
    }

    void Interpreter::initBuiltins() {
        builtins["print"] = [](const std::vector<RuntimeValue> &args) -> RuntimeValue {
            for (size_t i = 0; i < args.size(); i++) {
                if (i > 0)
                    std::cout << " ";
                const auto &arg = args[i];
                switch (arg.type) {
                case RuntimeValue::TypeKind::INT:
                    std::cout << arg.getInt();
                    break;
                case RuntimeValue::TypeKind::FLOAT:
                    std::cout << arg.getFloat();
                    break;
                case RuntimeValue::TypeKind::BOOL:
                    std::cout << (arg.getBool() ? "true" : "false");
                    break;
                case RuntimeValue::TypeKind::STR:
                    std::cout << arg.getStr();
                    break;
                case RuntimeValue::TypeKind::NONE:
                    std::cout << "none";
                    break;
                default:
                    std::cout << "?";
                    break;
                }
            }
            std::cout << std::endl;
            return RuntimeValue();
        };

        builtins["len"] = [](const std::vector<RuntimeValue> &args) -> RuntimeValue {
            if (args.empty()) {
                std::cerr << "Runtime Error: len() expects 1 argument" << std::endl;
                return RuntimeValue();
            }
            if (!args[0].isStr()) {
                std::cerr << "Runtime Error: len() expects string argument" << std::endl;
                return RuntimeValue();
            }
            return RuntimeValue(static_cast<int>(args[0].getStr().length()));
        };

        builtins["int"] = [](const std::vector<RuntimeValue> &args) -> RuntimeValue {
            if (args.empty())
                return RuntimeValue();
            const auto &arg = args[0];
            switch (arg.type) {
            case RuntimeValue::TypeKind::INT:
                return arg;
            case RuntimeValue::TypeKind::FLOAT:
                return RuntimeValue(static_cast<int>(arg.getFloat()));
            case RuntimeValue::TypeKind::BOOL:
                return RuntimeValue(arg.getBool() ? 1 : 0);
            case RuntimeValue::TypeKind::STR:
                try {
                    return RuntimeValue(std::stoi(arg.getStr()));
                } catch (...) {
                    return RuntimeValue();
                }
            default:
                return RuntimeValue();
            }
        };

        builtins["float"] = [](const std::vector<RuntimeValue> &args) -> RuntimeValue {
            if (args.empty())
                return RuntimeValue();
            const auto &arg = args[0];
            switch (arg.type) {
            case RuntimeValue::TypeKind::INT:
                return RuntimeValue(static_cast<double>(arg.getInt()));
            case RuntimeValue::TypeKind::FLOAT:
                return arg;
            case RuntimeValue::TypeKind::BOOL:
                return RuntimeValue(arg.getBool() ? 1.0 : 0.0);
            case RuntimeValue::TypeKind::STR:
                try {
                    return RuntimeValue(std::stod(arg.getStr()));
                } catch (...) {
                    return RuntimeValue();
                }
            default:
                return RuntimeValue();
            }
        };

        builtins["str"] = [](const std::vector<RuntimeValue> &args) -> RuntimeValue {
            if (args.empty())
                return RuntimeValue("");
            return RuntimeValue(args[0].toString());
        };
    }

    bool Interpreter::execute(AST::Program *program) {
        if (!program) {
            std::cerr << "Runtime Error: Null program" << std::endl;
            return false;
        }

        std::cout << "Program Started." << std::endl;

        try {
            program->accept(this);
        } catch (const std::exception &e) {
            std::cerr << "Runtime Error: " << e.what() << std::endl;
            return false;
        }

        std::cout << "Program Finished." << std::endl;

        return true;
    }

    // ==================== ASTVisitor 实现 ====================
    VISIT_ASTNODEO(Interpreter, AST::ASTNode) {
    }
    VISIT_ASTNODEO(Interpreter, AST::Statement) {
    }
    VISIT_ASTNODEO(Interpreter, AST::Expression) {
    }

    VISIT_ASTNODEO(Interpreter, AST::Program) {
        for (auto stmt : node->getStatements()) {
            if (stmt)
                stmt->accept(this);
            if (returnFlag)
                break;
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::Block) {
        enterScope();
        for (auto stmt : node->getStatements()) {
            if (stmt)
                stmt->accept(this);
            if (returnFlag || breakFlag || continueFlag)
                break;
        }
        exitScope();
    }

    VISIT_ASTNODEO(Interpreter, AST::Declaration) {
        const std::string &name = node->getName();
        RuntimeValue value;

        if (node->getInitializer()) {
            node->getInitializer()->accept(this);
            value = valueStack.top();
            valueStack.pop();
        }

        declareVariable(name, value);
    }

    VISIT_ASTNODEO(Interpreter, AST::ExpressionStatement) {
        if (node->getExpression()) {
            node->getExpression()->accept(this);
            valueStack.pop();
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::NumberLiteral) {
        double val = node->getValue();
        if (val == static_cast<int>(val)) {
            valueStack.push(RuntimeValue(static_cast<int>(val)));
        } else {
            valueStack.push(RuntimeValue(val));
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::StringLiteral) {
        valueStack.push(RuntimeValue(node->getValue()));
    }

    VISIT_ASTNODEO(Interpreter, AST::BooleanLiteral) {
        valueStack.push(RuntimeValue(node->getValue()));
    }

    VISIT_ASTNODEO(Interpreter, AST::FormatString) {
        std::string result = node->getValue();
        const auto &vars = node->getVariables();

        for (int i = vars.size() - 1; i >= 0; i--) {
            const auto &var = vars[i];
            var.value->accept(this);
            RuntimeValue val = valueStack.top();
            valueStack.pop();

            std::string valStr = val.toString();
            if (val.isStr() && valStr.front() == '"' && valStr.back() == '"') {
                valStr = valStr.substr(1, valStr.length() - 2);
            }

            std::string varName;
            size_t start = var.posInValue + 1; // 跳过 '{'
            size_t end = result.find('}', start);
            if (end != std::string::npos) {
                varName = result.substr(start, end - start);
            }
            result.replace(var.posInValue, varName.length() + 2, valStr);
        }

        valueStack.push(RuntimeValue(result));
    }

    VISIT_ASTNODEO(Interpreter, AST::Identifier) {
        const std::string &name = node->getName();
        RuntimeValue *val = getVariable(name);
        if (val) {
            valueStack.push(*val);
        } else {
            std::cerr << "Runtime Error: Undefined variable '" << name << "'" << std::endl;
            valueStack.push(RuntimeValue());
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::BinaryExpression) {
        const std::string &op = node->getOperator();

        if (op == "=") {
            auto *left = dynamic_cast<AST::Identifier *>(node->getLeft());
            if (!left) {
                std::cerr << "Runtime Error: Left side of assignment must be identifier" << std::endl;
                valueStack.push(RuntimeValue());
                return;
            }

            node->getRight()->accept(this);
            RuntimeValue val = valueStack.top();
            valueStack.pop();
            setVariable(left->getName(), val);
            valueStack.push(val);
            return;
        }

        node->getLeft()->accept(this);
        node->getRight()->accept(this);

        RuntimeValue right = valueStack.top();
        valueStack.pop();
        RuntimeValue left = valueStack.top();
        valueStack.pop();

        if (op == "+") {
            if (left.isInt() && right.isInt()) {
                valueStack.push(RuntimeValue(left.getInt() + right.getInt()));
            } else if (left.isFloat() || right.isFloat()) {
                double l = left.isInt() ? left.getInt() : left.getFloat();
                double r = right.isInt() ? right.getInt() : right.getFloat();
                valueStack.push(RuntimeValue(l + r));
            } else if (left.isStr() || right.isStr()) {
                valueStack.push(RuntimeValue(left.toString() + right.toString()));
            } else {
                std::cerr << "Runtime Error: Invalid operands for +" << std::endl;
                valueStack.push(RuntimeValue());
            }
        } else if (op == "-") {
            double l = left.isInt() ? left.getInt() : left.getFloat();
            double r = right.isInt() ? right.getInt() : right.getFloat();
            valueStack.push(RuntimeValue(l - r));
        } else if (op == "*") {
            double l = left.isInt() ? left.getInt() : left.getFloat();
            double r = right.isInt() ? right.getInt() : right.getFloat();
            valueStack.push(RuntimeValue(l * r));
        } else if (op == "/") {
            double l = left.isInt() ? left.getInt() : left.getFloat();
            double r = right.isInt() ? right.getInt() : right.getFloat();
            if (r == 0) {
                std::cerr << "Runtime Error: Division by zero" << std::endl;
                valueStack.push(RuntimeValue());
            } else {
                valueStack.push(RuntimeValue(l / r));
            }
        } else if (op == "%") {
            if (left.isInt() && right.isInt()) {
                if (right.getInt() == 0) {
                    std::cerr << "Runtime Error: Modulo by zero" << std::endl;
                    valueStack.push(RuntimeValue());
                } else {
                    valueStack.push(RuntimeValue(left.getInt() % right.getInt()));
                }
            } else {
                std::cerr << "Runtime Error: Modulo requires integers" << std::endl;
                valueStack.push(RuntimeValue());
            }
        } else if (op == "==") {
            if (left.type != right.type) {
                valueStack.push(RuntimeValue(false));
            } else {
                switch (left.type) {
                case RuntimeValue::TypeKind::INT:
                    valueStack.push(RuntimeValue(left.getInt() == right.getInt()));
                    break;
                case RuntimeValue::TypeKind::FLOAT:
                    valueStack.push(RuntimeValue(left.getFloat() == right.getFloat()));
                    break;
                case RuntimeValue::TypeKind::BOOL:
                    valueStack.push(RuntimeValue(left.getBool() == right.getBool()));
                    break;
                case RuntimeValue::TypeKind::STR:
                    valueStack.push(RuntimeValue(left.getStr() == right.getStr()));
                    break;
                default:
                    valueStack.push(RuntimeValue(false));
                }
            }
        } else if (op == "!=") {
            if (left.type != right.type) {
                valueStack.push(RuntimeValue(true));
            } else {
                switch (left.type) {
                case RuntimeValue::TypeKind::INT:
                    valueStack.push(RuntimeValue(left.getInt() != right.getInt()));
                    break;
                case RuntimeValue::TypeKind::FLOAT:
                    valueStack.push(RuntimeValue(left.getFloat() != right.getFloat()));
                    break;
                case RuntimeValue::TypeKind::BOOL:
                    valueStack.push(RuntimeValue(left.getBool() != right.getBool()));
                    break;
                case RuntimeValue::TypeKind::STR:
                    valueStack.push(RuntimeValue(left.getStr() != right.getStr()));
                    break;
                default:
                    valueStack.push(RuntimeValue(true));
                }
            }
        } else if (op == "<" || op == "<=" || op == ">" || op == ">=") {
            double l = left.isInt() ? left.getInt() : left.getFloat();
            double r = right.isInt() ? right.getInt() : right.getFloat();

            if (op == "<")
                valueStack.push(RuntimeValue(l < r));
            else if (op == "<=")
                valueStack.push(RuntimeValue(l <= r));
            else if (op == ">")
                valueStack.push(RuntimeValue(l > r));
            else if (op == ">=")
                valueStack.push(RuntimeValue(l >= r));
        } else if (op == "&&") {
            if (!left.isBool() || !right.isBool()) {
                std::cerr << "Runtime Error: && requires boolean operands" << std::endl;
                valueStack.push(RuntimeValue(false));
            } else {
                valueStack.push(RuntimeValue(left.getBool() && right.getBool()));
            }
        } else if (op == "||") {
            if (!left.isBool() || !right.isBool()) {
                std::cerr << "Runtime Error: || requires boolean operands" << std::endl;
                valueStack.push(RuntimeValue(false));
            } else {
                valueStack.push(RuntimeValue(left.getBool() || right.getBool()));
            }
        } else {
            std::cerr << "Runtime Error: Unknown operator " << op << std::endl;
            valueStack.push(RuntimeValue());
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::UnaryExpression) {
        node->getOperand()->accept(this);
        RuntimeValue val = valueStack.top();
        valueStack.pop();
        const std::string &op = node->getOperator();

        if (op == "-") {
            if (val.isInt()) {
                valueStack.push(RuntimeValue(-val.getInt()));
            } else if (val.isFloat()) {
                valueStack.push(RuntimeValue(-val.getFloat()));
            } else {
                std::cerr << "Runtime Error: Cannot negate non-numeric value" << std::endl;
                valueStack.push(val);
            }
        } else if (op == "!") {
            if (val.isBool()) {
                valueStack.push(RuntimeValue(!val.getBool()));
            } else {
                valueStack.push(RuntimeValue(false));
            }
        } else {
            valueStack.push(val);
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::IfStatement) {
        node->getCondition()->accept(this);
        RuntimeValue cond = valueStack.top();
        valueStack.pop();

        bool condition = false;
        if (cond.isBool()) {
            condition = cond.getBool();
        } else if (cond.isInt()) {
            condition = cond.getInt() != 0;
        } else if (cond.isFloat()) {
            condition = cond.getFloat() != 0.0;
        }

        if (condition) {
            if (node->getThenBranch()) {
                node->getThenBranch()->accept(this);
            }
        } else {
            if (node->getElseBranch()) {
                node->getElseBranch()->accept(this);
            }
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::WhileStatement) {
        loopDepth++;

        while (true) {
            node->getCondition()->accept(this);
            RuntimeValue cond = valueStack.top();
            valueStack.pop();

            bool condition = false;
            if (cond.isBool()) {
                condition = cond.getBool();
            } else if (cond.isInt()) {
                condition = cond.getInt() != 0;
            } else if (cond.isFloat()) {
                condition = cond.getFloat() != 0.0;
            }

            if (!condition)
                break;

            if (node->getBody()) {
                node->getBody()->accept(this);
            }

            if (breakFlag) {
                breakFlag = false;
                break;
            }
            if (returnFlag)
                break;
            if (continueFlag) {
                continueFlag = false;
            }
        }

        loopDepth--;
    }

    VISIT_ASTNODEO(Interpreter, AST::ForStatement) {
        const std::string &loopVar = node->getLoopVariable();

        // 解析 range 表达式
        node->getIterable()->accept(this);
        if (valueStack.empty())
            return;

        RuntimeValue rangeVal = valueStack.top();
        valueStack.pop();
        if (!rangeVal.isRange()) {
            std::cerr << "Runtime Error: For loop requires range expression" << std::endl;
            return;
        }

        auto range = rangeVal.getRange();

        // 进入循环作用域
        enterScope();
        declareVariable(loopVar, RuntimeValue(range.start));

        loopDepth++;

        // 执行循环
        bool ascending = range.step > 0;
        while (true) {
            RuntimeValue *currentVar = getVariable(loopVar);
            if (!currentVar || !currentVar->isInt())
                break;

            int current = currentVar->getInt();

            // 检查循环条件
            if (ascending) {
                if (current >= range.end)
                    break;
            } else {
                if (current <= range.end)
                    break;
            }

            // 执行循环体
            if (node->getBody()) {
                node->getBody()->accept(this);
            }

            // 检查控制流
            if (breakFlag) {
                breakFlag = false;
                break;
            }
            if (returnFlag)
                break;

            // 更新循环变量（如果是 continue，也要更新）
            setVariable(loopVar, RuntimeValue(current + range.step));

            if (continueFlag) {
                continueFlag = false;
            }
        }

        loopDepth--;
        exitScope();
    }

    VISIT_ASTNODEO(Interpreter, AST::ReturnStatement) {
        if (node->getValue()) {
            node->getValue()->accept(this);
            returnValue = valueStack.top();
            valueStack.pop();
        } else {
            returnValue = RuntimeValue();
        }
        returnFlag = true;
    }

    VISIT_ASTNODEO(Interpreter, AST::BreakStatement) {
        if (loopDepth > 0) {
            breakFlag = true;
        } else {
            std::cerr << "Runtime Error: break outside loop" << std::endl;
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::ContinueStatement) {
        if (loopDepth > 0) {
            continueFlag = true;
        } else {
            std::cerr << "Runtime Error: continue outside loop" << std::endl;
        }
    }

    VISIT_ASTNODEO(Interpreter, AST::RangeExpression) {
        // 解析 range(start, end, step)
        std::vector<int> args;

        for (auto *arg : node->getArguments()) {
            arg->accept(this);
            if (valueStack.empty()) {
                std::cerr << "Runtime Error: Failed to parse range argument" << std::endl;
                valueStack.push(RuntimeValue());
                return;
            }

            RuntimeValue val = valueStack.top();
            valueStack.pop();
            if (!val.isInt()) {
                std::cerr << "Runtime Error: Range arguments must be integers" << std::endl;
                valueStack.push(RuntimeValue());
                return;
            }
            args.push_back(val.getInt());
        }

        // 检查参数个数
        if (args.size() < 2 || args.size() > 3) {
            std::cerr << "Runtime Error: range() expects 2 or 3 arguments" << std::endl;
            valueStack.push(RuntimeValue());
            return;
        }

        int start = args[0];
        int end = args[1];
        int step = (args.size() == 3) ? args[2] : 1;

        if (step == 0) {
            std::cerr << "Runtime Error: range() step cannot be zero" << std::endl;
            valueStack.push(RuntimeValue());
            return;
        }

        // 创建 Range 对象
        valueStack.push(RuntimeValue(RuntimeValue::RangeData(start, end, step)));
    }

    // 导入语句 - 在解释器中不需要实际执行
    VISIT_ASTNODEO(Interpreter, AST::ImportStatement) {
        // 导入已经在语义分析阶段处理，运行时不需要做任何事
    }

    // 模块声明 - 在解释器中不需要实际执行
    VISIT_ASTNODEO(Interpreter, AST::ModuleStatement) {
        // 模块声明已经在语义分析阶段处理，运行时不需要做任何事
    }

    // 参数 - 在函数调用时由参数绑定处理，这里不需要单独实现
    VISIT_ASTNODEO(Interpreter, AST::Parameter) {
        // 参数节点在函数定义时已经被处理，这里不需要做任何事
    }

    // 类型节点 - 运行时不需要处理
    VISIT_ASTNODEO(Interpreter, AST::Type) {
        // 类型信息只在编译时使用，运行时不需要
    }

    // 数组类型 - 用于声明数组变量
    VISIT_ASTNODEO(Interpreter, AST::ArrayType) {
        // 数组类型本身不需要执行，但在声明数组变量时需要处理
        // 这里只计算大小表达式
        if (node->getSize()) {
            node->getSize()->accept(this);
            RuntimeValue sizeVal = valueStack.top();
            valueStack.pop();
            if (!sizeVal.isInt()) {
                std::cerr << "Runtime Error: Array size must be integer" << std::endl;
            }
            // 数组类型不产生值，只是类型信息
        }
    }

    // 数组索引访问 - arr[index]
    VISIT_ASTNODEO(Interpreter, AST::ArrayIndex) {
        // 计算数组表达式
        node->getArray()->accept(this);
        RuntimeValue arrayVal = valueStack.top();
        valueStack.pop();

        // 计算索引表达式
        node->getIndex()->accept(this);
        RuntimeValue indexVal = valueStack.top();
        valueStack.pop();

        if (!indexVal.isInt()) {
            std::cerr << "Runtime Error: Array index must be integer" << std::endl;
            valueStack.push(RuntimeValue());
            return;
        }

        int index = indexVal.getInt();

        // 简化：假设数组值存储为字符串表示
        // 实际应该实现真正的数组类型
        if (arrayVal.isStr()) {
            // 假装是字符串数组
            valueStack.push(RuntimeValue("array[" + std::to_string(index) + "]"));
        } else {
            std::cerr << "Runtime Error: Cannot index non-array value" << std::endl;
            valueStack.push(RuntimeValue());
        }
    }

    // 成员访问 - obj.member
    VISIT_ASTNODEO(Interpreter, AST::MemberAccess) {
        // 计算对象表达式
        node->getObject()->accept(this);
        RuntimeValue objVal = valueStack.top();
        valueStack.pop();

        const std::string &member = node->getMember();

        // 处理内置模块的成员访问
        if (auto *id = dynamic_cast<AST::Identifier *>(node->getObject())) {
            if (id->getName() == "io") {
                // io.print 这样的调用会在 FunctionCall 中处理
                // 这里只返回一个标记值
                valueStack.push(RuntimeValue("io." + member));
                return;
            }
        }

        // 普通对象暂不支持成员访问
        std::cerr << "Runtime Error: Member access not supported for this type" << std::endl;
        valueStack.push(RuntimeValue());
    }

    // 函数调用 - 完整实现
    VISIT_ASTNODEO(Interpreter, AST::FunctionCall) {
        std::string funcName;
        std::vector<RuntimeValue> args;

        // 获取函数名
        if (auto *id = dynamic_cast<AST::Identifier *>(node->getCallee())) {
            funcName = id->getName();
        } else if (auto *member = dynamic_cast<AST::MemberAccess *>(node->getCallee())) {
            if (auto *obj = dynamic_cast<AST::Identifier *>(member->getObject())) {
                funcName = obj->getName() + "." + member->getMember();
            }
        }

        // 计算参数
        if (node->getArguments()) {
            for (auto *arg : *node->getArguments()) {
                arg->accept(this);
                args.push_back(valueStack.top());
                valueStack.pop();
            }
        }

        // 1. 检查内置函数
        auto it = builtins.find(funcName);
        if (it != builtins.end()) {
            valueStack.push(it->second(args));
            return;
        }

        // 2. 检查模块函数 (如 io.print)
        if (funcName.find('.') != std::string::npos) {
            size_t dotPos = funcName.find('.');
            std::string module = funcName.substr(0, dotPos);
            std::string member = funcName.substr(dotPos + 1);

            if (module == "io" && member == "print") {
                // 实现 io.print
                for (size_t i = 0; i < args.size(); i++) {
                    if (i > 0)
                        std::cout << " ";
                    const auto &arg = args[i];
                    switch (arg.type) {
                    case RuntimeValue::TypeKind::INT:
                        std::cout << arg.getInt();
                        break;
                    case RuntimeValue::TypeKind::FLOAT:
                        std::cout << arg.getFloat();
                        break;
                    case RuntimeValue::TypeKind::BOOL:
                        std::cout << (arg.getBool() ? "true" : "false");
                        break;
                    case RuntimeValue::TypeKind::STR:
                        std::cout << arg.getStr();
                        break;
                    case RuntimeValue::TypeKind::NONE:
                        std::cout << "none";
                        break;
                    default:
                        std::cout << "?";
                        break;
                    }
                }
                std::cout << std::endl;
                valueStack.push(RuntimeValue());
                return;
            }
        }

        // 3. 查找用户定义函数
        RuntimeValue *funcVal = nullptr;
        for (int i = environments.size() - 1; i >= 0; i--) {
            auto it = environments[i].find(funcName);
            if (it != environments[i].end()) {
                funcVal = &(it->second);
                break;
            }
        }

        if (!funcVal || !funcVal->isFunction()) {
            std::cerr << "Runtime Error: Function '" << funcName << "' not defined" << std::endl;
            valueStack.push(RuntimeValue());
            return;
        }

        const auto &func = funcVal->getFunction();

        // 保存当前环境大小
        size_t envSize = environments.size();

        // 创建函数调用环境
        enterScope(); // 函数局部作用域

        // 绑定参数
        for (size_t i = 0; i < func.params.size(); i++) {
            if (i < args.size()) {
                declareVariable(func.params[i], args[i]);
            } else {
                // 缺少参数，用默认值
                declareVariable(func.params[i], RuntimeValue());
            }
        }

        // 执行函数体
        if (func.body) {
            func.body->accept(this);
        }

        // 获取返回值
        RuntimeValue result;
        if (returnFlag) {
            result = returnValue;
            returnFlag = false;
            returnValue = RuntimeValue();
        }

        // 恢复到函数调用前的环境
        while (environments.size() > envSize) {
            exitScope();
        }

        valueStack.push(result);
    }

    VISIT_ASTNODEO(Interpreter, AST::Function) {
        // 函数定义 - 存储函数到环境
        const std::string &funcName = node->getName();

        // 收集参数名
        std::vector<std::string> params;
        if (node->getParameters()) {
            for (auto *param : *node->getParameters()) {
                params.push_back(param->getName());
            }
        }

        // 创建函数值
        FunctionValue func(funcName, params, node->getBody(), environments.size());

        // 在全局作用域声明函数
        if (environments.empty())
            environments.emplace_back();
        environments[0][funcName] = RuntimeValue(func);
    }

    // 组表达式 - (expr)
    VISIT_ASTNODEO(Interpreter, AST::GroupedExpression) {
        node->getExpression()->accept(this);
        // 结果已经在栈上，不需要额外操作
    }

} // namespace interpreter
