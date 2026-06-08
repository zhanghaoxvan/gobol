use std::process::Command;
use std::fs;
use std::path::PathBuf;
use std::sync::Once;
use std::time::Instant;

static INIT: Once = Once::new();

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
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let bin_path = format!("{}/target/release/gobol", manifest_dir);
    let output = Command::new(&bin_path)
        .arg(file_path)
        .output()
        .expect("执行gobol二进制失败");

    TestResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(1),
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
    let nanos = Instant::now().elapsed().as_nanos();
    let pid = std::process::id();
    let file_path = temp_dir.join(format!("test_{}_{}.gbl", pid, nanos));
    fs::write(&file_path, content).unwrap();
    let res = run_gobol(file_path.to_str().unwrap(), false);
    let _ = fs::remove_file(file_path);
    res
}
