好！我们把 `language.md` 改成**中英双语对照**格式，和你原来的风格保持一致。

---

# Gobol Language Documentation

**Author**: zhanghaoxvan  
**File Extension**: `.gbl`  
**License**: GPL3  
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
```

### 2.3 Export Statement / 导出语句

```gobol
export(add, multiply, Point) // Export multiple symbols / 导出多个符号
```

### 2.4 Internal Attribute / 内部属性

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
val (a, b) = (1, 2);
val Point { x, y } = p;
```

---

## 4. Types / 类型

### 4.1 Basic Types / 基础类型

| Type / 类型 | Description / 描述 | C Mapping / C 映射 | Example / 示例 |
|:---|:---|:---|:---|
| `int` | Integer / 整数 | `int64_t` | `42`, `-7`, `0` |
| `float` | Floating-point / 浮点数 | `double` | `3.14`, `-0.5`, `2.0` |
| `str` | String / 字符串 | `const char*` | `"hello"`, `"世界"` |
| `bool` | Boolean / 布尔值 | `bool` | `true`, `false` |

### 4.2 Nullable Types / 可空类型

```gobol
var opt: int? = null;
opt = 42;
```

Nullable types are marked with `?` suffix. A nullable variable can hold either a value of its base type or `null`.

可空类型用 `?` 后缀标记。可空变量可以持有其基础类型的值或 `null`。

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
    a / b
}

// No return value / 无返回值
func greet(name: str) {
    io.print("Hello, " + name);
}

// Multiple return values / 多返回值
func divmod(a: int, b: int): (int, int) {
    (a / b, a % b)
}
```

### 5.2 Generic Functions / 泛型函数

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

```gobol
impl Point {
    func new(x: int, y: int): Point {
        self.x = x;
        self.y = y;
        self           // Implicit return / 隐式返回
    }
    
    func from_origin(): Point {
        Point.new(0, 0)
    }
}
```

### 6.3 Constructor Call / 构造函数调用

```gobol
// Both syntaxes are supported / 两种语法都支持
var p1 = Point.new(1, 2);   // Explicit style / 显式风格
var p2 = Point(1, 2);       // Syntactic sugar (calls new) / 语法糖
```

### 6.4 Methods / 方法

```gobol
impl Point {
    func distance(self): float {
        math.sqrt((self.x * self.x + self.y * self.y) as float)
    }
    
    func add(self, other: Point): Point {
        Point.new(self.x + other.x, self.y + other.y)
    }
}

var p = Point(1, 2);
var dist = p.distance();
```

---

## 7. Enumerations / 枚举

### 7.1 Enum Definition / 枚举定义

```gobol
enum Option<T> {
    Some(T),
    None,
}

enum Result<T, E> {
    Ok(T),
    Err(E),
}

enum Shape {
    Circle(center: Point, radius: float),
    Rectangle(tl: Point, br: Point),
    Line(p1: Point, p2: Point),
}
```

---

## 8. Control Flow / 控制流

### 8.1 if-else / 条件语句

```gobol
// Statement form / 语句形式
if x > 10 {
    io.print("large");
} else {
    io.print("small");
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
    io.print(i);
}

// With index and value / 带索引和值
for i, v in items {
    io.print("{i}: {v}");
}

// String iteration / 字符串遍历
for ch in "hello" {
    io.print(ch);
}

// Array iteration / 数组遍历
for item in [1, 2, 3] {
    io.print(item);
}
```

### 8.4 while Loop / while 循环

```gobol
var i = 0;
while i < 10 {
    io.print(i);
    i += 1;
}
```

### 8.5 break / continue / 中断与继续

```gobol
for i in 0..100 {
    if i % 2 == 0 { continue; }
    if i > 50 { break; }
    io.print(i);
}
```

---

## 9. Special Methods (Protocols) / 特殊方法（协议）

Special methods implement built-in behaviors. They are defined with `func` keyword, except `convert`.

特殊方法实现内置行为。它们使用 `func` 关键字定义，除了 `convert`。

### 9.1 Constructor / 构造函数: `func new`

```gobol
func new(args): Type {
    self
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
            return (null, false);
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

```gobol
// Unary / 一元
operator - (self: Point): Point {
    Point.new(-self.x, -self.y)
}

// Binary / 二元
operator + (left: Point, right: Point): Point {
    Point.new(left.x + right.x, left.y + right.y)
}

// Index / 索引
operator [] (self: vec<T>, index: int): T {
    self.get(index)
}

operator []= (self: vec<T>, index: int, value: T) {
    self.set(index, value)
}

// Comparison / 比较
operator == (left: Point, right: Point): bool {
    left.x == right.x && left.y == right.y
}
```

---

## 11. Standard Library / 标准库

### 11.1 io Module / io 模块

```gobol
import io

io.print("Hello");     // Print without newline / 不换行打印
io.println("World");   // Print with newline / 换行打印
var input = io.read(); // Read a line / 读取一行
```

### 11.2 range Type / range 类型

```gobol
var r1 = range(0, 10);          // 0..9, step 1
var r2 = range(0, 10, 2);       // 0,2,4,6,8
var r3 = 0..10;                 // Syntactic sugar / 语法糖

// Methods / 方法
r1.start();                     // 0
r1.end();                       // 10
r1.len();                       // 10
r1.contains(5);                 // true

// Convert to array / 转换为数组
var arr: int[] = r1;
```

### 11.3 vec<T> Type / vec<T> 类型

```gobol
var v = vec<int>.new();
v.push(10);
v.push(20);
var x = v[0];           // 10

// From array / 从数组创建
var v2 = vec<int>.from_array([1, 2, 3]);

// Iteration / 迭代
for i, v in my_vec {
    io.print("{i}: {v}");
}
```

---

## 12. Built-in Functions / 内置函数

| Function / 函数 | Description / 描述 |
|:---|:---|
| `panic(msg: str)` | Trigger a runtime error / 触发运行时错误 |
| `range(start, end, step?)` | Create a range object / 创建 range 对象 |

---

## 13. Compiler Attributes / 编译器属性

```gobol
#[library_features(hidden = true)]
#[internal]
#[inline]
#[deprecated("use new_func instead")]
```

| Attribute / 属性 | Description / 描述 |
|:---|:---|
| `#[library_features(hidden = true)]` | Hide module prefix / 隐藏模块前缀 |
| `#[internal]` | Internal implementation / 内部实现 |
| `#[inline]` | Inline hint / 内联提示 |
| `#[deprecated]` | Mark as deprecated / 标记为弃用 |

---

## 14. Complete Example / 完整示例

```gobol
import io;
import math;

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
        math.sqrt((self.x * self.x + self.y * self.y) as float)
    }
    
    convert str(self): str {
        @"({self.x}, {self.y})"
    }
}

func main(): int {
    var name = "Gobol";
    io.print(@"Hello from {name}\n");
    
    var arr: [int] = [1, 2, 3, 4, 5];
    for i, v in arr {
        io.print("{i}: {v}");
    }
    
    var p = Point(3, 4);
    io.print(@"Point: {p as str}\n");
    io.print(@"Distance: {p.distance()}\n");
    
    return 0;
}
```

---

**Gobol — A safe, modern, and expressive programming language** 🚀

**Gobol — 安全、现代、富有表达力的编程语言** 🚀
