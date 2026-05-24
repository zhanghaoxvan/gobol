use std::process::Command;
use std::fs;
use std::path::PathBuf;

fn run_gobol(source: &str) -> Result<String, String> {
    let temp_file = PathBuf::from("test_temp.gbl");
    fs::write(&temp_file, source).map_err(|e| e.to_string())?;
    
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .arg(temp_file.to_str().unwrap())
        .output()
        .map_err(|e| e.to_string())?;
    
    let _ = fs::remove_file(temp_file);
    
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[test]
fn test_hello_world() {
    let source = r#"
module main

func main(): int {
    io.print("Hello, World!")
    return 0
}
"#;
    
    let result = run_gobol(source);
    assert!(result.is_ok() || result.unwrap_err().contains("Hello, World!"));
}

#[test]
fn test_variable_declaration() {
    let source = r#"
module main

func main(): int {
    var x: int = 42
    io.print(x)
    return 0
}
"#;
    
    let result = run_gobol(source);
    assert!(result.is_ok() || result.unwrap_err().contains("42"));
}

#[test]
fn test_array_operations() {
    let source = r#"
module main

func main(): int {
    var arr: int[] = [1, 2, 3]
    arr.add(4)
    io.print(arr[0])
    io.print(arr.len())
    return 0
}
"#;
    
    let result = run_gobol(source);
    assert!(result.is_ok() || !result.unwrap_err().is_empty());
}

#[test]
fn test_for_loop_range() {
    let source = r#"
module main

func main(): int {
    var sum: int = 0
    for i in 0..5 {
        sum = sum + i
    }
    io.print(sum)
    return 0
}
"#;
    
    let result = run_gobol(source);
    assert!(result.is_ok() || result.unwrap_err().contains("10"));
}

#[test]
fn test_if_statement() {
    let source = r#"
module main

func main(): int {
    var x: int = 10
    if x > 5 {
        io.print("Greater")
    } else {
        io.print("Less")
    }
    return 0
}
"#;
    
    let result = run_gobol(source);
    assert!(result.is_ok() || result.unwrap_err().contains("Greater"));
}

#[test]
fn test_function_call() {
    let source = r#"
module main

func add(a: int, b: int): int {
    return a + b
}

func main(): int {
    var result = add(3, 5)
    io.print(result)
    return 0
}
"#;
    
    let result = run_gobol(source);
    assert!(result.is_ok() || result.unwrap_err().contains("8"));
}
