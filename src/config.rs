//! `grape.toml` build configuration.
//!
//! This module models the optional `[build]` and `[optimize]` sections of
//! `grape.toml`:
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
//!
//! [optimize]
//! debug = 0                         # Cranelift opt level for debug builds
//! release = 2                       # Cranelift opt level for release builds
//! ```
//!
//! The structs are shared between the `grape` package manager and the `gobol`
//! compiler driver: `grape` reads them and translates the fields into `gobol`
//! CLI flags (`--target`, `--entry-point`, `--link-script`, `--no-std`,
//! `--no-gc`, `--opt-level`), with CLI arguments overriding the file values.
//!
//! Optimization levels are 0–2 (mapped to Cranelift's `none` / `speed` /
//! `speed_and_size`); any other value in `[optimize]` is an error.

use serde::{Deserialize, Serialize};

/// The `[optimize]` section of `grape.toml`: per-profile Cranelift
/// optimization levels (0–2). All fields are optional.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OptimizeConfig {
    /// Cranelift opt level for debug (unoptimized/`--debug`) builds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debug: Option<usize>,
    /// Cranelift opt level for release (`--release`) builds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release: Option<usize>,
}

impl OptimizeConfig {
    /// Every configured level must be 0–2 (Cranelift only has three real
    /// levels). Returns an error message otherwise.
    pub fn validate(&self) -> Result<(), String> {
        let check = |level: Option<usize>| -> Result<(), String> {
            if let Some(n) = level {
                if n > 2 {
                    return Err(format!(
                        "invalid optimization level {} (must be 0, 1, or 2)",
                        n
                    ));
                }
            }
            Ok(())
        };
        check(self.debug)?;
        check(self.release)?;
        Ok(())
    }
}

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
    /// Effective Cranelift optimization level (0 = none, 1 = speed,
    /// 2 = speed_and_size), after merging CLI `-O`, the `[optimize]` section,
    /// and per-profile defaults.
    pub opt_level: usize,
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
        cli_opt_level: Option<usize>,
        optimize: &Option<OptimizeConfig>,
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

        // Effective opt level: CLI -O wins, then the [optimize] section for
        // the active profile, then the per-profile default (release = 2,
        // debug = 0).
        let opt_level = cli_opt_level
            .or_else(|| {
                optimize
                    .as_ref()
                    .and_then(|o| if release { o.release } else { o.debug })
            })
            .unwrap_or(if release { 2 } else { 0 });

        ResolvedBuild {
            target,
            entry_point,
            no_std,
            no_gc,
            link_script,
            no_main,
            release,
            opt_level,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn resolve_with(
        cli_release: bool,
        cli_opt: Option<usize>,
        opt: Option<OptimizeConfig>,
    ) -> ResolvedBuild {
        BuildConfig::default().resolve(
            None,
            None,
            None,
            cli_release,
            false,
            false,
            cli_opt,
            &opt,
        )
    }

    #[test]
    fn default_levels_debug_zero_release_two() {
        // No [optimize] section, no -O: debug → 0, release → 2.
        assert_eq!(resolve_with(false, None, None).opt_level, 0);
        assert_eq!(resolve_with(true, None, None).opt_level, 2);
    }

    #[test]
    fn optimize_section_per_profile() {
        let opt = Some(OptimizeConfig {
            debug: Some(1),
            release: Some(2),
        });
        // debug build picks optimize.debug, release picks optimize.release.
        assert_eq!(resolve_with(false, None, opt.clone()).opt_level, 1);
        assert_eq!(resolve_with(true, None, opt).opt_level, 2);
    }

    #[test]
    fn cli_o_overrides_optimize_section() {
        let opt = Some(OptimizeConfig {
            debug: Some(1),
            release: Some(2),
        });
        assert_eq!(resolve_with(false, Some(0), opt.clone()).opt_level, 0);
        assert_eq!(resolve_with(true, Some(1), opt).opt_level, 1);
    }

    #[test]
    fn partial_optimize_section_falls_back_to_default() {
        // Only debug configured: release falls back to default 2.
        let opt = Some(OptimizeConfig {
            debug: Some(1),
            release: None,
        });
        assert_eq!(resolve_with(false, None, opt.clone()).opt_level, 1);
        assert_eq!(resolve_with(true, None, opt).opt_level, 2);
    }

    #[test]
    fn validate_rejects_levels_above_two() {
        assert!(OptimizeConfig { debug: Some(4), release: None }.validate().is_err());
        assert!(OptimizeConfig { debug: None, release: Some(3) }.validate().is_err());
        assert!(OptimizeConfig { debug: Some(0), release: Some(2) }.validate().is_ok());
    }
}
