#ifndef ENVIRONMENT_HPP
#define ENVIRONMENT_HPP

#include <AST/AST.hpp>
#include <iostream>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace env {

    enum DataType { INT, FLOAT, STR, BOOL, NONE, UNKNOWN };
    enum class SymbolType { VARIABLE, FUNCTION, MODULE };

    struct Symbol {
        std::string name;
        SymbolType type;
        DataType dataType;
        int scopeLevel;
        std::string moduleName;
        bool isMut = false;

        bool isArray = false; // 是否是数组
        int arraySize = 0;    // 数组大小（如果是常量）
        AST::Expression *arraySizeExpr = nullptr;

        // 构造函数
        Symbol() : type(SymbolType::VARIABLE), dataType(UNKNOWN), scopeLevel(0) {
        }

        Symbol(std::string n, SymbolType t, DataType dt, int scope)
            : name(std::move(n)), type(t), dataType(dt), scopeLevel(scope) {
        }

        Symbol(std::string n, const std::string &module, DataType dt, int scope)
            : name(std::move(n)), type(SymbolType::FUNCTION), dataType(dt), scopeLevel(scope), moduleName(module) {
        }
        Symbol(std::string n, SymbolType t, DataType dt, int scope, int size, bool mut)
            : name(std::move(n)), type(t), dataType(dt), scopeLevel(scope), isMut(mut), isArray(true), arraySize(size) {
        }

        Symbol(std::string n, SymbolType t, DataType dt, int scope, AST::Expression *sizeExpr, bool mut)
            : name(std::move(n)), type(t), dataType(dt), scopeLevel(scope), isMut(mut), isArray(true),
              arraySizeExpr(sizeExpr) {
        }
    };

    std::string dataTypeToString(DataType type);

    class Environment {
    private:
        std::vector<std::unordered_map<std::string, Symbol>> scopes;

    public:
        // 构造函数
        Environment() {
            // 初始化全局作用域
            scopes.emplace_back();
        }

        // 作用域管理
        void enterScope() {
            scopes.emplace_back();
        }

        void exitScope() {
            if (!scopes.empty()) {
                scopes.pop_back();
            }
        }

        [[nodiscard]] int getCurrentScope() const {
            return scopes.size() - 1; // 返回当前作用域索引（0-based）
        }

        [[nodiscard]] size_t getScopeCount() const {
            return scopes.size();
        }

        // 符号声明
        bool declareVariable(const std::string &name, DataType type);
        bool declareFunction(const std::string &name, DataType returnType, const std::string &moduleName);
        bool declareModule(const std::string &name);
        bool declareArray(const std::string &name, DataType elementType, int size, bool isMut);
        bool declareArray(const std::string &name, DataType elementType, AST::Expression *sizeExpr, bool isMut);

        // 符号查找
        Symbol *lookupSymbol(const std::string &name);
        [[nodiscard]] const Symbol *lookupSymbol(const std::string &name) const;
        [[nodiscard]] bool isDeclared(const std::string &name) const;
        [[nodiscard]] bool isDeclaredInCurrentScope(const std::string &name) const;
        [[nodiscard]] DataType getSymbolType(const std::string &name) const;

        // 类型检查
        static bool isTypeCompatible(DataType target, DataType source);
        static bool isNumericType(DataType type);

        // 工具函数
        void reset();
        void printScope() const;     // 调试用
        void printAllScopes() const; // 调试用
    };
} // namespace env
#endif // ENVIRONMENT_HPP
