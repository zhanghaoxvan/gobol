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

#[test]
fn test_std_time_clocks_and_sleep() {
    let result = run_inline_test(
        r#"import std::time;

func main() {
    var started = time::now_ms();
    var sleep_status = time::sleep_ms(30);
    var elapsed = time::now_ms() - started;
    io::println(sleep_status);
    io::println(elapsed >= 20);
    io::println(time::unix_time_ms() > 1700000000000);
    io::println(time::sleep_ms(-1));
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        ["0", "true", "true", "-1"]
    );
}

#[test]
fn test_heterogeneous_tuples_fields_types_and_destructuring() {
    let result = run_inline_test(
        r#"func make_pair(): (int, str) {
    (42, "answer")
}

func second(pair: (int, str)): str {
    pair.1
}

func identity<T>(value: T): T {
    value
}

func main() {
    var tuple: (int, str, bool, float, char) = (1, "two", true, 3.5, 'x');
    tuple.0 = 7;
    io::println(tuple.0);
    io::println(tuple.1);
    io::println(tuple.2);
    io::println(tuple.3);
    io::println(tuple.4);
    var (number, word) = make_pair();
    io::println(number);
    io::println(word);
    io::println(second(make_pair()));
    io::println(identity((9, "generic")).1);
    var one_element: (bool,) = (true,);
    io::println(one_element.0);
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        [
            "7", "two", "true", "3.5", "x", "42", "answer", "answer", "generic", "true"
        ]
    );
}

#[test]
fn test_integer_bitwise_operators_and_assignments() {
    let result = run_inline_test(
        r#"func main() {
    io::println(1 | 2 ^ 3 & 1);
    io::println(1 + 2 << 2);
    io::println(20 >> 2);
    io::println(~0);

    var value = 12;
    value &= 10;
    value |= 1;
    value ^= 3;
    value <<= 1;
    value >>= 2;
    io::println(value);
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        ["3", "12", "5", "-1", "5"]
    );
}

#[test]
fn test_tuple_immutability_and_bounds_are_semantic_errors() {
    let immutable = run_inline_test(
        r#"func main() {
    val tuple = (1, "two");
    tuple.0 = 3;
}"#,
    );
    immutable.assert_failure(ExitCode::CompileError);
    assert!(
        immutable
            .stderr
            .contains("Cannot assign to tuple in constant variable")
    );

    let out_of_bounds = run_inline_test(
        r#"func main() {
    val tuple = (1, "two");
    io::println(tuple.2);
}"#,
    );
    out_of_bounds.assert_failure(ExitCode::CompileError);
    assert!(out_of_bounds.stderr.contains("out of bounds"));

    let mismatched_arity = run_inline_test(
        r#"func main() {
    var (first, second) = (1, "two", true);
}"#,
    );
    mismatched_arity.assert_failure(ExitCode::CompileError);
    assert!(mismatched_arity.stderr.contains("expects 2 values, got 3"));

    let mismatched_types = run_inline_test(
        r#"func main() {
    var pair: (int, str) = ("one", 2);
}"#,
    );
    mismatched_types.assert_failure(ExitCode::CompileError);
}

#[test]
fn test_struct_literals_initialize_fields_without_calling_new() {
    let result = run_inline_test(
        r#"struct Point {
    x: int,
    y: int,
}

impl Point {
    func new(x: int, y: int): Point {
        Point { x: x + 100, y: y }
    }

    func get_x(self): int {
        self.x
    }
}

func main() {
    var direct = Point {
        y: 4,
        x: 2,
    };
    var constructed = Point::new(3, 5);
    io::println(direct.x);
    io::println(direct.get_x());
    io::println(constructed.x);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.lines().collect::<Vec<_>>(), ["2", "2", "103"]);
}

#[test]
fn test_generic_struct_literal_initializes_named_fields() {
    let result = run_inline_test(
        r#"struct Box<T> {
    value: T,
}

func main() {
    var boxed = Box<int> {
        value: 42,
    };
    io::println(boxed.value);
}"#,
    );
    result.assert_success();
    assert_eq!(result.stdout.lines().collect::<Vec<_>>(), ["42"]);
}

#[test]
fn test_self_requires_explicit_method_parameter_and_field_access() {
    let missing_self = run_inline_test(
        r#"struct Point {
    x: int,
}

impl Point {
    func broken(): int {
        self.x
    }
}

func main() {}"#,
    );
    missing_self.assert_failure(ExitCode::CompileError);
    assert!(missing_self.stderr.contains("without a self parameter"));

    let implicit_field = run_inline_test(
        r#"struct Point {
    x: int,
}

impl Point {
    func broken(self): int {
        x
    }
}

func main() {}"#,
    );
    implicit_field.assert_failure(ExitCode::CompileError);
    assert!(implicit_field.stderr.contains("Undeclared identifier: 'x'"));

    let field_without_receiver = run_inline_test(
        r#"struct Point {
    x: int,
}

impl Point {
    func broken(): Point {
        x = 1;
        self
    }
}

func main() {}"#,
    );
    field_without_receiver.assert_failure(ExitCode::CompileError);
    assert!(
        field_without_receiver
            .stderr
            .contains("Undeclared identifier: 'x'")
    );
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
fn test_string_index() {
    let result = run_inline_test(
        r#"func main() {
    var s = "hello";
    io::println(s[1]);
    io::println(s[1] as int);
    io::println(s.len());
    var unicode = "A世😀";
    io::println(unicode.len());
    io::println(unicode[1] as int);
    var letter: char = unicode[1];
    io::println(letter);
    io::println(unicode[2]);
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        ["e", "101", "5", "3", "19990", "世", "😀"]
    );
}

#[test]
fn test_string_to_int_cast() {
    let result = run_inline_test(
        r#"func main() {
    io::println("0" as int);
    io::println("40" as int);
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
            "40",
            "-12345",
            "42",
            "-9223372036854775808",
            "9223372036854775807"
        ]
    );
}

#[test]
fn test_read_string_to_int_cast() {
    let result = run_inline_test_with_stdin(
        r#"func main() {
    var input = io::read();
    io::println(input as int);
}"#,
        Some(b"40\n"),
    );
    result.assert_success();
    assert_eq!(result.stdout.lines().last().unwrap(), "40");
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

#[test]
fn test_unicode_character_literals_and_conversions() {
    let result = run_inline_test(
        r#"func main() {
    var letter: char = 'A';
    var converted: char = 66 as char;
    io::println(letter as int);
    io::println(letter);
    io::println(converted);
    io::println('世');
    io::println('\u{1F600}');
}"#,
    );
    result.assert_success();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        ["65", "A", "B", "世", "😀"]
    );
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
