use gobol::ast_builder::AstBuilder;
use gobol::executor::Executor;
use gobol::lexer::Lexer;

fn execute_source(source: &str) -> Result<i32, Vec<String>> {
    let lexer = Lexer::new(source.to_string());
    let mut builder = AstBuilder::new(lexer);
    let prog = builder.build().expect("Failed to build AST");
    
    let mut executor = Executor::new();
    executor.execute(&prog)
}

#[test]
fn test_execute_addition() {
    let source = r#"
module main

func main(): int {
    var x = 10 + 20
    return x
}
"#;
    let result = execute_source(source);
    assert_eq!(result.unwrap_or(0), 30);
}

#[test]
fn test_execute_multiplication() {
    let source = r#"
module main

func main(): int {
    var x = 5 * 6
    return x
}
"#;
    let result = execute_source(source);
    assert_eq!(result.unwrap_or(0), 30);
}

#[test]
fn test_execute_division() {
    let source = r#"
module main

func main(): int {
    var x: int = 10 / 3
    // io.print(x)
    return x
}
"#;
    let result = execute_source(source);
    assert_eq!(result.unwrap_or(0), 3);
}

#[test]
fn test_execute_comparison() {
    let source = r#"
module main

func main(): int {
    var x: int = 10
    var y: int = 20
    if x < y {
        return 1
    }
    return 0
}
"#;
    let result = execute_source(source);
    assert_eq!(result.unwrap_or(0), 1);
}

#[test]
fn test_execute_array_index() {
    let source = r#"
module main

func main(): int {
    var arr: int[] = [10, 20, 30]
    return arr[1]
}
"#;
    let result = execute_source(source);
    assert_eq!(result.unwrap_or(0), 20);
}

#[test]
fn test_execute_function_call() {
    let source = r#"
module main

func add(a: int, b: int): int {
    return a + b
}

func main(): int {
    return add(5, 7)
}
"#;
    let result = execute_source(source);
    assert_eq!(result.unwrap_or(0), 12);
}

#[test]
fn test_execute_fibonacci() {
    let source = r#"
module main

func fib(n: int): int {
    if n <= 1 {
        return n
    }
    return fib(n - 1) + fib(n - 2)
}

func main(): int {
    return fib(10)
}
"#;
    let result = execute_source(source);
    assert_eq!(result.unwrap_or(0), 55);
}
