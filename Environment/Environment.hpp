#ifndef ENVIRONMENT_HPP
#define ENVIRONMENT_HPP

#include <AST/AST.hpp>
#include <iostream>
#include <memory>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

namespace env {

    enum DataType { INT, FLOAT, STR, BOOL, NONE, UNKNOWN };
    enum class SymbolType { VARIABLE, FUNCTION, MODULE };

    // 数组维度信息
    struct ArrayDimension {
        bool isConstant;           // 是否是常量大小
        int constantSize;          // 常量大小
        AST::Expression *sizeExpr; // 表达式大小

        ArrayDimension(int size) : isConstant(true), constantSize(size), sizeExpr(nullptr) {
        }
        ArrayDimension(AST::Expression *expr) : isConstant(false), constantSize(0), sizeExpr(expr) {
        }
    };

    struct Symbol {
        std::string name;
        SymbolType type;
        DataType dataType;
        int scopeLevel;
        std::string moduleName;
        bool isMut = false;

        // 数组相关
        bool isArray = false;
        std::vector<ArrayDimension> dimensions; // 存储每一维的信息

        // 默认构造函数
        Symbol() : type(SymbolType::VARIABLE), dataType(UNKNOWN), scopeLevel(0) {
        }

        // 构造函数 - 普通变量
        Symbol(std::string n, SymbolType t, DataType dt, int scope)
            : name(std::move(n)), type(t), dataType(dt), scopeLevel(scope) {
        }

        // 构造函数 - 函数
        Symbol(std::string n, const std::string &module, DataType dt, int scope)
            : name(std::move(n)), type(SymbolType::FUNCTION), dataType(dt), scopeLevel(scope), moduleName(module) {
        }

        // 构造函数 - 模块 (使用 VARIABLE 类型但标记为模块？需要明确)
        // 这个可能需要单独处理，或者用上面的普通变量构造函数

        // 构造函数 - 数组 (常量大小)
        Symbol(std::string n, SymbolType t, DataType dt, int scope, const std::vector<int> &sizes, bool mut)
            : name(std::move(n)), type(t), dataType(dt), scopeLevel(scope), isMut(mut), isArray(true) {
            for (int size : sizes) {
                dimensions.emplace_back(size);
            }
        }

        // 构造函数 - 数组 (表达式大小)
        Symbol(std::string n, SymbolType t, DataType dt, int scope, const std::vector<AST::Expression *> &sizeExprs,
               bool mut)
            : name(std::move(n)), type(t), dataType(dt), scopeLevel(scope), isMut(mut), isArray(true) {
            for (auto *expr : sizeExprs) {
                dimensions.emplace_back(expr);
            }
        }

        // 获取维度数量
        int getDimension() const {
            return dimensions.size();
        }

        // 获取指定维度的大小（如果是常量）
        int getConstantSize(int dim) const {
            if (dim < 0 || dim >= static_cast<int>(dimensions.size()))
                return 0;
            return dimensions[dim].isConstant ? dimensions[dim].constantSize : 0;
        }

        // 获取指定维度的表达式
        AST::Expression *getSizeExpr(int dim) const {
            if (dim < 0 || dim >= static_cast<int>(dimensions.size()))
                return nullptr;
            return dimensions[dim].sizeExpr;
        }

        // 判断指定维度是否是常量
        bool isDimensionConstant(int dim) const {
            if (dim < 0 || dim >= static_cast<int>(dimensions.size()))
                return false;
            return dimensions[dim].isConstant;
        }
    };

    std::string dataTypeToString(DataType type);

    class Environment {
    private:
        std::vector<std::unordered_map<std::string, Symbol>> scopes;

    public:
        // 构造函数
        Environment() {
            scopes.emplace_back(); // 初始化全局作用域
        }

        // 作用域管理
        void enterScope() {
            scopes.emplace_back();
        }

        void exitScope() {
            if (!scopes.empty())
                scopes.pop_back();
        }

        int getCurrentScope() const {
            return scopes.size() - 1;
        }
        size_t getScopeCount() const {
            return scopes.size();
        }

        // 符号声明
        bool declareVariable(const std::string &name, DataType type, bool isMut = false);
        bool declareFunction(const std::string &name, DataType returnType, const std::string &moduleName);
        bool declareModule(const std::string &name);

        // 数组声明 - 支持多维
        bool declareArray(const std::string &name, DataType elementType, const std::vector<int> &sizes, bool isMut);
        bool declareArray(const std::string &name, DataType elementType,
                          const std::vector<AST::Expression *> &sizeExprs, bool isMut);

        // 符号查找
        Symbol *lookupSymbol(const std::string &name);
        const Symbol *lookupSymbol(const std::string &name) const;
        bool isDeclared(const std::string &name) const;
        bool isDeclaredInCurrentScope(const std::string &name) const;
        DataType getSymbolType(const std::string &name) const;

        // 类型检查
        static bool isTypeCompatible(DataType target, DataType source);
        static bool isNumericType(DataType type);

        // 工具函数
        void reset();
        void printScope() const;
        void printAllScopes() const;
    };

} // namespace env

#endif // ENVIRONMENT_HPP
