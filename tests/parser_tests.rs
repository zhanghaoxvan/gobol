use gobol::ast_builder::AstBuilder;
use gobol::lexer::Lexer;

fn parse_source(source: &str) -> Result<(), Vec<String>> {
    let lexer = Lexer::new(source.to_string());
    let mut builder = AstBuilder::new(lexer);
    let prog = builder.build();
    
    if builder.has_error() {
        Err(builder.get_error_message().clone())
    } else {
        assert!(prog.is_some());
        Ok(())
    }
}

#[test]
fn test_parse_module_declaration() {
    let source = r#"
module test

func main(): int {
    return 0
}
"#;
    assert!(parse_source(source).is_ok());
}

#[test]
fn test_parse_struct_definition() {
    let source = r#"
module test

struct Point {
    x: int,
    y: int
}

func main(): int {
    return 0
}
"#;
    assert!(parse_source(source).is_ok());
}

#[test]
fn test_parse_impl_block() {
    let source = r#"
module test

struct Point {
    x: int,
    y: int
}

impl Point {
    constructor(x: int, y: int): Point {
        return self
    }
    
    func distance(self): float {
        return sqrt(self.x * self.x + self.y * self.y)
    }
}

func main(): int {
    return 0
}
"#;
    assert!(parse_source(source).is_ok());
}

#[test]
fn test_parse_generic_struct() {
    let source = r#"
module test

struct Pair<T> {
    first: T,
    second: T
}

func main(): int {
    return 0
}
"#;
    assert!(parse_source(source).is_ok());
}

#[test]
fn test_parse_export_statement() {
    let source = r#"
module test

func add(a: int, b: int): int {
    return a + b
}

export(add)

func main(): int {
    return 0
}
"#;
    assert!(parse_source(source).is_ok());
}

#[test]
fn test_parse_array_type() {
    let source = r#"
module test

func main(): int {
    var arr: int[] = [1, 2, 3]
    var arr2: int[10] = []
    var matrix: int[][] = [[1, 2], [3, 4]]
    return 0
}
"#;
    assert!(parse_source(source).is_ok());
}

#[test]
fn test_parse_nullable_type() {
    let source = r#"
module test

func main(): int {
    var opt: int? = null
    opt = 42
    return 0
}
"#;
    assert!(parse_source(source).is_ok());
}
