#ifndef OPCODE_HPP
#define OPCODE_HPP

#include <sstream>
#include <string>
#include <vector>

namespace opCode {

    enum class OpCode {
        LOAD_VAL,
        LOAD_VAR,
        STORE_VAL,
        STORE_VAR,

        ALLOC_ARRAY,
        ARRAY_GET,
        ARRAY_SET,
        ARRAY_LEN,

        LOAD_GLOBAL_VAL,
        LOAD_GLOBAL_VAR,
        STORE_GLOBAL_VAL,
        STORE_GLOBAL_VAR,

        LOAD_CONST,

        ADD,
        SUB,
        MUL,
        DIV,

        LE,
        LT,
        GE,
        GT,
        EQ,
        NE,

        JMP,
        JMP_TRUE,
        JMP_FALSE,

        SWAP,
        FORMAT,
        NOT,

        CALL,
        RET,
        BUILTIN,

        HALT
    };

    // OpCode.hpp 中修改 Instruction 类
    class Instruction {
        OpCode op;
        int intOperand1 = -1;
        int intOperand2 = -1; // 添加第二个整数操作数
        std::string strOperand;

    public:
        // 现有构造函数
        Instruction(OpCode opc) : op(opc) {
        }
        Instruction(OpCode opc, int intOp) : op(opc), intOperand1(intOp) {
        }
        Instruction(OpCode opc, const std::string &strOp) : op(opc), strOperand(strOp) {
        }
        Instruction(OpCode opc, int intOp, const std::string &strOp) : op(opc), intOperand1(intOp), strOperand(strOp) {
        }

        // 新增：两个整数操作数的构造函数
        Instruction(OpCode opc, int intOp1, int intOp2) : op(opc), intOperand1(intOp1), intOperand2(intOp2) {
        }

        OpCode getOp() const {
            return op;
        }
        // getter
        int getIntOperand1() const {
            return intOperand1;
        }
        int getIntOperand2() const {
            return intOperand2;
        }
        const std::string &getStrOperand() const {
            return strOperand;
        }

        bool hasIntOperand1() const {
            return intOperand1 != -1;
        }
        bool hasIntOperand2() const {
            return intOperand2 != -1;
        }
        bool hasStrOperand() const {
            return !strOperand.empty();
        }

        std::string toString() const {
            std::string result = opCodeToString(op);
            if (hasIntOperand1()) {
                result += " " + std::to_string(intOperand1);
            }
            if (hasIntOperand2()) {
                result += "," + std::to_string(intOperand2);
            }
            if (hasStrOperand()) {
                result += " \"" + strOperand + "\"";
            }
            return result;
        }
        static std::string opCodeToString(OpCode op) {
            switch (op) {
            case OpCode::LOAD_VAL:
                return "LOAD_VAL";
            case OpCode::LOAD_VAR:
                return "LOAD_VAR";
            case OpCode::STORE_VAL:
                return "STORE_VAL";
            case OpCode::STORE_VAR:
                return "STORE_VAR";
            case OpCode::LOAD_GLOBAL_VAL:
                return "LOAD_GLOBAL_VAL";
            case OpCode::LOAD_GLOBAL_VAR:
                return "LOAD_GLOBAL_VAR";
            case OpCode::STORE_GLOBAL_VAL:
                return "STORE_GLOBAL_VAL";
            case OpCode::STORE_GLOBAL_VAR:
                return "STORE_GLOBAL_VAR";
            case OpCode::LOAD_CONST:
                return "LOAD_CONST";
            case OpCode::ALLOC_ARRAY:
                return "ALLOC_ARRAY";
            case OpCode::ARRAY_GET:
                return "ARRAY_GET";
            case OpCode::ARRAY_SET:
                return "ARRAY_SET";
            case OpCode::ARRAY_LEN:
                return "ARRAY_LEN";
            case OpCode::ADD:
                return "ADD";
            case OpCode::SUB:
                return "SUB";
            case OpCode::MUL:
                return "MUL";
            case OpCode::DIV:
                return "DIV";
            case OpCode::LE:
                return "LE";
            case OpCode::LT:
                return "LT";
            case OpCode::GE:
                return "GE";
            case OpCode::GT:
                return "GT";
            case OpCode::EQ:
                return "EQ";
            case OpCode::NE:
                return "NE";
            case OpCode::NOT:
                return "NOT";
            case OpCode::JMP:
                return "JMP";
            case OpCode::JMP_TRUE:
                return "JMP_TRUE";
            case OpCode::JMP_FALSE:
                return "JMP_FALSE";
            case OpCode::SWAP:
                return "SWAP";
            case OpCode::FORMAT:
                return "FORMAT";
            case OpCode::CALL:
                return "CALL";
            case OpCode::RET:
                return "RET";
            case OpCode::BUILTIN:
                return "BUILTIN";
            case OpCode::HALT:
                return "HALT";
            default:
                return "UNKNOWN";
            }
        }
    };

} // namespace opCode

#endif
