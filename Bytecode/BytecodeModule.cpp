#include "BytecodeModule.hpp"
#include <iostream>

namespace vm {

    // 添加指令 - 基础版本
    int BytecodeModule::addInstruction(const opCode::Instruction &instr) {
        int pos = code.size();
        code.push_back(instr);
        return pos;
    }

    // 添加指令 - 无操作数
    int BytecodeModule::addInstruction(opCode::OpCode op) {
        return addInstruction(opCode::Instruction(op));
    }

    // 添加指令 - 整数操作数
    int BytecodeModule::addInstruction(opCode::OpCode op, int intOp) {
        return addInstruction(opCode::Instruction(op, intOp));
    }

    int BytecodeModule::addInstruction(opCode::OpCode op, int intOp1, int intOp2) {
        return addInstruction(opCode::Instruction(op, intOp1, intOp2));
    }

    // 添加指令 - 字符串操作数
    int BytecodeModule::addInstruction(opCode::OpCode op, const std::string &strOp) {
        return addInstruction(opCode::Instruction(op, strOp));
    }

    // 添加指令 - 双操作数
    int BytecodeModule::addInstruction(opCode::OpCode op, int intOp, const std::string &strOp) {
        return addInstruction(opCode::Instruction(op, intOp, strOp));
    }

    // 常量池操作 - 添加常量
    int BytecodeModule::addConstant(const RuntimeValue &val) {
        constants.push_back(val);
        return constants.size() - 1;
    }

    // 常量池操作 - 获取常量
    const RuntimeValue &BytecodeModule::getConstant(int index) const {
        return constants[index];
    }

    // 字符串表操作 - 添加字符串
    int BytecodeModule::addString(const std::string &str) {
        strings.push_back(str);
        return strings.size() - 1;
    }

    // 字符串表操作 - 获取字符串
    const std::string &BytecodeModule::getString(int index) const {
        return strings[index];
    }

    // 标签操作 - 添加标签
    void BytecodeModule::addLabel(const std::string &name) {
        labels[name] = code.size();
    }

    // 标签操作 - 获取标签地址
    int BytecodeModule::getLabel(const std::string &name) const {
        auto it = labels.find(name);
        if (it != labels.end()) {
            return it->second;
        }
        return -1;
    }

    // 回填跳转指令
    void BytecodeModule::patchJump(int instructionIndex, int targetAddress) {
        if (instructionIndex >= 0 && instructionIndex < code.size()) {
            // 创建新的指令替换原来的
            opCode::Instruction oldInstr = code[instructionIndex];
            code[instructionIndex] = opCode::Instruction(oldInstr.getOp(), targetAddress, oldInstr.getStrOperand());
        }
    }

    // 获取当前指令位置
    int BytecodeModule::getCurrentPosition() const {
        return code.size();
    }

    // 获取所有指令
    const std::vector<opCode::Instruction> &BytecodeModule::getCode() const {
        return code;
    }

    // 调试输出
    void BytecodeModule::dump() {
#ifdef DEBUG
        for (size_t i = 0; i < code.size(); i++) {
            std::cout << i << ": " << code[i].toString() << std::endl;
        }
#endif
    }

} // namespace vm
