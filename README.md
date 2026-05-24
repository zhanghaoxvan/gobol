# GoBol

[![Rust](https://img.shields.io/badge/rust-1.95%2B-blue.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-GPLv3-red.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/zhanghaoxvan/gobol)

**GoBol** — 一门静态类型、支持泛型的现代编程语言。

---

## 📦 项目结构

```
gobol/
├── src/                      # 编译器源代码
│   ├── main.rs
│   ├── lexer.rs              # 词法分析器
│   ├── ast.rs                # AST 定义
│   ├── ast_builder.rs        # 语法分析器
│   ├── ast_printer.rs        # AST 打印器
│   ├── environment.rs        # 符号表环境
│   ├── semantic_analyzer.rs  # 语义分析器
│   ├── executor.rs           # 解释器
│   ├── token.rs
│   └── value.rs
├── lib/                      # 标准库
│   ├── __builtin__.gbl
│   ├── __setup__.gbl
│   ├── io.gbl
│   ├── range.gbl
│   └── vec.gbl
├── example.gbl
├── Cargo.toml
├── README.md
└── LICENSE
```

---

## 🚀 快速开始

### 编译

```bash
cargo build --release
```

### 运行

```bash
cargo run --release -- example.gbl
```

---

## 📄 示例

`example.gbl`:

```go
module main

func main(): int {
    io.print("Hello, GoBol!")
    return 0
}
```

---

## 📜 许可证

**GNU General Public License v3.0**

详见 [LICENSE](LICENSE) 文件。
