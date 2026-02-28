// Bytecode/Compiler.hpp
#ifndef COMPILER_HPP
#define COMPILER_HPP

#include "BytecodeModule.hpp"
#include "FormatPiece.hpp"
#include "OpCode.hpp"
#include "RuntimeValue.hpp"
#include <AST/AST.hpp>
#include <stack>
#include <string>
#include <unordered_map>
#include <vector>

namespace vm {

    class Compiler : public AST::ASTVisitor {
    private:
        BytecodeModule *module;
        std::stack<RuntimeValue> valueStack;

        std::vector<int> breakTargets;
        std::vector<int> continueTargets;
        int loopDepth;

        struct FunctionInfo {
            std::string name;
            std::vector<std::string> params;
            int entryPoint;
            bool isDefined;
        };
        std::unordered_map<std::string, FunctionInfo> functions;
        std::string currentFunction;

        std::unordered_map<std::string, int> labels;
        std::vector<std::pair<int, int>> pendingPatches;

        std::unordered_map<std::string, int> stringConstants;
        std::unordered_map<int, int> intConstants;
        std::unordered_map<double, int> floatConstants;
        std::unordered_map<bool, int> boolConstants;

        struct FormatStringInfo {
            std::vector<FormatPiece> pieces;
            int index;
        };
        std::vector<FormatStringInfo> formatStrings;

    public:
        Compiler();
        ~Compiler() {
            delete module;
        }

        const std::vector<opCode::Instruction> &getOpCodes() const {
            return module->getCode();
        }

        BytecodeModule *compile(AST::Program *program);

        // ==================== ASTVisitor 接口实现 ====================
        // 注意：这里需要加空格，避免宏展开问题
        VISIT_ASTNODEI(AST::ASTNode)
        VISIT_ASTNODEI(AST::Statement)
        VISIT_ASTNODEI(AST::Expression)
        VISIT_ASTNODEI(AST::Program)
        VISIT_ASTNODEI(AST::Block)
        VISIT_ASTNODEI(AST::Declaration)
        VISIT_ASTNODEI(AST::ExpressionStatement)
        VISIT_ASTNODEI(AST::NumberLiteral)
        VISIT_ASTNODEI(AST::StringLiteral)
        VISIT_ASTNODEI(AST::BooleanLiteral)
        VISIT_ASTNODEI(AST::FormatString)
        VISIT_ASTNODEI(AST::Identifier)
        VISIT_ASTNODEI(AST::BinaryExpression)
        VISIT_ASTNODEI(AST::UnaryExpression)
        VISIT_ASTNODEI(AST::FunctionCall)
        VISIT_ASTNODEI(AST::MemberAccess)
        VISIT_ASTNODEI(AST::GroupedExpression)
        VISIT_ASTNODEI(AST::IfStatement)
        VISIT_ASTNODEI(AST::WhileStatement)
        VISIT_ASTNODEI(AST::ForStatement)
        VISIT_ASTNODEI(AST::ReturnStatement)
        VISIT_ASTNODEI(AST::BreakStatement)
        VISIT_ASTNODEI(AST::ContinueStatement)
        VISIT_ASTNODEI(AST::RangeExpression)
        VISIT_ASTNODEI(AST::ImportStatement)
        VISIT_ASTNODEI(AST::ModuleStatement)
        VISIT_ASTNODEI(AST::Parameter)
        VISIT_ASTNODEI(AST::Type)
        VISIT_ASTNODEI(AST::ArrayType)
        VISIT_ASTNODEI(AST::ArrayIndex)
        VISIT_ASTNODEI(AST::Function)

    private:
        int addConstant(const RuntimeValue &value);
        int addString(const std::string &str);
        int addFormatString(const std::vector<FormatPiece> &pieces);

        void emit(opCode::OpCode op);
        void emit(opCode::OpCode op, int operand);
        void emit(opCode::OpCode op, const std::string &operand);
        void emit(opCode::OpCode op, int intOp, const std::string &strOp);
        void emit(opCode::OpCode op, int intOp1, int intOp2);

        int emitJump(opCode::OpCode op);
        void patchJump(int instructionIndex);
        void patchJump(int instructionIndex, int targetAddress);

        void enterLoop(int continueAddr, int breakAddr);
        void exitLoop();

        void beginFunction(const std::string &name, const std::vector<std::string> &params);
        void endFunction();

        // 修改这里：使用 AST::FormatString::VariablePosition
        std::vector<FormatPiece> parseFormatString(const std::string &str,
                                                   const std::vector<AST::FormatString::VariablePosition> &vars);
    };

} // namespace vm

#endif // COMPILER_HPP
