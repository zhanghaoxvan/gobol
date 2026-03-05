#include "Environment.hpp"
#include <algorithm>
#include <iostream>

namespace env {

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

    bool Environment::declareVariable(const std::string &name, DataType type, bool isMut) {
        if (scopes.empty())
            scopes.emplace_back();

        auto &currentScope = scopes.back();
        if (currentScope.find(name) != currentScope.end()) {
            std::cerr << "Semantic Error: Variable '" << name << "' already declared" << std::endl;
            return false;
        }

        Symbol sym(name, SymbolType::VARIABLE, type, getCurrentScope());
        sym.isMut = isMut;
        currentScope[name] = sym;
        return true;
    }

    bool Environment::declareFunction(const std::string &name, DataType returnType, const std::string &moduleName) {
        if (scopes.empty())
            scopes.emplace_back();

        auto &globalScope = scopes[0];
        std::string fullName = moduleName + "." + name;

        if (globalScope.find(fullName) != globalScope.end()) {
            std::cerr << "Semantic Error: Function '" << fullName << "' already declared" << std::endl;
            return false;
        }

        globalScope[fullName] = Symbol(name, moduleName, returnType, 0);
        return true;
    }

    bool Environment::declareModule(const std::string &name) {
        if (scopes.empty())
            scopes.emplace_back();

        auto &globalScope = scopes[0];
        if (globalScope.find(name) != globalScope.end()) {
            if (globalScope[name].type != SymbolType::MODULE) {
                std::cerr << "Semantic Error: Name '" << name << "' already used" << std::endl;
                return false;
            }
            return true;
        }

        globalScope[name] = Symbol(name, SymbolType::MODULE, NONE, 0);
        return true;
    }

    bool Environment::declareArray(const std::string &name, DataType elementType, const std::vector<int> &sizes,
                                   bool isMut) {
        if (scopes.empty())
            scopes.emplace_back();

        auto &currentScope = scopes.back();
        if (currentScope.find(name) != currentScope.end()) {
            std::cerr << "Semantic Error: Variable '" << name << "' already declared" << std::endl;
            return false;
        }

        currentScope[name] = Symbol(name, SymbolType::VARIABLE, elementType, getCurrentScope(), sizes, isMut);
        return true;
    }

    bool Environment::declareArray(const std::string &name, DataType elementType,
                                   const std::vector<AST::Expression *> &sizeExprs, bool isMut) {
        if (scopes.empty())
            scopes.emplace_back();

        auto &currentScope = scopes.back();
        if (currentScope.find(name) != currentScope.end()) {
            std::cerr << "Semantic Error: Variable '" << name << "' already declared" << std::endl;
            return false;
        }

        currentScope[name] = Symbol(name, SymbolType::VARIABLE, elementType, getCurrentScope(), sizeExprs, isMut);
        return true;
    }

    Symbol *Environment::lookupSymbol(const std::string &name) {
        for (int i = static_cast<int>(scopes.size()) - 1; i >= 0; i--) {
            auto &scopeMap = scopes[i];
            auto it = scopeMap.find(name);
            if (it != scopeMap.end())
                return &(it->second);
        }
        return nullptr;
    }

    const Symbol *Environment::lookupSymbol(const std::string &name) const {
        for (int i = static_cast<int>(scopes.size()) - 1; i >= 0; i--) {
            const auto &scopeMap = scopes[i];
            auto it = scopeMap.find(name);
            if (it != scopeMap.end())
                return &(it->second);
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
        return sym ? sym->dataType : UNKNOWN;
    }

    bool Environment::isTypeCompatible(DataType target, DataType source) {
        if (target == source)
            return true;
        if (target == FLOAT && source == INT)
            return true;
        return false;
    }

    bool Environment::isNumericType(DataType type) {
        return type == INT || type == FLOAT;
    }

    void Environment::reset() {
        scopes.clear();
        scopes.emplace_back();
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
                if (symbol.isArray) {
                    std::cout << " array[";
                    for (size_t i = 0; i < symbol.dimensions.size(); i++) {
                        if (i > 0)
                            std::cout << ",";
                        if (symbol.dimensions[i].isConstant) {
                            std::cout << symbol.dimensions[i].constantSize;
                        } else {
                            std::cout << "expr";
                        }
                    }
                    std::cout << "]";
                }
                std::cout << (symbol.isMut ? " (var)" : " (val)") << std::endl;
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
                    if (symbol.isArray) {
                        std::cout << " array[";
                        for (size_t j = 0; j < symbol.dimensions.size(); j++) {
                            if (j > 0)
                                std::cout << ",";
                            if (symbol.dimensions[j].isConstant) {
                                std::cout << symbol.dimensions[j].constantSize;
                            } else {
                                std::cout << "expr";
                            }
                        }
                        std::cout << "]";
                    }
                    std::cout << (symbol.isMut ? " (var)" : " (val)") << std::endl;
                }
            }
        }
    }

} // namespace env
