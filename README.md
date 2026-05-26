# GoBol

[![Rust](https://img.shields.io/badge/rust-1.95%2B-blue.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-GPLv3-red.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/zhanghaoxvan/gobol)

**GoBol** — A statically-typed, modular programming language with generics support.

---

## 📦 Project Structure

```
gobol/
├── src/                      # Compiler source code
│   ├── main.rs
│   ├── lexer.rs              # Lexer
│   ├── ast.rs                # AST definition
│   ├── ast_builder.rs        # Parser
│   ├── ast_printer.rs        # AST printer
│   ├── environment.rs        # Symbol table environment
│   ├── semantic_analyzer.rs  # Semantic analyzer
│   ├── executor.rs           # Executor
│   ├── token.rs
│   └── value.rs
├── lib/                      # Standard library
│   ├── ***.gbl
│   └── ***.gbl
├── example.gbl
├── Cargo.toml
├── README.md
└── LICENSE
```

---

## 🚀 Quick Start

### Compile

```bash
cargo build --release
```

### Run

```bash
cargo run --release -- example.gbl
```

---

## 📄 Example

`example.gbl`:

```gobol
module main

func main(): int {
    io.print("Hello, GoBol!")
    return 0
}
```

See [Language Docs](language.md) for more details.

---

## 📜 License

**GNU General Public License v3.0**

See [LICENSE](LICENSE) for details.
