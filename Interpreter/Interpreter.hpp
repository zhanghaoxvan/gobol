#ifndef INTERPRETER_HPP
#define INTERPRETER_HPP

#include <AST/AST.hpp>
#include <cmath>
#include <functional>
#include <iostream>
#include <memory> // 添加智能指针
#include <optional>
#include <stack>
#include <string>
#include <unordered_map>
#include <variant>
#include <vector>

namespace interpreter {

    // 前置声明
    struct FunctionValue;
    struct RangeValue;

    // ==================== 函数类型（完整定义）====================
    struct FunctionValue {
        std::string name;
        std::vector<std::string> params;
        AST::Block *body;
        int closureLevel;

        FunctionValue();
        FunctionValue(std::string n, const std::vector<std::string> &p, AST::Block *b, int level);

        // 确保 FunctionValue 是可复制/移动的
        FunctionValue(const FunctionValue &) = default;
        FunctionValue &operator=(const FunctionValue &) = default;
        FunctionValue(FunctionValue &&) = default;
        FunctionValue &operator=(FunctionValue &&) = default;
    };

    // ==================== 运行时值 ====================
    struct RuntimeValue {
        enum class TypeKind { INT, FLOAT, STR, BOOL, NONE, FUNCTION, RANGE };

        // Range 数据类型
        struct RangeData {
            int start;
            int end;
            int step;
            bool isActive;

            RangeData() : start(0), end(0), step(1), isActive(false) {
            }
            RangeData(int s, int e, int st) : start(s), end(e), step(st), isActive(true) {
            }
        };

        // variant 类型定义 - 现在 FunctionValue 是完整类型
        using ValueType = std::variant<std::monostate, // NONE
                                       int,            // INT
                                       double,         // FLOAT
                                       bool,           // BOOL
                                       std::string,    // STR
                                       FunctionValue,  // FUNCTION - 现在完整了
                                       RangeData       // RANGE
                                       >;

        ValueType value;
        TypeKind type;

        // 构造函数
        RuntimeValue();
        RuntimeValue(int v);
        RuntimeValue(double v);
        RuntimeValue(bool v);
        RuntimeValue(const std::string &v);
        RuntimeValue(const FunctionValue &v);
        RuntimeValue(const RangeData &v);

        // 类型检查
        bool isInt() const;
        bool isFloat() const;
        bool isBool() const;
        bool isStr() const;
        bool isNone() const;
        bool isFunction() const;
        bool isRange() const;

        // 值获取
        int getInt() const;
        double getFloat() const;
        bool getBool() const;
        const std::string &getStr() const;
        const FunctionValue &getFunction() const;
        const RangeData &getRange() const;

        std::string toString() const;
    };

    // ==================== 解释器类 ====================
    class Interpreter : public AST::ASTVisitor {
    private:
        // 环境：作用域链
        std::vector<std::unordered_map<std::string, RuntimeValue>> environments;

        // 表达式结果栈
        std::stack<RuntimeValue> valueStack;

        // 控制流状态
        bool returnFlag = false;
        bool breakFlag = false;
        bool continueFlag = false;
        RuntimeValue returnValue;

        // 循环嵌套深度
        int loopDepth = 0;

        // 内置函数表
        std::unordered_map<std::string, std::function<RuntimeValue(const std::vector<RuntimeValue> &)>> builtins;

        // 辅助函数
        void enterScope();
        void exitScope();
        bool declareVariable(const std::string &name, const RuntimeValue &value);
        bool setVariable(const std::string &name, const RuntimeValue &value);
        RuntimeValue *getVariable(const std::string &name);
        void initBuiltins();

    public:
        Interpreter();
        ~Interpreter() = default;

        // 主执行函数
        bool execute(AST::Program *program);

        // ==================== ASTVisitor 接口实现 ====================
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
    };

} // namespace interpreter

#endif // INTERPRETER_HPP
