#ifndef BYTECODE_MODULE_HPP
#define BYTECODE_MODULE_HPP

#include "FormatPiece.hpp"
#include "OpCode.hpp"
#include "RuntimeValue.hpp"
#include <string>
#include <unordered_map>
#include <vector>

namespace vm {

    class BytecodeModule {
    private:
        std::vector<opCode::Instruction> code;
        std::vector<RuntimeValue> constants;         // 常量池
        std::vector<std::string> strings;            // 字符串表
        std::unordered_map<std::string, int> labels; // 标签名 -> 地址
        std::vector<std::vector<FormatPiece>> formatPieces;

    public:
        // 添加指令
        int addInstruction(const opCode::Instruction &instr);
        int addInstruction(opCode::OpCode op);
        int addInstruction(opCode::OpCode op, int intOp);
        int addInstruction(opCode::OpCode op, int intOp1, int intOp2);
        int addInstruction(opCode::OpCode op, const std::string &strOp);
        int addInstruction(opCode::OpCode op, int intOp, const std::string &strOp);

        // 常量池操作
        int addConstant(const RuntimeValue &val);
        const RuntimeValue &getConstant(int index) const;

        // 字符串表操作
        int addString(const std::string &str);
        const std::string &getString(int index) const;

        // 标签操作
        void addLabel(const std::string &name);
        int getLabel(const std::string &name) const;

        // 回填跳转指令
        void patchJump(int instructionIndex, int targetAddress);

        // 获取当前指令位置
        int getCurrentPosition() const;

        // 获取所有指令
        const std::vector<opCode::Instruction> &getCode() const;

        // 调试输出
        void dump();

        int addFormatPieces(const std::vector<FormatPiece> &pieces) {
            formatPieces.push_back(pieces);
            return formatPieces.size() - 1;
        }

        const std::vector<FormatPiece> &getFormatPieces(int index) const {
            return formatPieces[index];
        }

        const std::vector<RuntimeValue> &getConstants() const {
            return constants;
        }

        size_t getConstantsSize() const {
            return constants.size();
        }
    };

} // namespace vm

#endif // BYTECODE_MODULE_HPP
