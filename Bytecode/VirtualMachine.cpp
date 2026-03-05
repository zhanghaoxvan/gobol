#include "VirtualMachine.hpp"
#include <algorithm>
#include <iostream>
#include <sstream>

namespace vm {

    // ==================== 构造函数/析构函数 ====================
    VirtualMachine::VirtualMachine()
        : module(nullptr), pc(0), running(false), returnFlag(false), breakFlag(false), continueFlag(false),
          loopDepth(0) {
        initBuiltins();
    }

    VirtualMachine::~VirtualMachine() {
        evalStack.clear();
        globalStack.clear();
        callStack.clear();
        evalStack.clear();
        globalStack.clear();
    }

    // ==================== 栈操作 ====================
    void VirtualMachine::push(const RuntimeValue &value) {
        evalStack.push_back(value);
    }

    RuntimeValue VirtualMachine::pop() {
        if (evalStack.empty()) {
            std::cerr << "FATAL: Cannot pop from empty eval stack at pc=" << pc << std::endl;
            std::cerr << "Current instruction: " << module->getCode()[pc - 1].toString() << std::endl;
            dumpState();
            throw std::runtime_error("Cannot pop from empty eval stack");
        }
        RuntimeValue val = evalStack.back();
        evalStack.pop_back();
        return val;
    }

    RuntimeValue VirtualMachine::peek() const {
        if (evalStack.empty()) {
            throw std::runtime_error("Cannot peek empty eval stack");
        }
        return evalStack.back();
    }

    std::vector<RuntimeValue> VirtualMachine::popArgs(int count) {
        std::vector<RuntimeValue> args;
        for (int i = 0; i < count; i++) {
            args.push_back(pop());
        }
        std::reverse(args.begin(), args.end()); // 恢复正确顺序
        return args;
    }

    // ==================== 作用域管理 ====================
    void VirtualMachine::enterScope(const std::string &scopeName) {
        callStack.emplace_back(scopeName, pc, callStack.empty() ? 0 : callStack.back().getVarStackSize() + 1);
    }

    void VirtualMachine::exitScope() {
        if (!callStack.empty()) {
            callStack.pop_back();
        }
    }

    // ==================== 变量操作 ====================
    bool VirtualMachine::declareVariable(const std::string &name, const RuntimeValue &value) {
        if (callStack.empty()) {
            return false;
        }
        return callStack.back().declareVariable(name, value);
    }

    bool VirtualMachine::setVariable(const std::string &name, const RuntimeValue &value) {
        for (int i = callStack.size() - 1; i >= 0; i--) {
            if (callStack[i].setVariable(name, value)) {
                return true;
            }
        }
        return false;
    }

    RuntimeValue *VirtualMachine::getVariable(const std::string &name) {
#ifdef DEBUG
        std::cout << "Looking up variable: '" << name << "' in " << callStack.size() << " frames" << std::endl;
#endif

        for (int i = callStack.size() - 1; i >= 0; i--) {
#ifdef DEBUG
            std::cout << "  Checking frame " << i << ": " << callStack[i].getFunctionName() << std::endl;
#endif
            RuntimeValue *val = callStack[i].getVariable(name);
            if (val) {
#ifdef DEBUG
                std::cout << "    Found: " << val->toString() << std::endl;
#endif
                return val;
            }
        }
#ifdef DEBUG
        std::cout << "Variable '" << name << "' not found!" << std::endl;
#endif
        return nullptr;
    }
    bool VirtualMachine::hasVariable(const std::string &name) const {
        for (int i = callStack.size() - 1; i >= 0; i--) {
            if (callStack[i].hasVariable(name)) {
                return true;
            }
        }
        return false;
    }

    // ==================== 全局变量操作 ====================
    void VirtualMachine::setGlobal(const std::string &name, const RuntimeValue &value) {
        globalStack[name] = value;
    }

    RuntimeValue *VirtualMachine::getGlobal(const std::string &name) {
        auto it = globalStack.find(name);
        if (it != globalStack.end()) {
            return &(it->second);
        }
        return nullptr;
    }

    bool VirtualMachine::hasGlobal(const std::string &name) const {
        return globalStack.find(name) != globalStack.end();
    }

    // ==================== 函数调用 ====================
    void VirtualMachine::callFunction(const std::string &name, int argCount) {
        // 1. 获取参数
        std::vector<RuntimeValue> args = popArgs(argCount);

        // 2. 保存返回地址
        int returnAddr = pc;

        // 3. 创建新帧（重要：传入当前 varStack 大小）
        int currentVarSize = callStack.empty() ? 0 : callStack.back().getVarStackSize();
        callStack.emplace_back(name, returnAddr, currentVarSize + 1);

        // 4. 绑定参数（假设参数名为 p0, p1, ...）
        CallFrame *frame = &callStack.back();
        for (size_t i = 0; i < args.size(); i++) {
            std::string paramName = "p" + std::to_string(i);
            frame->declareVariable(paramName, args[i]);
#ifdef DEBUG
            std::cout << "  Bound param " << paramName << " = " << args[i].toString() << std::endl;
#endif
        }

        // 5. 跳转到函数入口（需要从符号表获取）
        // pc = getFunctionAddress(name);
    }

    void VirtualMachine::returnFromFunction() {
        if (callStack.empty()) {
            std::cerr << "Runtime Error: return from empty call stack" << std::endl;
            return;
        }

        // 获取返回值
        RuntimeValue retVal = pop();

        // 弹出当前帧
        CallFrame frame = callStack.back();
        callStack.pop_back();
#ifdef DEBUG
        std::cout << "Returning from " << frame.getFunctionName() << " with value " << retVal.toString() << std::endl;
#endif

        if (callStack.empty()) {
            pc = -1;
            push(retVal);
            return;
        }

        // 恢复返回地址
        pc = frame.getReturnAddress();

        // 将返回值压栈
        push(retVal);
    }

    CallFrame *VirtualMachine::getCurrentFrame() {
        if (callStack.empty())
            return nullptr;
        return &callStack.back();
    }

    const CallFrame *VirtualMachine::getCurrentFrame() const {
        if (callStack.empty())
            return nullptr;
        return &callStack.back();
    }

    /// VirtualMachine.cpp

    // ==================== 主执行函数 ====================
    bool VirtualMachine::run(BytecodeModule *mod) {
        module = mod;
        pc = 0;
        running = true;

        // 重要：创建全局帧！否则变量无处存放
        callStack.emplace_back("global", 0, 0);

#ifdef DEBUG
        std::cout << "Created global frame" << std::endl;
#endif

        const auto &code = module->getCode();

        while (running && pc >= 0 && pc < static_cast<int>(code.size())) {
            const auto &instr = code[pc];
#ifdef DEBUG
            std::cout << "Executing [" << pc << "]: " << instr.toString() << std::endl;
            dumpEvalStack();
#endif
            pc++;
            execute(instr);
        }

        while (!callStack.empty()) {
            callStack.pop_back();
        }

        return true;
    }

    // ==================== 指令执行 ====================
    void VirtualMachine::execute(const opCode::Instruction &instr) {
        switch (instr.getOp()) {
        case opCode::OpCode::LOAD_CONST: {
            int idx = instr.getIntOperand1();
            push(module->getConstant(idx));
            break;
        }

        case opCode::OpCode::LOAD_VAL:
        case opCode::OpCode::LOAD_VAR: {
            std::string name = instr.getStrOperand();
            RuntimeValue *val = getVariable(name);
            if (val) {
                push(*val);
            } else {
                std::cerr << "Runtime Error: Undefined variable '" << name << "'" << std::endl; // 保留错误
                push(RuntimeValue());
            }
            break;
        }

        case opCode::OpCode::ALLOC_ARRAY: {
            // 栈上顺序: [dim1, dim2, ..., dimN, elementType, dimCount]
            // 例如 int[10][5] 的栈布局: [10, 5, Type::INT, 2]

#ifdef DEBUG
            std::cout << "ALLOC_ARRAY: stack size before = " << evalStack.size() << std::endl;
#endif

            // 1. 先弹出维度数量
            RuntimeValue dimCountVal = pop();
            if (!dimCountVal.isInt()) {
                std::cerr << "Runtime Error: Dimension count must be integer" << std::endl;
                push(RuntimeValue());
                break;
            }
            int dimCount = dimCountVal.getInt();

#ifdef DEBUG
            std::cout << "  dimCount = " << dimCount << std::endl;
#endif

            // 2. 弹出元素类型
            RuntimeValue typeVal = pop();
            if (!typeVal.isInt()) {
                std::cerr << "Runtime Error: Element type must be integer" << std::endl;
                push(RuntimeValue());
                break;
            }

            // 确定元素类型
            Type elementType = Type::INT;
            int typeCode = typeVal.getInt();
            switch (typeCode) {
            case 0:
                elementType = Type::INT;
                break;
            case 1:
                elementType = Type::FLOAT;
                break;
            case 2:
                elementType = Type::BOOL;
                break;
            case 3:
                elementType = Type::STRING;
                break;
            default:
                elementType = Type::INT;
            }

            // 3. 弹出所有维度的大小（从内到外）
            std::vector<int> dims;
            for (int i = 0; i < dimCount; i++) {
                RuntimeValue dimVal = pop();
                if (!dimVal.isInt()) {
                    std::cerr << "Runtime Error: Array dimension must be integer" << std::endl;
                    push(RuntimeValue());
                    break;
                }
                int dim = dimVal.getInt();
                if (dim < 0) {
                    std::cerr << "Runtime Error: Array dimension cannot be negative" << std::endl;
                    push(RuntimeValue());
                    break;
                }
                dims.push_back(dim);
            }

            // 维度是从内到外的，需要反转
            std::reverse(dims.begin(), dims.end());

#ifdef DEBUG
            std::cout << "  dimensions: ";
            for (int d : dims)
                std::cout << d << " ";
            std::cout << std::endl;
            std::cout << "  element type: " << static_cast<int>(elementType) << std::endl;
#endif

            // 4. 递归创建多维数组
            std::function<RuntimeValue(int)> createMultiDimArray = [&](int level) -> RuntimeValue {
                if (level == static_cast<int>(dims.size()) - 1) {
                    // 最后一维，创建一维数组
                    ArrayTypeInfo typeInfo(elementType, dims[level]);
                    return RuntimeValue::createArray(typeInfo);
                } else {
                    // 还有更深维度，先创建子数组
                    ArrayTypeInfo typeInfo(Type::ARRAY, dims[level]);
                    std::vector<RuntimeValue> arr;
                    arr.reserve(dims[level]);
                    for (int i = 0; i < dims[level]; i++) {
                        arr.push_back(createMultiDimArray(level + 1));
                    }
                    return RuntimeValue(arr);
                }
            };

            RuntimeValue array = createMultiDimArray(0);
            push(array);

#ifdef DEBUG
            std::cout << "  multi-dimensional array created, pushing to stack" << std::endl;
#endif
            break;
        }
        case opCode::OpCode::ARRAY_GET: {
            RuntimeValue indexVal = pop();
            RuntimeValue arrayVal = pop();

            std::cout << "ARRAY_GET: array is " << (arrayVal.isArray() ? "array" : "not array") << std::endl;
            if (arrayVal.isArray()) {
                std::cout << "  array size = " << arrayVal.getArraySize() << std::endl;
                std::cout << "  array type = " << static_cast<int>(arrayVal.getType()) << std::endl;
            }

            if (!arrayVal.isArray()) {
                std::cerr << "Runtime Error: Cannot index non-array value" << std::endl;
                push(RuntimeValue());
                break;
            }

            if (!indexVal.isInt()) {
                std::cerr << "Runtime Error: Array index must be integer" << std::endl;
                push(RuntimeValue());
                break;
            }

            int index = indexVal.getInt();
            if (index < 0 || index >= arrayVal.getArraySize()) {
                std::cerr << "Runtime Error: Array index out of bounds" << std::endl;
                push(RuntimeValue());
                break;
            }

            RuntimeValue element = arrayVal.getElement(index);
            std::cout << "  element is " << (element.isArray() ? "array" : "not array") << std::endl;
            std::cout << "  element type = " << static_cast<int>(element.getType()) << std::endl;

            push(element);
            break;
        }

        case opCode::OpCode::ARRAY_SET: {
            RuntimeValue valueVal = pop();
            RuntimeValue indexVal = pop();
            RuntimeValue arrayVal = pop();

            if (!arrayVal.isArray()) {
                std::cerr << "Runtime Error: Cannot index non-array value" << std::endl;
                push(RuntimeValue());
                break;
            }

            int index = indexVal.getInt();
            if (index < 0 || index >= arrayVal.getArraySize()) {
                std::cerr << "Runtime Error: Array index out of bounds" << std::endl;
                push(RuntimeValue());
                break;
            }

            // 关键：设置元素后，数组的维度信息应该保持不变
            arrayVal.setElement(index, valueVal);

            // 把修改后的数组压回栈
            push(arrayVal);
            break;
        }

        case opCode::OpCode::ARRAY_LEN: {
            RuntimeValue arrayVal = pop();
            if (!arrayVal.isArray()) {
                std::cerr << "Runtime Error: Cannot get length of non-array value" << std::endl;
                push(RuntimeValue());
                break;
            }
            push(RuntimeValue(arrayVal.getArraySize()));
            break;
        }

        case opCode::OpCode::LOAD_GLOBAL_VAL:
        case opCode::OpCode::LOAD_GLOBAL_VAR: {
            std::string name = instr.getStrOperand();
            RuntimeValue *val = getGlobal(name);
            if (val) {
                push(*val);
            } else {
                std::cerr << "Runtime Error: Undefined global '" << name << "'" << std::endl; // 保留错误
                push(RuntimeValue());
            }
            break;
        }

        case opCode::OpCode::STORE_VAL:
        case opCode::OpCode::STORE_VAR: {
            std::string name = instr.getStrOperand();
            RuntimeValue val = pop();
            if (!setVariable(name, val)) {
                declareVariable(name, val);
            }
            break;
        }

        case opCode::OpCode::STORE_GLOBAL_VAL:
        case opCode::OpCode::STORE_GLOBAL_VAR: {
            std::string name = instr.getStrOperand();
            RuntimeValue val = pop();
            setGlobal(name, val);
            break;
        }

        case opCode::OpCode::ADD: {
            RuntimeValue right = pop();
            RuntimeValue left = pop();

            // 处理 none 的情况
            if (left.isNone() || right.isNone()) {
                std::cerr << "Runtime Error: Cannot add none value" << std::endl; // 保留错误
                push(RuntimeValue());
                break;
            }

            if (left.isInt() && right.isInt()) {
                push(RuntimeValue(left.getInt() + right.getInt()));
            } else {
                double l = left.isInt() ? left.getInt() : left.getFloat();
                double r = right.isInt() ? right.getInt() : right.getFloat();
                push(RuntimeValue(l + r));
            }
            break;
        }

        case opCode::OpCode::JMP: {
            int target = instr.getIntOperand1();
#ifdef DEBUG
            std::cout << "JMP: jumping to " << target << std::endl;
#endif
            pc = target;
            break;
        }

        case opCode::OpCode::JMP_FALSE: {
            RuntimeValue cond = pop();
            int target = instr.getIntOperand1();

#ifdef DEBUG
            std::cout << "JMP_FALSE: condition = " << cond.toString() << std::endl;
#endif

            bool condition = cond.asBoolean();

            if (!condition) {
#ifdef DEBUG
                std::cout << "  Jumping to " << target << std::endl;
#endif
                pc = target;
            }
            break;
        }

        case opCode::OpCode::CALL: {
            std::string name = instr.getStrOperand();
            int argCount = instr.getIntOperand1();
            callFunction(name, argCount);
            break;
        }

        case opCode::OpCode::RET: {
            returnFromFunction();
            break;
        }

        case opCode::OpCode::BUILTIN: {
            std::string name = instr.getStrOperand();
            int argCount = instr.getIntOperand1();
            auto args = popArgs(argCount);

            auto it = builtins.find(name);
            if (it != builtins.end()) {
                push(it->second(args));
            } else {
                std::cerr << "Runtime Error: Unknown builtin '" << name << "'" << std::endl; // 保留错误
                push(RuntimeValue());
            }
            break;
        }

        case opCode::OpCode::NOT: {
            if (evalStack.empty()) {
                std::cerr << "Runtime Error: NOT requires operand" << std::endl; // 保留错误
                push(RuntimeValue());
                break;
            }
            RuntimeValue val = pop();
            push(RuntimeValue(!val.asBoolean()));
            break;
        }

        case opCode::OpCode::SWAP: {
            if (evalStack.size() < 2) {
                std::cerr << "Runtime Error: SWAP requires 2 operands" << std::endl; // 保留错误
                push(RuntimeValue());
                break;
            }
            RuntimeValue a = pop();
            RuntimeValue b = pop();
            push(a);
            push(b);
            break;
        }

        case opCode::OpCode::LT: {
            RuntimeValue right = pop();
            RuntimeValue left = pop();

#ifdef DEBUG
            std::cout << "LT: " << left.toString() << " < " << right.toString() << std::endl;
#endif

            if (left.isNone() || right.isNone()) {
                std::cerr << "Runtime Error: Cannot compare none" << std::endl; // 保留错误
                push(RuntimeValue(false));
                break;
            }

            double l = left.isInt() ? left.getInt() : left.getFloat();
            double r = right.isInt() ? right.getInt() : right.getFloat();

            push(RuntimeValue(l < r));
            break;
        }

        case opCode::OpCode::LE: {
            RuntimeValue right = pop();
            RuntimeValue left = pop();

            if (left.isNone() || right.isNone()) {
                std::cerr << "Runtime Error: Cannot compare none" << std::endl; // 保留错误
                push(RuntimeValue(false));
                break;
            }

            double l = left.isInt() ? left.getInt() : left.getFloat();
            double r = right.isInt() ? right.getInt() : right.getFloat();

            push(RuntimeValue(l <= r));
            break;
        }

        case opCode::OpCode::GT: {
            RuntimeValue right = pop();
            RuntimeValue left = pop();

            if (left.isNone() || right.isNone()) {
                std::cerr << "Runtime Error: Cannot compare none" << std::endl; // 保留错误
                push(RuntimeValue(false));
                break;
            }

            double l = left.isInt() ? left.getInt() : left.getFloat();
            double r = right.isInt() ? right.getInt() : right.getFloat();

            push(RuntimeValue(l > r));
            break;
        }

        case opCode::OpCode::GE: {
            RuntimeValue right = pop();
            RuntimeValue left = pop();

            if (left.isNone() || right.isNone()) {
                std::cerr << "Runtime Error: Cannot compare none" << std::endl; // 保留错误
                push(RuntimeValue(false));
                break;
            }

            double l = left.isInt() ? left.getInt() : left.getFloat();
            double r = right.isInt() ? right.getInt() : right.getFloat();

            push(RuntimeValue(l >= r));
            break;
        }

        case opCode::OpCode::EQ: {
            RuntimeValue right = pop();
            RuntimeValue left = pop();

            if (left.isNone() || right.isNone()) {
                push(RuntimeValue(left.isNone() && right.isNone()));
                break;
            }

            if (left.getType() != right.getType()) {
                push(RuntimeValue(false));
                break;
            }

            switch (left.getType()) {
            case Type::INT:
                push(RuntimeValue(left.getInt() == right.getInt()));
                break;
            case Type::FLOAT:
                push(RuntimeValue(left.getFloat() == right.getFloat()));
                break;
            case Type::BOOL:
                push(RuntimeValue(left.getBool() == right.getBool()));
                break;
            case Type::STRING:
                push(RuntimeValue(left.getString() == right.getString()));
                break;
            default:
                push(RuntimeValue(false));
            }
            break;
        }

        case opCode::OpCode::NE: {
            RuntimeValue right = pop();
            RuntimeValue left = pop();

            if (left.isNone() || right.isNone()) {
                push(RuntimeValue(!(left.isNone() && right.isNone())));
                break;
            }

            if (left.getType() != right.getType()) {
                push(RuntimeValue(true));
                break;
            }

            switch (left.getType()) {
            case Type::INT:
                push(RuntimeValue(left.getInt() != right.getInt()));
                break;
            case Type::FLOAT:
                push(RuntimeValue(left.getFloat() != right.getFloat()));
                break;
            case Type::BOOL:
                push(RuntimeValue(left.getBool() != right.getBool()));
                break;
            case Type::STRING:
                push(RuntimeValue(left.getString() != right.getString()));
                break;
            default:
                push(RuntimeValue(true));
            }
            break;
        }

        case opCode::OpCode::FORMAT: {
            int strIdx = instr.getIntOperand1();
            int argCount = instr.getIntOperand2();

#ifdef DEBUG
            std::cout << "FORMAT: strIdx=" << strIdx << ", argCount=" << argCount << std::endl;
            std::cout << "Stack before FORMAT: ";
            dumpEvalStack();
#endif

            if (strIdx < 0 || strIdx >= module->getConstants().size()) {
                std::cerr << "Runtime Error: Invalid format string index " << strIdx << std::endl; // 保留错误
                push(RuntimeValue());
                break;
            }

            RuntimeValue formatVal = module->getConstant(strIdx);
            if (!formatVal.isString()) {
                std::cerr << "Runtime Error: FORMAT expected string, got " << static_cast<int>(formatVal.getType())
                          << std::endl; // 保留错误
                push(RuntimeValue());
                break;
            }
            std::string formatStr = formatVal.getString();

#ifdef DEBUG
            std::cout << "  Format string: \"" << formatStr << "\"" << std::endl;
#endif

            if (evalStack.size() < static_cast<size_t>(argCount)) {
                std::cerr << "Runtime Error: FORMAT missing arguments. Need " << argCount << ", have "
                          << evalStack.size() << std::endl; // 保留错误
                push(RuntimeValue());
                break;
            }

            std::vector<RuntimeValue> args;
            for (int i = 0; i < argCount; i++) {
                args.push_back(pop());
#ifdef DEBUG
                std::cout << "  Popped arg " << i << ": " << args.back().toString() << std::endl;
#endif
            }

            std::reverse(args.begin(), args.end());

#ifdef DEBUG
            std::cout << "  Args after reverse: ";
            for (const auto &arg : args) {
                std::cout << arg.toString() << " ";
            }
            std::cout << std::endl;
#endif

            std::string result;
            size_t lastPos = 0;
            size_t pos = 0;
            int varIndex = 0;

            while (pos < formatStr.length()) {
                if (formatStr[pos] == '{') {
                    if (pos > lastPos) {
                        result += formatStr.substr(lastPos, pos - lastPos);
                    }

                    size_t endPos = formatStr.find('}', pos);
                    if (endPos == std::string::npos) {
                        result += formatStr.substr(pos);
                        break;
                    }

                    std::string varName = formatStr.substr(pos + 1, endPos - pos - 1);

                    if (varIndex < static_cast<int>(args.size())) {
                        result += args[varIndex].toString();
#ifdef DEBUG
                        std::cout << "  Replaced {" << varName << "} with " << args[varIndex].toString() << std::endl;
#endif
                    } else {
                        std::cerr << "Runtime Warning: FORMAT missing value for {" << varName << "}"
                                  << std::endl; // 保留警告
                    }

                    varIndex++;
                    pos = endPos + 1;
                    lastPos = pos;
                } else {
                    pos++;
                }
            }

            if (lastPos < formatStr.length()) {
                result += formatStr.substr(lastPos);
            }

#ifdef DEBUG
            std::cout << "  Result: \"" << result << "\"" << std::endl;
#endif

            push(RuntimeValue(result));
            break;
        }

        case opCode::OpCode::HALT: {
            running = false;
            break;
        }

        default:
            std::cerr << "Runtime Error: Unknown opcode" << std::endl; // 保留错误
            break;
        }
    }

    void VirtualMachine::fetchAndExecute() {
        if (module && pc < static_cast<int>(module->getCode().size())) {
            execute(module->getCode()[pc]);
            pc++;
        }
    }

    // ==================== 内置函数初始化 ====================
    void VirtualMachine::initBuiltins() {
        builtins["print"] = [](const std::vector<RuntimeValue> &args) -> RuntimeValue {
            for (size_t i = 0; i < args.size(); i++) {
                if (i > 0)
                    std::cout << " ";
                std::cout << args[i].toString();
            }
            // std::cout << std::endl;
            return RuntimeValue();
        };

        // 添加其他内置函数...
    }

    // ==================== 调试输出 ====================
    void VirtualMachine::dumpState() const {
#ifdef DEBUG
        std::cout << "\n=== VM State ===" << std::endl;
        std::cout << "PC: " << pc << std::endl;
        std::cout << "Running: " << (running ? "yes" : "no") << std::endl;
        dumpEvalStack();
        dumpCallStack();
        dumpGlobals();
#endif
    }

    void VirtualMachine::dumpEvalStack() const {
#ifdef DEBUG
        std::cout << "EvalStack [" << evalStack.size() << "]: ";
        for (const auto &val : evalStack) {
            std::cout << val.toString() << " ";
        }
        std::cout << std::endl;
#endif
    }

    void VirtualMachine::dumpCallStack() const {
#ifdef DEBUG
        std::cout << "CallStack [" << callStack.size() << "]:" << std::endl;
        for (size_t i = 0; i < callStack.size(); i++) {
            std::cout << "  [" << i << "] " << callStack[i].toString() << std::endl;
        }
#endif
    }

    void VirtualMachine::dumpGlobals() const {
#ifdef DEBUG
        std::cout << "Globals:" << std::endl;
        for (const auto &[name, val] : globalStack) {
            std::cout << "  " << name << " = " << val.toString() << std::endl;
        }
#endif
    }

} // namespace vm
