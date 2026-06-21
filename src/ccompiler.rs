// ccompiler.rs — cross-platform C compiler abstraction.
//
// Detects the system C compiler (GCC/Clang on Unix, MSVC/MinGW on Windows)
// and exposes a single `compile` entry point that produces a native executable.
//
// Usage:
//   let cc = CCompiler::detect();
//   cc.compile(&["src.c", "lib.c"], "output")?;

use std::env;
use std::path::Path;
use std::process::{Command, ExitStatus};
use colored::*;
use crate::error::ErrorFormatter;

/// Compilation error with formatted output
#[derive(Debug)]
pub struct CompileError {
    pub message: String,
    pub status: ExitStatus,
    pub stderr: String,
    pub stdout: String,
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CompileError {}

/// Represents a detected C compiler.
pub struct CCompiler {
    /// The compiler executable (e.g. "cc", "gcc", "cl.exe")
    program: String,
    /// Whether this is MSVC (affects flag style)
    is_msvc: bool,
    /// Whether this is MinGW (GCC on Windows)
    is_mingw: bool,
    /// Error formatter for displaying errors
    error_formatter: Option<ErrorFormatter>,
}

impl CCompiler {
    /// Detect the system C compiler.
    ///
    /// Respects `$CC`; falls back to `cc` on Unix or `cl.exe` on Windows.
    pub fn detect() -> Self {
        let program = env::var("CC").unwrap_or_else(|_| default_compiler());
        
        // More robust detection
        let program_lower = program.to_lowercase();
        let is_msvc = program_lower.contains("cl") || program_lower.contains("msvc");
        let is_mingw = program_lower.contains("gcc") || program_lower.contains("mingw");
        
        CCompiler {
            program,
            is_msvc,
            is_mingw,
            error_formatter: None,
        }
    }

    /// Set an error formatter for better error messages
    pub fn with_error_formatter(mut self, formatter: ErrorFormatter) -> Self {
        self.error_formatter = Some(formatter);
        self
    }

    /// Compile one or more C source files into a native executable.
    ///
    /// `sources` — paths to `.c` files (companion files first, then generated).
    /// `output`  — name of the resulting executable.
    pub fn compile(&self, sources: &[impl AsRef<Path>], output: &str) -> Result<ExitStatus, CompileError> {
        let mut cmd = Command::new(&self.program);

        // Add compiler flags
        self.add_compiler_flags(&mut cmd);
        
        // Output file
        if self.is_msvc {
            cmd.arg(&format!("/Fe:{}", output));
        } else {
            cmd.arg("-o").arg(output);
        }

        // Source files
        for src in sources {
            cmd.arg(src.as_ref());
        }

        // Platform-specific libraries
        self.add_platform_libraries(&mut cmd);

        // Debug: print command for diagnostics
        if env::var("GOBOL_DEBUG").is_ok() {
            eprintln!("{}", format!("Compiling: {:?}", cmd).red());
        }

        // Execute and capture output
        let output = cmd.output().map_err(|e| CompileError {
            message: format!("Failed to execute compiler '{}': {}", self.program, e),
            status: ExitStatus::default(),
            stderr: String::new(),
            stdout: String::new(),
        })?;

        // Check if compilation succeeded
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            
            let message = self.format_compile_error(&stderr, &stdout);
            
            return Err(CompileError {
                message,
                status: output.status,
                stderr,
                stdout,
            });
        }

        Ok(output.status)
    }

    fn format_compile_error(&self, stderr: &str, stdout: &str) -> String {
        let mut result = String::new();
        
        // Use error formatter if available
        if let Some(formatter) = &self.error_formatter {
            // Try to parse compiler error messages
            let errors = self.parse_compiler_errors(stderr, stdout);
            
            if !errors.is_empty() {
                for error in errors {
                    result.push_str(&formatter.format_error(
                        error.line,
                        error.col,
                        error.span,
                        "error",
                        &error.message,
                        true,
                    ));
                    result.push('\n');
                }
                
                // Add note about compiler
                result.push_str(&formatter.format_note(
                    &format!("compiler '{}' returned exit code", self.program),
                    true,
                ));
                result.push('\n');
                
                return result;
            }
        }
        
        // Fallback: raw compiler output
        if !stderr.is_empty() {
            result.push_str(&format!("{}\n", stderr.red()));
        }
        if !stdout.is_empty() {
            result.push_str(&format!("{}\n", stdout));
        }
        
        if result.is_empty() {
            result.push_str(&format!("Compilation failed with unknown error\n"));
        }
        
        result
    }

    fn parse_compiler_errors(&self, stderr: &str, stdout: &str) -> Vec<CompileErrorInfo> {
        let mut errors = Vec::new();
        let combined = format!("{}\n{}", stderr, stdout);
        
        for line in combined.lines() {
            // Parse GCC/Clang format: file:line:col: error: message
            if let Some(info) = self.parse_gcc_style_error(line) {
                errors.push(info);
            }
            // Parse MSVC format: file(line,col): error Cxxxx: message
            else if let Some(info) = self.parse_msvc_style_error(line) {
                errors.push(info);
            }
        }
        
        errors
    }

    fn parse_gcc_style_error(&self, line: &str) -> Option<CompileErrorInfo> {
        // Pattern: file.c:10:5: error: message
        // or: file.c:10: error: message
        let parts: Vec<&str> = line.splitn(4, ':').collect();
        if parts.len() >= 4 {
            let file = parts[0].trim();
            let line_str = parts[1].trim();
            let col_str = parts[2].trim();
            let rest = parts[3].trim();
            
            if let Ok(line) = line_str.parse::<i32>() {
                let col = col_str.parse::<i32>().unwrap_or(0);
                let message = rest.trim_start_matches("error:").trim();
                return Some(CompileErrorInfo {
                    file: file.to_string(),
                    line,
                    col: col - 1, // Convert to 0-based for ErrorFormatter
                    span: 1,
                    message: message.to_string(),
                });
            }
        }
        None
    }

    fn parse_msvc_style_error(&self, line: &str) -> Option<CompileErrorInfo> {
        // Pattern: file.c(10,5): error Cxxxx: message
        let parts: Vec<&str> = line.split('(').collect();
        if parts.len() >= 2 {
            let file = parts[0].trim();
            let rest = parts[1];
            if let Some((line_col, message)) = rest.split_once("):") {
                let line_col_parts: Vec<&str> = line_col.split(',').collect();
                if line_col_parts.len() >= 2 {
                    if let (Ok(line), Ok(col)) = (
                        line_col_parts[0].trim().parse::<i32>(),
                        line_col_parts[1].trim().parse::<i32>(),
                    ) {
                        let msg = message.trim();
                        if msg.contains("error") || msg.contains("Error") {
                            return Some(CompileErrorInfo {
                                file: file.to_string(),
                                line,
                                col: col - 1,
                                span: 1,
                                message: msg.to_string(),
                            });
                        }
                    }
                }
            }
        }
        None
    }

    fn add_compiler_flags(&self, cmd: &mut Command) {
        // Use environment CFLAGS if available
        if let Ok(cflags) = env::var("CFLAGS") {
            for flag in cflags.split_whitespace() {
                cmd.arg(flag);
            }
            return;
        }

        // Default flags
        if self.is_msvc {
            // MSVC flags
            cmd.arg("/O2")      // Optimize for speed
                .arg("/W3")     // Warning level 3
                .arg("/MD")     // Dynamic CRT
                .arg("/EHsc")   // C++ exception handling (also works for C)
                .arg("/nologo") // No copyright banner
                .arg("/FC");    // Full path in diagnostics
        } else {
            // GCC/Clang flags
            cmd.arg("-O2")
                .arg("-Wall")
                .arg("-Wextra")
                .arg("-Wpedantic")
                .arg("-std=c11");
            
            // Position-independent code for Linux
            if cfg!(target_os = "linux") {
                cmd.arg("-fPIC");
            }
            
            // macOS version
            if cfg!(target_os = "macos") {
                cmd.arg("-mmacosx-version-min=10.15");
            }
            
            // MinGW static linking
            if self.is_mingw {
                cmd.arg("-static");
            }
        }
    }

    fn add_platform_libraries(&self, cmd: &mut Command) {
        if self.is_msvc {
            // MSVC: math libs are automatically linked
            return;
        }

        // GCC/Clang/MinGW
        let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        
        // Always link math library on Unix
        if target_os != "windows" {
            cmd.arg("-lm");
        }
        
        // Additional libraries for specific platforms
        if target_os == "linux" {
            // For dynamic linking with glibc
            if !self.is_mingw {
                // No extra flags needed for glibc
            }
        }
        
        if target_os == "macos" {
            // macOS framework
            // cmd.arg("-framework").arg("CoreFoundation");
        }
    }

    /// Check if the compiler is available.
    pub fn is_available(&self) -> bool {
        let output = Command::new(&self.program)
            .arg(if self.is_msvc { "/?" } else { "--version" })
            .output();
        
        matches!(output, Ok(o) if o.status.success())
    }

    /// Returns the compiler program name (for diagnostics).
    pub fn name(&self) -> &str {
        &self.program
    }

    /// Returns compiler type (for diagnostics).
    pub fn compiler_type(&self) -> &'static str {
        if self.is_msvc {
            "MSVC"
        } else if self.is_mingw {
            "MinGW"
        } else if cfg!(target_os = "windows") {
            "Unknown (Windows)"
        } else {
            "GCC/Clang"
        }
    }
}

#[derive(Debug, Clone)]
struct CompileErrorInfo {
    #[allow(dead_code)]
    file: String,
    line: i32,
    col: i32,
    span: usize,
    message: String,
}

fn default_compiler() -> String {
    // Try environment first
    if let Ok(cc) = env::var("CC") {
        return cc;
    }

    // Platform defaults
    if cfg!(target_os = "windows") {
        // Try to detect MSVC, fallback to MinGW
        if is_msvc_available() {
            "cl.exe".to_string()
        } else if is_mingw_available() {
            "gcc".to_string()
        } else {
            "cl.exe".to_string() // Let it fail gracefully
        }
    } else {
        "cc".to_string()
    }
}

fn is_msvc_available() -> bool {
    Command::new("cl.exe")
        .arg("/?")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn is_mingw_available() -> bool {
    Command::new("gcc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Helper to compile and verify output exists
pub fn compile_and_verify(
    compiler: &CCompiler,
    sources: &[impl AsRef<Path>],
    output: &str,
) -> Result<(), CompileError> {
    let status = compiler.compile(sources, output)?;
    
    if !status.success() {
        return Err(CompileError {
            message: format!("Compilation failed with status: {}", status),
            status,
            stderr: String::new(),
            stdout: String::new(),
        });
    }
    
    // Verify the executable was created
    let output_path = Path::new(output);
    if !output_path.exists() {
        return Err(CompileError {
            message: format!("Output file '{}' not created", output),
            status,
            stderr: String::new(),
            stdout: String::new(),
        });
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_detection() {
        let cc = CCompiler::detect();
        println!("Detected compiler: {} ({})", cc.name(), cc.compiler_type());
        assert!(cc.is_available(), "Compiler should be available");
    }

    #[test]
    fn test_compile_simple() {
        use std::fs;
        use std::path::PathBuf;

        let cc = CCompiler::detect();
        if !cc.is_available() {
            println!("Skipping test: compiler not available");
            return;
        }

        // Write a simple C program
        let source = PathBuf::from("test_hello.c");
        fs::write(&source, r#"
            #include <stdio.h>
            int main() { printf("Hello, world!\n"); return 0; }
        "#).unwrap();

        let output = if cfg!(target_os = "windows") {
            "test_hello.exe"
        } else {
            "test_hello"
        };

        // Compile
        let result = compile_and_verify(&cc, &[&source], output);
        assert!(result.is_ok(), "Compilation failed: {:?}", result);

        // Cleanup
        let _ = fs::remove_file(&source);
        let _ = fs::remove_file(output);
    }

    #[test]
    fn test_compile_error_formatting() {
        use std::fs;
        use std::path::PathBuf;

        let cc = CCompiler::detect();
        if !cc.is_available() {
            println!("Skipping test: compiler not available");
            return;
        }

        // Write invalid C code
        let source = PathBuf::from("test_error.c");
        fs::write(&source, r#"
            #include <stdio.h>
            int main() { 
                printf("Hello, world!\n")  // Missing semicolon
                return 0;
            }
        "#).unwrap();

        let output = if cfg!(target_os = "windows") {
            "test_error.exe"
        } else {
            "test_error"
        };

        // Compile (should fail)
        let result = cc.compile(&[&source], output);
        assert!(result.is_err(), "Compilation should have failed");

        if let Err(e) = result {
            println!("Compilation error:\n{}", e.message);
            assert!(!e.message.is_empty());
        }

        // Cleanup
        let _ = fs::remove_file(&source);
        let _ = fs::remove_file(output);
    }
}
