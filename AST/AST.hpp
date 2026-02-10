//
// Created by 35921 on 2026/2/5.
//

#ifndef GOBOL_AST_HPP
#define GOBOL_AST_HPP

#include <Lexer/Token.hpp>
#include <vector>

namespace AST {
    class ASTVisitor;

    enum class ASTNodeType {
        PROGRAM,

        IMPORT_DECL,
        FUNCTION_DECL,
        VAR_DECL,
        MODULE_DECL,

        RETURN_STMT,
        EXPR_STMT,
        BLOCK_STMT,
        IF_STMT,
        WHILE_STMT,
        FOR_STMT,

        IDENTIFIER,
        KEYWORD,
        NUMBER,
        STRING,
        FORMAT_STRING,
        OPERATOR,
        END_OF_LINE,
        END_OF_FILE,

        UNKNOWN,
    };

    struct ASTNode {
        ASTNode *parent;
        std::vector<ASTNode *> children;
        lexer::token::Token token;
        ASTNodeType type;
        ASTNode(ASTNode *parent, lexer::token::Token token);
        ASTNode(ASTNode *parent, lexer::token::Token token, ASTNodeType type);
        virtual ~ASTNode();
        ASTNode(const ASTNode &) = delete;
        ASTNode &operator=(const ASTNode &) = delete;
        ASTNode(ASTNode &&) = default;
        ASTNode &operator=(ASTNode &&) = default;

        virtual void accept(ASTVisitor &visitor) = 0;
    };

    // ------------- Begin Statement Node Declaration -------------

    struct ImportDeclNode final : ASTNode {};

    struct FunctionDeclNode final : ASTNode {};

    struct VarDeclNode final : ASTNode {};

    // ------------- End Statement Node Declaration -------------

    class ASTManager {
        ASTNode *root;

    public:
        ASTManager() = delete;
        explicit ASTManager(ASTNode *root);
        virtual ~ASTManager() = default;

        virtual void accept() = 0;
    };

} // namespace AST

#endif // GOBOL_AST_HPP
