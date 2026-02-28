// Bytecode/FormatPiece.hpp
#ifndef FORMAT_PIECE_HPP
#define FORMAT_PIECE_HPP

#include <string>

namespace vm {

    struct FormatPiece {
        enum class Type {
            TEXT,    // 普通文本
            VARIABLE // 变量占位符
        };

        Type type;
        std::string content; // 如果是TEXT: 文本内容; 如果是VARIABLE: 变量名

        FormatPiece(Type t, const std::string &c) : type(t), content(c) {
        }

        bool isText() const {
            return type == Type::TEXT;
        }
        bool isVariable() const {
            return type == Type::VARIABLE;
        }
    };

} // namespace vm

#endif // FORMAT_PIECE_HPP
