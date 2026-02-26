#if !defined(AST_PRINTER_HPP) && defined(DEBUG)
#define AST_PRINTER_HPP

#include "AST.hpp" // 包含宏定义
#include <iostream>
#include <string>

namespace AST {

    class ASTPrinter : public ASTVisitor {
        int indentLevel = 0;
        void printIndent() const {
            std::cout << std::string(indentLevel * 2, ' ');
        }

    public:
        VISIT_ASTNODE(ASTNode) {
            printIndent();
            std::cout << "ASTNode" << std::endl;
        }

        VISIT_ASTNODE(Program) {
            printIndent();
            std::cout << "Program" << std::endl;
            ++indentLevel;
            for (auto &stmt : node->getStatements()) {
                if (stmt) {
                    stmt->accept(this);
                }
            }
            --indentLevel;
        }

        VISIT_ASTNODE(Statement) {
            printIndent();
            std::cout << "Statement" << std::endl;
        }

        VISIT_ASTNODE(Expression) {
            printIndent();
            std::cout << "Expression" << std::endl;
        }

        VISIT_ASTNODE(Block) {
            printIndent();
            std::cout << "Block" << std::endl;
            ++indentLevel;
            for (auto &stmt : node->getStatements()) {
                stmt->accept(this);
            }
            --indentLevel;
        }

        VISIT_ASTNODE(Function) {
            printIndent();
            std::cout << "Function" << std::endl;
            ++indentLevel;

            printIndent();
            std::cout << "name: " << node->getName() << std::endl;

            printIndent();
            std::cout << "parameters:" << std::endl;
            ++indentLevel;
            if (node->getParameters()) {
                for (auto &param : *node->getParameters()) {
                    param->accept(this);
                }
            }
            --indentLevel;

            printIndent();
            std::cout << "return-type: ";
            node->getReturnType()->accept(this);
            std::cout << std::endl;

            printIndent();
            std::cout << "body:" << std::endl;
            ++indentLevel;
            node->getBody()->accept(this);
            --indentLevel;
            --indentLevel;
        }

        VISIT_ASTNODE(Parameter) {
            printIndent();
            std::cout << node->getName() << ": ";
            node->getType()->accept(this);
            std::cout << std::endl;
        }

        VISIT_ASTNODE(Type) {
            std::cout << node->getName(); // 不缩进，因为可能在内联使用
        }

        VISIT_ASTNODE(ArrayType) {
            std::cout << node->getName() << "[";
            if (node->getSize()) {
                node->getSize()->accept(this); // 这里会调用 NumberLiteral 的打印
            } else {
                std::cout << "?"; // 未知大小
            }
            std::cout << "]";
        }

        VISIT_ASTNODE(IfStatement) {
            printIndent();
            std::cout << "IfStatement" << std::endl;
            ++indentLevel;

            printIndent();
            std::cout << "condition:" << std::endl;
            ++indentLevel;
            node->getCondition()->accept(this);
            --indentLevel;

            printIndent();
            std::cout << "then:" << std::endl;
            node->getThenBranch()->accept(this);

            if (node->getElseBranch()) {
                printIndent();
                std::cout << "else:" << std::endl;
                node->getElseBranch()->accept(this);
            }

            --indentLevel;
        }

        VISIT_ASTNODE(WhileStatement) {
            printIndent();
            std::cout << "WhileStatement" << std::endl;
            ++indentLevel;

            printIndent();
            std::cout << "condition:" << std::endl;
            ++indentLevel;
            node->getCondition()->accept(this);
            --indentLevel;

            printIndent();
            std::cout << "body:" << std::endl;
            node->getBody()->accept(this);

            --indentLevel;
        }

        VISIT_ASTNODE(ForStatement) {
            printIndent();
            std::cout << "ForStatement" << std::endl;
            ++indentLevel;

            printIndent();
            std::cout << "variable: " << node->getLoopVariable() << std::endl;

            printIndent();
            std::cout << "iterable: ";
            ++indentLevel;
            node->getIterable()->accept(this);
            std::cout << std::endl;
            --indentLevel;

            printIndent();
            std::cout << "body:" << std::endl;
            ++indentLevel;
            node->getBody()->accept(this);

            --indentLevel;

            --indentLevel;
        }

        VISIT_ASTNODE(ReturnStatement) {
            printIndent();
            std::cout << "ReturnStatement";
            if (node->getValue()) {
                std::cout << " ";
                node->getValue()->accept(this);
            }
            std::cout << std::endl;
        }

        VISIT_ASTNODE(BreakStatement) {
            printIndent();
            std::cout << "BreakStatement" << std::endl;
        }

        VISIT_ASTNODE(ContinueStatement) {
            printIndent();
            std::cout << "ContinueStatement" << std::endl;
        }

        VISIT_ASTNODE(Declaration) {
            printIndent();
            std::cout << node->getKeyword() << " " << node->getName();
            if (node->getType()) {
                std::cout << ": ";
                node->getType()->accept(this);
            }
            if (node->getInitializer()) {
                std::cout << " = ";
                node->getInitializer()->accept(this);
            }
            std::cout << std::endl;
        }

        VISIT_ASTNODE(ExpressionStatement) {
            printIndent();
            node->getExpression()->accept(this);
            std::cout << ";" << std::endl;
        }

        VISIT_ASTNODE(ImportStatement) {
            printIndent();
            std::cout << "Import(moduleName = " << node->getModuleName() << ")" << std::endl;
        }

        VISIT_ASTNODE(ModuleStatement) {
            printIndent();
            std::cout << "Module(moduleName = " << node->getModuleName() << ")" << std::endl;
        }

        VISIT_ASTNODE(BinaryExpression) {
            std::cout << "(";
            node->getLeft()->accept(this);
            std::cout << " " << node->getOperator() << " ";
            node->getRight()->accept(this);
            std::cout << ")";
        }

        VISIT_ASTNODE(UnaryExpression) {
            std::cout << node->getOperator();
            node->getOperand()->accept(this);
        }

        VISIT_ASTNODE(FunctionCall) {
            node->getCallee()->accept(this);
            std::cout << "(";
            if (node->getArguments()) {
                for (size_t i = 0; i < node->getArguments()->size(); ++i) {
                    if (i > 0)
                        std::cout << ", ";
                    (*node->getArguments())[i]->accept(this);
                }
            }
            std::cout << ")";
        }

        VISIT_ASTNODE(MemberAccess) {
            node->getObject()->accept(this);
            std::cout << "." << node->getMember();
        }

        VISIT_ASTNODE(ArrayIndex) {
            node->getArray()->accept(this);
            std::cout << "[";
            node->getIndex()->accept(this);
            std::cout << "]";
        }

        VISIT_ASTNODE(GroupedExpression) {
            std::cout << "(";
            node->getExpression()->accept(this);
            std::cout << ")";
        }

        VISIT_ASTNODE(Identifier) {
            std::cout << node->getName();
        }

        VISIT_ASTNODE(NumberLiteral) {
            std::cout << node->getValue();
        }

        VISIT_ASTNODE(StringLiteral) {
            std::cout << "\"" << node->getValue() << "\"";
        }

        VISIT_ASTNODE(BooleanLiteral) {
            std::cout << (node->getValue() ? "true" : "false");
        }

        VISIT_ASTNODE(FormatString) {
            std::cout << "@\"" << node->getValue() << "\"";
            if (!node->getVariables().empty()) {
                std::cout << " [";
                bool first = true;
                for (const auto &variable : node->getVariables()) {
                    if (!first)
                        std::cout << ", ";
                    first = false;
                    if (auto *id = dynamic_cast<Identifier *>(variable.value)) {
                        std::cout << id->getName() << ":" << variable.posInValue;
                    } else {
                        std::cout << "?@" << variable.posInValue;
                    }
                }
                std::cout << "]";
            }
        }

        VISIT_ASTNODE(RangeExpression) {
            std::cout << "range(";
            const auto &args = node->getArguments();
            for (size_t i = 0; i < args.size(); ++i) {
                if (i > 0)
                    std::cout << ", ";
                args[i]->accept(this);
            }
            std::cout << ")";
        }
    };

} // namespace AST

#endif // AST_PRINTER_HPP
