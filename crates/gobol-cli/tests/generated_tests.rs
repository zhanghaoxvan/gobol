mod common;
use common::*;

/// 用例：advanced/bubble_sort.gbl | 预期正常运行
#[test]
fn test_advanced_bubble_sort() {
    let path = fixture_path("fixtures/advanced/bubble_sort.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：advanced/fibonacci.gbl | 预期正常运行
#[test]
fn test_advanced_fibonacci() {
    let path = fixture_path("fixtures/advanced/fibonacci.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：advanced/prime_numbers.gbl | 预期正常运行
#[test]
fn test_advanced_prime_numbers() {
    let path = fixture_path("fixtures/advanced/prime_numbers.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：arrays/array_add.gbl | 预期正常运行
#[test]
fn test_arrays_array_add() {
    let path = fixture_path("fixtures/arrays/array_add.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：arrays/array_index.gbl | 预期正常运行
#[test]
fn test_arrays_array_index() {
    let path = fixture_path("fixtures/arrays/array_index.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

#[test]
fn test_byte_array_length() {
    let result = run_inline_test(
        r#"func main() {
    var bytes: byte[] = [65, 66, 67];
    io::println(bytes.len() as str);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "3");
}

#[test]
fn test_vec_generic_constructor_forms() {
    let result = run_inline_test(
        r#"import vec;
func main() {
    var typed: Vec<int> = Vec<int>::new([1, 2, 3]);
    var inferred: Vec<int> = Vec::new([4, 5]);
    var empty: Vec<int> = Vec::new();
    var dotted = Vec.new();
    io::println((typed.len() + inferred.len() + empty.len()) as str);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "5");
}

#[test]
fn test_higher_order_function_signatures_and_abi() {
    let result = run_inline_test(
        r#"func apply_float(f: func(float, float): float, a: float, b: float): float {
    f(a, b)
}
func sum_float(a: float, b: float): float {
    a + b
}
func apply_bool(f: func(int): bool, value: int): bool {
    f(value)
}
func is_positive(value: int): bool {
    value > 0
}
func apply_void(f: func(int), value: int) {
    f(value);
}
func noop(value: int) {}
func main() {
    var sum = apply_float(&sum_float, 1.5, 2.25);
    var positive = apply_bool(&is_positive, 1);
    apply_void(&noop, 7);
    io::println(sum as str);
    io::println(positive as str);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.lines().collect::<Vec<_>>(), ["3.75", "true"]);
}

#[test]
fn test_generic_struct_method_specializes_by_receiver_type() {
    let result = run_inline_test(
        r#"import vec;
func main() {
    var raw: float[] = [1.25, 2.5];
    io::println(raw[0] as str);
    var values: Vec<float> = Vec<float>::new([1.25, 2.5]);
    io::println(values.len() as str);
    values.set(0, 3.5);
    io::println(values.get(0) as str);
    io::println(values.get(1) as str);
    var counts: Vec<int> = Vec<int>::new([4, 7]);
    io::println(counts.get(1) as str);
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        ["1.25", "2", "3.5", "2.5", "7"]
    );
}

#[test]
fn test_enum_qualified_match_patterns() {
    let result = run_inline_test(
        r#"enum Color {
    Red,
    Green,
    Blue,
}
func main() {
    var color = Color::Green;
    var value = match color {
        Color::Red => 1,
        Color::Green => 2,
        _ => 3,
    };
    io::println(value as str);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "2");
}

#[test]
fn test_enum_multiple_payload_construction_and_match_binding() {
    let result = run_inline_test(
        r#"enum Entry {
    KeyValue(str, int),
            Pair(int, int),
            Measure(float),
            Empty,
        }
        func main() {
            var entry = Entry::KeyValue("answer", 42);
            var value = match entry {
                Entry::KeyValue(key, count) => {
                    io::println(key);
                    count
                },
                Entry::Pair(_, count) => count,
                Entry::Empty => 0,
            };
    io::println(value as str);
            var measurement = Entry::Measure(1.5);
            match measurement {
                Entry::Measure(number) => {
                    io::println(number as str);
                    0
                },
                _ => 0,
            };
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.trim().replace("\r\n", "\n"),
        "answer\n42\n1.5"
    );
}

#[test]
fn test_enum_payload_pattern_arity_mismatch_is_rejected() {
    let result = run_inline_test(
        r#"enum Entry {
    KeyValue(str, int),
}
func main() {
    var entry = Entry::KeyValue("answer", 42);
    match entry {
        Entry::KeyValue(value) => value,
    };
}"#,
    );
    result.assert_failure(ExitCode::CompileError);
    assert!(
        result.stderr.contains("pattern provides 1"),
        "{}",
        result.stderr
    );
}

#[test]
fn test_generic_enum_single_payload_remains_compatible() {
    let result = run_inline_test(
        r#"enum Maybe<T> {
    Some(T),
    None,
}
func main() {
    var maybe: Maybe<int> = Maybe<int>::Some(42);
    var value = match maybe {
        Maybe::Some(payload) => payload,
        Maybe::None => 0,
    };
    io::println(value as str);
    var decimal: Maybe<float> = Maybe<float>::Some(1.25);
    match decimal {
        Maybe::Some(payload) => {
            io::println(payload as str);
            0
        },
        Maybe::None => 0,
    };
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim().replace("\r\n", "\n"), "42\n1.25");
}

#[test]
fn test_generic_enum_variant_return_keeps_type_arguments() {
    let result = run_inline_test(
        r#"enum Option<T> {
    Some(T),
    None,
}

func make_none(): Option<int> {
    Option<int>::None
}

func main() {
    var value = make_none();
    match value {
        Option::None => io::println("ok"),
        Option::Some(v) => io::println(v as str),
    };
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "ok");
}

#[test]
fn test_semantic_diagnostics_report_source_lines() {
    let result = run_inline_test(
        "func main() {\n    var value = 1;\n    missing();\n    for item in true {\n        item;\n    }\n}\n",
    );
    result.assert_failure(ExitCode::CompileError);
    assert!(result.stderr.contains(":3:"), "{}", result.stderr);
    assert!(result.stderr.contains(":4:"), "{}", result.stderr);
}

#[test]
fn test_debugger_breakpoint_and_single_step() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    init_test_env();
    let temp_dir = std::env::temp_dir().join(format!(
        "gobol_debug_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_dir).expect("create debugger test directory");
    let source_path = temp_dir.join("main.gbl");
    std::fs::write(
        temp_dir.join("grape.toml"),
        "[project]\nname = \"debug_test\"\nversion = \"0.1.0\"\nentry = \"main.gbl\"\n\n[dependencies]\n",
    )
    .expect("write debugger test manifest");
    std::fs::write(
        &source_path,
        "func main() {\n    var value = 1;\n    value = value + 1;\n    io::println(value as str);\n}\n",
    )
    .expect("write debugger test source");

    let compiler_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("release");
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let build_status = Command::new("cargo")
        .args(["build", "--release", "--bin", "grape"])
        .current_dir(&workspace_root)
        .status()
        .expect("build grape binary");
    assert!(build_status.success(), "grape binary build failed");
    let mut search_paths = vec![compiler_dir];
    search_paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(search_paths).expect("build compiler PATH");
    let mut grape = workspace_root.join("target").join("release").join("grape");
    if cfg!(windows) {
        grape.set_extension("exe");
    }
    let mut command = Command::new(grape);
    command
        .current_dir(&temp_dir)
        .args(["debug", "-b", "4"])
        .env("PATH", path)
        .env_remove("GOBOL_INSTALL_DIR")
        .env_remove("GOBOL_HOME")
        .env_remove("GOBOL_RUNTIME")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command.spawn().expect("launch grape debug");
    child
        .stdin
        .as_mut()
        .expect("debugger stdin")
        .write_all(b"s\nc\nc\n")
        .expect("send debugger step and continue commands");
    drop(child.stdin.take());
    let output = child
        .wait_with_output()
        .expect("collect debugger test program output");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "grape debug failed:\n{}\n{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("Stopped at line 2"), "{stdout}");
    assert!(stdout.contains("Stopped at line 3"), "{stdout}");
    assert!(stdout.contains("Stopped at line 4"), "{stdout}");
    assert!(stdout.contains("> 2"), "{stdout}");

    std::fs::remove_dir_all(&temp_dir).expect("remove debugger test directory");
}

#[test]
fn test_tcp_receive_bytes_and_peer_eof() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local TCP test server");
    let port = listener.local_addr().unwrap().port();
    listener
        .set_nonblocking(true)
        .expect("set local test listener nonblocking");
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(accepted) => break accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        return None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(error) => panic!("accept Gobol client: {error}"),
            }
        };
        let mut request = [0; 1024];
        let request_len = match stream.read(&mut request) {
            Ok(length) => length,
            Err(_) => return Some(false),
        };
        if request_len == 0 {
            return Some(false);
        }
        Some(
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                )
                .is_ok(),
        )
    });

    let source = r#"import net;
import result;
func main() {
    var socket = net::TcpStream::connect("127.0.0.1", $PORT, 1000).take();
    socket.send_str("GET / HTTP/1.1\r\nConnection: close\r\n\r\n").take();
    var response = "";
    while true {
        var chunk = socket.recv_bytes(4096).take();
        io::println(chunk.len() as str);
        if chunk.len() == 0 { break; }
        response = response + chunk as str;
    }
    io::println(response);
    socket.close().take();
}"#
    .replace("$PORT", &port.to_string());
    let result = run_inline_test(&source);
    result.assert_success();
    assert!(
        server.join().expect("local TCP server thread") == Some(true),
        "Gobol client did not send a request. stdout:\n{}\nstderr:\n{}",
        result.stdout,
        result.stderr
    );
    let output: Vec<_> = result.stdout.lines().collect();
    assert!(result.stdout.contains("HTTP/1.1 200 OK"));
    assert_eq!(output.last(), Some(&"hello"));
    assert!(
        output.iter().any(|line| *line == "0"),
        "EOF must yield an empty chunk: {output:?}"
    );
}

#[test]
fn test_string_index() {
    let result = run_inline_test(
        r#"func main() {
    var s = "hello";
    io::println(s[1]);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "e");
}

#[test]
fn test_process_arguments() {
    let result = run_inline_test_with_args(
        r#"import args;
func main() {
    io::println(args::argc());
    io::println(builtins::gobol_str_len(args::argv(0)) > 0);
    io::println(args::argv(1));
    io::println(args::argv(2));
    io::println(builtins::gobol_str_len(args::argv(3)) == 0);
}"#,
        &["first", "second"],
    );
    result.assert_success();
    assert!(result.stdout.lines().any(|line| line == "3"));
    assert!(result.stdout.lines().filter(|line| *line == "true").count() >= 2);
    assert!(result.stdout.lines().any(|line| line == "first"));
    assert!(result.stdout.lines().any(|line| line == "second"));
}

#[test]
fn test_thread_join_preserves_64_bit_return_value() {
    let result = run_inline_test(
        r#"import thread;

func worker(value: int): int {
    value + 4294967296
}

func main() {
    val worker_thread = Thread::spawn(worker, 7);
    io::println(worker_thread.join());
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "4294967303");
}

#[test]
fn test_print_file_contents_as_string() {
    let result = run_inline_test(
        r#"import fs;
func main() {
    var f = File::open("std/fs.gbl", "r");
    var content = f.read_all();
    io::println(content);
    f.close();
}"#,
    );
    result.assert_success();
    result.assert_stdout_contains("// std/fs.gbl");
}

#[test]
fn test_pointer_dereference() {
    let result = run_inline_test(
        r#"func main() {
    var x = 5;
    var p: *int = &x;
    io::println(*p);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "5");
}

#[test]
fn test_pointer_parameter_mutates_argument() {
    let result = run_inline_test(
        r#"func increment(p: *int) {
    *p = *p + 1;
}
func main() {
    var x = 5;
    increment(&x);
    io::println(x);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "6");
}

#[test]
fn test_swap_through_pointer_parameters() {
    let result = run_inline_test(
        r#"func swap(a: *int, b: *int) {
    var tmp = *a;
    *a = *b;
    *b = tmp;
}
func main() {
    var x = 1;
    var y = 2;
    swap(&x, &y);
    io::println(x);
    io::println(y);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.lines().collect::<Vec<_>>(), ["2", "1"]);
}

#[test]
fn test_reference_parameter_mutates_argument() {
    let result = run_inline_test(
        r#"func fun(x: &int) {
    x = 1;
}
func main() {
    var y = 5;
    fun(&y);
    io::println(y);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "1");
}

#[test]
fn test_reference_parameter_auto_dereferences() {
    let result = run_inline_test(
        r#"func read_value(x: &int): int {
    x + 1
}
func main() {
    var y = 5;
    io::println(read_value(&y));
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "6");
}

#[test]
fn test_string_to_int_cast() {
    let result = run_inline_test(
        r#"func main() {
    io::println("0" as int);
    io::println("12345" as int);
    io::println("-12345" as int);
    io::println("+42" as int);
    io::println("-9223372036854775808" as int);
    io::println("9223372036854775807" as int);
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        [
            "0",
            "12345",
            "-12345",
            "42",
            "-9223372036854775808",
            "9223372036854775807"
        ]
    );
}

#[test]
fn test_string_concatenation_continues_after_operator_newline() {
    let result = run_inline_test(
        r#"func main() {
    var path = "/";
    var host = "localhost";
    var request = "GET " + path + " HTTP/1.1 " +
                  "Host: " + host + " " +
                  "Connection: close";
    io::print(request);
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout,
        "GET / HTTP/1.1 Host: localhost Connection: close"
    );
}

#[test]
fn test_variable_shadowing_restores_outer_binding() {
    let result = run_inline_test(
        r#"func main() {
    var value = "outer";
    if true {
        var value = 42;
        io::println(value);
    }
    io::println(value);

    var count = 1;
    if true {
        count = 2;
    }
    io::println(count);

    var i = 9;
    for i in 0..2 {
        io::println(i);
    }
    io::println(i);
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        ["42", "outer", "2", "0", "1", "9"]
    );
}

#[test]
fn test_read_stdin_then_print_integer() {
    let result = run_inline_test_with_stdin(
        r#"func main() {
    val n = io::read() as int;
    io::println(n);
}"#,
        b"40\n",
    );
    result.assert_success();
    assert_eq!(result.stdout.trim(), "40");
}

#[test]
fn test_invalid_string_to_int_cast_traps() {
    let result = run_inline_test(
        r#"func main() {
    io::println("12x" as int);
}"#,
    );
    assert!(
        !result.success,
        "invalid integer text must not become a value"
    );
}

#[test]
fn test_overflowing_string_to_int_cast_traps() {
    let result = run_inline_test(
        r#"func main() {
    io::println("9223372036854775808" as int);
}"#,
    );
    assert!(!result.success, "overflowing integer text must trap");
}

/// 用例：arrays/array_literal.gbl | 预期正常运行
#[test]
fn test_arrays_array_literal() {
    let path = fixture_path("fixtures/arrays/array_literal.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：basic/hello_world.gbl | 预期正常运行
#[test]
fn test_basic_hello_world() {
    let path = fixture_path("fixtures/basic/hello_world.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：basic/variables.gbl | 预期正常运行
#[test]
fn test_basic_variables() {
    let path = fixture_path("fixtures/basic/variables.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：control_flow/break_continue.gbl | 预期正常运行
#[test]
fn test_control_flow_break_continue() {
    let path = fixture_path("fixtures/control_flow/break_continue.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：control_flow/for_loop.gbl | 预期正常运行
#[test]
fn test_control_flow_for_loop() {
    let path = fixture_path("fixtures/control_flow/for_loop.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：control_flow/if_else.gbl | 预期正常运行
#[test]
fn test_control_flow_if_else() {
    let path = fixture_path("fixtures/control_flow/if_else.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：control_flow/if_else_if.gbl | 预期正常运行
#[test]
fn test_control_flow_if_else_if() {
    let path = fixture_path("fixtures/control_flow/if_else_if.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：control_flow/while_loop.gbl | 预期正常运行
#[test]
fn test_control_flow_while_loop() {
    let path = fixture_path("fixtures/control_flow/while_loop.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：errors/assign_to_const.gbl | 预期编译失败
#[test]
fn test_errors_assign_to_const() {
    let path = fixture_path("fixtures/errors/assign_to_const.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_failure(ExitCode::CompileError);
}

/// 用例：errors/missing_return.gbl | 预期编译失败
#[test]
fn test_errors_missing_return() {
    let path = fixture_path("fixtures/errors/missing_return.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_failure(ExitCode::CompileError);
}

/// 用例：errors/type_mismatch.gbl | 预期编译失败
#[test]
fn test_errors_type_mismatch() {
    let path = fixture_path("fixtures/errors/type_mismatch.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_failure(ExitCode::CompileError);
}

/// 用例：errors/undefined_variable.gbl | 预期编译失败
#[test]
fn test_errors_undefined_variable() {
    let path = fixture_path("fixtures/errors/undefined_variable.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_failure(ExitCode::CompileError);
}

/// 用例：errors/missing_impl.gbl | 预期编译失败
#[test]
fn test_errors_missing_impl() {
    let path = fixture_path("fixtures/errors/missing_impl.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_failure(ExitCode::CompileError);
}

/// 用例：functions/add.gbl | 预期正常运行
#[test]
fn test_functions_add() {
    let path = fixture_path("fixtures/functions/add.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：functions/factorial.gbl | 预期正常运行
#[test]
fn test_functions_factorial() {
    let path = fixture_path("fixtures/functions/factorial.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：functions/void_return.gbl | 预期正常运行
#[test]
fn test_functions_void_return() {
    let path = fixture_path("fixtures/functions/void_return.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：functions/global_var.gbl | 预期正常运行
#[test]
fn test_functions_global_var() {
    let path = fixture_path("fixtures/functions/global_var.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
    result.assert_stdout_contains("after 3 bumps: 3");
    result.assert_stdout_contains("after +10: 13");
}

/// 用例：modules/lib/math.gbl | 预期编译失败（库文件，无main，AOT链接缺main符号）
#[test]
fn test_modules_lib_math() {
    let path = fixture_path("fixtures/modules/std/math.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    // Library file without main — AOT linking fails (no main symbol) → CompileError
    result.assert_failure(ExitCode::CompileError);
}

/// 用例：modules/main.gbl | 预期正常运行
#[test]
fn test_modules_main() {
    let path = fixture_path("fixtures/modules/main.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：structs/constructor.gbl | 预期正常运行
#[test]
fn test_structs_constructor() {
    let path = fixture_path("fixtures/structs/constructor.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

#[test]
fn test_multiline_struct_literal_tail_expression() {
    let result = run_inline_test(
        r#"struct Point {
    x: int,
    y: int,
};

impl New for Point {
    func new(x: int, y: int): Point {
        Point {
            x: x,
            y: y
        }
    }
}

func main() {
    var point = Point(4, 7);
    io::println(point.x + point.y);
}"#,
    );
    result.assert_success();
    assert!(result.stdout.lines().any(|line| line == "11"));
}

/// 用例：structs/point.gbl | 预期正常运行
#[test]
fn test_structs_point() {
    let path = fixture_path("fixtures/structs/point.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：structs/rectangle.gbl | 预期正常运行
#[test]
fn test_structs_rectangle() {
    let path = fixture_path("fixtures/structs/rectangle.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：types/type_conversion.gbl | 预期正常运行
#[test]
fn test_types_type_conversion() {
    let path = fixture_path("fixtures/types/type_conversion.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：expressions/match_expr.gbl | 预期正常运行
#[test]
fn test_expressions_match_expr() {
    let path = fixture_path("fixtures/expressions/match_expr.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：expressions/block_expr.gbl | 预期正常运行
#[test]
fn test_expressions_block_expr() {
    let path = fixture_path("fixtures/expressions/block_expr.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：expressions/implicit_return.gbl | 预期正常运行
#[test]
fn test_expressions_implicit_return() {
    let path = fixture_path("fixtures/expressions/implicit_return.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：expressions/short_circuit.gbl | 预期正常运行
#[test]
fn test_expressions_short_circuit() {
    let path = fixture_path("fixtures/expressions/short_circuit.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：control_flow/for_index_value.gbl | 预期正常运行
#[test]
fn test_control_flow_for_index_value() {
    let path = fixture_path("fixtures/control_flow/for_index_value.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：control_flow/for_string.gbl | 预期正常运行
#[test]
fn test_control_flow_for_string() {
    let path = fixture_path("fixtures/control_flow/for_string.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：types/println.gbl | 预期正常运行
#[test]
fn test_types_println() {
    let path = fixture_path("fixtures/types/println.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：traits/add.gbl | 预期正常运行
#[test]
fn test_traits_add() {
    let path = fixture_path("fixtures/traits/add.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：traits/operator_chain.gbl | 预期正常运行
#[test]
fn test_traits_operator_chain() {
    let path = fixture_path("fixtures/traits/operator_chain.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：traits/custom_struct.gbl | 预期正常运行
#[test]
fn test_traits_custom_struct() {
    let path = fixture_path("fixtures/traits/custom_struct.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：traits/multiple_impl.gbl | 预期正常运行
#[test]
fn test_traits_multiple_impl() {
    let path = fixture_path("fixtures/traits/multiple_impl.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：namespace/import.gbl | 预期正常运行
#[test]
fn test_namespace_import() {
    let path = fixture_path("fixtures/namespace/import.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：namespace/path_access.gbl | 预期正常运行
#[test]
fn test_namespace_path_access() {
    let path = fixture_path("fixtures/namespace/path_access.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：metaprogramming/expand.gbl | 预期正常运行
#[test]
fn test_metaprogramming_expand() {
    let path = fixture_path("fixtures/metaprogramming/expand.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：ffi/variadic.gbl | 预期正常运行
#[test]
fn test_ffi_variadic() {
    let path = fixture_path("fixtures/ffi/variadic.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：stdlib/ref_test.gbl | 预期正常运行
#[test]
fn test_stdlib_ref_test() {
    let path = fixture_path("fixtures/stdlib/ref_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：stdlib/result_test.gbl | 预期正常运行
#[test]
fn test_stdlib_result_test() {
    let path = fixture_path("fixtures/stdlib/result_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：stdlib/enum_test.gbl | 预期正常运行
#[test]
fn test_stdlib_enum_test() {
    let path = fixture_path("fixtures/stdlib/enum_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：stdlib/fs_test.gbl | 文件系统测试
#[test]
fn test_stdlib_fs_test() {
    let path = fixture_path("fixtures/stdlib/fs_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：stdlib/math_test.gbl | 数学函数测试
#[test]
fn test_stdlib_math_test() {
    let path = fixture_path("fixtures/stdlib/math_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：stdlib/net_test.gbl | 网络模块测试
#[test]
fn test_stdlib_net_test() {
    let path = fixture_path("fixtures/stdlib/net_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// The standard range iterator implements Iterator and returns Option values.
#[test]
fn test_stdlib_range_iterator_trait() {
    let path = fixture_path("fixtures/stdlib/range_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：stdlib/str_test.gbl | 字符串测试
#[test]
fn test_stdlib_str_test() {
    let path = fixture_path("fixtures/stdlib/str_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}

/// 用例：functions/lambda.gbl | 预期正常运行
#[test]
fn test_functions_lambda() {
    let path = fixture_path("fixtures/functions/lambda.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
}
