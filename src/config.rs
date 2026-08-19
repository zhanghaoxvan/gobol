//! `grape.toml` build configuration.
//!
//! This module models the optional `[build]` section of `grape.toml`:
//!
//! ```toml
//! [project]
//! name = "my_kernel"
//! version = "0.1.0"
//! entry = "boot.gbl"
//!
//! [build]
//! target = "aarch64-unknown-none"   # omitted => host target
//! entry_point = "_start"            # default "main"; != "main" => no_main
//! no_std = true                     # don't link the C runtime
//! no_gc = true                      # global #[![no_gc]] file attribute
//! link_script = "kernel.ld"         # custom linker script
//! opt_level = "release"             # debug | release | size
//! ```
//!
//! The struct is shared between the `grape` package manager and the `gobol`
//! compiler driver: `grape` reads it and translates the fields into `gobol`
//! CLI flags (`--target`, `--entry-point`, `--link-script`, `--no-std`,
//! `--no-gc`), with CLI arguments overriding the file values.

use serde::{Deserialize, Serialize};

/// The `[build]` section of `grape.toml`. All fields are optional so a plain
/// `[project]`-only manifest keeps working unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Target triple (e.g. `x86_64-pc-windows-msvc`, `aarch64-unknown-none`).
    /// When `None`, the host target is auto-detected via `target_lexicon::HOST`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Entry-point symbol name. Defaults to `"main"`. When set to anything
    /// else (e.g. `"_start"`), the compiler does NOT require a `main` function
    /// and the linker is told to use this symbol as the entry point — the
    /// implicit `no_main` behaviour for kernel / bare-metal builds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_point: Option<String>,
    /// Don't link the C runtime. Implied for bare-metal triples.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_std: Option<bool>,
    /// Globally disable GC by injecting a synthetic `#![no_gc]` file
    /// attribute into the compiled source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_gc: Option<bool>,
    /// Path to a custom linker script (bare-metal / kernel builds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_script: Option<String>,
    /// Optimisation level: `"debug"` (default), `"release"`, or `"size"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opt_level: Option<String>,
}

/// Effective build settings after merging the `[build]` section with CLI
/// overrides. CLI flags always win over the manifest.
#[derive(Debug, Clone, Default)]
pub struct ResolvedBuild {
    /// Resolved target triple (manifest value, or host if unset). CLI
    /// `--target` overrides this when merged.
    pub target: Option<String>,
    /// Resolved entry point (manifest value, or `"main"` default).
    pub entry_point: Option<String>,
    pub no_std: bool,
    pub no_gc: bool,
    pub link_script: Option<String>,
    /// `true` when a custom non-main entry point is in effect — the
    /// compiler should not require a `main` function.
    pub no_main: bool,
    pub release: bool,
}

impl BuildConfig {
    /// Resolve this `[build]` section against CLI overrides. `cli_target` and
    /// `cli_entry_point` come from `gobol`/`grape` flags and take precedence
    /// over the manifest. `cli_release`/`cli_no_std`/`cli_no_gc` likewise
    /// override the manifest when set.
    pub fn resolve(
        &self,
        cli_target: Option<&str>,
        cli_entry_point: Option<&str>,
        cli_link_script: Option<&str>,
        cli_release: bool,
        cli_no_std: bool,
        cli_no_gc: bool,
    ) -> ResolvedBuild {
        let target = cli_target
            .map(|s| s.to_string())
            .or_else(|| self.target.clone());
        let entry_point = cli_entry_point
            .map(|s| s.to_string())
            .or_else(|| self.entry_point.clone());
        let link_script = cli_link_script
            .map(|s| s.to_string())
            .or_else(|| self.link_script.clone());

        let no_std = cli_no_std
            || self.no_std.unwrap_or(false)
            || target
                .as_deref()
                .map(crate::cranelift::target_is_bare_metal)
                .unwrap_or(false);
        let no_gc = cli_no_gc || self.no_gc.unwrap_or(false);

        // entry_point != "main" => implicit no_main.
        let no_main = entry_point
            .as_deref()
            .map(|e| e != "main")
            .unwrap_or(false);

        let release = cli_release
            || matches!(self.opt_level.as_deref(), Some("release") | Some("size"));

        ResolvedBuild {
            target,
            entry_point,
            no_std,
            no_gc,
            link_script,
            no_main,
            release,
        }
    }
}

impl ResolvedBuild {
    /// Convenience: the effective target triple, falling back to the host.
    pub fn target_or_host(&self) -> String {
        self.target
            .clone()
            .unwrap_or_else(crate::cranelift::host_target_string)
    }
}
