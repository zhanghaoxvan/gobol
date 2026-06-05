# Gobol Language Documentation

## 1. Overview

Gobol is a statically-typed, modular programming language with generics support. It combines Rust's safety features with Go's simplicity. The compiler is written in Rust and targets both interpretation and native code generation.

Gobol 是一门静态类型、支持泛型的模块化编程语言。它融合了 Rust 的安全性特性和 Go 的简洁语法。编译器使用 Rust 编写，支持解释执行和原生代码生成。

**Author**: zhanghaoxvan

**File Extension**: `.gbl`  

**License**: GPL3

---

## 2. Basic Syntax

### 2.1 Comments

```gobol
// Single-line comment

/*
   Multi-line comment
   Can span multiple lines
*/
```

### 2.2 Module Declaration

Every `.gbl` file must start with a module declaration:

```gobol
module main
```

每个 `.gbl` 文件必须以模块声明开头。

### 2.3 Import Statement

```gobol
import io
import range
```

### 2.4 Export Statement

```gobol
export(func_name, struct_name, const_name)
```

---

## 3. Variables and Constants

### 3.1 Declaration

```gobol
var x: int = 10        // Mutable variable
val y: str = "hello"   // Immutable constant

var a: int             // Default value 0
var b: str             // Default value ""
```

### 3.2 Type Inference

```gobol
var x = 10             // Inferred as int
val name = "Gobol"     // Inferred as str
```

### 3.3 Basic Types

| Type | Description | Example |
|:---|:---|:---|
| `int` | Integer | `42`, `-7`, `0` |
| `float` | Floating-point | `3.14`, `-0.5`, `2.0` |
| `str` | String | `"hello"`, `"世界"` |
| `bool` | Boolean | `true`, `false` |

### 3.4 Nullable Types

```gobol
var opt: int? = null
opt = 42
```

Nullable types are marked with `?` suffix. A nullable variable can hold either a value of its base type or `null`.

可空类型用 `?` 后缀标记。可空变量可以持有其基础类型的值或 `null`。

---

## 4. Arrays (Dynamic Arrays)

```gobol
var arr: int[] = []              // Empty array
var arr2: int[10] = []           // Pre-allocated capacity 10
var arr3: int[] = [1, 2, 3]      // Array literal

arr.add(10)                      // Add element
arr.add(20)
io.print(arr.len())              // Get length
io.print(arr[0])                 // Index access
arr[0] = 100                     // Index assignment

// Multi-dimensional arrays
var matrix: int[][] = [[1, 2], [3, 4]]
```

Arrays in Gobol are dynamic and automatically grow when capacity is exceeded. The `int[10]` syntax pre-allocates capacity but does not set length.

Gobol 中的数组是动态的，当容量不足时会自动扩容。`int[10]` 语法预分配容量但不设置长度。

---

## 5. Functions

### 5.1 Function Definition

```gobol
func add(a: int, b: int): int {
    return a + b
}

// No return value
func greet(name: str) {
    io.print("Hello, " + name)
}

// Multiple parameters
func sum(nums: int[]): int {
    var total = 0
    for i in nums {
        total += i
    }
    return total
}
```

### 5.2 Generic Functions

```gobol
func identity<T>(x: T): T {
    return x
}

func first<T>(arr: T[]): T {
    return arr[0]
}
```

Generic functions allow writing code that works with multiple types. The type parameter `<T>` is inferred from arguments.

泛型函数允许编写适用于多种类型的代码。类型参数 `<T>` 从参数中推导。

---

## 6. Control Flow

### 6.1 if-else

```gobol
if x > 10 {
    io.print("x > 10")
} else if x > 5 {
    io.print("x > 5")
} else {
    io.print("x <= 5")
}
```

### 6.2 while Loop

```gobol
var i = 0
while i < 10 {
    io.print(i)
    i += 1
}
```

### 6.3 for Loop (Range)

```gobol
// Forward range
for i in 0..10 {
    io.print(i)          // 0,1,2,3,4,5,6,7,8,9
}

// Reverse range (auto-detected)
for i in 10..0 {
    io.print(i)          // 10,9,8,7,6,5,4,3,2,1
}

// Explicit range function with step
for i in range(0, 10, 2) {
    io.print(i)          // 0,2,4,6,8
}

// Iterate over array
var arr: int[] = [1, 2, 3, 4, 5]
for i in arr {
    io.print(i)
}
```

The `..` operator automatically detects direction: if `start > end`, it generates a descending range with step -1.

`..` 运算符自动检测方向：如果 `start > end`，则生成步长为 -1 的降序范围。

### 6.4 break / continue

```gobol
for i in 0..100 {
    if i % 2 == 0 {
        continue
    }
    if i > 50 {
        break
    }
    io.print(i)
}
```

### 6.5 return

```gobol
func factorial(n: int): int {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}
```

---

## 7. Structs

### 7.1 Struct Definition

```gobol
struct Point {
    x: int,
    y: int
}
```

### 7.2 Generic Structs

```gobol
struct Pair<T> {
    first: T,
    second: T
}
```

### 7.3 Impl Blocks (Methods)

```gobol
struct Point {
    x: int,
    y: int
}

impl Point {
    constructor(x: int, y: int) {
        self.x = x
        self.y = y
    }
    
    func distance(self): float {
        return sqrt(self.x * self.x + self.y * self.y)
    }
}
```

### 7.4 Constructor

```gobol
let p = Point{x: 10, y: 20}
```

### 7.5 Field Access

```gobol
io.print(p.x)
p.x = 30
```

Fields starting with `_` (underscore) are private and only accessible within the struct's own impl blocks.

以 `_`（下划线）开头的字段是私有的，只能在结构体自己的 impl 块内访问。

---

## 8. Operator Overloading

```gobol
operator +(left: Point, right: Point): Point {
    return Point{x: left.x + right.x, y: left.y + right.y}
}
```

Operator overloading allows defining custom behavior for operators like `+`, `-`, `*`, `/`, `==`, etc. on user-defined types.

运算符重载允许为用户定义的类型定义运算符（如 `+`、`-`、`*`、`/`、`==` 等）的自定义行为。

---

## 9. Type Conversion

```gobol
impl range {
    convert int[](self): int[] {
        var result: int[]
        var i = self._start
        while i < self._end {
            result.add(i)
            i += self._step
        }
        return result
    }
}
```

The `convert` keyword defines an implicit conversion from one type to another. This is automatically invoked when assigning to a variable of the target type.

`convert` 关键字定义从一个类型到另一个类型的隐式转换。当赋值给目标类型的变量时会自动调用。

---

## 10. Iterators

```gobol
impl vec<T> {
    iter(self) {
        return vec_iterator(_data, _len)
    }
}

impl vec_iterator<T> {
    func next(self): (T, bool) {
        if _index >= _len { return (null, false) }
        var value: T = _data[_index]
        _index += 1
        return (value, true)
    }
}
```

The `iter` method returns an iterator object that implements the `next` protocol, enabling `for` loops to work with custom types.

`iter` 方法返回一个实现 `next` 协议的迭代器对象，使 `for` 循环能够处理自定义类型。

---

## 11. Input/Output (io module)

```gobol
import io

io.print("Hello, World!")      // Print without newline
io.println("Hello!")           // Print with newline
var input: str = io.read()     // Read a line
```

The `io` module is automatically available through `__setup__` and does not require explicit import in most cases.

`io` 模块通过 `__setup__` 自动可用，在大多数情况下不需要显式导入。

---

## 12. Range Type

```gobol
// Create range
var r: int[] = range(0, 10)           // 0..9, step 1
var r2: int[] = range(0, 10, 2)       // 0,2,4,6,8
var r3: int[] = 0..10                 // Equivalent to range(0,10,1)
var r4: int[] = 10..0                 // Equivalent to range(10,0,-1)

// Methods
io.print(r.start())            // 0
io.print(r.end())              // 10
io.print(r.step())             // 1
io.print(r.len())              // 10
io.print(r.contains(5))        // true

// Convert to array
var arr: int[] = r
```

The `range` type represents an integer sequence and supports bidirectional iteration. The `..` operator is syntactic sugar that automatically determines the direction.

`range` 类型表示整数序列，支持双向迭代。`..` 运算符是自动确定方向的语法糖。

---

## 13. Built-in Functions

| Function | Description |
|:---|:---|
| `range(start, end, step?)` | Create a range object |
| `panic(msg: str)` | Trigger a runtime error |

---

## 14. Compiler Attributes

```gobol
#[library_features(hidden = true)]
module internal_lib
```

| Attribute | Description |
|:---|:---|
| `#[library_features(hidden = true)]` | Hide module prefix, users access exports directly |
| `#[internal]` | Mark as internal implementation, not exposed to users |

Compiler attributes provide metadata to guide compilation. The `hidden` attribute allows re-exporting symbols without requiring the module prefix.

编译器属性提供元数据以指导编译。`hidden` 属性允许在不要求模块前缀的情况下重新导出符号。

---

## 15. Operator Precedence (highest to lowest)

| Level | Operators | Associativity |
|:---:|:---|:---:|
| 1 | `()` `[]` `.` | Left |
| 2 | `-` `!` | Right |
| 3 | `*` `/` `%` | Left |
| 4 | `+` `-` | Left |
| 5 | `<` `>` `<=` `>=` | Left |
| 6 | `==` `!=` | Left |
| 7 | `&&` | Left |
| 8 | `||` | Left |
| 9 | `=` `+=` `-=` `*=` `/=` | Right |

---

## 16. Complete Example

```gobol
module main

func main(): int {
    // Variables
    var name = "Gobol"
    io.print(@"Hello from {name}\n")
    
    // Dynamic array
    var arr: int[] = [1, 2, 3, 4, 5]
    for i in arr {
        io.print(i)
    }
    
    // Range and conversion
    var r: int[] = 0..10
    var nums: int[] = r
    io.print(nums[0])
    
    // Struct
    struct Point {
        x: int,
        y: int
    }
    
    impl Point {
        func distance(self): float {
            return sqrt(self.x * self.x + self.y * self.y)
        }
    }
    
    var p: Point = Point(3, 4)
    io.print(p.distance())     // 5.0
    
    return 0
}
```

---

## 17. Running Gobol Programs

```bash
cargo run --release -- example.gbl
```

---

## 18. Language Feature Summary

| Feature | Support | Example |
|:---|:---:|:---|
| Static typing | ✅ | `var x: int` |
| Type inference | ✅ | `var x = 10` |
| Generics | ✅ | `struct Pair<T>` |
| Structs | ✅ | `struct Point { x, y }` |
| Methods | ✅ | `impl Point { func area(self) }` |
| Operator overloading | ✅ | `operator +` |
| Nullable types | ✅ | `int?` |
| Dynamic arrays | ✅ | `int[]` |
| Range type | ✅ | `0..10` |
| Module system | ✅ | `module` / `import` / `export` |
| Iterators | ✅ | `for i in arr` |
| Attributes | ✅ | `#[...]` |
| Format strings | ✅ | `@"Hello {name}"` |

---

**Gobol — A safe, modern, and expressive programming language** 🚀
