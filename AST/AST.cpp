/**
 * @file AST.cpp
 * @brief 抽象语法树（AST）节点的核心实现文件
 *
 * 该文件实现了 ASTNode 类的构造函数、析构函数，定义了 AST 节点的基础结构，
 * 包括父节点、子节点、关联的 Token 以及节点类型，是语法分析阶段的核心数据结构。
 * @author （可补充作者信息）
 * @date （可补充日期信息）
 */

#include "AST.hpp"
#include <utility>

namespace AST {

    ASTNode::ASTNode(ASTNode *parent, lexer::token::Token token)
        : parent(parent), token(std::move(token)), type(ASTNodeType::UNKNOWN) {
        // 将当前节点添加到父节点的子节点列表
        this->parent->children.emplace_back(this);
    }

    ASTNode::ASTNode(ASTNode *parent, lexer::token::Token token, const ASTNodeType type)
        : parent(parent), token(std::move(token)), type(type) {
    }

    ASTNode::~ASTNode() {
        for (ASTNode *child : this->children) {
            delete child; // 递归删除子节点
        }
        this->children.clear(); // 清空子节点列表（可选，析构时容器会自动清理）
    }

    ImportDeclNode::ImportDeclNode(ASTNode *parent, lexer::token::Token token)
        : ASTNode(parent, std::move(token), ASTNodeType::IMPORT_DECL) {
    }

    void ImportDeclNode::accept(ASTVisitor &visitor) {
        visitor.visit(*this);
    }

    FunctionDeclNode::FunctionDeclNode(ASTNode *parent, lexer::token::Token token)
        : ASTNode(parent, std::move(token), ASTNodeType::FUNCTION_DECL) {
    }

    void FunctionDeclNode::accept(ASTVisitor &visitor) {
        visitor.visit(*this);
    }

} // namespace AST
