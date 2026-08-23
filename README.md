# Gobol

[![Rust](https://img.shields.io/badge/rust-1.95%2B-blue.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-GPLv3-red.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)](https://github.com/zhanghaoxvan/gobol)

**Gobol** — A statically-typed, modular programming language with generics support.

**Gobol** — 一门静态类型、支持泛型的模块化编程语言。

---

## 🚀 Quick Start / 快速开始

### Prerequisites / 前置要求

- Rust (1.95+)
- Python (3.7+)
- Git

### Installation / 安装

```bash
git clone https://github.com/zhanghaoxvan/gobol.git
cd gobol
python3 install.py
```

That's it! The installer will:
- Build the compiler, package manager, and LSP server
- Copy binaries to the installation directory (default `~/.gobol`)
- Add `~/.gobol/bin` to your PATH
- Copy the standard library to `~/.gobol/lib/std`

就是这样！安装程序会：
- 编译编译器、包管理器和 LSP 服务器
- 复制二进制文件到安装目录（默认 `~/.gobol`）
- 添加 `~/.gobol/bin` 到 PATH
- 复制标准库到 `~/.gobol/lib/std`

#### Custom Install Directory / 自定义安装目录

```bash
# Set environment variable to specify installation location / 设置环境变量指定安装位置
export GOBOL_INSTALL_DIR=/my/custom/path
python3 install.py
```

Or enter a custom path when prompted during installation.

或者在安装过程中根据提示输入自定义路径。

#### Options / 选项

| Option / 选项 | Description / 描述 |
|---|---|
| `GOBOL_INSTALL_DIR` env / 环境变量 | Installation directory / 安装目录 |
| `--no-build` | Skip building (use existing binaries) / 跳过编译（使用现有二进制文件）|

#### Verify Installation / 验证安装

```bash
gobol --version
grape --version
gobol-lsp --version
```

#### Uninstall / 卸载

```bash
python3 install.py
# Select "Uninstall Gobol" from the menu / 选择菜单中的 "Uninstall Gobol"
```

Or manually remove:

或手动删除：

```bash
rm -rf ~/.gobol
# And clean up GOBOL_HOME related lines in .bashrc/.zshrc
# 并清理 .bashrc/.zshrc 中的 GOBOL_HOME 相关行
```

### Verify Installation / 验证安装

```bash
gobol --version
grape --help
```

---

## 📦 Package Manager (Grape) / 包管理器（Grape）

Grape is the package manager for Gobol, similar to Cargo.

Grape 是 Gobol 的包管理器，类似于 Cargo。

```bash
# Initialize a new project / 初始化新项目
grape init

# Add a dependency (format: user/repo@tag) / 添加依赖（格式：作者/仓库@标签）
grape add gobol-org/math@0.1.0

# Remove a dependency / 移除依赖
grape remove math

# Update dependencies / 更新依赖
grape update

# List all dependencies / 列出所有依赖
grape list

# Run the project / 运行项目
grape run

# Compile to native binary / 编译为原生二进制
grape run --compile

# Clean build artifacts and cached packages / 清理编译产物与缓存包
grape clean

# Show help / 显示帮助
grape help
```

Like Cargo, all compilation-local data lives under the project root `target/`:
build artifacts (`target/{triple}/{debug|release}/`), intermediate object files
(`.o`/`.obj`) and cached dependency packages (`target/grape/packages/`).

与 Cargo 类似，所有编译相关数据都集中在项目根目录的 `target/` 下：编译产物
（`target/{triple}/{debug|release}/`）、中间目标文件（`.o`/`.obj`）以及依赖包缓存
（`target/grape/packages/`）。

---

## 🏃 Run Gobol Directly / 直接运行 Gobol

```bash
# Run a single file / 运行单个文件
gobol example.gbl

# Run with verbose output / 带详细输出运行
gobol example.gbl --verbose

# Compile to native binary / 编译为原生二进制
gobol example.gbl --compile -o myapp
./myapp
```

---

## 📄 Example / 示例

Create a file `main.gbl`:

创建文件 `main.gbl`：

```gobol
import std;

func main() {
    io::println("Hello, Gobol!")
}
```

Then run:

然后运行：

```bash
grape run
# or / 或
gobol main.gbl
```

---

## 📖 Documentation / 文档

- [Language Specification](language.md) — Complete language reference / 完整语言参考
- [Examples](example.gbl) — Sample programs / 示例程序

---

## 📜 License / 许可证

**GNU General Public License v3.0**

See [LICENSE](LICENSE) for details.

详见 [LICENSE](LICENSE) 文件。
