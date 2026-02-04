//
// Created by 35921 on 2026/1/14.
//

#ifndef GOBOL_LEXER_HPP
#define GOBOL_LEXER_HPP

#include <Lexer/Token.hpp>
#include <string_view>
#include <unordered_set>

namespace lexer {

    /**
     * @brief Core lexical analyzer (Lexer) for GOBOL language
     * @details Converts the input source code string into a sequential stream of lexical tokens.
     *          Serves as the first phase of the compiler/interpreter pipeline, providing
     *          standardized tokens for the subsequent syntax parser. Implements features
     *          including keyword recognition, comment skipping, identifier/number/string
     *          parsing, operator detection and basic error handling.
     * @note Adopts std::string_view for zero-copy efficient source code traversal.
     *       Implements single-pointer sequential parsing, not thread-safe.
     */
    class Lexer {
        /** Source code string to be parsed (read-only, zero-copy) */
        std::string_view source;
        /** Current character position index in source code (starts from 0) */
        size_t currentPosition;
        /** Current line number for error location tracking (starts from 1) */
        int line;
        /** Current column number for error location tracking (starts from 0) */
        int col;
        /** Set of GOBOL language keywords, matched during identifier parsing */
        const std::unordered_set<std::string> keywords = {"if",  "else", "for", "return", "int",    "float",
                                                          "str", "func", "var", "val",    "module", "import"};

        /**
         * @brief Check if the parser has reached the end of the source code
         * @return True if at end of source, false otherwise
         */
        [[nodiscard]] bool isSourceEnd() const;

        /**
         * @brief Peek the current character without advancing the parse pointer
         * @return Current character at currentPosition, '\0' if at end of source
         */
        [[nodiscard]] char peek() const;

        /**
         * @brief Peek the next character (1-character lookahead) without advancing the parse pointer
         * @details Used for multi-character pattern matching such as operators and comment delimiters
         * @return Next character at currentPosition+1, '\0' if at end of source
         */
        [[nodiscard]] char peekNext() const;

        /**
         * @brief Consume the current character and update parsing state
         * @details Advances currentPosition, increments column number. Resets column to 0
         *          and increments line number if the consumed character is a newline ('\n').
         * @return Consumed character, '\0' if at end of source
         */
        char consume();

        /**
         * @brief Skip single-line comments (// ... to end of line)
         * @details Consumes all characters from "//" until newline or end of source.
         *          No lexical tokens are generated for comment content.
         */
        void skipLineComment();

        /**
         * @brief Skip block comments (/ * ... * /)
         * @details Consumes all characters from "/ *" until "* /". Supports multi-line
         *          comments and skips newlines within comment blocks. No tokens generated.
         * @return True if comment is properly closed with "* /", false if EOF is reached
         */
        bool skipBlockComment();

        /**
         * @brief Parse identifiers or language keywords
         * @details Identifiers start with a letter or underscore, followed by letters,
         *          digits or underscores. Matches parsed string against keyword set:
         *          returns keyword token if matched, otherwise identifier token.
         * @return Corresponding Token object (KEYWORD or IDENTIFIER)
         */
        token::Token parseIdentifier();

        /**
         * @brief Parse numeric literals (integers and floating-point numbers)
         * @details Supports decimal integers and valid floating-point numbers.
         *          Rejects invalid numeric formats (leading dot, trailing dot, multiple dots).
         * @return NUMBER Token for valid format, WRONG_TOKEN for invalid numeric format
         */
        token::Token parseNumber();

        /**
         * @brief Parse string literals enclosed in double quotes
         * @details Supports escape sequences (e.g., " , \) within string content.
         *          Parses characters from opening " until closing " or end of source.
         * @return STRING Token for properly closed string, WRONG_TOKEN for unclosed string
         */
        token::Token parseString();

    public:
        /**
         * @brief Constructor: initialize lexical analyzer state
         * @param source Source code string to be parsed (std::string_view for zero-copy)
         * @note Explicit constructor to prevent implicit type conversion from string types
         */
        explicit Lexer(std::string_view source);

        /**
         * @brief Get the next valid lexical token from the source code (core public interface)
         * @details The primary entry point for the lexical analyzer that performs sequential tokenization.
         *          Iteratively skips non-lexical content (whitespace except newlines, single/multi-line comments)
         *          then parses the next valid character/sequence into a strongly-typed Token. Implements
         *          fault-tolerant parsing: unrecognized characters are marked as WRONG_TOKEN without halting
         *          the entire parsing process. All parsing state updates (currentPosition, line, col) are
         *          managed exclusively through the consume() method to ensure consistency.
         * @steps
         *  1. Skip all whitespace (space/tab/carriage return) while preserving newlines as END_OF_LINE tokens
         *  2. Detect and skip single-line (//) and multi-line (/ * * /) comments without generating tokens
         *  3. Check for end-of-source after skipping non-lexical content and return END_OF_FILE if reached
         *  4. Process newline characters as independent END_OF_LINE tokens to preserve line structure
         *  5. Dispatch composite token parsing (identifier/number/string) based on leading character pattern
         *  6. Parse single-character operators and delimiters with immediate state consumption
         *  7. Mark unrecognized characters as WRONG_TOKEN and consume them to continue parsing
         * @note Newlines are preserved as END_OF_LINE tokens to support line-based syntax analysis
         * @note All character classification uses static_cast<unsigned char> to avoid undefined behavior for signed
         * chars
         * @note Fault-tolerant design ensures parsing continues even if invalid characters are encountered
         * @note No out-of-bounds access: all character reads use peek()/peekNext() with built-in boundary checks
         * @return A valid token::Token object with corresponding TokenType and raw string value;
         *         returns END_OF_FILE token when the end of source code is reached
         */
        token::Token getNextToken();
    };

} // namespace lexer

#endif // GOBOL_LEXER_HPP