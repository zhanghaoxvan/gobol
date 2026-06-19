哈哈 **你说得对！** `~/.local/bin/gobol` 是默认位置，但用户完全可以用 `--install-dir` 自定义。

---


## ✅ 修正后的 README

```markdown
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
- Build the compiler and package manager
- Copy binaries to the installation directory[default is `~/.local/bin`(Unix) or `%USERPROFILE%\.local\bin`(Windows)]
- Add them to your PATH
- Copy the standard library

就是这样！安装程序会：
- 编译编译器和包管理器
- 复制二进制文件到安装目录[默认是 `~/.local/bin`(Unix) 上的 `%USERPROFILE%\.local\bin`(Windows) 上的 PATH]
- 复制标准库

### Custom Install Directory / 自定义安装目录

```bash
python3 install.py --install-dir /my/custom/path
```

### Options / 选项

| Option / 选项 | Description / 描述 |
|---------------|-------------------|
| `--install-dir DIR` | Installation directory / 安装目录 |
| `--no-build` | Skip building (use existing binaries) / 跳过编译（使用现有二进制文件） |
| `--uninstall` | Uninstall / 卸载 |
| `--help` | Show help / 显示帮助 |

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

# Clean cached packages / 清理缓存包
grape clean

# Show help / 显示帮助
grape help
```

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
import io

func main() {
    io.println("Hello, Gobol!")
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
```

