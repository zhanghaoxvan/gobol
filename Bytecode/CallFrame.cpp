#include "CallFrame.hpp"
#include <sstream>

namespace vm {

    CallFrame::CallFrame(const std::string &name, int retAddr, int varSize)
        : functionName(name), returnAddress(retAddr), varStackSize(varSize) {
    }

    bool CallFrame::declareVariable(const std::string &name, const RuntimeValue &value) {
        if (localVars.find(name) != localVars.end()) {
            return false; // 变量已存在
        }
        localVars[name] = value;
        return true;
    }

    bool CallFrame::setVariable(const std::string &name, const RuntimeValue &value) {
        auto it = localVars.find(name);
        if (it != localVars.end()) {
            it->second = value;
            return true;
        }
        return false;
    }

    RuntimeValue *CallFrame::getVariable(const std::string &name) {
        auto it = localVars.find(name);
        if (it != localVars.end()) {
            return &(it->second);
        }
        return nullptr;
    }

    bool CallFrame::hasVariable(const std::string &name) const {
        return localVars.find(name) != localVars.end();
    }

    const std::string &CallFrame::getFunctionName() const {
        return functionName;
    }

    int CallFrame::getReturnAddress() const {
        return returnAddress;
    }

    void CallFrame::setReturnAddress(int addr) {
        returnAddress = addr;
    }

    int CallFrame::getVarStackSize() const {
        return varStackSize;
    }

    std::string CallFrame::toString() const {
        std::stringstream ss;
        ss << "Frame[" << functionName << "] retAddr=" << returnAddress << " vars=" << localVars.size();
        return ss.str();
    }

} // namespace vm
