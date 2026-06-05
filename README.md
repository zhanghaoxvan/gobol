```markdown
# Gobol

[![Rust](https://img.shields.io/badge/rust-1.95%2B-blue.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-GPLv3-red.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/zhanghaoxvan/gobol)

**Gobol** — A statically-typed, modular programming language with generics support.

**Gobol** — 一门静态类型、支持泛型的模块化编程语言。

---

## 📦 Project Structure / 项目结构

```
gobol/
├── src/                      # Compiler source code / 编译器源码
│   ├── bin/                  # Binary files' source / 二进制文件源码
│   │   ├── gobol.rs          # Main entry point of `gobol` / `gobol` 主入口点
│   │   └── grape.rs          # Main entry point of `grape` / `grape` 主入口点
│   ├── lexer.rs              # Lexer / 词法分析器
│   ├── ast.rs                # AST definition / 抽象语法树定义
│   ├── ast_builder.rs        # Parser / 解析器
│   ├── ast_printer.rs        # AST printer / 抽象语法树打印器
│   ├── environment.rs        # Symbol table environment / 符号表环境
│   ├── semantic_analyzer.rs  # Semantic analyzer / 语义分析器
│   ├── executor.rs           # Executor / 执行器
│   ├── token.rs
│   └── value.rs
├── lib/                      # Standard library / 标准库
│   ├── ***.gbl
│   └── ***.gbl
├── example.gbl
├── Cargo.toml
├── README.md
└── LICENSE
```

---

## 🚀 Quick Start / 快速开始

### Prerequisites / 前置要求

- Rust (1.95+)
- Git

### Installation / 安装

Install from source:

从源码安装：

```bash
cargo install --path .
```

Or build locally:

或本地编译：

```bash
cargo build --release
```

### Package Manager (Grape) / 包管理器（Grape）

Grape is the package manager for Gobol, similar to Cargo.

Grape 是 Gobol 的包管理器，类似于 Cargo。

```bash
# Initialize a new project / 初始化新项目
cargo run --bin grape init

# Add a dependency (format: user/repo@tag) / 添加依赖（格式：作者/仓库@标签）
cargo run --bin grape add gobol-org/math@0.1.0

# Add optional dependency / 添加可选依赖
cargo run --bin grape add gobol-org/test@0.2.0 --optional

# Remove a dependency / 移除依赖
cargo run --bin grape remove math

# Update dependencies / 更新依赖
cargo run --bin grape update

# List all dependencies / 列出所有依赖
cargo run --bin grape list

# Run the project / 运行项目
cargo run --bin grape run

# Run with verbose output / 带详细输出运行
cargo run --bin grape run --verbose

# Clean cached packages / 清理缓存包
cargo run --bin grape clean

# Show help / 显示帮助
cargo run --bin grape help
```

### Run Gobol Directly / 直接运行 Gobol

```bash
# Run a single file / 运行单个文件
cargo run --bin gobol example.gbl

# Run with verbose output (shows tokens, AST, semantic analysis) / 带详细输出运行（显示词法单元、抽象语法树、语义分析）
cargo run --bin gobol example.gbl --verbose
```

### Example / 示例

Create a file `main.gbl`:

创建文件 `main.gbl`：

```gobol
module main

func main(): int {
    io.print("Hello, Gobol!")
    return 0
}
```

Then run:

然后运行：

```bash
cargo run --bin grape run
# or directly but not recommended / 或直接运行，但不推荐
cargo run --bin gobol main.gbl
```

### After Installation / 安装完成后

If you ran `cargo install --path .`, you can use the commands directly:

如果你执行了 `cargo install --path .`，可以直接使用以下命令：

```bash
grape init
grape run
gobol main.gbl
```

---

## 📄 Example / 示例

`example.gbl`:

```gobol
module main

func main(): int {
    io.print("Hello, Gobol!")
    return 0
}
```

See [Language Docs](language.md) for more details.

更多详情请参阅[语言文档](language.md)。

---

## 📜 License / 许可证

**GNU General Public License v3.0**

See [LICENSE](LICENSE) for details.

详见 [LICENSE](LICENSE) 文件。
```
