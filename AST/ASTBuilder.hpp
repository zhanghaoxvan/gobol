//
// Created by 35921 on 2026/1/16.
//

#ifndef AST_BUILDER_HPP
#define AST_BUILDER_HPP

#include "AST.hpp"

/**
 * @namespace AST
 * @brief 抽象语法树（AST）相关命名空间
 *
 * 包含 AST 节点类型定义、基础节点结构、具体节点实现和访问者接口，封装 GOBOL 语言的语法树结构。
 */
namespace AST {
    class ASTBuilder {
        ASTNode *root;
        ASTNode *current;

    public:
        ASTBuilder();
        ~ASTBuilder();

        void initRoot(const lexer::token::Token &program);
    };

} // namespace AST

#endif // AST_BUILDER_HPP
