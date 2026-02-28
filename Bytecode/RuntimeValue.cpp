#include "RuntimeValue.hpp"
#include <iomanip>
#include <sstream>
#include <stdexcept>
#include <vector>

namespace vm {

    // ==================== 构造函数 ====================
    RuntimeValue::RuntimeValue() : type(Type::NONE), value(std::monostate{}) {
    }

    RuntimeValue::RuntimeValue(int v) : type(Type::INT), value(v) {
    }

    RuntimeValue::RuntimeValue(double v) : type(Type::FLOAT), value(v) {
    }

    RuntimeValue::RuntimeValue(bool v) : type(Type::BOOL), value(v) {
    }

    RuntimeValue::RuntimeValue(const std::string &v) : type(Type::STRING), value(v) {
    }

    RuntimeValue::RuntimeValue(const std::vector<RuntimeValue> &arr) : type(Type::ARRAY), value(arr) {
    }

    // ==================== 类型检查 ====================
    Type RuntimeValue::getType() const {
        return type;
    }

    // ==================== 值获取 ====================
    int RuntimeValue::getInt() const {
        if (type != Type::INT) {
            throw std::runtime_error("Value is not an integer");
        }
        return std::get<int>(value);
    }

    double RuntimeValue::getFloat() const {
        if (type != Type::FLOAT) {
            throw std::runtime_error("Value is not a float");
        }
        return std::get<double>(value);
    }

    bool RuntimeValue::getBool() const {
        if (type != Type::BOOL) {
            throw std::runtime_error("Value is not a boolean");
        }
        return std::get<bool>(value);
    }

    const std::string &RuntimeValue::getString() const {
        if (type != Type::STRING) {
            throw std::runtime_error("Value is not a string");
        }
        return std::get<std::string>(value);
    }

    // ==================== 类型转换 ====================
    bool RuntimeValue::asBoolean() const {
        switch (type) {
        case Type::NONE:
            return false;
        case Type::INT:
            return std::get<int>(value) != 0;
        case Type::FLOAT:
            return std::get<double>(value) != 0.0;
        case Type::BOOL:
            return std::get<bool>(value);
        case Type::STRING:
            return !std::get<std::string>(value).empty();
        default:
            return false;
        }
    }

    std::string RuntimeValue::toString() const {
        std::stringstream ss;

        switch (type) {
        case Type::NONE:
            return "none";
        case Type::INT:
            return std::to_string(std::get<int>(value));
        case Type::FLOAT: {
            double d = std::get<double>(value);
            ss << std::fixed << std::setprecision(6) << d;
            std::string str = ss.str();
            while (str.size() > 1 && str.back() == '0')
                str.pop_back();
            if (str.back() == '.')
                str.pop_back();
            return str;
        }
        case Type::BOOL:
            return std::get<bool>(value) ? "true" : "false";
        case Type::STRING:
            return std::get<std::string>(value);
        default:
            return "unknown";
        }
    }

    bool RuntimeValue::operator==(const RuntimeValue &other) const {
        if (type != other.type)
            return false;

        switch (type) {
        case Type::NONE:
            return true;
        case Type::INT:
            return std::get<int>(value) == std::get<int>(other.value);
        case Type::FLOAT:
            return std::get<double>(value) == std::get<double>(other.value);
        case Type::BOOL:
            return std::get<bool>(value) == std::get<bool>(other.value);
        case Type::STRING:
            return std::get<std::string>(value) == std::get<std::string>(other.value);
        default:
            return false;
        }
    }

} // namespace vm
