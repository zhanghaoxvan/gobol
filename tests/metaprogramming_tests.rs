// Regression tests for the #[expand] AST-level macro and the related parser
// / backend bugs uncovered while implementing Task 4.
//
// These are kept separate from `generated_tests.rs` (which is auto-generated
// and must not be hand-edited). Each test targets a specific behaviour or a
// specific bug fix so future regressions are caught.

mod common;
use common::*;

// ============ Fixtures ============

/// `expand_ast.gbl` is the headline #[expand] deliverable: non-literal
/// argument substitution, recursive macros, and literal folding, all in one
/// program. It must compile, run, and print the four expected lines.
#[test]
fn test_expand_ast_fixture() {
    let path = fixture_path("fixtures/metaprogramming/expand_ast.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
    result.assert_stdout_contains("z = 15");   // add(x, y)
    result.assert_stdout_contains("w = 26");   // add(x + 1, y * 2)
    result.assert_stdout_contains("d = 10");   // dbl(x) -> add(x, x)
    result.assert_stdout_contains("lit = 7");  // add(3, 4) constant-folds
}

/// `file_attrs.gbl` exercises file-level `#![no_gc]` attribute propagation.
#[test]
fn test_file_attrs_fixture() {
    let path = fixture_path("fixtures/metaprogramming/file_attrs.gbl");
    let result = run_gobol(path.to_str().unwrap(), false);
    result.assert_success();
    result.assert_stdout_contains("file-level #![no_gc] propagated, x = 42");
    result.assert_stdout_contains("y = 7");
}

// ============ #[expand] macro behaviour ============

/// A macro called with non-literal argument expressions must inline the body
/// with the arguments substituted, evaluated at runtime.
#[test]
fn test_expand_substitutes_argument_expressions() {
    let src = r#"import std;

#[expand]
func add(a: int, b: int): int { return a + b }

func main() {
    var x = 5
    var y = 10
    var z = add(x, y)
    std::io::println(@"z = {z}")
    var w = add(x + 1, y * 2)
    std::io::println(@"w = {w}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("z = 15");
    result.assert_stdout_contains("w = 26");
}

/// A macro body that calls another #[expand] macro must be recursively
/// inlined.
#[test]
fn test_expand_recursive_macro() {
    let src = r#"import std;

#[expand]
func add(a: int, b: int): int { return a + b }

#[expand]
func dbl(a: int): int { return add(a, a) }

func main() {
    var x = 5
    std::io::println(@"d = {dbl(x)}")
    std::io::println(@"q = {dbl(x + 1)}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("d = 10");   // dbl(5) -> 5 + 5
    result.assert_stdout_contains("q = 12");   // dbl(6) -> 6 + 6
}

/// All-literal macro calls still constant-fold to a compile-time literal.
#[test]
fn test_expand_literal_args_constant_fold() {
    let src = r#"import std;

#[expand]
func square(n: int): int { return n * n }

func main() {
    var x = square(7)
    std::io::println(@"x = {x}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("x = 49");
}

/// A macro call embedded directly in a format string interpolation must also
/// be inlined (the format-string visitor shares the macro table with the
/// surrounding builder).
#[test]
fn test_expand_call_inside_format_string() {
    let src = r#"import std;

#[expand]
func add(a: int, b: int): int { return a + b }

func main() {
    var x = 5
    var y = 10
    std::io::println(@"z = {add(x, y)}")
    std::io::println(@"w = {add(x + 1, y * 2)}")
    std::io::println(@"lit = {add(3, 4)}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("z = 15");
    result.assert_stdout_contains("w = 26");
    result.assert_stdout_contains("lit = 7");
}

// ============ Regression: multi-statement blocks (tail flag) ============

/// Regression for the "you cannot add an instruction to a block already
/// filled" panic. A function body with several newline-terminated expression
/// statements (no semicolons) used to mark *every* such statement as a tail
/// return, emitting a terminator after the first one and panicking on the next.
/// Only the last bare expression (immediately before '}') may be a tail.
#[test]
fn test_multiple_expression_statements_without_semicolons() {
    let src = r#"import std;

func main() {
    std::io::println("first")
    std::io::println("second")
    std::io::println("third")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("first");
    result.assert_stdout_contains("second");
    result.assert_stdout_contains("third");
}

/// A bare expression followed by more statements (arithmetic in a `var`) must
/// not turn the expression into a stray return.
#[test]
fn test_arithmetic_after_expression_statement() {
    let src = r#"import std;

func main() {
    var x = 5
    var y = 10
    std::io::println("hello")
    var w = (x + 1) + (y * 2)
    std::io::println(@"w = {w}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("hello");
    result.assert_stdout_contains("w = 26");
}

// ============ Regression: binary operators in format strings ============

/// Format-string interpolations now parse arithmetic, comparison, and grouped
/// expressions. Previously `@"{a + b}"` silently dropped the binary operand.
#[test]
fn test_binary_operators_in_format_string() {
    let src = r#"import std;

func main() {
    var x = 5
    var y = 10
    std::io::println(@"add = {x + y}")
    std::io::println(@"sub = {y - x}")
    std::io::println(@"mul = {x * y}")
    std::io::println(@"grouped = {(x + 1) + (y * 2)}")
    std::io::println(@"precedence = {x + y * 2}")
    std::io::println(@"cmp = {x < y}")
    std::io::println(@"unary = {x + -y}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("add = 15");
    result.assert_stdout_contains("sub = 5");
    result.assert_stdout_contains("mul = 50");
    result.assert_stdout_contains("grouped = 26");
    result.assert_stdout_contains("precedence = 25");   // 5 + 10*2
    result.assert_stdout_contains("cmp = true");
    result.assert_stdout_contains("unary = -5");
}

// ============ Regression: generic function monomorphization ============

/// A top-level generic `identity<T>` must instantiate per concrete type and
/// rewrite call sites so the compiled program prints the right values. This
/// exercises inference for int/float/str/bool, and the fix that resolves the
/// dangling `Struct("T")` placeholder left in the caller's declarations.
#[test]
fn test_generic_identity_multiple_concrete_types() {
    let src = r#"import std;

func identity<T>(x: T): T { return x }

func main() {
    var i = identity(5)
    var f = identity(3.5)
    var s = identity("hi")
    var b = identity(true)
    std::io::println(@"i={i} f={f} s={s} b={b}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("i=5 f=3.5 s=hi b=true");
}

/// Chained generic calls (`identity(identity(7))`) must infer the outer call
/// from the returned instance type and produce correct results.
#[test]
fn test_generic_chained_calls() {
    let src = r#"import std;

func identity<T>(x: T): T { return x }

func main() {
    var i = identity(identity(7))
    var f = identity(identity(2.5))
    std::io::println(@"i={i} f={f}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("i=7 f=2.5");
}

/// When one call site cannot be inferred (a `null` argument yields no useful
/// type), the template must be kept as a fallback so that call site still
/// links — while the inferable call site is still specialized. Regresses the
/// "successful instance + uninferred call site" dangling-symbol bug.
#[test]
fn test_generic_uninferred_call_keeps_template() {
    let src = r#"import std;

func identity<T>(x: T): T { return x }

func main() {
    var i = identity(5)
    var n = identity(null)
    std::io::println(@"i={i}")
}
"#;
    let result = run_inline_test(src);
    result.assert_success();
    result.assert_stdout_contains("i=5");
}
