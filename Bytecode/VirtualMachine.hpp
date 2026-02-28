#ifndef VIRTUAL_MACHINE_HPP
#define VIRTUAL_MACHINE_HPP

#include "BytecodeModule.hpp"
#include "CallFrame.hpp"
#include "OpCode.hpp"
#include "RuntimeValue.hpp"
#include <functional>
#include <iostream>
#include <stack>
#include <unordered_map>
#include <vector>

namespace vm {

    // 内置函数类型
    using BuiltinFunction = std::function<RuntimeValue(const std::vector<RuntimeValue> &)>;

    class VirtualMachine {
    private:
        // 核心数据结构
        std::vector<RuntimeValue> evalStack;                       // 求值栈
        std::unordered_map<std::string, RuntimeValue> globalStack; // 全局变量
        std::vector<CallFrame> callStack;                          // 调用栈（包含局部变量）

        // 执行状态
        BytecodeModule *module; // 当前执行的字节码模块
        int pc;                 // 程序计数器
        bool running;           // 运行状态

        // 控制流标志
        bool returnFlag;
        bool breakFlag;
        bool continueFlag;
        RuntimeValue returnValue;

        // 循环嵌套深度
        int loopDepth;

        // 内置函数表
        std::unordered_map<std::string, BuiltinFunction> builtins;

        // 私有辅助函数
        void initBuiltins();

    public:
        VirtualMachine();
        ~VirtualMachine();

        // 禁用拷贝构造和赋值
        VirtualMachine(const VirtualMachine &) = delete;
        VirtualMachine &operator=(const VirtualMachine &) = delete;

        // 主执行函数
        bool run(BytecodeModule *mod);

        // 栈操作
        void push(const RuntimeValue &value);
        RuntimeValue pop();
        RuntimeValue peek() const;
        std::vector<RuntimeValue> popArgs(int count);
        bool isEvalStackEmpty() const {
            return evalStack.empty();
        }
        size_t getEvalStackSize() const {
            return evalStack.size();
        }

        // 作用域管理
        void enterScope(const std::string &scopeName = "");
        void exitScope();

        // 变量操作
        bool declareVariable(const std::string &name, const RuntimeValue &value);
        bool setVariable(const std::string &name, const RuntimeValue &value);
        RuntimeValue *getVariable(const std::string &name);
        bool hasVariable(const std::string &name) const;

        // 全局变量操作
        void setGlobal(const std::string &name, const RuntimeValue &value);
        RuntimeValue *getGlobal(const std::string &name);
        bool hasGlobal(const std::string &name) const;

        // 函数调用
        void callFunction(const std::string &name, int argCount);
        void returnFromFunction();
        CallFrame *getCurrentFrame();
        const CallFrame *getCurrentFrame() const;

        // 指令执行
        void execute(const opCode::Instruction &instr);
        void fetchAndExecute();

        // 调试输出
        void dumpState() const;
        void dumpEvalStack() const;
        void dumpCallStack() const;
        void dumpGlobals() const;

        // 获取状态
        bool isRunning() const {
            return running;
        }
        int getPC() const {
            return pc;
        }
    };

} // namespace vm

#endif // VIRTUAL_MACHINE_HPP
