mod common;
use common::*;

#[test]
fn test_stdlib_thread_spawn_forwards_arg() {
    let path = fixture_path("fixtures/stdlib/thread_test.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
    result.assert_stdout_contains("Hello from thread");
    result.assert_stdout_contains("add_one got: 41");
    result.assert_stdout_contains("double_arg got: 21");
}
