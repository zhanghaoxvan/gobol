use std::process::Command;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once};

static INIT: Once = Once::new();
// Serialize gobol build invocations so parallel tests don't fight over the
// cargo target dir lock / linker.
static BUILD_LOCK: Mutex<()> = Mutex::new(());

// Monotonic counter used to derive unique temp-file names. `Instant::now()
// .elapsed()` is ~0, so pid+nanos collisions were guaranteed under parallel
// test execution, leading to "Cannot open file" races. A process-wide counter
// removes the collision.
static UNIQUIFIER: AtomicU64 = AtomicU64::new(0);

fn unique_id() -> u64 {
    UNIQUIFIER.fetch_add(1, Ordering::Relaxed)
}

#[allow(dead_code)]
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ExitCode {
    Success = 0,
    CompileError = 1,
    RuntimePanic = 2,
}

#[derive(Debug)]
pub struct TestResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl TestResult {
    pub fn assert_success(&self) {
        assert!(
            self.success,
            "测试执行失败 | stderr:\n{}",
            self.stderr
        );
    }

    pub fn assert_failure(&self, expected_code: ExitCode) {
        assert!(
            !self.success,
            "预期失败但程序正常退出 | stdout:\n{}",
            self.stdout
        );
        assert_eq!(
            self.exit_code, expected_code as i32,
            "错误码不符，预期{} 实际{}",
            expected_code as i32, self.exit_code
        );
    }

    #[allow(dead_code)]
    pub fn assert_stdout_contains(&self, expected: &str) {
        assert!(
            self.stdout.contains(expected),
            "输出未包含「{}」\n实际输出：{}",
            expected, self.stdout
        );
    }
}

/// Path to the prebuilt `gobol` release binary.
fn gobol_binary() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("target");
    p.push("release");
    p.push("gobol");
    p
}

pub fn init_test_env() {
    INIT.call_once(|| {
        let status = Command::new("cargo")
            .args(&["build", "--release", "--bin", "gobol"])
            .status()
            .expect("cargo build --release 编译失败");
        assert!(status.success(), "Gobol Release构建失败");
    });
}

pub fn run_gobol(file_path: &str, _verbose: bool) -> TestResult {
    init_test_env();

    let temp_dir = std::env::temp_dir();
    let uid = unique_id();
    let temp_bin = temp_dir.join(format!("gobol_test_bin_{}.out", uid));

    // Run the prebuilt gobol binary directly (avoid `cargo run` which
    // contends on the target dir lock when tests run in parallel).
    let _guard = BUILD_LOCK.lock().unwrap();
    let build_output = Command::new(gobol_binary())
        .args([
            "build", file_path, "-o", temp_bin.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run gobol build");
    drop(_guard);

    let build_stdout = String::from_utf8_lossy(&build_output.stdout).to_string();
    let build_stderr = String::from_utf8_lossy(&build_output.stderr).to_string();
    let build_success = build_output.status.success();

    if !build_success {
        let _ = fs::remove_file(&temp_bin);
        return TestResult {
            success: false,
            stdout: build_stdout,
            stderr: build_stderr,
            exit_code: ExitCode::CompileError as i32,
        };
    }

    let run_output = Command::new(&temp_bin)
        .output()
        .expect("failed to run compiled binary");

    let run_stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
    let run_stderr = String::from_utf8_lossy(&run_output.stderr).to_string();
    let run_exit_code = run_output.status.code().unwrap_or(ExitCode::RuntimePanic as i32);

    let _ = fs::remove_file(&temp_bin);

    let final_exit_code = if run_output.status.success() {
        ExitCode::Success as i32
    } else if run_exit_code == ExitCode::RuntimePanic as i32 {
        ExitCode::RuntimePanic as i32
    } else {
        ExitCode::RuntimePanic as i32
    };

    TestResult {
        success: run_output.status.success(),
        stdout: format!("{}{}", build_stdout, run_stdout),
        stderr: format!("{}{}", build_stderr, run_stderr),
        exit_code: final_exit_code,
    }
}

pub fn fixture_path(relative_path: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    for part in relative_path.split('/') {
        path.push(part);
    }
    path
}

#[allow(dead_code)]
pub fn run_fixture_test(relative_path: &str) -> TestResult {
    let path = fixture_path(relative_path);
    run_gobol(path.to_str().unwrap(), false)
}

#[allow(dead_code)]
pub fn run_inline_test(content: &str) -> TestResult {
    let temp_dir = std::env::temp_dir();
    let uid = unique_id();
    let file_path = temp_dir.join(format!("gobol_inline_{}.gbl", uid));
    fs::write(&file_path, content).unwrap();
    let res = run_gobol(file_path.to_str().unwrap(), false);
    let _ = fs::remove_file(file_path);
    res
}
