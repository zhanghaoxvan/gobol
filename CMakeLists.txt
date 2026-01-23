cmake_minimum_required(VERSION 4.1)
project(Gobol)

set(CMAKE_CXX_STANDARD 20)

add_executable(Gobol main.cpp
        Lexer/Lexer.cpp
        Lexer/Lexer.hpp
        Lexer/Token.hpp
        AST/AST.cpp
        AST/AST.hpp
)
