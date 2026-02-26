#include <AST/ASTBuilder.hpp>
#include <AST/ASTPrinter.hpp>
#include <Environment/SemanticAnalyzer.hpp>
#include <Interpreter/Interpreter.hpp>
#include <Lexer/Lexer.hpp>
#include <fstream>
#include <iostream>
#include <sstream>

std::string getSource(const std::string &file) {
    std::ifstream ifs(file, std::ios::binary); // 以二进制模式打开

    // 检查文件是否成功打开
    if (!ifs.is_open()) {
        std::cerr << "Error: Cannot open file '" << file << "'" << std::endl;
        return "";
    }

    // 读取整个文件
    std::stringstream buf;
    buf << ifs.rdbuf();
    std::string source = buf.str();

    return source;
}
int main(int argc, char *argv[]) {

    if (argc == 1) {
        std::cout << "Usage:" << std::endl;
        std::cout << "  " << argv[0] << " <filename>" << std::endl;
        return 0;
    }

    std::string source = getSource(argv[1]);

    lexer::Lexer lexer(source);
    auto tk = lexer.getNextToken();
#ifdef DEBUG
    std::cout << "===== Step 0: Reprint Source =====" << std::endl;
    std::cout << source << std::endl;
    std::cout << "===== Step 1: Tokenize =====" << std::endl;
    while (tk.type != lexer::token::TokenType::END_OF_FILE) {
        std::cout << "Token(Type=" << lexer::token::tokenTypeToString(tk.type) << ", Val='"
                  << (tk.value == "\n" ? "\\n" : tk.value) << "')" << std::endl;
        tk = lexer.getNextToken();
    }
    std::cout << std::endl << std::endl;
    std::cout << "======= Step 2: AST =======" << std::endl;
    lexer.resetPosition();
#endif
    AST::ASTBuilder builder(lexer);
    AST::Program *prog = builder.build();
    if (builder.hasError()) {
        for (const auto &msg : builder.getErrorMessage()) {
            std::cerr << "Builder Error: " << msg << std::endl;
        }
        return 1;
    }
#ifdef DEBUG
    AST::ASTPrinter printer;
    printer.visit(prog);
    std::cout << std::endl << std::endl;
    std::cout << "======= Step 3: Semantic Analysis =======" << std::endl;
#endif
    analyzer::SemanticAnalyzer semanticAnalyzer;
    bool semanticPassed = semanticAnalyzer.analyze(prog);
    if (!semanticPassed) {
        return 1;
    }
#ifdef DEBUG
    std::cout << std::endl << std::endl;
    std::cout << "======= Step 4: Interpreter =======" << std::endl;
#endif
    interpreter::Interpreter interpreter;
    interpreter.execute(prog);
    return 0;
}
