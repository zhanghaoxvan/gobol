#include "Environment.hpp"
#include <algorithm>
#include <iostream>

namespace env {

    // 辅助函数：数据类型转字符串（用于错误信息）
    std::string dataTypeToString(DataType type) {
        switch (type) {
        case INT:
            return "int";
        case FLOAT:
            return "float";
        case STR:
            return "str";
        case BOOL:
            return "bool";
        case NONE:
            return "none";
        default:
            return "unknown";
        }
    }

    bool Environment::declareVariable(const std::string &name, DataType type) {
        // 确保至少有一个作用域
        if (scopes.empty()) {
            scopes.emplace_back();
        }

        // 检查当前作用域是否已存在同名变量
        auto &currentScope = scopes.back();
        if (currentScope.find(name) != currentScope.end()) {
            std::cerr << "Semantic Error: Variable '" << name << "' is already declared in current scope" << std::endl;
            return false;
        }

        // 声明新变量
        currentScope[name] = Symbol(name, SymbolType::VARIABLE, type, static_cast<int>(scopes.size() - 1));
        return true;
    }

    bool Environment::declareFunction(const std::string &name, DataType returnType, const std::string &moduleName) {
        if (scopes.empty()) {
            scopes.emplace_back();
        }

        auto &globalScope = scopes[0];

        // 函数名使用 "模块名.函数名" 作为唯一标识
        std::string fullName = moduleName + "." + name;

        if (globalScope.find(fullName) != globalScope.end()) {
            std::cerr << "Semantic Error: Function '" << fullName << "' is already declared" << std::endl;
            return false;
        }

        // 存储完整名称，但保留模块信息
        globalScope[fullName] = Symbol(name, moduleName, returnType, 0);
        return true;
    }

    bool Environment::declareModule(const std::string &name) {
        if (scopes.empty()) {
            scopes.emplace_back();
        }

        auto &globalScope = scopes[0];

        // 模块名单独存储，不与函数冲突
        if (globalScope.find(name) != globalScope.end()) {
            // 如果已经存在，检查是否是模块
            if (globalScope[name].type != SymbolType::MODULE) {
                std::cerr << "Semantic Error: Name '" << name << "' is already used" << std::endl;
                return false;
            }
            return true; // 模块已存在，不报错
        }

        globalScope[name] = Symbol(name, SymbolType::MODULE, NONE, 0);
        return true;
    }

    bool Environment::declareArray(const std::string &name, DataType elementType, int size, bool isMut) {
        if (scopes.empty())
            scopes.emplace_back();

        auto &currentScope = scopes.back();
        if (currentScope.find(name) != currentScope.end()) {
            std::cerr << "Semantic Error: Variable '" << name << "' already declared" << std::endl;
            return false;
        }

        currentScope[name] =
            Symbol(name, SymbolType::VARIABLE, elementType, static_cast<int>(scopes.size() - 1), size, isMut);
        return true;
    }

    bool Environment::declareArray(const std::string &name, DataType elementType, AST::Expression *sizeExpr,
                                   bool isMut) {
        if (scopes.empty())
            scopes.emplace_back();

        auto &currentScope = scopes.back();
        if (currentScope.find(name) != currentScope.end()) {
            std::cerr << "Semantic Error: Variable '" << name << "' already declared" << std::endl;
            return false;
        }

        currentScope[name] =
            Symbol(name, SymbolType::VARIABLE, elementType, static_cast<int>(scopes.size() - 1), sizeExpr, isMut);
        return true;
    }

    Symbol *Environment::lookupSymbol(const std::string &name) {
        // 从当前作用域开始向上查找
        for (int i = static_cast<int>(scopes.size()) - 1; i >= 0; i--) {
            auto &scopeMap = scopes[i];
            auto it = scopeMap.find(name);
            if (it != scopeMap.end()) {
                return &(it->second);
            }
        }
        return nullptr;
    }

    const Symbol *Environment::lookupSymbol(const std::string &name) const {
        // 从当前作用域开始向上查找
        for (int i = static_cast<int>(scopes.size()) - 1; i >= 0; i--) {
            const auto &scopeMap = scopes[i];
            auto it = scopeMap.find(name);
            if (it != scopeMap.end()) {
                return &(it->second);
            }
        }
        return nullptr;
    }

    bool Environment::isDeclared(const std::string &name) const {
        return lookupSymbol(name) != nullptr;
    }

    bool Environment::isDeclaredInCurrentScope(const std::string &name) const {
        if (scopes.empty())
            return false;

        const auto &currentScope = scopes.back();
        return currentScope.find(name) != currentScope.end();
    }

    DataType Environment::getSymbolType(const std::string &name) const {
        const Symbol *sym = lookupSymbol(name);
        if (sym) {
            return sym->dataType;
        }
        return UNKNOWN;
    }

    bool Environment::isTypeCompatible(DataType target, DataType source) {
        if (target == source)
            return true;

        // 允许整数到浮点数的隐式转换
        if (target == FLOAT && source == INT)
            return true;

        return false;
    }

    bool Environment::isNumericType(DataType type) {
        return type == INT || type == FLOAT;
    }

    void Environment::reset() {
        scopes.clear();
        scopes.emplace_back(); // 重新创建全局作用域
    }

    void Environment::printScope() const {
        if (scopes.empty()) {
            std::cout << "No scopes available" << std::endl;
            return;
        }

        int currentScope = static_cast<int>(scopes.size()) - 1;
        std::cout << "=== Current Scope (level " << currentScope << ") ===" << std::endl;

        const auto &scope = scopes.back();
        if (scope.empty()) {
            std::cout << "  (empty)" << std::endl;
        } else {
            for (const auto &[name, symbol] : scope) {
                std::cout << "  " << name << " : ";
                switch (symbol.type) {
                case SymbolType::VARIABLE:
                    std::cout << "variable";
                    break;
                case SymbolType::FUNCTION:
                    std::cout << "function";
                    break;
                case SymbolType::MODULE:
                    std::cout << "module";
                    break;
                }
                std::cout << " (" << dataTypeToString(symbol.dataType) << ")";
                if (!symbol.moduleName.empty()) {
                    std::cout << " [module=" << symbol.moduleName << "]";
                }
                std::cout << std::endl;
            }
        }
    }

    void Environment::printAllScopes() const {
        std::cout << "=== All Scopes (" << scopes.size() << " levels) ===" << std::endl;

        for (size_t i = 0; i < scopes.size(); i++) {
            std::cout << "Scope " << i << ":" << std::endl;
            const auto &scope = scopes[i];
            if (scope.empty()) {
                std::cout << "  (empty)" << std::endl;
            } else {
                for (const auto &[name, symbol] : scope) {
                    std::cout << "  " << name << " : ";
                    switch (symbol.type) {
                    case SymbolType::VARIABLE:
                        std::cout << "variable";
                        break;
                    case SymbolType::FUNCTION:
                        std::cout << "function";
                        break;
                    case SymbolType::MODULE:
                        std::cout << "module";
                        break;
                    }
                    std::cout << " (" << dataTypeToString(symbol.dataType) << ")";
                    if (!symbol.moduleName.empty()) {
                        std::cout << " [module=" << symbol.moduleName << "]";
                    }
                    std::cout << std::endl;
                }
            }
        }
    }
} // namespace env
