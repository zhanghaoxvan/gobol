#ifndef RUNTIME_VALUE_HPP
#define RUNTIME_VALUE_HPP

#include <iostream>
#include <memory>
#include <string>
#include <variant>
#include <vector>

namespace vm {

    enum class Type { NONE, INT, FLOAT, BOOL, STRING, ARRAY };

    // 类型描述符，用于表示数组类型
    struct ArrayTypeInfo {
        Type elementType;                      // 元素类型（INT、FLOAT等，或者又是ARRAY）
        size_t size;                           // 当前维度的大小
        std::unique_ptr<ArrayTypeInfo> nested; // 下一维度的信息（如果是多维数组）

        ArrayTypeInfo(Type elem, size_t sz) : elementType(elem), size(sz), nested(nullptr) {
        }

        ArrayTypeInfo(Type elem, size_t sz, ArrayTypeInfo *next) : elementType(elem), size(sz), nested(next) {
        }
    };

    class RuntimeValue {
    private:
        Type type;
        std::variant<std::monostate, int, double, bool, std::string, std::vector<RuntimeValue>> value;

    public:
        // 构造函数
        RuntimeValue();
        RuntimeValue(int value);
        RuntimeValue(double value);
        RuntimeValue(bool value);
        RuntimeValue(const std::string &value);
        explicit RuntimeValue(const std::vector<RuntimeValue> &arr);

        // 类型检查
        Type getType() const;

        // 值获取 - 需要指定具体类型
        int getInt() const;
        double getFloat() const;
        bool getBool() const;
        const std::string &getString() const;

        // 类型转换
        bool asBoolean() const;
        std::string toString() const;

        bool operator==(const RuntimeValue &other) const;

        ~RuntimeValue() = default;
        bool isString() const {
            return type == Type::STRING;
        }

        bool isInt() const {
            return type == Type::INT;
        }

        bool isFloat() const {
            return type == Type::FLOAT;
        }

        bool isBool() const {
            return type == Type::BOOL;
        }

        bool isNone() const {
            return type == Type::NONE;
        }

        static RuntimeValue createArray(const ArrayTypeInfo &typeInfo) {
            std::vector<RuntimeValue> arr;
            arr.reserve(typeInfo.size);

            for (size_t i = 0; i < typeInfo.size; i++) {
                if (typeInfo.nested) {
                    // 还有下一维，递归创建
                    arr.push_back(createArray(*typeInfo.nested));
                } else {
                    // 最后一维，创建元素
                    RuntimeValue defaultValue;
                    switch (typeInfo.elementType) {
                    case Type::INT:
                        defaultValue = RuntimeValue(0);
                        break;
                    case Type::FLOAT:
                        defaultValue = RuntimeValue(0.0);
                        break;
                    case Type::BOOL:
                        defaultValue = RuntimeValue(false);
                        break;
                    case Type::STRING:
                        defaultValue = RuntimeValue("");
                        break;
                    default:
                        defaultValue = RuntimeValue();
                    }
                    arr.push_back(defaultValue);
                }
            }

            return RuntimeValue(arr);
        }

        // 类型检查
        bool isArray() const {
            return type == Type::ARRAY;
        }

        // 数组操作
        const std::vector<RuntimeValue> &getArray() const {
            if (!isArray())
                throw std::runtime_error("Not an array");
            return std::get<std::vector<RuntimeValue>>(value);
        }

        std::vector<RuntimeValue> &getArray() {
            if (!isArray())
                throw std::runtime_error("Not an array");
            return std::get<std::vector<RuntimeValue>>(value);
        }

        // 获取数组元素
        RuntimeValue getElement(int index) const {
            const auto &arr = getArray();
            if (index < 0 || index >= static_cast<int>(arr.size())) {
                throw std::runtime_error("Array index out of bounds");
            }
            return arr[index];
        }

        // 设置数组元素
        void setElement(int index, const RuntimeValue &val) {
            auto &arr = getArray();
            if (index < 0 || index >= static_cast<int>(arr.size())) {
                throw std::runtime_error("Array index out of bounds");
            }
            arr[index] = val;
        }

        // 获取数组大小
        int getArraySize() const {
            return static_cast<int>(getArray().size());
        }
    };

} // namespace vm

#endif // RUNTIME_VALUE_HPP
