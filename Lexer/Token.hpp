//
// Created by 35921 on 2026/1/14.
//

#ifndef GOBOL_TOKEN_HPP
#define GOBOL_TOKEN_HPP

#include <string>

enum class TokenType {
    IDENTIFIER,       // like variable, function, class...
    NUMBER,           // like 1, 2, 3, 4, 114514...
    STRING,           // like "Hello world!"...
    OP_PLUS,          // +
    OP_MINUS,         // -
    OP_MULT,          // *
    OP_DIV,           // /
    OP_LEFT_BRACKET,  // (
    OP_RIGHT_BRACKET, // )
    OP_LEFT_BRACE,    // {
    OP_RIGHT_BRACE,   // }
    OP_LEFT_SQUARE,   // [
    OP_RIGHT_SQUARE,  // ]
    OP_ASSIGN,        // =
    OP_FORMAT,        // @ in @STRING (STRING is "... {IDENFIER} ...")
    END_OF_LINE,      // \n
    END_OF_FILE,      // \0
};

namespace gobol {
    class Token {};
} // namespace gobol

#endif // GOBOL_TOKEN_HPP