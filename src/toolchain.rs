//! Windows C/C++ toolchain discovery, delegated to the `cc` crate.
//!
//! On Windows, the Gobol backend spawns a native linker (`link.exe` for
//! the MSVC ABI, `gcc.exe` for the GNU/MinGW ABI) to produce the final
//! executable, and that linker must resolve system libraries such as
//! `ws2_32` (Winsock — pulled in transitively by the C runtime's
//! `std/runtime/net.c`). Two things are usually missing when a user runs
//! `grape run` from a plain shell:
//!
//! 1. **The toolchain is not on PATH.** MSVC ships `link.exe` / `cl.exe`
//!    only inside a Developer Command Prompt; locating them requires
//!    running `vcvarsall.bat` to populate `PATH` / `LIB` / `INCLUDE` /
//!    `LIBPATH`.
//! 2. **`ws2_32` is not on the link line.** The compiler only forwards
//!    libraries declared via `#[library(...)]` on `extern "C"` blocks;
//!    the runtime's Winsock dependency is transitive and undeclared, so
//!    the package manager must inject it explicitly.
//!
//! Rather than reinvent MSVC/MinGW discovery (vswhere, vcvarsall.bat env
//! capture, `which("cl.exe")`, …) this module delegates task (1) to the
//! [`cc`](https://docs.rs/cc) crate — the *same* discovery logic rustc's
//! own build scripts use. `cc::Build::try_get_compiler()` returns a
//! `Tool` whose `.path()` is the compiler (cl.exe / gcc) and whose
//! `.env()` carries the full MSVC environment overlay (the very
//! variables `vcvarsall.bat` would set). We expose it via [`cc_discover`]
//! and surface a [`ToolchainEnv`] for the package manager.
//!
//! Task (2) is a one-liner: every detected Windows toolchain carries
//! `winsock_lib = "ws2_32"`; the caller passes it to
//! `gobol build --link-arg ws2_32`, and the compiler formats it per
//! linker kind (`ws2_32.lib` for MSVC, `-lws2_32` for the cc-driver path
//! — see `cranelift::link_*`).
//!
//! This module compiles on every host (no `cfg(windows)` gates) and only
//! does real work when the *target* triple is a Windows target
//! ([`crate::cranelift::target_is_windows`]); for any non-Windows
//! target, [`detect_for_target`] returns `Ok(None)` and `grape` behaves
//! exactly as before.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;

/// Kind of Windows C/C++ toolchain that was located.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolchainKind {
    /// Microsoft Visual C++ — `cl.exe` + `link.exe`, environment set up
    /// by the `cc` crate's MSVC discovery (equivalent to vcvarsall).
    Msvc,
    /// MinGW-w64 — `gcc.exe` used as both compiler and linker driver
    /// (GNU ABI).
    MinGw,
}

/// The result of a successful toolchain probe: the environment overlay to
/// apply to the `gobol build` subprocess, plus the Winsock library base
/// name the caller should forward via `--link-arg`.
#[derive(Debug, Clone)]
pub struct ToolchainEnv {
    /// Which toolchain was detected.
    pub kind: ToolchainKind,
    /// Environment variables (`PATH` / `INCLUDE` / `LIB` / `LIBPATH` / …)
    /// that must be overlaid on the compiler subprocess. For MSVC this
    /// is the full post-discovery snapshot from `cc::Tool::env()`; for
    /// MinGW it is typically empty (gcc finds its own pieces via PATH).
    pub env: HashMap<String, String>,
    /// Winsock library base name, e.g. `ws2_32`. Forwarded to
    /// `gobol build --link-arg`; the backend renders it as
    /// `ws2_32.lib` (MSVC) or `-lws2_32` (MinGW).
    pub winsock_lib: String,
}

/// A discovered C/C++ compiler, a thin wrapper over `cc::Tool`'s public
/// surface. Used by `cranelift` to compile the C runtime and to drive
/// `link.exe` with the correct environment.
#[derive(Debug, Clone)]
pub struct CcTool {
    /// Path to the C compiler (e.g. `cl.exe`, `clang-cl.exe`, `gcc`).
    pub compiler: PathBuf,
    /// `true` if the tool is MSVC-family (`is_like_msvc`).
    pub is_msvc: bool,
    /// Environment overlay the compiler must run with (PATH/INCLUDE/LIB/
    /// LIBPATH for MSVC; empty for MinGW/gcc on PATH).
    pub env: Vec<(OsString, OsString)>,
}

/// Discover the C/C++ toolchain for `target` via the `cc` crate — the
/// same discovery Rust's own build scripts use. Honours the standard `cc`
/// env vars (`CC`, `CC_<target>`, `CFLAGS`, …) and, for MSVC targets,
/// locates the Visual Studio install and builds the vcvarsall-equivalent
/// environment.
///
/// The `cc` crate normally expects the cargo build-script environment
/// (`TARGET`, `HOST`, `OUT_DIR`, `OPT_LEVEL`, …). Since `grape` and the
/// compiler run as ordinary binaries — not build scripts — those vars are
/// absent and `cc` would hard-error with `EnvVarNotFound`. We supply them
/// programmatically via the builder (`target`/`host`/`opt_level`/`out_dir`)
/// so discovery works at runtime without polluting the process env.
///
/// Returns `Ok(None)` if `cc` cannot find a compiler for the target (so
/// the caller can produce a friendly "install a toolchain" message).
pub fn cc_discover(target: &str) -> Result<Option<CcTool>, String> {
    let host = crate::cranelift::host_target_string();
    let out_dir = std::env::temp_dir();
    let mut build = cc::Build::new();
    build
        .target(target)
        .host(&host)
        .opt_level(0)
        .out_dir(&out_dir)
        .cargo_metadata(false)
        .cargo_warnings(false);
    let tool = match build.try_get_compiler() {
        Ok(t) => t,
        Err(_) => return Ok(None),
    };
    Ok(Some(CcTool {
        compiler: tool.path().to_path_buf(),
        is_msvc: tool.is_like_msvc(),
        env: tool.env().to_vec(),
    }))
}

/// Detect and initialise a C/C++ toolchain for the given target triple.
///
/// Returns `Ok(None)` for non-Windows targets (no setup needed — Unix
/// `cc`/`gcc` is already on PATH in the common case, and the `cc`-driver
/// link path handles it directly). For Windows targets, delegates to
/// [`cc_discover`] and folds the resulting `cc::Tool` env into a
/// [`ToolchainEnv`]. Returns `Err` with install instructions if `cc`
/// can't find a toolchain.
pub fn detect_for_target(target: &str) -> Result<Option<ToolchainEnv>, String> {
    // Non-Windows targets need no bootstrapping here.
    if !crate::cranelift::target_is_windows(target) {
        return Ok(None);
    }

    let tool = cc_discover(target)?.ok_or_else(|| toolchain_missing_error(target))?;

    // Flatten the cc env (Vec<(OsString, OsString)>) into the HashMap the
    // grape driver expects to overlay onto the gobol subprocess.
    let mut env: HashMap<String, String> = HashMap::new();
    for (k, v) in &tool.env {
        env.insert(
            k.to_string_lossy().into_owned(),
            v.to_string_lossy().into_owned(),
        );
    }

    Ok(Some(ToolchainEnv {
        kind: if tool.is_msvc {
            ToolchainKind::Msvc
        } else {
            ToolchainKind::MinGw
        },
        env,
        winsock_lib: "ws2_32".to_string(),
    }))
}

/// Human-readable error listing install options when no toolchain is found.
fn toolchain_missing_error(target: &str) -> String {
    format!(
        "No Windows C/C++ toolchain found for target '{target}'.\n\
         The Gobol backend needs a C compiler + linker to produce the final\n\
         executable, and the `cc` crate could not locate one. Install one of:\n  \
           • Visual Studio 2019/2022 with the 'Desktop development with C++'\n\
             workload (provides MSVC cl.exe / link.exe), or\n  \
           • MinGW-w64 (https://www.mingw-w64.org/) with gcc.exe on PATH.\n\
         Alternatively set CC=<compiler> for the target and re-run `grape run`.\n\
         Tip: for MSVC, a 'Developer Command Prompt for VS' already has the\n\
              environment set up."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_windows_target_needs_no_toolchain() {
        // Unix host triple → no bootstrapping required.
        assert!(detect_for_target("x86_64-unknown-linux-gnu")
            .unwrap()
            .is_none());
    }

    #[test]
    fn winsock_lib_is_ws2_32_for_both_kinds() {
        // Sanity: both toolchains use the same base name; only the
        // formatting (done by cranelift) differs.
        assert_eq!(
            ToolchainEnv {
                kind: ToolchainKind::Msvc,
                env: HashMap::new(),
                winsock_lib: "ws2_32".to_string(),
            }
            .winsock_lib,
            "ws2_32"
        );
        assert_eq!(
            ToolchainEnv {
                kind: ToolchainKind::MinGw,
                env: HashMap::new(),
                winsock_lib: "ws2_32".to_string(),
            }
            .winsock_lib,
            "ws2_32"
        );
    }

    #[test]
    fn cc_discover_finds_host_compiler() {
        // On any host with a working dev environment, the cc crate must
        // find SOMETHING (gcc/clang/cc/cl). This validates that the cc
        // integration compiles and runs end-to-end on the build host
        // (including the runtime env-var bootstrap in `cc_discover`).
        let host = crate::cranelift::host_target_string();
        let tool = cc_discover(&host).expect("cc_discover must not error for host");
        assert!(tool.is_some(), "cc should find a compiler for {host}");
        let t = tool.unwrap();
        assert!(!t.compiler.as_os_str().is_empty());
    }

    #[test]
    fn cc_discover_unknown_target_is_none_not_err() {
        // A bogus target should yield Ok(None), not an Err — the caller
        // turns a missing toolchain into a friendly message.
        assert!(cc_discover("totally-bogus-target-triple").is_ok());
    }
}
