// ============================================================
// 自动生成集成测试 | 禁止手动修改
// 更新用例后重新执行 ./test.sh 刷新本文件
// ============================================================
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

/// 用例：modules/lib/math.gbl | 预期正常运行
#[test]
fn test_modules_lib_math() {
    let path = fixture_path("fixtures/modules/lib/math.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
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

/// 用例：types/nullable.gbl | 预期正常运行
#[test]
fn test_types_nullable() {
    let path = fixture_path("fixtures/types/nullable.gbl");
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
