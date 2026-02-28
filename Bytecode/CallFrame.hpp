#ifndef CALL_FRAME_HPP
#define CALL_FRAME_HPP

#include "RuntimeValue.hpp"
#include <string>
#include <unordered_map>

namespace vm {

    class CallFrame {
    private:
        std::string functionName;
        int returnAddress;
        int varStackSize;
        std::unordered_map<std::string, RuntimeValue> localVars;

    public:
        CallFrame(const std::string &name, int retAddr, int varSize);
        ~CallFrame() = default;

        // 变量操作
        bool declareVariable(const std::string &name, const RuntimeValue &value);
        bool setVariable(const std::string &name, const RuntimeValue &value);
        RuntimeValue *getVariable(const std::string &name);
        bool hasVariable(const std::string &name) const;

        // Getter/Setter
        const std::string &getFunctionName() const;
        int getReturnAddress() const;
        void setReturnAddress(int addr);
        int getVarStackSize() const;

        // 调试
        std::string toString() const;
    };

} // namespace vm

#endif // CALL_FRAME_HPP
