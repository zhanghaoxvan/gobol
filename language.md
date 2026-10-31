# Gobol Language Documentation

**Author**: zhanghaoxvan
**File Extension**: `.gbl`
**License**: MIT
**Version**: 0.2.0

---

## 1. Overview / 概述

Gobol is a statically-typed, modular programming language with generics support. It combines Rust's safety features with Go's simplicity and Python's expressiveness. The compiler is written in Rust and targets both interpretation and native code generation.

Gobol 是一门静态类型、支持泛型的模块化编程语言。它融合了 Rust 的安全性、Go 的简洁和 Python 的表达力。编译器使用 Rust 编写，支持解释执行和原生代码生成。

---

## 2. Module System / 模块系统

### 2.1 Module Declaration / 模块声明

Every `.gbl` file is a module. **The module name is derived from the file name** (e.g., `math.gbl` → module `math`). No explicit `module` keyword is required.

每个 `.gbl` 文件都是一个模块。**模块名由文件名决定**（如 `math.gbl` → 模块 `math`）。不需要显式的 `module` 关键字。

### 2.2 Import Statement / 导入语句

```gobol
import io                    // Import module / 导入模块
import math as m             // Import with alias / 带别名导入
from package::math import sqrt // Import a member from a nested module / 从嵌套模块导入成员
```

### 2.3 Prelude / 预置导入

The compiler automatically injects `import basic::xxx` for the language-fundamental standard-library modules in the entry program. The `basic` namespace maps to a bundled `basic/` tree when present and falls back to the corresponding `std/` module in the default distribution. The implicit modules include `assert`, `io`, the primitive type implementations, and their operator/comparison traits. Optional facilities such as `math`, `fs`, `net`, `option`, `range`, `time`, and `vec` must be imported explicitly.

编译器会在入口程序中自动注入语言基础标准库模块的 `import basic::xxx;`。如果存在独立的 `basic/` 目录则优先使用它；默认发行版会回退到对应的 `std/` 模块。隐式导入包括 `assert`、`io`、基本类型实现及其运算/比较 trait；`math`、`fs`、`net`、`option`、`range`、`time`、`vec` 等可选功能必须显式导入。

### 2.3.1 Time / 时间

The hosted `time` module provides a monotonic millisecond clock for elapsed-time measurements, Unix epoch time in milliseconds, and millisecond sleep:

```gobol
import std::time;

func main() {
    var started = time::now_ms();
    time::sleep_ms(10);
    io::println(time::now_ms() - started);
    io::println(time::unix_time_ms());
}
```

`now_ms()` is monotonic and has an arbitrary origin; use differences rather than treating it as a date. `unix_time_ms()` returns milliseconds since 1970-01-01 UTC. `sleep_ms()` returns `0` on success and `-1` for a negative duration or a platform error. These functions require the hosted runtime and are unavailable with `--no-std`.

Hosted `time` 模块提供用于测量耗时的单调毫秒时钟、Unix 毫秒时间戳和毫秒级休眠。`now_ms()` 的起点不固定，应通过差值测量耗时；`unix_time_ms()` 返回自 1970-01-01 UTC 起的毫秒数；`sleep_ms()` 成功返回 `0`，负时长或平台错误返回 `-1`。这些功能依赖 hosted runtime，不适用于 `--no-std`。

Note: This injection only applies to the main program file. Standard library modules and dependencies must declare their own imports explicitly.

注意：此注入仅对主程序文件生效。标准库模块和依赖包必须显式声明自己的导入。


### 2.4 Export Statement / 导出语句

```gobol
export(add, multiply, Point) // Export multiple symbols / 导出多个符号
```

### 2.5 Member Access: `.` vs `::` / 成员访问

Gobol uses two distinct operators for accessing members.

Gobol 使用两种不同的运算符来访问成员。

**`::` — Namespace / module access / 命名空间与模块访问**

The double-colon operator accesses functions, constants, and constructors at the module or type level.

双冒号运算符用于访问模块或类型级别的函数、常量和构造函数。

| Context / 上下文 | Example / 示例 | Resolves to / 解析为 |
|:---|:---|:---|
| Module function / 模块函数 | `io::println("hi")` | Qualified function call / 限定函数调用 |
| Module alias / 模块别名 | `m::add(5, 3)` | Aliased function call / 别名函数调用 |
| Struct constructor / 结构体构造函数 | `Point::new(1, 2)` | Static / constructor call / 静态构造函数调用 |
| Fully-qualified path / 完全限定路径 | `std::io::println("hello")` | Full namespace path / 完整命名空间路径 |
| Trait path / Trait 路径 | `impl std::ops::Add for Point` | Trait implementation / Trait 实现 |

**`.` — Instance member access / 实例成员访问**

The dot operator accesses fields and methods on values (instances, not modules or types).

点运算符用于访问值（实例）上的字段和方法，不能用于模块或类型。

| Receiver / 接收者 | Example / 示例 | Resolves to / 解析为 |
|:---|:---|:---|
| Struct instance method / 结构体实例方法 | `p.distance()` | Instance method call / 实例方法调用 |
| Struct field / 结构体字段 | `p.x` | Field access / 字段访问 |
| Array method / 数组方法 | `arr.len()` | Built-in method / 内置方法 |
| Array method / 数组方法 | `arr.add(10)` | Built-in method / 内置方法 |
| Self reference / Self 引用 | `self.x` | Field access in impl / impl 内字段访问 |

**Rule of thumb / 经验法则**: Use `::` for compile-time namespacing (modules, types, constructors). Use `.` for run-time value access (fields, instance methods). This is similar to Rust's path vs. method syntax.

**经验法则**: `::` 用于编译时命名空间（模块、类型、构造函数）。`.` 用于运行时值访问（字段、实例方法）。这类似于 Rust 的路径与方法语法。

**Design rationale / 设计理念**: Keeping module access and instance access syntactically distinct (`::` vs `.`) makes code easier to read — you can immediately tell whether a call is a free function (`io::println`) or a method on a value (`p.distance()`).

**设计理念**: 将模块访问和实例访问在语法上分开（`::` vs `.`）使代码更易读——你可以立即判断一个调用是自由函数（`io::println`）还是值上的方法（`p.distance()`）。

### 2.6 External C Functions / 外部 C 函数

Gobol can call C runtime functions via `extern "C"` blocks. The `#[header("path.h")]` attribute points to the C header declaring these functions; the compiler validates that every function in the block is declared in that header.

Gobol 可以通过 `extern "C"` 块调用 C 运行时函数。`#[header("path.h")]` 属性指向声明这些函数的 C 头文件；编译器会验证块中的每个函数都在该头文件中声明。

```gobol
#[header("std/runtime.h")]
extern "C" {
    func gobol_print(s: int);
    func gobol_println(s: int);
    func gobol_read(): str;
    func gobol_alloc(size: int): int;
}
```

The `builtins` module (`std/builtins.gbl`) declares the core I/O and memory runtime functions this way. It is auto-loaded by the compiler so that `io::print`, `panic`, etc. resolve correctly.

`builtins` 模块（`std/builtins.gbl`）通过这种方式声明了核心 I/O 和内存运行时函数。编译器会自动加载它，以便 `io::print`、`panic` 等正确解析。

### 2.7 Internal Attribute / 内部属性

```gobol
#[internal]
func helper() { ... }
```

Symbols marked `#[internal]` are not exported and remain private to the module.

标记了 `#[internal]` 的符号不会被导出，仅对模块内部可见。

---

## 3. Variables and Constants / 变量与常量

### 3.1 Declaration / 声明

```gobol
var x: int = 10        // Mutable variable / 可变变量
val y: str = "hello"   // Immutable constant / 不可变常量

var a: int             // Default value 0 / 默认值 0
var b: str             // Default value "" / 默认值 ""
```

### 3.2 Type Inference / 类型推导

```gobol
var x = 10             // Inferred as int / 推导为 int
val name = "Gobol"     // Inferred as str / 推导为 str
```

### 3.3 Destructuring / 解构

```gobol
val (a, b) = (1, "two");
val Point { x, y } = p;
```

Tuple patterns bind each element in order. Use `var` for mutable bindings or `val` for immutable bindings. Tuple destructuring requires an initializer, and the number of names must match the number of elements.

元组解构会按顺序绑定每个元素。使用 `var` 创建可变绑定，使用 `val` 创建不可变绑定。元组解构必须有初始化表达式，名称数量也必须与元素数量一致。

---

## 4. Types / 类型

### 4.1 Basic Types / 基础类型

| Type / 类型 | Description / 描述 | C Mapping / C 映射 | Example / 示例 |
|:---|:---|:---|:---|
| `int` | Integer / 整数 | `int64_t` | `42`, `-7`, `0` |
| `float` | Floating-point / 浮点数 | `double` | `3.14`, `-0.5`, `2.0` |
| `str` | String / 字符串 | `const char*` | `"hello"`, `"世界"` |
| `bool` | Boolean / 布尔值 | `bool` | `true`, `false` |
| `char` | Unicode scalar value / Unicode 标量值 | `uint32_t` | `'A'`, `'世'`, `'\u{1F600}'` |

### 4.2 Tuples / 元组

Tuples are fixed-size, ordered values whose elements can have different types. Write tuple literals with parentheses and commas; a trailing comma may be used for a one-element tuple. Use `.N` to access the element at zero-based index `N`. Tuple types use the corresponding parenthesized type list:

```gobol
var point_and_name: (int, str) = (42, "answer");
io::println(point_and_name.0);
io::println(point_and_name.1);

var (number, word) = point_and_name;
var one_element: (bool,) = (true,);
```

`var` permits assigning to tuple fields (for example, `point_and_name.0 = 7`); a tuple bound with `val` is immutable. Tuple indexes are checked against the tuple's statically known size. Unlike arrays, tuples may contain heterogeneous element types and have a fixed shape.

元组是固定大小、有序且可包含不同类型元素的值。使用圆括号和逗号书写元组字面量；单元素元组可以使用尾随逗号。使用从零开始的 `.N` 访问元素；元组类型使用相应的圆括号类型列表。`var` 允许修改元组字段（例如 `point_and_name.0 = 7`），`val` 绑定的元组不可变。元组索引会根据编译期已知的长度检查。与数组不同，元组可以包含异构元素并具有固定结构。

---

## 5. Functions / 函数

### 5.1 Function Definition / 函数定义

**Rules / 规则**:
- Statements end with `;` / 语句以 `;` 结尾
- Expressions do not end with `;` / 表达式不以 `;` 结尾
- The last expression in a function body is the return value / 函数体中最后一个表达式作为返回值

```gobol
// Implicit return / 隐式返回
func add(a: int, b: int): int {
    a + b
}

// Explicit return / 显式返回
func divide(a: int, b: int): int {
    if b == 0 {
        return 0;
    }
    return a / b;
}

// No return value / 无返回值
func greet(name: str) {
    io::print(@"Hello, {name}");
}

// Multiple return values / 多返回值
func divmod(a: int, b: int): (int, int) {
    (a / b, a % b)
}
```

### 5.2 Lambda Functions / Lambda 函数

A lambda is an anonymous function written as `lambda(parameters): ReturnType { body }`. Its
last expression is the return value, just like a named function. Lambdas can be invoked
immediately, passed to a higher-order function, and can capture variables from the
surrounding scope.

Lambda 使用 `lambda(参数): 返回类型 { 函数体 }` 表示匿名函数。与具名函数一样，
函数体的最后一个表达式是返回值。Lambda 可以立即调用、作为高阶函数的参数传递，
并且可以捕获外层作用域中的变量。

```gobol
// Immediate invocation / 立即调用
var incremented = lambda(x: int): int {
    x + 1
}(41);

// Capture variables from the enclosing scope / 捕获外层变量
var offset = 10;
var result = lambda(x: int): int {
    x + offset
}(32);

// Pass a lambda to a function parameter / 作为函数参数传递
func apply(f: func(int): int, value: int): int {
    f(value)
}

var doubled = apply(lambda(x: int): int { x * 2 }, 21);
```

The parameter type of a higher-order function uses the function type syntax
`func(T1, T2): ReturnType`. A named function can be passed by reference with `&name`.
Lambda type parameters may be written after `lambda`, for example
`lambda<T>(value: T): T { value }`.

高阶函数的参数类型使用函数类型语法 `func(T1, T2): ReturnType`。具名函数可以使用
`&name` 取得函数引用并传递。Lambda 的泛型参数可以写在 `lambda` 后面，例如
`lambda<T>(value: T): T { value }`。

### 5.3 Generic Functions / 泛型函数

```gobol
func identity<T>(x: T): T {
    x
}

func max<T: Comparable>(a: T, b: T): T {
    if a > b { a } else { b }
}
```

---

## 6. Structures / 结构体

### 6.1 Struct Definition / 结构体定义

```gobol
// Simple struct / 简单结构体
struct Point {
    x: int,
    y: int,
};

// Generic struct / 泛型结构体
struct Pair<T, U> {
    first: T,
    second: U,
};

// Tuple struct / 元组结构体
struct Color(int, int, int);

// Empty struct / 空结构体
struct Empty;
```

### 6.2 Constructors / 构造函数

Create a struct value directly by listing its fields. Named struct literals initialize the fields as written and do not call `new` or any `New` trait implementation:

```gobol
var p = Point {
    x: 3,
    y: 4,
};

struct Pair<T, U> {
    first: T,
    second: U,
}

var pair = Pair<int, str> {
    first: 1,
    second: "one",
};
```

`StructName { field: value, ... }` is direct field initialization. All field accesses in methods use an explicit receiver, such as `self.x`; `self` must be declared as a method parameter. A method without a `self` parameter is an associated function and cannot access an instance.

结构体值可以直接通过列出字段创建。命名结构体字面量只初始化所列字段，不会调用 `new` 或任何 `New` trait 实现。方法中必须显式使用 `self.x` 等接收者访问字段，并且 `self` 必须是方法参数。没有 `self` 参数的方法是关联函数，不能访问实例。

For an associated constructor, return a struct literal:

```gobol
impl Point {
    func new(x: int, y: int): Point {
        Point { x: x, y: y }
    }
}
```

### 6.3 Constructor Call / 构造函数调用

```gobol
// Three equivalent forms / 三种等价形式
var p1 = Point::new(1, 2);   // Explicit :: style / 显式 :: 风格
var p2 = new Point(1, 2);    // `new Type(args)` sugar / new Type(args) 语法糖
var p3 = Point(1, 2);        // Bare constructor sugar / 裸构造器语法糖
```

`new Type(args)` is syntactic sugar that desugars to `Type::new(args)`. It is the recommended style for value construction.

`new Type(args)` 是语法糖，等价于 `Type::new(args)`。这是推荐的构造值的方式。

The `New<T>` trait (defined in `std/mem.gbl`) documents the constructor contract. The `#[dynamic]` attribute on the trait's `new` method signals that each implementer may define its own functions; the compiler skips arity checking against the trait declaration. This enables `new Range(0, 10)`, `new Point(3, 4)`, `new Channel<int>()`, etc.

`New<T>` trait（定义于 `std/mem.gbl`）约定了构造器协议。trait 中 `new` 方法上的 `#[dynamic]` 属性表示每个实现者可以自由定义自己的函数；编译器会跳过对 trait 声明的参数数量检查。这使得 `new Range(0, 10)`、`new Point(3, 4)`、`new Channel<int>()` 等形式都能正常工作。

```gobol
// std/mem.gbl
trait New<T> {
    #[dynamic]
    func new(): T
}

// Associated constructor / 关联构造函数
impl Point {
    func new(x: int, y: int): Point {   // Any arity allowed / 允许任意参数
        Point { x: x, y: y }
    }
}
```

### 6.4 Methods / 方法

```gobol
impl Point {
    func distance(self): float {
        math::sqrt((self.x * self.x + self.y * self.y) as float)
    }
    
    func add(self, other: Point): Point {
        Point::new(self.x + other.x, self.y + other.y)
    }
}

var p = Point(1, 2);
var dist = p.distance();
```

---

## 7. Enumerations / 枚举

### 7.1 Enum Definition / 枚举定义

Enums are lowered to tagged structs at compile time. Each variant becomes a constructor function in the enum's namespace. Variants can carry an optional payload type.

枚举在编译时被降级为标记结构体。每个变体成为枚举命名空间中的构造函数。变体可以携带可选的负载类型。

```gobol
// Unit enum — no payload / 无负载枚举
enum Color {
    Red,
    Green,
    Blue,
}

var c = Color::Red();

// Enum with generic payload / 带泛型负载的枚举
enum Option<T> {
    Some(T),
    None,
}

var opt = Option::Some(42);
var none = Option::None;

// Enum with multiple generic params / 多泛型参数的枚举
enum Result<T, E> {
    Ok(T),
    Err(E),
}

var ok = Result::Ok(100);
var err = Result::Err("not found");
```

The compiler lowers each enum to a tagged struct with a `_tag` field (integer discriminant) and indexed payload fields (`_0`, `_1`, …). Variant constructors are generated as methods that set the tag and payload, then return `self`.

编译器将每个枚举降级为标记结构体，包含 `_tag` 字段（整数判别值）和索引负载字段（`_0`、`_1`……）。变体构造函数被生成为设置 tag 和负载并返回 `self` 的方法。

> **Note on named fields**: The planned syntax `Variant(name: Type)` with named fields is not yet supported; variants currently carry a single anonymous payload type: `Variant(Type)`.

> **关于命名字段**: 规划中的 `Variant(name: Type)` 命名字段语法尚未支持；变体目前仅携带单一匿名负载类型：`Variant(Type)`。

### 7.2 Result Type / Result 类型

The `Result<T, E>` type (`std/result.gbl`) provides error handling via a tagged struct pattern. It has a `_tag` field (0 = Ok, 1 = Err) and `_value`/`_error` payloads.

`Result<T, E>` 类型（`std/result.gbl`）通过标记结构体模式提供错误处理。它包含 `_tag` 字段（0 = Ok，1 = Err）以及 `_value`/`_error` 负载。

| Method / 方法 | Signature / 签名 | Description / 描述 |
|:---|:---|:---|
| `ok(value)` | `(T) → Result<T, E>` | Create an Ok variant / 创建 Ok 变体 |
| `err(error)` | `(E) → Result<T, E>` | Create an Err variant / 创建 Err 变体 |
| `take(self)` | `→ T` | Unwrap value, panic if Err / 解包值，若为 Err 则 panic |
| `value(self, default: T)` | `(T) → T` | Return value or default if Err / 返回值，出错返回默认值 |
| `or_else(self, fallback)` | `(func(E): T) → T` | Compute fallback from error / 从错误计算回退值 |
| `unwrap_or(self, default: T)` | `(T) → T` | Alias for value / value 的别名 |
| `is_ok(self)` | `→ bool` | True if Ok / 若为 Ok 返回 true |
| `is_err(self)` | `→ bool` | True if Err / 若为 Err 返回 true |
| `map<U>(self, f)` | `(func(T): U) → Result<U, E>` | Transform Ok value / 转换 Ok 值 |

### 7.3 Result propagation / Result 传播

The postfix `?` operator is only the `Result` propagation operator. Nullable types (`T?`) and the `null` literal are not part of Gobol; use `Option<T>` with `Some(value)` and `Option::None` instead.

后缀 `?` 运算符仅用于 `Result` 传播。Gobol 不支持可空类型（`T?`）和 `null` 字面量；请使用 `Option<T>`、`Some(value)` 和 `Option::None`。

```gobol
import result;

func read_config(): Result<int, str> {
    var raw = io::read();
    if raw.is_empty() {
        return Result<int, str>::err("empty input");
    }
    var val = parse_int(raw)?;  // If Err, return it; else unwrap
    Ok(val * 2)
}
```

At IR level, `expr?` desugars to:

在 IR 层面，`expr?` 会被解糖为：

```gobol
let __tmp = expr;
if __tmp._tag == 1 { return __tmp; }   // Err → early return
__tmp.take()                            // Ok → unwrap
```

---

## 8. Control Flow / 控制流

### 8.1 if-else / 条件语句

```gobol
// Statement form / 语句形式
if x > 10 {
    io::print("large");
} else {
    io::print("small");
}

// Expression form (returns value) / 表达式形式（返回值）
val result = if x > 10 {
    "large"
} else {
    "small"
};
```

### 8.2 match / 匹配表达式

```gobol
val grade = match score {
    100 => "A+",
    90..99 => "A",
    80..89 => "B",
    70..79 => "C",
    _ => "F"
};
```

### 8.3 for Loop / for 循环

```gobol
// Range / 范围
for i in 0..10 {
    io::print(i);
}

// Range with explicit step / 带步长的范围
for i in 0..10..2 {
    io::print(i);              // 0, 2, 4, 6, 8
}

// Reverse range / 反向范围
for i in 5..1 {
    io::print(i);              // 5, 4, 3, 2
}

// With index and value / 带索引和值
for i, v in items {
    io::print("{i}: {v}");
}

// String iteration / 字符串遍历
for ch in "hello" {
    io::print(ch);
}

// Array iteration / 数组遍历
for item in [1, 2, 3] {
    io::print(item);
}
```

### 8.4 while Loop / while 循环

```gobol
var i = 0;
while i < 10 {
    io::print(i);
    i += 1;
}
```

### 8.5 break / continue / 中断与继续

```gobol
for i in 0..100 {
    if i % 2 == 0 { continue; }
    if i > 50 { break; }
    io::print(i);
}
```

---

## 9. Special Methods (Protocols) / 特殊方法（协议）

Special methods implement built-in behaviors. They are defined with `func` keyword, except `convert`.

特殊方法实现内置行为。它们使用 `func` 关键字定义，除了 `convert`。

### 9.1 Memory Protocol / 内存协议: `New` and `Drop`

The `New<T>` and `Drop` traits (`std/mem.gbl`) form the memory management protocol. Every owned value type should implement both so the compiler can reason about construction and cleanup uniformly.

`New<T>` 和 `Drop` trait（`std/mem.gbl`）构成了内存管理协议。每个拥有所有权的值类型都应实现这两个 trait，以便编译器统一处理构造和清理。

```gobol
// std/mem.gbl
trait New<T> {
    #[dynamic]
    func new(): T
}

trait Drop {
    func drop(self)
}
```

**`New<T>`** — Construction trait. The `#[dynamic]` attribute allows each implementer to define its own functions; the compiler skips arity checking against the trait. Three constructor call forms are supported:

**`New<T>`** — 构造器 trait。`#[dynamic]` 属性允许每个实现者自由定义参数列表；编译器跳过对 trait 声明的参数数量检查。支持三种构造器调用形式：

```gobol
var p1 = Point::new(3, 4);     // explicit :: call / 显式 :: 调用
var p2 = new Point(3, 4);      // new Type(args) sugar / new Type(args) 语法糖
var p3 = Point(3, 4);          // bare constructor sugar / 裸构造器语法糖
```

**`Drop`** — Cleanup trait. Values that own heap memory or other resources (file handles, sockets, `Ref<T>` references) should implement `Drop`. The compiler inserts `drop(self)` calls when values go out of scope.

**`Drop`** — 清理 trait。拥有堆内存或其他资源（文件句柄、socket、`Ref<T>` 引用）的值应实现 `Drop`。编译器会在值离开作用域时插入 `drop(self)` 调用。

```gobol
impl Drop for File {
    func drop(self) {
        if _handle != 0 {
            fs::close(_handle);
        }
    }
}
```

### 9.2 Type Conversion / 类型转换: `convert Type`

```gobol
convert TargetType(self): TargetType {
    // Return self converted to TargetType / 返回转换后的值
}
```

Called when using `value as TargetType`.

当使用 `value as TargetType` 时调用。

Character literals have type `char`. Cast `char` to `int` to get its Unicode
code point; casting an `int` to `char` keeps the low 32 bits, and string
conversion replaces invalid Unicode scalar values with U+FFFD.
String indexing returns a `char`, and `str.len()` counts Unicode scalar values
rather than UTF-8 bytes.

字符字面量的类型是 `char`。将 `char` 转为 `int` 可取得 Unicode 码点；
将 `int` 转为 `char` 会保留低 32 位，转成字符串时无效的 Unicode
标量值会替换为 U+FFFD。字符串索引返回 `char`，`str.len()` 统计 Unicode
标量值的数量，而不是 UTF-8 字节数。

### 9.3 Iterator Protocol / 迭代器协议

```gobol
impl MyCollection {
    func iter(self): MyIterator {
        MyIterator.new(self._data)
    }
}

impl MyIterator {
    func next(self): (T, bool) {
        if _index >= _len {
            return Option::None;
        }
        var value = _data[_index];
        _index += 1;
        (value, true)
    }
}

// Used in for loops / 用于 for 循环
for item in my_collection { ... }
```

### 9.4 Collection Protocols / 集合协议

```gobol
// Length / 长度
func len(self): int { ... }

// Member check / 成员检查
func contains(self, value: T): bool { ... }

// Check if empty / 判断空
func is_empty(self): bool { ... }
```

### 9.5 String Conversion / 字符串转换

```gobol
func to_str(self): str {
    // Custom string representation / 自定义字符串表示
}
```

---

## 10. Operator Overloading / 运算符重载

### 10.1 Integer Bitwise Operators / 整数位运算

Integer expressions support bitwise AND (`&`), OR (`|`), XOR (`^`), NOT
(`~`), left shift (`<<`), and arithmetic right shift (`>>`). Compound
assignments `&=`, `|=`, `^=`, `<<=`, and `>>=` are also available. These
operators require `int` operands.

Logical `&&` and `||` bind less tightly than bitwise operators. Precedence,
from lower to higher among the listed operators, is `|`, `^`, `&`, equality,
comparison, shifts, addition, and multiplication. Unary `~` binds more tightly
than binary operators.

```gobol
func main() {
    var flags = 1 | 2;
    flags |= 4
    io::println(flags & 5);   // 5
    io::println(1 << 3);      // 8
    io::println(~0);          // -1
}
```

整数表达式支持按位与（`&`）、或（`|`）、异或（`^`）、取反（`~`）、左移
（`<<`）和算术右移（`>>`）。同时支持 `&=`、`|=`、`^=`、`<<=` 和 `>>=`
复合赋值。这些运算符的操作数必须是 `int`。

优先级从低到高依次为：`|`、`^`、`&`、相等比较、关系比较、移位、加减、乘除。
逻辑运算符 `&&` 和 `||` 的优先级低于位运算；一元 `~` 的优先级高于二元运算符。

```gobol
// Operators are provided by traits rather than operator declarations.
impl std::ops::Add for Point {
    func add(self, other: Point): Point {
        Point::new(self.x + other.x, self.y + other.y)
    }
}
```

---

## 11. Standard Library / 标准库

### 11.1 io Module / io 模块

```gobol
import io

io::print("Hello");       // Print without newline / 不换行打印
io::println("World");     // Print with newline / 换行打印
io::eprint("error!");     // Print to stderr / 打印到标准错误
io::eprintln("error!");   // Print to stderr with newline / 带换行打印到标准错误
var input = io::read();   // Read a line / 读取一行
```

### 11.2 debug Module / debug 模块

Debug I/O writes to stderr only in debug mode (`--debug`). In release mode, all debug calls are removed at compile time (zero overhead).

Debug I/O 仅在 debug 模式（`--debug`）下写入 stderr。在 release 模式下，所有 debug 调用在编译时被移除（零开销）。

```gobol
import debug;
debug::println("This only appears in debug builds");
```

### 11.3 Result Type / Result 类型

`std/result.gbl` — see §7.2 and §7.3 for full documentation.

`std/result.gbl` — 完整文档见 §7.2 和 §7.3。

```gobol
import result;

var ok = Result<int, str>::ok(42);
var val = ok.take();              // 42
var def = ok.value(0);            // 42
```

### 11.4 Thread & Channel / 线程与通道

`std/thread.gbl` provides concurrency primitives backed by the cross-platform C runtime (`std/runtime/`).

`std/thread.gbl` 提供由跨平台 C 运行时（`std/runtime/`）支持的并发原语。

**Thread / 线程:**

```gobol
import thread;

func worker(): int {
    io::println("Hello from thread");
    0
}

func add_one(n: int): int {
    n + 1
}

var t = Thread::spawn(&worker);
var ret = t.join();       // Wait for thread, get exit code / 等待线程，获取退出码

var t2 = Thread::spawn(&add_one, 41);
var ret2 = t2.join();     // 42 — spawn forwards `arg` to the worker / spawn 会把 arg 转发给工作函数
```

**Channel / 通道:**

```gobol
import thread;

var ch = new Channel<int>();
ch.send(42);
var result = ch.recv();   // Result<int, int> — Ok(value) or Err(code)
var val = result.take();  // 42
ch.drop();
```

### 11.5 New & Drop Traits / New 与 Drop Trait

`std/mem.gbl` — see §9.1 for full documentation.

`std/mem.gbl` — 完整文档见 §9.1。

### 11.6 builtins Module / builtins 模块

`std/builtins.gbl` declares the C runtime functions (I/O, memory) via `extern "C"`. It is auto-loaded by the compiler so that `io::print` and `panic` resolve correctly. User code rarely needs to import it directly.

`std/builtins.gbl` 通过 `extern "C"` 声明 C 运行时函数（I/O、内存）。编译器自动加载它，因此 `io::print` 和 `panic` 能正确解析。用户代码很少需要直接导入它。

### 11.7 Range Type / Range 类型

`Range` is a half-open integer range: the start is included and the end is
excluded. Positive and negative steps are supported; an empty or zero-step
range has length zero.

`Range` 是左闭右开的整数范围：包含起点，不包含终点。支持正负步长；空范围和零步长范围的长度为零。

```gobol
var r1 = new Range(0, 10);      // 0..9, step 1
var r2 = new Range(0, 10, 2);   // 0,2,4,6,8
var r3 = 0..10;                 // Syntactic sugar for range::new(0, 10) / 语法糖
var r4 = 0..10..2;              // Three-param sugar: range::new(0, 10, 2) / 三参数语法糖

// Methods / 方法
r1.start();                     // 0
r1.end();                       // 10
r1.len();                       // 10
r1.contains(5);                 // true
var descending = new Range(5, 0, -2); // 5,3,1

// Convert to array / 转换为数组
var arr: int[] = r1;
```

### 11.8 math Module / math 模块

```gobol
import math;

val pi = math::PI;
val abs_x = math::abs(-5);
val root = math::sqrt(16.0);
val s = math::sin(1.0);
```

### 11.9 Vec<T> Type / Vec<T> 类型

`Vec<T>` is a growable, owned vector backed by a contiguous `T[]`. It starts
empty, doubles capacity when full, and uses `Option<T>` for iterator results;
there is no nullable backing array and no `null` sentinel.

`Vec<T>` 是由连续 `T[]` 支持的可增长拥有型向量。初始为空，满时容量翻倍；迭代器结果使用 `Option<T>`，不再使用可空数组或 `null` 哨兵。

```gobol
var v = Vec<int>::new();
v.push(10);
v.push(20);
var x = v[0];           // 10

// From array / 从数组创建
var v2 = Vec<int>.from_array([1, 2, 3]);

// Iteration / 迭代
for i, v in my_vec {
    io::print("{i}: {v}");
}

var next: Option<int> = v.iter().next();
if next.is_some() {
    io::print(next.take());
}
```

### 11.10 Core Library and `std` Configuration / Core 库与 `std` 配置

Gobol's `core` library contains allocation-free language fundamentals,
including `Option<T>`, `Result<T, E>`, primitive types, comparison and
arithmetic traits, and the `Iterator` trait. It does not declare hosted I/O or
depend on the C runtime.

Hosted standard-library support is enabled by default. Use the file-level
attribute `#![std(false)]` to select the `core` prelude instead of the hosted
`basic` prelude. `#[std(false)]` on a function prevents that function from
calling hosted std functions; `#[std(true)]` enables hosted support for that
function even when the file default is disabled. The runtime is linked when
the file or any function requires hosted std. Bare-metal targets ending in
`-none` cannot use hosted std. The equivalent `--no-std` compiler flag and
`no_std = true` manifest setting remain supported as compatibility forms.
The `[build].std` manifest setting defaults to `true`. Settings are resolved
in this order: manifest, file attribute, then function attribute.

The core modules are also available explicitly with imports such as
`import core::option;`. Existing imports such as `import option;` remain
available in hosted builds and forward to the core definitions, adding
hosted-only convenience methods such as `take()`.

`core` does not provide I/O, a heap allocator, or a process entry point.
Bare-metal programs must provide their own entry point and linker setup, and
must provide any runtime or allocation hooks required by the operations they
use.

Gobol 的 `core` 库提供无需分配的语言基础，包括 `Option<T>`、`Result<T, E>`、
基本类型、比较与算术 trait，以及 `Iterator` trait。它不声明宿主 I/O，也不依赖
C 运行时。

宿主标准库默认启用。文件级属性 `#![std(false)]` 会选择 `core` prelude，而不是
宿主环境的 `basic` prelude。函数上的 `#[std(false)]` 禁止该函数调用宿主 std
函数；即使文件默认禁用了 std，函数上的 `#[std(true)]` 也能为该函数启用宿主
支持。文件或任一函数需要宿主 std 时会链接运行时。以 `-none` 结尾的裸机目标
不能使用宿主 std。编译器参数 `--no-std` 和清单配置 `no_std = true` 仍作为兼容
写法支持。`[build].std` 清单项默认是 `true`，覆盖顺序为：清单、文件属性、函数
属性。

也可以显式导入 core 模块，例如 `import core::option;`。宿主构建中原有的
`import option;` 等导入仍然可用，并转发到 core 定义，同时提供 `take()` 等仅供
宿主环境使用的便利方法。

`core` 不提供 I/O、堆分配器或进程入口。裸机程序需要自行提供入口和链接配置，
并为所用操作提供所需的运行时或分配钩子。

---

## 12. Built-in Functions / 内置函数

| Function / 函数 | Signature / 签名 | Description / 描述 |
|:---|:---|:---|
| `panic` | `panic(msg: str)` | Print message and abort / 打印消息并终止 |
| `exit` | `exit(code: int)` | Exit with status code / 以状态码退出 |
| `new Type` | `new Type(args)` | Constructor sugar → `Type::new(args)` / 构造器语法糖 |

`panic` and `exit` are compiler-level builtins available without any import.

`panic` 和 `exit` 是编译器级别的内置函数，无需导入即可使用。

---

## 13. Compiler Attributes / 编译器属性

```gobol
#[dynamic_args]
#[expand]
#[header("std/runtime.h")]
#![library_features(hidden = true)]
#[internal]
#[debug]
#[intrinsic("runtime_name")]
#[inline]
#[deprecated("use new_func instead")]
```

| Attribute / 属性 | Applies to / 适用于 | Description / 描述 |
|:---|:---|:---|
| `#[dynamic]` | Trait method / Trait 方法 | Allow impls to define arbitrary functions / 允许实现者自由定义函数 |
| `#[expand]` | Function / 函数 | AST-level macro; inline body at every call site (see §14) / AST 级宏；在每个调用点内联函数体（见 §14） |
| `#[header("path")]` | `extern "C"` block / 外部块 | C header for function validation / 用于函数验证的 C 头文件 |
| `#![library_features(hidden = true)]` | File / 文件 | Hide module prefix in qualified names / 隐藏模块前缀 |
| `#[internal]` | Function, Struct, Trait | Not exported; module-private / 不导出，模块私有 |
| `#[debug]` | Function / 函数 | Remove in release mode (zero overhead) / Release 模式下移除（零开销） |
| `#[intrinsic("name")]` | Function / 函数 | Map to named C runtime function / 映射到指定 C 运行时函数 |
| `#[inline]` | Function / 函数 | Inline hint to codegen / 代码生成内联提示 |
| `#![std(false)]`, `#[std(false)]` | File, Function / 文件、函数 | Disable hosted std for the file or function / 为文件或函数禁用宿主 std |
| `#![gc(false)]`, `#[gc(false)]` | File, Function, Struct, Enum / 文件、函数、结构体、枚举 | Use non-GC allocation / 使用非 GC 分配 |
| `#[no_gc]` | Struct, Enum | Legacy alias for `#[gc(false)]` / `#[gc(false)]` 的旧别名 |
| `#[deprecated("msg")]` | Function, Struct, Trait | Mark as deprecated / 标记为弃用 |
| `#[no_export]` | Function, Struct, Trait | Exclude from module exports / 从模块导出中排除 |

### Memory Management / 内存管理

Gobol uses a mark-sweep garbage collector for heap-allocated values. By default, all struct and enum allocations use `gobol_gc_alloc`, which registers the object with the GC. The GC sweeps unmarked objects, freeing unreachable memory.

Gobol 使用标记-清除垃圾回收器管理堆分配的值。默认情况下，所有结构体和枚举分配使用 `gobol_gc_alloc`，将对象注册到 GC 中。GC 会清除未标记的对象，释放不可达内存。

**`gc(false)` — Manual memory / 手动内存管理:**

GC is enabled by default. Set `gc = false` in `[build]`, use `#![gc(false)]` for a file, or use `#[gc(false)]` for a function/type to route allocations through `gobol_alloc` (manual `calloc`). A node-level setting overrides the file setting, which overrides `grape.toml`. `#[no_gc]` remains a compatibility alias. Types that manage OS resources or raw heap memory should use non-GC allocation and implement the `Drop` trait for cleanup.

GC 默认启用。可以在 `[build]` 中设置 `gc = false`，用文件级 `#![gc(false)]`，或用函数/类型级 `#[gc(false)]` 将分配转到 `gobol_alloc`（手动 `calloc`）。节点级设置覆盖文件级设置，文件级设置覆盖 `grape.toml`。`#[no_gc]` 仍是兼容别名。管理 OS 资源或原始堆内存的类型应使用非 GC 分配，并实现 `Drop` trait 进行清理。

```gobol
// GC-managed by default / 默认由 GC 管理
struct Point { x: int, y: int }
var p = new Point(3, 4);  // allocated via gobol_gc_alloc

// Manual memory — must implement Drop / 手动管理——必须实现 Drop
#[gc(false)]
struct File {
    _handle: int,
}

impl File {
    func drop(self) {
        fs::close(_handle);
    }
}
```

All standard library types are GC-managed by default. `#[gc(false)]` (or its legacy alias `#[no_gc]`) is intended for advanced use cases where a type implements its own allocator or wraps externally-managed memory.

所有标准库类型默认由 GC 管理。`#[gc(false)]`（或旧别名 `#[no_gc]`）仅用于高级场景，如类型实现了自定义分配器或包装了外部管理的内存。

---

## 14. Metaprogramming / 元编程

### 14.1 `#[expand]` Macros / `#[expand]` 宏

Gobol provides AST-level macros via the `#[expand]` attribute. A macro is declared like a normal function, but the compiler **inlines its body at every call site** instead of emitting a real function call. This enables zero-cost abstractions: no call overhead, no stack frame, and the body becomes visible to constant folding and other optimizations.

Gobol 通过 `#[expand]` 属性提供 AST 级别的宏。宏像普通函数一样声明，但编译器会在每个调用点**内联其函数体**，而不是生成真正的函数调用。这实现了零开销抽象：没有调用开销，没有栈帧，并且函数体可被常量折叠等优化识别。

**Definition / 定义:**

```gobol
#[expand]
func add(a: int, b: int): int {
    return a + b
}

// Implicit-return form is also allowed / 隐式返回形式同样允许
#[expand]
func square(n: int): int {
    n * n
}

// String macros work too / 字符串宏同样适用
#[expand]
func greet(name: str): str {
    "hello, " + name
}
```

**Invocation / 调用:**

A macro is called exactly like a regular function — there is no special call syntax.

宏的调用与普通函数完全一致——没有特殊的调用语法。

```gobol
var z   = add(5, 10);             // 15
var s   = square(7);               // 49
var msg = greet("world");          // "hello, world"
```

### 14.2 Two Expansion Modes / 两种展开模式

The compiler chooses the mode based on whether every argument at the call site is a compile-time literal.

编译器根据调用点每个参数是否为编译期字面量来选择展开模式。

| Mode / 模式 | Condition / 条件 | Behavior / 行为 |
|:---|:---|:---|
| **Constant folding / 常量折叠** | All arguments are literals / 所有参数均为字面量 | Body is evaluated at compile time; the call is replaced with a single constant / 函数体在编译期求值；调用被替换为单个常量 |
| **Argument substitution / 参数替换** | Any argument is a variable or expression / 任一参数为变量或表达式 | Body is inlined at the call site with each parameter replaced by its (parenthesized) argument; evaluated at runtime / 函数体在调用点内联，每个参数被（带括号的）实参替换；运行期求值 |

```gobol
#[expand]
func add(a: int, b: int): int { return a + b }

var x = 5;
var y = 10;

var lit = add(3, 4);             // literal args → compile-time → 7
var z   = add(x, y);             // expr args → inlined to (x) + (y) → 15
var w   = add(x + 1, y * 2);     // inlined to (x + 1) + (y * 2) → 26
```

Arguments are parenthesized during substitution so that operator precedence inside the body is never altered by the surrounding expression.

替换时实参会加上括号，因此函数体内部的运算符优先级不会受外层表达式影响。

### 14.3 Recursive Expansion / 递归展开

A macro body may call other `#[expand]` macros. The compiler recursively inlines them until no macro calls remain.

宏的函数体可以调用其他 `#[expand]` 宏。编译器会递归内联，直到不再含有宏调用为止。

```gobol
#[expand]
func add(a: int, b: int): int { return a + b }

#[expand]
func dbl(a: int): int { return add(a, a) }

var d = dbl(5);                  // dbl(5) → add(5, 5) → (5) + (5) → 10
var q = dbl(x + 1);              // → add(x + 1, x + 1) → (x + 1) + (x + 1)
```

### 14.4 Macros in Format Strings / 格式串中的宏

Macro calls are recognized inside `@"{...}"` interpolations, including calls whose arguments use binary operators. The format-string visitor shares the macro table with the surrounding builder, so the same inlining rules apply.

宏调用在 `@"{...}"` 插值中也能识别，包括参数含二元运算符的情况。格式串访问器与外层构建器共享宏表，因此适用相同的内联规则。

```gobol
io::println(@"z = {add(x, y)}");           // 15
io::println(@"w = {add(x + 1, y * 2)}");    // 26
io::println(@"lit = {add(3, 4)}");          // 7 (constant-folded)
```

### 14.5 Compile-Time Intrinsics / 编译期内置函数

Inside an `#[expand]` body, the `file()` and `line()` intrinsics return the current source path and line number as compile-time values. They are only valid within `#[expand]` context.

在 `#[expand]` 函数体内，`file()` 和 `line()` 内置函数以编译期值返回当前源文件路径和行号。它们仅在 `#[expand]` 上下文中有效。

```gobol
#[expand]
func source_file(): str {
    file()
}

var f = source_file();           // → the current .gbl file path
```

### 14.6 Rules & Caveats / 规则与注意事项

- `#[expand]` applies to `func` declarations only / 仅适用于 `func` 声明。
- A macro must have a fully-typed signature; parameter types are required / 宏必须具有完整类型签名；参数类型不可省略。
- Macros are **not first-class values**: they cannot be passed as arguments, stored in variables, or returned / 宏不是一等值：不能作为参数传递、存入变量或被返回。
- Macro recursion must terminate at a non-macro call; non-terminating macro recursion is a compile error / 宏递归必须终止于非宏调用；非终止的宏递归会触发编译错误。
- Because inlining duplicates the body at every call site, prefer `#[expand]` for small, frequently-called hot functions / 由于内联会在每个调用点复制函数体，`#[expand]` 宜用于短小且频繁调用的热点函数。

---

## 15. Complete Example / 完整示例

```gobol
import io;
import math;
import result;

struct Point {
    x: int,
    y: int,
};

impl Point {
    func new(x: int, y: int): Point {
        self.x = x;
        self.y = y;
        self
    }

    func distance(self): float {
        math::sqrt((self.x * self.x + self.y * self.y) as float)
    }

    convert str(self): str {
        @"({self.x}, {self.y})"
    }
}

// Function returning Result demonstrates the ? operator
func safe_divide(a: int, b: int): Result<int, str> {
    if b == 0 {
        return Result<int, str>::err("division by zero");
    }
    a / b
}

func main(): int {
    var name = "Gobol";
    io::print(@"Hello from {name}\n");

    // Array literal and iteration
    var arr: [int] = [1, 2, 3, 4, 5];
    for i, v in arr {
        io::print(@"arr[{i}] = {v}\n");
    }

    // Constructor via `new Type(args)` sugar
    var p = new Point(3, 4);
    io::print(@"Point: {p as str}\n");
    io::print(@"Distance: {p.distance()}\n");

    // Error handling with ?
    var ok = safe_divide(10, 2);
    io::print(@"10 / 2 = {ok.take()}\n");
    var err = safe_divide(10, 0);
    io::print(@"10 / 0 is_err = {err.is_err()}\n");

    return 0;
}
```

---

## 16. Runtime / 运行时

Gobol's C runtime (`std/runtime/`) is a cross-platform library supporting both POSIX (Linux, macOS) and Windows via `platform.h` abstractions. It is organized into modular components:

Gobol 的 C 运行时（`std/runtime/`）是一个跨平台库，通过 `platform.h` 抽象层同时支持 POSIX（Linux、macOS）和 Windows。它被组织成模块化组件：

```
std/runtime/
├── platform.h        Cross-platform abstractions / 跨平台抽象层
│                     (threads, sockets, mutex, condvar, getline)
├── types.h           Shared types (GobolArray) / 共享类型
├── io.h / io.c       Standard I/O / 标准输入输出
├── str.h / str.c     String conversion & manipulation / 字符串转换与操作
├── mem.h / mem.c     Memory allocation & raw access / 内存分配与原始访问
├── array.h / array.c Growable array (Vec<T> backing) / 可增长数组
├── math.h / math.c   Math intrinsics (sin, cos, pow) / 数学内置函数
├── fs.h / fs.c       File system operations / 文件系统操作
├── net.h / net.c     TCP networking / TCP 网络
├── thread.h / thread.c  Thread spawn/join / 线程创建与等待
├── channel.h / channel.c Message queue / 消息队列
├── gc.h / gc.c       Mark-sweep garbage collector / 标记-清除垃圾回收器
└── entry.h / entry.c main() → gbl_main() entry point / 入口点
```

The master files `std/runtime.c` and `std/runtime.h` include all sub-modules for single-translation-unit compilation. The compiler links `runtime.c` when producing standalone executables.

主文件 `std/runtime.c` 和 `std/runtime.h` 包含所有子模块，用于单编译单元编译。编译器在生成独立可执行文件时链接 `runtime.c`。

### Platform Abstraction / 平台抽象

| Feature / 功能 | POSIX | Windows |
|:---|:---|:---|
| Threads / 线程 | `pthread_create` / `pthread_join` | `CreateThread` / `WaitForSingleObject` |
| Mutex / 互斥锁 | `pthread_mutex_*` | `CRITICAL_SECTION` |
| Condition variable / 条件变量 | `pthread_cond_*` | `CONDITION_VARIABLE` |
| Sockets / 套接字 | BSD sockets | Winsock2 (`WSAStartup`, `closesocket`) |
| File exists / 文件存在 | `access(path, F_OK)` | `_access(path, 0)` |
| `getline(3)` | Standard / 标准库 | Custom implementation / 自定义实现 |

---

**Gobol — A safe, modern, and expressive programming language** 🚀

**Gobol — 安全、现代、富有表达力的编程语言** 🚀
