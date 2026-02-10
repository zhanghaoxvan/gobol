cmake_minimum_required(VERSION 4.1)
project(Gobol)

set(CMAKE_CXX_STANDARD 20)

add_executable(Gobol main.cpp
        Lexer/Lexer.cpp
        Lexer/Lexer.hpp
        Lexer/Token.hpp
        AST/ASTBuilder.cpp
        AST/ASTBuilder.hpp
        Lexer/Token.cpp
        AST/AST.cpp
        AST/AST.hpp
)
include_directories(${CMAKE_SOURCE_DIR})

if (${CMAKE_BUILD_TYPE} STREQUAL "Debug")
    add_compile_definitions(DEBUG)
endif ()
