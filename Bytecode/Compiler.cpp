#include "Compiler.hpp"
#include <algorithm>
#include <cassert>
#include <iostream>
#include <sstream>

namespace vm {

    // ==================== 构造函数 ====================
    Compiler::Compiler() : module(nullptr), loopDepth(0), currentFunction("") {
    }

    // ==================== 主编译函数 ====================
    BytecodeModule *Compiler::compile(AST::Program *program) {
        module = new BytecodeModule();

        // 重置状态
        valueStack = std::stack<RuntimeValue>();
        breakTargets.clear();
        continueTargets.clear();
        functions.clear();
        labels.clear();
        pendingPatches.clear();
        currentFunction = "";

        // 清空常量池映射
        stringConstants.clear();
        intConstants.clear();
        floatConstants.clear();
        boolConstants.clear();
        formatStrings.clear();

        // 编译程序
        if (program) {
            program->accept(this);
        }

        // 确保程序以HALT结束
        emit(opCode::OpCode::HALT);

        // 回填所有待处理的跳转
        for (const auto &patch : pendingPatches) {
            patchJump(patch.first, patch.second);
        }

        return module;
    }

    // ==================== 常量池管理 ====================
    int Compiler::addConstant(const RuntimeValue &value) {
        switch (value.getType()) {
        case Type::INT: {
            int v = value.getInt();
            auto it = intConstants.find(v);
            if (it != intConstants.end())
                return it->second;
            int idx = module->addConstant(value);
            intConstants[v] = idx;
            return idx;
        }
        case Type::FLOAT: {
            double v = value.getFloat();
            auto it = floatConstants.find(v);
            if (it != floatConstants.end())
                return it->second;
            int idx = module->addConstant(value);
            floatConstants[v] = idx;
            return idx;
        }
        case Type::BOOL: {
            bool v = value.getBool();
            auto it = boolConstants.find(v);
            if (it != boolConstants.end())
                return it->second;
            int idx = module->addConstant(value);
            boolConstants[v] = idx;
            return idx;
        }
        case Type::STRING: {
            std::string v = value.getString();
            auto it = stringConstants.find(v);
            if (it != stringConstants.end())
                return it->second;
            int idx = module->addConstant(value);
            stringConstants[v] = idx;
            return idx;
        }
        default:
            return module->addConstant(value);
        }
    }

    int Compiler::addString(const std::string &str) {
        return addConstant(RuntimeValue(str));
    }

    int Compiler::addFormatString(const std::vector<FormatPiece> &pieces) {
        formatStrings.push_back({pieces, (int)formatStrings.size()});
        return formatStrings.size() - 1;
    }

    // ==================== 指令发射 ====================
    void Compiler::emit(opCode::OpCode op) {
        module->addInstruction(op);
    }

    void Compiler::emit(opCode::OpCode op, int operand) {
        module->addInstruction(op, operand);
    }

    void Compiler::emit(opCode::OpCode op, const std::string &operand) {
        module->addInstruction(op, operand);
    }

    void Compiler::emit(opCode::OpCode op, int intOp1, int intOp2) {
        module->addInstruction(op, intOp1, intOp2);
    }

    void Compiler::emit(opCode::OpCode op, int intOp, const std::string &strOp) {
        module->addInstruction(op, intOp, strOp);
    }

    int Compiler::emitJump(opCode::OpCode op) {
        int pos = module->getCurrentPosition();
        emit(op, 0);
        return pos;
    }

    void Compiler::patchJump(int instructionIndex) {
        int target = module->getCurrentPosition();
        patchJump(instructionIndex, target);
    }

    void Compiler::patchJump(int instructionIndex, int targetAddress) {
        module->patchJump(instructionIndex, targetAddress);
    }

    // ==================== 循环控制 ====================
    void Compiler::enterLoop(int continueAddr, int breakAddr) {
        loopDepth++;
        continueTargets.push_back(continueAddr);
        breakTargets.push_back(breakAddr);
    }

    void Compiler::exitLoop() {
        loopDepth--;
        continueTargets.pop_back();
        breakTargets.pop_back();
    }

    // ==================== 函数管理 ====================
    void Compiler::beginFunction(const std::string &name, const std::vector<std::string> &params) {
        currentFunction = name;
        FunctionInfo info;
        info.name = name;
        info.params = params;
        info.entryPoint = module->getCurrentPosition();
        info.isDefined = true;
        functions[name] = info;
    }

    void Compiler::endFunction() {
        currentFunction = "";
    }

    // ==================== 格式化字符串解析 ====================
    std::vector<FormatPiece> Compiler::parseFormatString(const std::string &str,
                                                         const std::vector<AST::FormatString::VariablePosition> &vars) {
        std::vector<FormatPiece> pieces;
        size_t lastPos = 0;

        for (const auto &var : vars) {
            // 添加前面的文本
            if (static_cast<size_t>(var.posInValue) > lastPos) {
                pieces.emplace_back(FormatPiece::Type::TEXT, str.substr(lastPos, var.posInValue - lastPos));
            }

            // 从 var.value 获取变量名
            std::string varName;
            if (auto *id = dynamic_cast<AST::Identifier *>(var.value)) {
                varName = id->getName();
            } else {
                varName = "?";
            }

            // 添加变量占位符
            pieces.emplace_back(FormatPiece::Type::VARIABLE, varName);

            lastPos = var.posInValue + varName.length() + 2;
        }

        // 添加剩余的文本
        if (lastPos < str.length()) {
            pieces.emplace_back(FormatPiece::Type::TEXT, str.substr(lastPos));
        }

        return pieces;
    }

    // ==================== ASTVisitor 实现 ====================

    void Compiler::visit(AST::Program *node) {
        for (auto stmt : node->getStatements()) {
            if (stmt)
                stmt->accept(this);
        }
    }

    void Compiler::visit(AST::Block *node) {
        for (auto stmt : node->getStatements()) {
            if (stmt)
                stmt->accept(this);
        }
    }

    void Compiler::visit(AST::Declaration *node) {
        const std::string &name = node->getName();
        bool isVar = (node->getKeyword() == "var");

        // 检查是否是数组类型
        if (auto *arrayType = dynamic_cast<AST::ArrayType *>(node->getType())) {
            // 1. 收集所有维度表达式
            std::vector<AST::Expression *> dimExprs;
            std::vector<int> dimValues; // 保存维度值
            AST::Type *current = arrayType;

            while (auto *arrType = dynamic_cast<AST::ArrayType *>(current)) {
                dimExprs.push_back(arrType->getSize());

                // 如果是数字常量，保存其值
                if (auto *num = dynamic_cast<AST::NumberLiteral *>(arrType->getSize())) {
                    dimValues.push_back(num->getValue());
                }

                current = arrType->getElementType();
            }

            // 2. 保存维度信息到符号表
            if (dimValues.size() == dimExprs.size()) { // 全是常量
                arrayDimensions[name] = dimValues;     // 需要添加这个成员变量
            }

            // 3. 获取最内层的元素类型
            std::string elemTypeName;
            if (auto *baseType = dynamic_cast<AST::Type *>(current)) {
                elemTypeName = baseType->getName();
            }

            int typeCode = 0;
            if (elemTypeName == "int")
                typeCode = 0;
            else if (elemTypeName == "float")
                typeCode = 1;
            else if (elemTypeName == "bool")
                typeCode = 2;
            else if (elemTypeName == "str")
                typeCode = 3;

            // 4. 编译所有维度表达式
            for (auto *dim : dimExprs) {
                dim->accept(this);
            }

            // 5. 压入维度数量
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue((int)dimExprs.size())));

            // 6. 压入类型代码
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(typeCode)));

            // 7. ALLOC_ARRAY
            emit(opCode::OpCode::ALLOC_ARRAY);

            // 8. 存储数组
            emit(opCode::OpCode::STORE_VAR, node->getName());

            return;
        }

        // 普通变量处理
        if (node->getInitializer()) {
            node->getInitializer()->accept(this);
        } else {
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue()));
        }

        if (isVar) {
            emit(opCode::OpCode::STORE_VAR, name);
        } else {
            emit(opCode::OpCode::STORE_VAL, name);
        }
    }

    void Compiler::visit(AST::ExpressionStatement *node) {
        if (node->getExpression()) {
            node->getExpression()->accept(this);
            // 表达式结果不需要保留
        }
    }

    void Compiler::visit(AST::NumberLiteral *node) {
        double val = node->getValue();
        if (val == static_cast<int>(val)) {
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(static_cast<int>(val))));
        } else {
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(val)));
        }
    }

    void Compiler::visit(AST::StringLiteral *node) {
        emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(node->getValue())));
    }

    void Compiler::visit(AST::BooleanLiteral *node) {
        emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(node->getValue())));
    }

    void Compiler::visit(AST::FormatString *node) {
        const std::string &rawStr = node->getValue();
        const auto &vars = node->getVariables();

        // 1. 先加载格式字符串
        int strIdx = addConstant(RuntimeValue(rawStr));
        emit(opCode::OpCode::LOAD_CONST, strIdx);

        // 2. 再编译所有变量表达式
        for (const auto &var : vars) {
            if (var.value) {
                var.value->accept(this);
            }
        }

        // 3. 生成 FORMAT 指令
        emit(opCode::OpCode::FORMAT, strIdx, static_cast<int>(vars.size()));
    }

    void Compiler::visit(AST::Identifier *node) {
        const std::string &name = node->getName();
        emit(opCode::OpCode::LOAD_VAR, name);
    }

    void Compiler::visit(AST::BinaryExpression *node) {
        const std::string &op = node->getOperator();

        if (op == "=") {
            // 情况1: 数组元素赋值
            if (auto *arrIndex = dynamic_cast<AST::ArrayIndex *>(node->getLeft())) {
                /*// 1. 找到数组名和所有索引
                std::string arrName;
                std::vector<AST::Expression *> indices;
                AST::Expression *current = arrIndex;
                while (auto *nested = dynamic_cast<AST::ArrayIndex *>(current)) {
                    indices.push_back(nested->getIndex());
                    current = nested->getArray();
                }
                if (auto *id = dynamic_cast<AST::Identifier *>(current)) {
                    arrName = id->getName();
                }
                std::reverse(indices.begin(), indices.end());

                // 2. 加载数组
                emit(opCode::OpCode::LOAD_VAR, arrName);

                // 3. 编译所有索引
                for (auto *idx : indices) {
                    idx->accept(this);
                }

                // 4. 获取维度信息并计算扁平化索引
                auto it = arrayDimensions.find(arrName);
                if (it != arrayDimensions.end()) {
                    const auto &dims = it->second;
                    int stride = 1;
                    for (int d = dims.size() - 1; d >= 0; d--) {
                        if (d < dims.size() - 1) {
                            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(stride)));
                            emit(opCode::OpCode::MUL);
                        }
                        stride *= dims[d];
                    }
                    if (dims.size() >= 2) {
                        emit(opCode::OpCode::ADD);
                    }
                }

                // 5. 编译右值
                node->getRight()->accept(this);

                // 6. ARRAY_SET
                emit(opCode::OpCode::ARRAY_SET);

                // 7. 存回变量
                if (!arrName.empty()) {
                    emit(opCode::OpCode::STORE_VAR, arrName);
                }*/
                arrIndex->accept(this);

                // 编译右值
                node->getRight()->accept(this);

                // ARRAY_SET
                emit(opCode::OpCode::ARRAY_SET);

                // 存回变量
                // std::string arrName = getArrayName(arrIndex);
                // if (!arrName.empty()) {
                //     emit(opCode::OpCode::STORE_VAR, arrName);
                // }
                return;
            }

            // 情况2: 普通变量赋值 x = 0
            auto *left = dynamic_cast<AST::Identifier *>(node->getLeft());
            if (!left) {
                std::cerr << "Compile Error: Left side of assignment must be identifier or array element" << std::endl;
                return;
            }

            node->getRight()->accept(this);
            emit(opCode::OpCode::STORE_VAR, left->getName());
            return;
        }

        // 其他二元运算符...
        node->getLeft()->accept(this);
        node->getRight()->accept(this);

        if (op == "+")
            emit(opCode::OpCode::ADD);
        else if (op == "-")
            emit(opCode::OpCode::SUB);
        else if (op == "*")
            emit(opCode::OpCode::MUL);
        else if (op == "/")
            emit(opCode::OpCode::DIV);
        else if (op == "<")
            emit(opCode::OpCode::LT);
        else if (op == "<=")
            emit(opCode::OpCode::LE);
        else if (op == ">")
            emit(opCode::OpCode::GT);
        else if (op == ">=")
            emit(opCode::OpCode::GE);
        else if (op == "==")
            emit(opCode::OpCode::EQ);
        else if (op == "!=")
            emit(opCode::OpCode::NE);
        else {
            std::cerr << "Compile Error: Unknown operator " << op << std::endl;
        }
    }

    void Compiler::visit(AST::UnaryExpression *node) {
        node->getOperand()->accept(this);

        const std::string &op = node->getOperator();
        if (op == "-") {
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(0)));
            emit(opCode::OpCode::SWAP);
            emit(opCode::OpCode::SUB);
        } else if (op == "!") {
            // 逻辑非
            emit(opCode::OpCode::NOT);
        }
    }

    void Compiler::visit(AST::IfStatement *node) {
        node->getCondition()->accept(this);

        int elseJump = emitJump(opCode::OpCode::JMP_FALSE);

        if (node->getThenBranch()) {
            node->getThenBranch()->accept(this);
        }

        if (node->getElseBranch()) {
            int endJump = emitJump(opCode::OpCode::JMP);
            patchJump(elseJump);
            node->getElseBranch()->accept(this);
            patchJump(endJump);
        } else {
            patchJump(elseJump);
        }
    }

    void Compiler::visit(AST::WhileStatement *node) {
        int loopStart = module->getCurrentPosition();

        node->getCondition()->accept(this);

        int exitJump = emitJump(opCode::OpCode::JMP_FALSE);

        enterLoop(loopStart, module->getCurrentPosition());

        if (node->getBody()) {
            node->getBody()->accept(this);
        }

        emit(opCode::OpCode::JMP, loopStart);
        exitLoop();
        patchJump(exitJump);
    }

    void Compiler::visit(AST::ForStatement *node) {
        const std::string &loopVar = node->getLoopVariable();

        // range 表达式会把 start, end, step 三个值留在栈上
        node->getIterable()->accept(this);

        // 此时栈: [start, end, step] (step在栈顶)

        // 弹出 step 和 end 保存到临时变量
        emit(opCode::OpCode::STORE_VAR, "_step"); // 弹出 step
        emit(opCode::OpCode::STORE_VAR, "_end");  // 弹出 end

        // 初始化循环变量 i = start (start 现在在栈顶)
        emit(opCode::OpCode::STORE_VAR, loopVar); // 弹出 start 存入 i

        int loopStart = module->getCurrentPosition();

        // 条件判断: i < end
        emit(opCode::OpCode::LOAD_VAR, loopVar);
        emit(opCode::OpCode::LOAD_VAR, "_end");
        emit(opCode::OpCode::LT);

        int exitJump = emitJump(opCode::OpCode::JMP_FALSE);

        // 循环体
        enterLoop(module->getCurrentPosition(), module->getCurrentPosition());
        if (node->getBody()) {
            node->getBody()->accept(this);
        }

        // i = i + step
        emit(opCode::OpCode::LOAD_VAR, loopVar);
        emit(opCode::OpCode::LOAD_VAR, "_step");
        emit(opCode::OpCode::ADD);
        emit(opCode::OpCode::STORE_VAR, loopVar);

        emit(opCode::OpCode::JMP, loopStart);
        exitLoop();
        patchJump(exitJump);
    }

    void Compiler::visit(AST::ReturnStatement *node) {
        if (node->getValue()) {
            node->getValue()->accept(this);
        } else {
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue()));
        }
        emit(opCode::OpCode::RET);
    }

    void Compiler::visit(AST::BreakStatement *node) {
        if (loopDepth > 0 && !breakTargets.empty()) {
            emit(opCode::OpCode::JMP, breakTargets.back());
        } else {
            std::cerr << "Compile Error: break outside loop" << std::endl;
        }
    }

    void Compiler::visit(AST::ContinueStatement *node) {
        if (loopDepth > 0 && !continueTargets.empty()) {
            emit(opCode::OpCode::JMP, continueTargets.back());
        } else {
            std::cerr << "Compile Error: continue outside loop" << std::endl;
        }
    }

    void Compiler::visit(AST::RangeExpression *node) {
        const auto &args = node->getArguments();

        // range(0,10,1) 应该生成三个值在栈上
        for (auto arg : args) {
            arg->accept(this);
        }

        // 如果只有两个参数，添加默认 step=1
        if (args.size() == 2) {
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(1)));
        }

        // 现在栈上有: start, end, step (step在栈顶)
        // 注意顺序：第一个参数是 start，最后一个是 step
    }

    void Compiler::visit(AST::FunctionCall *node) {
        std::string funcName;

        if (auto *id = dynamic_cast<AST::Identifier *>(node->getCallee())) {
            funcName = id->getName();
        } else if (auto *member = dynamic_cast<AST::MemberAccess *>(node->getCallee())) {
            if (auto *obj = dynamic_cast<AST::Identifier *>(member->getObject())) {
                funcName = obj->getName() + "." + member->getMember();
            }
        }

        int argCount = 0;
        if (node->getArguments()) {
            for (auto *arg : *node->getArguments()) {
                arg->accept(this);
                argCount++;
            }
        }

        if (funcName == "print" || funcName == "io.print") {
            emit(opCode::OpCode::BUILTIN, argCount, "print");
        } else {
            emit(opCode::OpCode::CALL, argCount, funcName);
        }
    }

    void Compiler::visit(AST::MemberAccess *node) {
        // 成员访问由 FunctionCall 处理
    }

    void Compiler::visit(AST::GroupedExpression *node) {
        node->getExpression()->accept(this);
    }

    void Compiler::visit(AST::Function *node) {
        const std::string &funcName = node->getName();

        std::vector<std::string> params;
        if (node->getParameters()) {
            for (auto *param : *node->getParameters()) {
                params.push_back(param->getName());
            }
        }

        beginFunction(funcName, params);

        if (node->getBody()) {
            node->getBody()->accept(this);
        }

        // 检查函数体最后是否已经有 RET
        const auto &code = module->getCode();
        if (code.empty() || code.back().getOp() != opCode::OpCode::RET) {
            emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(0)));
            emit(opCode::OpCode::RET);
        }
        // 如果已经有 RET，就不添加
        // 注意：你的函数体内已经有 RET 了，所以不应该再添加！

        endFunction();
    }

    // 以下节点不需要编译
    void Compiler::visit(AST::ImportStatement *) {
    }
    void Compiler::visit(AST::ModuleStatement *) {
    }
    void Compiler::visit(AST::Parameter *) {
    }
    void Compiler::visit(AST::Type *) {
    }
    void Compiler::visit(AST::ArrayType *) {
    }
    void Compiler::visit(AST::ArrayIndex *node) {
        // 1. 找到数组名
        AST::Expression *current = node;
        while (auto *arrIndex = dynamic_cast<AST::ArrayIndex *>(current)) {
            current = arrIndex->getArray();
        }

        std::string arrName;
        if (auto *id = dynamic_cast<AST::Identifier *>(current)) {
            arrName = id->getName();
        } else {
            std::cerr << "Compile Error: Invalid array access" << std::endl;
            return;
        }

        // 2. 获取维度信息（需要从之前保存的 arrayDimensions 中获取）
        auto it = arrayDimensions.find(arrName);
        if (it == arrayDimensions.end()) {
            std::cerr << "Compile Error: Unknown array dimensions for " << arrName << std::endl;
            return;
        }
        const auto &dims = it->second;

        // 3. 收集所有索引
        std::vector<AST::Expression *> indices;
        current = node;
        while (auto *arrIndex = dynamic_cast<AST::ArrayIndex *>(current)) {
            indices.push_back(arrIndex->getIndex());
            current = arrIndex->getArray();
        }
        std::reverse(indices.begin(), indices.end());

        // 4. 加载数组
        emit(opCode::OpCode::LOAD_VAR, arrName);

        // 5. 编译所有索引
        for (auto *idx : indices) {
            idx->accept(this);
        }

        // 6. 计算扁平化索引
        // flatIndex = i * (dim2*dim3) + j * dim3 + k
        int stride = 1;
        for (int d = dims.size() - 1; d >= 0; d--) {
            if (d < dims.size() - 1) {
                emit(opCode::OpCode::LOAD_CONST, addConstant(RuntimeValue(stride)));
                emit(opCode::OpCode::MUL);
                if (d < dims.size() - 2) {
                    emit(opCode::OpCode::ADD);
                }
            }
            stride *= dims[d];
        }

        // 7. 执行数组访问
        emit(opCode::OpCode::ARRAY_GET);
    }

    void Compiler::visit(AST::ASTNode *) {
    }
    void Compiler::visit(AST::Statement *) {
    }
    void Compiler::visit(AST::Expression *) {
    }

} // namespace vm
