#include <Lexer/Lexer.hpp>
#include <Lexer/Token.hpp>
#include <iostream>
int main(int argc, char *argv[]) {
    lexer::Lexer lexer(R"(import io
func main(): int {
    var name: str = "Gobol"
    io.print(@"Hello from {name}")
    for i in range(0, 10, 1) {
        io.print(@"Number {i}: {name}\n")
    }
    return 0
})");
    auto tk = lexer.getNextToken();
#ifdef DEBUG
    std::cout << "===== Step 1: Tokenize =====" << std::endl;
    while (tk.type != lexer::token::TokenType::END_OF_FILE) {
        std::cout << "Token(Type=" << lexer::token::tokenTypeToString(tk.type) << ", Val='"
                  << (tk.value == "\n" ? "\\n" : tk.value) << "')" << std::endl;
        tk = lexer.getNextToken();
    }
    std::cout << std::endl << std::endl;
    std::cout << "======= Step 2: AST =======" << std::endl;
    // TODO
#endif

    return 0;
}
