//
// Created by 35921 on 2026/2/5.
//

#include "AST.hpp"

#include <utility>

namespace AST {

    ASTNode::ASTNode(ASTNode *parent, lexer::token::Token token)
        : parent(parent), token(std::move(token)), type(ASTNodeType::UNKNOWN) {
        this->parent->children.emplace_back(this);
    }

    ASTNode::ASTNode(ASTNode *parent, lexer::token::Token token, const ASTNodeType type)
        : parent(parent), token(std::move(token)), type(type) {
    }
    ASTNode::~ASTNode() {
        for (const ASTNode *child : this->children) {
            delete child;
        }
        this->children.clear();
    }

    ASTManager::ASTManager(ASTNode *root) : root(root) {
    }

} // namespace AST