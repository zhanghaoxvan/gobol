use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use colored::*;
use git2::{Repository, ResetType};

// ============ 常量 ============

const GRAPE_TOML: &str = "grape.toml";
const GRAPE_LOCK: &str = "grape.lock";
const GRAPE_ERR: &str = "grape.err";
const LIB_DIR: &str = "lib";

// ============ 错误处理 ============

#[derive(Debug)]
pub enum GrapeError {
    Io(io::Error),
    Toml(String),
    Git(git2::Error),
    NotFound(String),
    AlreadyExists(String),
    InvalidDependency(String),
    CommandFailed(String),
    NetworkError { dep: String, reason: String },
}

impl std::fmt::Display for GrapeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrapeError::Io(e) => write!(f, "IO error: {}", e),
            GrapeError::Toml(e) => write!(f, "TOML error: {}", e),
            GrapeError::Git(e) => write!(f, "Git error: {}", e),
            GrapeError::NotFound(s) => write!(f, "Not found: {}", s),
            GrapeError::AlreadyExists(s) => write!(f, "Already exists: {}", s),
            GrapeError::InvalidDependency(s) => write!(f, "Invalid dependency: {}", s),
            GrapeError::CommandFailed(s) => write!(f, "Command failed: {}", s),
            GrapeError::NetworkError { dep, reason } => {
                write!(f, "Network error for '{}': {}", dep, reason)
            }
        }
    }
}

impl std::error::Error for GrapeError {}

type Result<T> = std::result::Result<T, GrapeError>;

// ============ 数据结构 ============

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GrapeToml {
    pub project: Project,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    /// Optional `[build]` section: target triple, entry point, no_std,
    /// no_gc, link script, opt level. Absent for plain hosted apps.
    #[serde(default)]
    pub build: Option<gobol::config::BuildConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub entry: String,
    pub authors: Option<Vec<String>>,
    pub description: Option<String>,
    pub license: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DependencySpec {
    pub repo: String,
    pub tag: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub optional: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GrapeLock {
    pub version: u32,
    pub packages: HashMap<String, LockedPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LockedPackage {
    pub repo: String,
    pub tag: String,
    pub commit: String,
}

// ============ 路径工具 ============

/// Base directory for cached packages and build artifacts.
///
/// Like Cargo, all compilation-local data lives under the single project
/// root directory `target/`:
///   * build artifacts (executables, intermediate `.o`/`.obj`) —
///     `target/{triple}/{debug|release}/`
///   * cached dependency packages — `target/grape/packages/<name>`
fn target_dir() -> PathBuf {
    PathBuf::from("target")
}

/// Base directory for cached packages: `target/grape/packages/`
fn packages_dir() -> PathBuf {
    target_dir().join("grape").join("packages")
}

/// Project library directory: `lib/`
fn lib_dir() -> PathBuf {
    PathBuf::from(LIB_DIR)
}

impl DependencySpec {
    fn git_url(&self) -> String {
        format!("https://github.com/{}.git", self.repo)
    }

    fn local_name(&self) -> String {
        self.repo
            .split('/')
            .last()
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.repo.clone())
    }

    /// Cross-platform path to the cached package: `target/grape/packages/<name>`
    fn local_path(&self) -> PathBuf {
        packages_dir().join(self.local_name())
    }

    /// Cross-platform path to the materialised lib copy: `lib/<name>`
    fn lib_material_path(&self) -> PathBuf {
        lib_dir().join(self.local_name())
    }
}

// ============ 错误日志 ============

/// Append a failed dependency to `grape.err`.
fn log_failed_dep(name: &str, reason: &str) {
    let entry = format!("[{}] {}: {}\n",
        chrono_or_now(),
        name,
        reason,
    );
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(GRAPE_ERR)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
}

fn chrono_or_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_else(|_| "0".to_string())
}

/// Print previously failed dependencies, then remove the error log.
fn report_failed_deps() {
    if !Path::new(GRAPE_ERR).exists() {
        return;
    }
    if let Ok(contents) = fs::read_to_string(GRAPE_ERR) {
        if !contents.trim().is_empty() {
            eprintln!(
                "{}",
                "The following dependencies failed to resolve:".red().bold()
            );
            for line in contents.lines() {
                eprintln!("  {}", line.red());
            }
        }
    }
    let _ = fs::remove_file(GRAPE_ERR);
}

// ============ 主函数 ============

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    if let Err(e) = run_command(&args) {
        eprintln!("{} {}", "Error:".red().bold(), e);
        eprintln!();
        report_failed_deps();
        std::process::exit(1);
    }

    report_failed_deps();
}

fn run_command(args: &[String]) -> Result<()> {
    match args[1].as_str() {
        "init" => cmd_init(),
        "add" => cmd_add(&args[2..]),
        "remove" => cmd_remove(&args[2..]),
        "update" => cmd_update(&args[2..]),
        "list" => cmd_list(),
        "run" => cmd_run(&args[2..]),
        "clean" => cmd_clean(),
        "build" => cmd_build(&args[2..]),
        "help" | "--help" => {
            print_help();
            Ok(())
        }
        "version" | "--version" => {
            println!("Grape version: 0.2.0, binding with Gobol 0.2.0");
            Ok(())
        }
        _ => Err(GrapeError::NotFound(format!("Unknown command: {}", args[1]))),
    }
}

fn print_help() {
    println!("Grape - Package Manager for Gobol");
    println!();
    println!("Usage:");
    println!("  grape init                  Initialize a new Gobol project");
    println!("  grape add <dep>             Add a dependency (format: user/repo@tag)");
    println!("  grape add <dep> --optional  Add as optional dependency");
    println!("  grape remove <name>         Remove a dependency");
    println!("  grape update [name]         Update dependencies (use --latest for newest tag)");
    println!("  grape list                  List all dependencies");
    println!("  grape run [--verbose]       Build and run the Gobol program");
    println!("  grape build [-o <file>] [--release]  Compile to native binary");
    println!("  grape clean                 Clean build artifacts and cached packages");
    println!("  grape version               Show the version");
    println!("  grape help                  Show this help message");
    println!();
    println!("Examples:");
    println!("  grape add gobol-org/math@0.1.0");
    println!("  grape add gobol-org/test@0.2.0 --optional");
    println!("  grape remove math");
    println!("  grape update --latest");
    println!("  grape build --release -o myapp");
}

// ============ 命令实现 ============

fn cmd_init() -> Result<()> {
    if Path::new(GRAPE_TOML).exists() {
        return Err(GrapeError::AlreadyExists(
            "grape.toml already exists".to_string(),
        ));
    }

    println!("Initializing new Gobol project...");

    let current_dir = std::env::current_dir().map_err(GrapeError::Io)?;
    let project_name = current_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    print!("Author name (optional): ");
    io::stdout().flush().unwrap();
    let mut author = String::new();
    io::stdin().read_line(&mut author).ok();
    let author = author.trim();

    let authors = if author.is_empty() {
        None
    } else {
        Some(vec![author.to_string()])
    };

    let config = GrapeToml {
        project: Project {
            name: project_name,
            version: "0.1.0".to_string(),
            entry: "main.gbl".to_string(),
            authors,
            description: None,
            license: None,
        },
        dependencies: HashMap::new(),
        build: None,
    };

    let toml_str =
        toml::to_string_pretty(&config).map_err(|e| GrapeError::Toml(e.to_string()))?;
    fs::write(GRAPE_TOML, toml_str).map_err(GrapeError::Io)?;
    fs::create_dir_all(packages_dir()).map_err(GrapeError::Io)?;
    fs::create_dir_all(lib_dir()).map_err(GrapeError::Io)?;

    println!(" Project initialized successfully");

    if !Path::new("main.gbl").exists() {
        let content = r#"import std;

func main() {
    io::println("Hello, World!");
}
"#;
        fs::write("main.gbl", content).map_err(GrapeError::Io)?;
        println!("✓ Created example entry file: main.gbl");
    }

    println!("\nNext steps:");
    println!("  1. Edit grape.toml to configure your project");
    println!("  2. Add your own gobol modules in lib/ (e.g. lib/utils.gbl)");
    println!("  3. Add dependencies: grape add user/repo@tag");
    println!("  4. Run your program: grape run");

    Ok(())
}

fn cmd_add(deps: &[String]) -> Result<()> {
    if deps.is_empty() {
        return Err(GrapeError::InvalidDependency(
            "No dependency specified. Usage: grape add <user/repo@tag>".to_string(),
        ));
    }

    let mut config = read_grape_toml()?;
    let is_optional = deps.iter().any(|a| a == "--optional");
    let deps: Vec<&String> = deps.iter().filter(|d| !d.starts_with('-')).collect();

    if deps.is_empty() {
        return Err(GrapeError::InvalidDependency(
            "No dependency specified. Usage: grape add <user/repo@tag>".to_string(),
        ));
    }

    for dep in deps {
        println!(" Adding dependency: {}", dep);

        let (repo, tag) = dep.split_once('@').ok_or_else(|| {
            GrapeError::InvalidDependency(format!(
                "Invalid format '{}'. Expected: user/repo@tag",
                dep
            ))
        })?;

        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() != 2 {
            return Err(GrapeError::InvalidDependency(format!(
                "Invalid repo format '{}'. Expected: user/repo",
                repo
            )));
        }

        let var_name = parts[1].to_string();

        if config.dependencies.contains_key(&var_name) {
            return Err(GrapeError::AlreadyExists(format!(
                "Dependency '{}' already exists. Use 'grape update {}' to update",
                var_name, var_name
            )));
        }

        // Validate the tag exists on the remote before attempting download.
        let spec = DependencySpec {
            repo: repo.to_string(),
            tag: tag.to_string(),
            optional: if is_optional { Some(true) } else { None },
        };

        if !tag_exists_remote(&spec) {
            return Err(GrapeError::NetworkError {
                dep: var_name.clone(),
                reason: format!("Tag '{}' not found on remote {}", tag, spec.git_url()),
            });
        }

        println!("   Downloading from {}", spec.git_url());
        match download_package(&spec) {
            Ok(()) => {
                println!("   Downloaded to {}", spec.local_path().display());
            }
            Err(e) => {
                let msg = format!("{}", e);
                eprintln!("  {} {}", " Failed:".red(), msg.red());
                log_failed_dep(&var_name, &msg);
                continue;
            }
        }

        config.dependencies.insert(var_name.clone(), spec);
        println!("   Added dependency: {}", var_name);
    }

    save_grape_toml(&config)?;
    update_lock_file(&config)?;

    println!(" Dependencies processed");
    Ok(())
}

fn cmd_remove(deps: &[String]) -> Result<()> {
    if deps.is_empty() {
        return Err(GrapeError::InvalidDependency(
            "No dependency specified. Usage: grape remove <name>".to_string(),
        ));
    }

    let mut config = read_grape_toml()?;

    for dep in deps {
        if config.dependencies.remove(dep).is_some() {
            println!(" Removed dependency: {}", dep);

            let local_path = packages_dir().join(dep);
            if local_path.exists() {
                fs::remove_dir_all(&local_path).map_err(GrapeError::Io)?;
                println!("  Removed local files: {}", local_path.display());
            }

            let lib_path = lib_dir().join(dep);
            if lib_path.exists() {
                fs::remove_dir_all(&lib_path).map_err(GrapeError::Io)?;
            }
        } else {
            println!(" Dependency not found: {}", dep);
        }
    }

    save_grape_toml(&config)?;
    update_lock_file(&config)?;

    println!(" Dependencies removed successfully");
    Ok(())
}

fn cmd_update(args: &[String]) -> Result<()> {
    let mut config = read_grape_toml()?;
    let use_latest = args.iter().any(|a| a == "--latest");
    let specific_dep = args.iter().find(|a| !a.starts_with('-'));

    if use_latest {
        println!(" Fetching latest tags from GitHub...");
    } else {
        println!(" Updating dependencies...");
    }

    let deps_to_update: Vec<(String, DependencySpec)> = if let Some(name) = specific_dep {
        if let Some(spec) = config.dependencies.get(name).cloned() {
            vec![(name.clone(), spec)]
        } else {
            return Err(GrapeError::NotFound(format!(
                "Dependency '{}' not found",
                name
            )));
        }
    } else {
        config.dependencies.clone().into_iter().collect()
    };

    let mut any_change = false;

    for (name, mut spec) in deps_to_update {
        if use_latest {
            match fetch_latest_tag(&spec.repo) {
                Ok(latest_tag) if latest_tag != spec.tag => {
                    println!(
                        "  {} {}: {} → {}",
                        name, "upgrading".yellow(),
                        spec.tag, latest_tag.green()
                    );
                    spec.tag = latest_tag;
                    config.dependencies.insert(name.clone(), spec.clone());
                    any_change = true;
                }
                Ok(_) => {
                    println!("  {} is already at the latest version ({})", name, spec.tag);
                }
                Err(e) => {
                    eprintln!(
                        "  Could not fetch latest tag for {}: {}",
                        name,
                        e
                    );
                }
            }
        }

        println!("  Updating {}@{}", name, spec.tag);

        let local_path = spec.local_path();
        if local_path.exists() {
            fs::remove_dir_all(&local_path).map_err(GrapeError::Io)?;
        }

        match download_package(&spec) {
            Ok(()) => println!("   Updated {}", name),
            Err(e) => {
                let msg = format!("{}", e);
                eprintln!("  {} {}", " Failed:".red(), msg.red());
                log_failed_dep(&name, &msg);
            }
        }
    }

    if any_change || use_latest {
        save_grape_toml(&config)?;
    }
    update_lock_file(&config)?;

    if any_change {
        println!(" grape.toml updated with latest versions. Please review the changes.");
    }
    println!(" Dependencies updated successfully");
    Ok(())
}

fn cmd_list() -> Result<()> {
    let config = read_grape_toml()?;

    if config.dependencies.is_empty() {
        println!("No dependencies found.");
        println!("Add one with: grape add user/repo@tag");
        return Ok(());
    }

    println!("Dependencies:");
    for (name, spec) in &config.dependencies {
        let optional = if spec.optional.unwrap_or(false) {
            " (optional)"
        } else {
            ""
        };
        println!("     {} = {}{}", name, spec.repo, optional);
        println!("       tag: {}", spec.tag);

        let local_path = spec.local_path();
        if local_path.exists() {
            println!("        Downloaded");
        } else {
            println!("        Not downloaded (run 'grape update' to download)");
        }
    }

    Ok(())
}

// ============ cmd_run — build + optionally execute ============

fn cmd_run(args: &[String]) -> Result<()> {
    println!("{}", " Running Gobol program...".bold().green());
    // run => compile + execute.
    build_project(args, false)
}

// ============ cmd_build — independent build command ============

fn cmd_build(args: &[String]) -> Result<()> {
    println!("{}", " Building Gobol project...".bold().green());
    // build only, never execute.
    build_project(args, true)
}

/// Build options parsed from the `grape run`/`grape build` CLI, BEFORE merging
/// with the `[build]` section of `grape.toml`. `program_args` captures the
/// tokens after a `--` separator (passed to the target program on run).
#[derive(Default)]
struct BuildCli {
    verbose: bool,
    no_check: bool,
    release: bool,
    out: Option<String>,
    target: Option<String>,
    entry_point: Option<String>,
    link_script: Option<String>,
    no_std: bool,
    no_gc: bool,
    no_main: bool,
    program_args: Vec<String>,
}

fn parse_build_cli(args: &[String]) -> BuildCli {
    let mut cli = BuildCli::default();
    let mut i = 0;
    let mut after_dd = false; // past `--`
    while i < args.len() {
        let a = &args[i];
        if after_dd {
            cli.program_args.push(a.clone());
            i += 1;
            continue;
        }
        match a.as_str() {
            "--" => after_dd = true,
            "--verbose" | "-v" => cli.verbose = true,
            "--no-check" => cli.no_check = true,
            "--release" => cli.release = true,
            "--debug" => cli.release = false,
            "-o" | "--output" => {
                if let Some(v) = args.get(i + 1) {
                    cli.out = Some(v.clone());
                    i += 1;
                }
            }
            "--target" => {
                if let Some(v) = args.get(i + 1) {
                    cli.target = Some(v.clone());
                    i += 1;
                }
            }
            "--entry-point" => {
                if let Some(v) = args.get(i + 1) {
                    cli.entry_point = Some(v.clone());
                    i += 1;
                }
            }
            "--link-script" => {
                if let Some(v) = args.get(i + 1) {
                    cli.link_script = Some(v.clone());
                    i += 1;
                }
            }
            "--no-std" => cli.no_std = true,
            "--no-gc" => cli.no_gc = true,
            "--no-main" => cli.no_main = true,
            // `grape run --release` etc. handled above; unknown flags are
            // silently ignored here (gobol may know about them later).
            _ => {}
        }
        i += 1;
    }
    cli
}

/// Shared build logic used by both `grape run` and `grape build`.
///
/// * `compile_only`: true for `grape build` (skip execution).
///
/// Merges the `[build]` section of `grape.toml` with CLI overrides (CLI wins),
/// translates the result into `gobol` flags, builds, and — for `grape run` —
/// executes the result (directly on the host, or via QEMU for cross targets).
fn build_project(args: &[String], compile_only: bool) -> Result<()> {
    if !Path::new(GRAPE_TOML).exists() {
        return Err(GrapeError::NotFound(
            "grape.toml not found. Run 'grape init' first.".to_string(),
        ));
    }

    let config = read_grape_toml()?;
    let cli = parse_build_cli(args);

    // Merge the manifest's optional [build] section with CLI overrides.
    let build_cfg = config.build.clone().unwrap_or_default();
    let resolved = build_cfg.resolve(
        cli.target.as_deref(),
        cli.entry_point.as_deref(),
        cli.link_script.as_deref(),
        cli.release,
        cli.no_std,
        cli.no_gc,
    );

    let out_name = cli
        .out
        .clone()
        .unwrap_or_else(|| config.project.name.clone());

    // Lock file management
    if !cli.no_check && !Path::new(GRAPE_LOCK).exists() {
        if cli.verbose {
            println!("grape.lock not found, generating...");
        }
        update_lock_file(&config)?;
    } else if !cli.no_check {
        verify_lock_file(&config)?;
    }

    let entry_file = &config.project.entry;
    if !Path::new(entry_file).exists() {
        return Err(GrapeError::NotFound(format!(
            "Entry file '{}' not found.",
            entry_file
        )));
    }

    // Resolve dependencies into lib/<name>/
    let mut resolved_deps: HashSet<String> = HashSet::new();
    resolve_dependencies(&config, &mut resolved_deps, cli.verbose)?;

    let mut lib_paths = build_lib_paths(&config);

    // Always include the std library path so that `import std` resolves
    // correctly regardless of where the project lives.
    if let Some(std_path) = find_std_path() {
        if !lib_paths.contains(&std_path) {
            lib_paths.push(std_path);
        }
    }

    if cli.verbose {
        println!("Project: {}", config.project.name);
        println!("Entry: {}", entry_file);
        println!("Output: {}", out_name);
        println!("Target: {}", resolved.target_or_host());
        println!("Lib paths: {:?}", lib_paths);
    }

    // Build the final executable name. Output path spec:
    //   target/{target_triple}/{debug|release}/{project_name}
    // The plain `-o <name>` path is kept when the user passed it explicitly;
    // otherwise we emit into the canonical target dir so cross builds don't
    // clobber host builds of the same project.
    let profile = if resolved.release { "release" } else { "debug" };
    let target_triple = resolved.target_or_host();
    let target_dir = Path::new("target").join(&target_triple).join(profile);
    let exe_name = gobol::cranelift::ensure_exe_extension(
        &target_triple,
        &out_name,
    );
    let final_out: PathBuf = if cli.out.is_some() {
        // User asked for a specific path; honour it (still add .exe on Windows).
        PathBuf::from(gobol::cranelift::ensure_exe_extension(
            &target_triple,
            &out_name,
        ))
    } else {
        fs::create_dir_all(&target_dir).map_err(GrapeError::Io)?;
        target_dir.join(&exe_name)
    };

    // ===== Windows toolchain auto-adaptation =====
    // Before spawning `gobol build`, make sure a C/C++ toolchain is
    // reachable for Windows targets: the backend links with `link.exe`
    // (MSVC) or `gcc.exe` (MinGW), which are usually NOT on PATH in a
    // plain shell. This locates MSVC via vswhere + vcvarsall.bat (or
    // MinGW via gcc.exe) and returns the env overlay to apply below. It
    // is a no-op for non-Windows targets, so Unix builds are untouched.
    let toolchain = gobol::toolchain::detect_for_target(&target_triple)
        .map_err(GrapeError::CommandFailed)?;
    if let Some(tc) = &toolchain {
        if cli.verbose {
            println!(
                "Toolchain: {:?} (extra link arg: {})",
                tc.kind, tc.winsock_lib
            );
        }
    }

    // Invoke gobol, translating the resolved build options into flags.
    let mut cmd = process::Command::new("gobol");
    cmd.arg("build")
        .arg(entry_file)
        .arg("-o")
        .arg(final_out.to_string_lossy().as_ref());
    if resolved.release {
        cmd.arg("--release");
    } else {
        cmd.arg("--debug");
    }
    if let Some(t) = &resolved.target {
        cmd.arg("--target").arg(t);
    }
    if let Some(ep) = &resolved.entry_point {
        cmd.arg("--entry-point").arg(ep);
    }
    if let Some(ls) = &resolved.link_script {
        cmd.arg("--link-script").arg(ls);
    }
    if resolved.no_std {
        cmd.arg("--no-std");
    }
    if resolved.no_gc {
        cmd.arg("--no-gc");
    }
    if resolved.no_main {
        cmd.arg("--no-main");
    }
    for path in &lib_paths {
        cmd.arg("--lib-path").arg(path);
    }
    // Inject the Winsock library (ws2_32) that the C runtime pulls in
    // transitively via std/runtime/net.c. The compiler formats the base
    // name per linker kind: `ws2_32.lib` for MSVC, `-lws2_32` for MinGW.
    // Only Windows targets need it; toolchain is None on Unix.
    if let Some(tc) = &toolchain {
        cmd.arg("--link-arg").arg(&tc.winsock_lib);
    }
    if cli.verbose {
        cmd.arg("--verbose");
    }

    // Replay the toolchain env (PATH/INCLUDE/LIB/LIBPATH for MSVC, or the
    // MinGW-augmented PATH) onto the gobol subprocess. gobol inherits the
    // env by default; `Command::env` overlays individual vars on top.
    if let Some(tc) = &toolchain {
        for (k, v) in &tc.env {
            cmd.env(k, v);
        }
    }

    let status = cmd.status().map_err(|_| {
        GrapeError::CommandFailed(
            "Failed to run gobol. Make sure gobol is installed.".to_string(),
        )
    })?;

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    // Execute the compiled binary unless compile-only.
    if !compile_only {
        run_compiled_binary(&final_out, &target_triple, &cli.program_args, cli.verbose)?;
    }

    Ok(())
}

/// Run a freshly built binary. On the host target it is executed directly;
/// for cross-compiled targets an appropriate QEMU is invoked automatically
/// (`qemu-<arch>` for Linux user targets, `qemu-system-<arch> -kernel` for
/// bare-metal `*-unknown-none` targets).
fn run_compiled_binary(
    exe: &Path,
    target: &str,
    program_args: &[String],
    verbose: bool,
) -> Result<()> {
    let host = gobol::cranelift::host_target_string();
    if target == host {
        let run_status = process::Command::new(exe)
            .args(program_args)
            .status()
            .map_err(|e| {
                GrapeError::CommandFailed(format!("Failed to execute '{}': {}", exe.display(), e))
            })?;
        if !run_status.success() {
            process::exit(run_status.code().unwrap_or(1));
        }
        return Ok(());
    }

    // Cross target — choose a QEMU.
    let arch = target.split('-').next().unwrap_or("");
    let is_bare = gobol::cranelift::target_is_bare_metal(target);
    let qemu = if is_bare {
        format!("qemu-system-{}", arch)
    } else if target.contains("linux") {
        // qemu-user names: qemu-aarch64, qemu-arm, qemu-riscv64, qemu-x86_64
        format!("qemu-{}", arch)
    } else {
        // Non-Linux cross target (e.g. windows-msvc from linux) can't be run
        // directly — report clearly.
        eprintln!(
            "{}",
            format!(
                "Cannot run cross-compiled binary for target '{}' on host. \
                 Built at: {}",
                target,
                exe.display()
            )
            .yellow()
        );
        return Ok(());
    };

    if verbose {
        println!("Cross-running {} via {}", exe.display(), qemu);
    }
    let mut cmd = process::Command::new(&qemu);
    if is_bare {
        // Bare-metal: load the binary as the kernel image.
        cmd.arg("-kernel").arg(exe);
        // A common default: 256M RAM, no graphics.
        cmd.args(["-m", "256", "-nographic"]);
    } else {
        cmd.arg(exe);
    }
    cmd.args(program_args);
    let run_status = cmd.status().map_err(|e| {
        GrapeError::CommandFailed(format!(
            "Failed to run '{}' ({}). Is QEMU installed? {}",
            qemu,
            exe.display(),
            e
        ))
    })?;
    if !run_status.success() {
        process::exit(run_status.code().unwrap_or(1));
    }
    Ok(())
}

fn cmd_clean() -> Result<()> {
    println!("Cleaning build artifacts and cached packages...");

    // Cargo-style: nuke the whole `target/` dir (build artifacts,
    // intermediate .o/.obj, cached dependency packages).
    if target_dir().exists() {
        fs::remove_dir_all(&target_dir()).map_err(GrapeError::Io)?;
        println!("  Removed: {}", target_dir().display());
    }

    // Backwards compatibility: remove the legacy `.grape/` cache if it
    // was created by an older grape.
    if Path::new(".grape").exists() {
        fs::remove_dir_all(".grape").map_err(GrapeError::Io)?;
        println!("  Removed: legacy .grape/ cache");
    }

    if Path::new(GRAPE_LOCK).exists() {
        fs::remove_file(GRAPE_LOCK).map_err(GrapeError::Io)?;
        println!("  Removed: grape.lock");
    }

    if Path::new(GRAPE_ERR).exists() {
        fs::remove_file(GRAPE_ERR).map_err(GrapeError::Io)?;
    }

    println!(" Clean completed");
    Ok(())
}

// ============ 辅助函数 ============

fn read_grape_toml() -> Result<GrapeToml> {
    let contents = fs::read_to_string(GRAPE_TOML).map_err(GrapeError::Io)?;
    toml::from_str(&contents).map_err(|e| GrapeError::Toml(e.to_string()))
}

fn save_grape_toml(config: &GrapeToml) -> Result<()> {
    let toml_str =
        toml::to_string_pretty(config).map_err(|e| GrapeError::Toml(e.to_string()))?;
    fs::write(GRAPE_TOML, toml_str).map_err(GrapeError::Io)
}

fn read_lock_file() -> Result<GrapeLock> {
    if !Path::new(GRAPE_LOCK).exists() {
        return Ok(GrapeLock {
            version: 1,
            packages: HashMap::new(),
        });
    }
    let contents = fs::read_to_string(GRAPE_LOCK).map_err(GrapeError::Io)?;
    toml::from_str(&contents).map_err(|e| GrapeError::Toml(e.to_string()))
}

fn save_lock_file(lock: &GrapeLock) -> Result<()> {
    let toml_str =
        toml::to_string_pretty(lock).map_err(|e| GrapeError::Toml(e.to_string()))?;
    fs::write(GRAPE_LOCK, toml_str).map_err(GrapeError::Io)
}

fn update_lock_file(config: &GrapeToml) -> Result<()> {
    let mut lock = read_lock_file()?;

    for (name, spec) in &config.dependencies {
        let local_path = spec.local_path();
        if local_path.exists() {
            if let Ok(commit) = get_current_commit(&local_path) {
                lock.packages.insert(
                    name.clone(),
                    LockedPackage {
                        repo: spec.repo.clone(),
                        tag: spec.tag.clone(),
                        commit,
                    },
                );
            }
        }
    }

    save_lock_file(&lock)
}

fn verify_lock_file(config: &GrapeToml) -> Result<()> {
    let lock = read_lock_file()?;

    for (name, spec) in &config.dependencies {
        if let Some(locked) = lock.packages.get(name) {
            if locked.tag != spec.tag {
                println!(
                    " Warning: {} version mismatch (grape.toml: {}, grape.lock: {})",
                    name, spec.tag, locked.tag
                );
                println!("  Run 'grape update' to sync");
            }
        }
    }

    Ok(())
}

// ============ 依赖解析 ============

fn resolve_dependencies(
    config: &GrapeToml,
    resolved: &mut HashSet<String>,
    verbose: bool,
) -> Result<()> {
    let _ = verbose;
    for (name, spec) in &config.dependencies {
        if !resolved.insert(name.clone()) {
            continue; // cycle detected
        }

        let cache_path = spec.local_path();
        if !cache_path.exists() {
            match download_package(spec) {
                Ok(()) => {}
                Err(e) => {
                    let msg = format!("{}", e);
                    eprintln!(
                        "   Failed to download {}: {}",
                        name,
                        msg.red()
                    );
                    log_failed_dep(name, &msg);
                    continue; // don't block the build for one failed dep
                }
            }
        }

        // Materialise into lib/<name>/
        let lib_dest = spec.lib_material_path();
        if !lib_dest.exists() {
            if let Err(e) = fs::create_dir_all(&lib_dest) {
                eprintln!("Warning: could not create {}: {}", lib_dest.display(), e);
            }
        }

        // Recurse into sub-package
        let sub_toml = cache_path.join(GRAPE_TOML);
        if sub_toml.exists() {
            if let Ok(sub_contents) = fs::read_to_string(&sub_toml) {
                if let Ok(sub_config) = toml::from_str::<GrapeToml>(&sub_contents) {
                    resolve_dependencies(&sub_config, resolved, verbose)?;
                }
            }
        }
    }
    Ok(())
}

// ============ 包下载 ============

fn download_package(spec: &DependencySpec) -> Result<()> {
    let git_url = spec.git_url();
    let tag = &spec.tag;
    let target_dir = spec.local_path();

    if target_dir.exists() {
        return Ok(());
    }

    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent).map_err(GrapeError::Io)?;
    }

    println!("  Cloning {} (tag: {})", git_url, tag);

    match clone_tag_shallow(&git_url, tag, &target_dir) {
        Ok(()) => {
            println!("   Successfully cloned");
            Ok(())
        }
        Err(_shallow_err) => {
            println!(
                "  Shallow clone failed, retrying with full clone...",
            );
            clone_tag_full(&git_url, tag, &target_dir)?;
            println!("   Successfully cloned with full clone");
            Ok(())
        }
    }
}

fn clone_tag_shallow(git_url: &str, tag: &str, target_dir: &Path) -> Result<()> {
    let status = std::process::Command::new("git")
        .args(&[
            "clone", "--depth", "1", "--branch", tag,
            git_url,
            target_dir.to_str().unwrap(),
        ])
        .status()
        .map_err(|_| GrapeError::CommandFailed("git not found".to_string()))?;

    if status.success() {
        Ok(())
    } else {
        Err(GrapeError::CommandFailed(format!(
            "Failed to shallow-clone tag {}",
            tag
        )))
    }
}

fn clone_tag_full(git_url: &str, tag: &str, target_dir: &Path) -> Result<()> {
    let repo = Repository::clone(git_url, target_dir).map_err(GrapeError::Git)?;

    let tag_ref_name = format!("refs/tags/{}", tag);
    let branch_ref_name = format!("refs/heads/{}", tag);

    let commit_id = {
        if let Ok(reference) = repo.find_reference(&tag_ref_name) {
            let annotated = repo
                .reference_to_annotated_commit(&reference)
                .map_err(GrapeError::Git)?;
            annotated.id()
        } else if let Ok(reference) = repo.find_reference(&branch_ref_name) {
            let annotated = repo
                .reference_to_annotated_commit(&reference)
                .map_err(GrapeError::Git)?;
            annotated.id()
        } else {
            return Err(GrapeError::NotFound(format!(
                "Tag/branch '{}' not found",
                tag
            )));
        }
    };

    let commit = repo.find_commit(commit_id).map_err(GrapeError::Git)?;
    repo.reset(&commit.as_object(), ResetType::Hard, None)
        .map_err(GrapeError::Git)?;

    Ok(())
}

fn get_current_commit(repo_path: &Path) -> Result<String> {
    let repo = Repository::open(repo_path).map_err(GrapeError::Git)?;
    let head = repo.head().map_err(GrapeError::Git)?;
    let commit_id = head
        .target()
        .ok_or_else(|| GrapeError::NotFound("No commit found".to_string()))?;
    Ok(commit_id.to_string())
}

// ============ Tag 验证与获取 ============

/// Check whether a tag exists on the remote without cloning.
fn tag_exists_remote(spec: &DependencySpec) -> bool {
    let output = std::process::Command::new("git")
        .args(&["ls-remote", "--tags", &spec.git_url(), &spec.tag])
        .output();

    match output {
        Ok(o) => {
            // git ls-remote outputs refs/tags/<name>^{} if found
            !String::from_utf8_lossy(&o.stdout).trim().is_empty()
        }
        Err(_) => {
            // Can't reach remote — let the actual download decide.
            true
        }
    }
}

/// Fetch the latest tag from a GitHub repository using `git ls-remote`.
fn fetch_latest_tag(repo: &str) -> std::result::Result<String, String> {
    let git_url = format!("https://github.com/{}.git", repo);
    let output = std::process::Command::new("git")
        .args(&["ls-remote", "--tags", "--refs", &git_url])
        .output()
        .map_err(|e| format!("git ls-remote failed: {}", e))?;

    if !output.status.success() {
        return Err("git ls-remote exited with error".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut tags: Vec<&str> = stdout
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                parts[1].strip_prefix("refs/tags/")
            } else {
                None
            }
        })
        .filter(|t| !t.ends_with("^{}")) // skip peeled tags
        .collect();

    if tags.is_empty() {
        return Err("no tags found".to_string());
    }

    // Simple semver-aware sort: prefer `v1.2.3` or `1.2.3` patterns.
    tags.sort_by(|a, b| compare_tags(b, a));

    Ok(tags[0].to_string())
}

fn compare_tags(a: &str, b: &str) -> std::cmp::Ordering {
    let a_clean = a.trim_start_matches('v');
    let b_clean = b.trim_start_matches('v');

    let a_parts: Vec<u32> = a_clean
        .split('.')
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();
    let b_parts: Vec<u32> = b_clean
        .split('.')
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();

    a_parts.cmp(&b_parts)
}

// ============ 库路径构建 ============

/// Find a library search path that allows `import std` to resolve.
/// Returns the *parent* directory of the `std/` folder so that
/// `resolve_module_file("std")` can find `<path>/std/mod.gbl`.
fn find_std_path() -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = (|| {
        let mut v = Vec::new();

        // 1. GOBOL_INSTALL_DIR env var
        if let Ok(dir) = std::env::var("GOBOL_INSTALL_DIR") {
            let p = PathBuf::from(&dir);
            if p.join("std").exists() { v.push(p); }
            let p = PathBuf::from(&dir).join("lib");
            if p.join("std").exists() { v.push(p); }
        }

        // 2. Relative to the grape executable
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                // ~/.gobol/bin/ → ~/.gobol/  (std/ lives next to bin/)
                if let Some(parent) = dir.parent() {
                    if parent.join("std").exists() { v.push(parent.to_path_buf()); }
                }

                // target/debug/ → project_root/  (std/ is at project root)
                let p = dir.parent()
                    .and_then(|d| d.parent())
                    .filter(|d| d.join("std").exists())
                    .map(|d| d.to_path_buf());
                if let Some(p) = p { v.push(p); }
            }
        }

        // 3. Relative to current working directory
        if let Ok(cwd) = std::env::current_dir() {
            if cwd.join("std").exists() { v.push(cwd.clone()); }
            if cwd.join("lib").join("std").exists() { v.push(cwd.join("lib")); }
            if let Some(parent) = cwd.parent() {
                if parent.join("std").exists() { v.push(parent.to_path_buf()); }
            }
        }

        v
    })();

    candidates.into_iter().next()
}

fn build_lib_paths(config: &GrapeToml) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();

    // Local project lib/ directory.
    if lib_dir().exists() {
        paths.push(lib_dir());
    }

    for (var_name, spec) in &config.dependencies {
        // Preference 1: materialised lib/<name>/
        let mat = spec.lib_material_path();
        if mat.exists() {
            paths.push(mat);
        }

        // Preference 2: package cache src/
        let cache_src = spec.local_path().join("src");
        if cache_src.exists() {
            paths.push(cache_src);
        }

        // Preference 3: package cache root
        let cache_root = spec.local_path();
        if cache_root.exists() {
            paths.push(cache_root);
        }

        // legacy: direct lib/<name> for compat
        let legacy_lib = lib_dir().join(var_name);
        if legacy_lib.exists() && !paths.contains(&legacy_lib) {
            paths.push(legacy_lib);
        }
    }

    paths
}

// ============ 测试 ============

#[cfg(test)]
mod tests {
    use super::*;

    // ---- 路径测试 ----

    #[test]
    fn test_dependency_local_path() {
        let spec = DependencySpec {
            repo: "test/repo".to_string(),
            tag: "1.0.0".to_string(),
            optional: None,
        };
        assert_eq!(spec.git_url(), "https://github.com/test/repo.git");
        assert_eq!(spec.local_name(), "repo");
        assert_eq!(
            spec.local_path(),
            PathBuf::from("target").join("grape").join("packages").join("repo")
        );
        assert_eq!(
            spec.lib_material_path(),
            PathBuf::from("lib").join("repo")
        );
    }

    #[test]
    fn test_local_path_cross_platform() {
        let spec = DependencySpec {
            repo: "github/user/my-lib".to_string(),
            tag: "v2.0.0".to_string(),
            optional: None,
        };
        let p = spec.local_path();
        // Must be relative (not hardcoded to a specific OS root).
        assert!(p.is_relative(), "path should be relative, got {:?}", p);
        // Must use PathBuf join semantics (components, not string fmt).
        let comps: Vec<_> = p.components().collect();
        assert!(comps.len() >= 4, "expected at least 4 components, got {:?}", comps);
        // Verify the expected logical structure: target → grape → packages → name
        assert_eq!(comps[0].as_os_str(), "target");
        assert_eq!(comps[1].as_os_str(), "grape");
        assert_eq!(comps[2].as_os_str(), "packages");
        assert_eq!(comps[3].as_os_str(), "my-lib");
    }

    #[test]
    fn test_lib_paths_uses_pathbuf() {
        let config = GrapeToml {
            project: Project {
                name: "test".into(),
                version: "0.1.0".into(),
                entry: "main.gbl".into(),
                authors: None,
                description: None,
                license: None,
            },
            dependencies: {
                let mut m = HashMap::new();
                m.insert(
                    "repo".into(),
                    DependencySpec {
                        repo: "test/repo".into(),
                        tag: "1.0.0".into(),
                        optional: None,
                    },
                );
                m
            },
            build: None,
        };
        let paths = build_lib_paths(&config);
        // All paths must be relative (no hardcoded separators).
        for p in &paths {
            assert!(p.is_relative(), "expected relative path, got {:?}", p);
        }
    }

    // ---- Tag 解析测试 ----

    #[test]
    fn test_tag_comparison_semver() {
        assert_eq!(compare_tags("1.0.0", "0.9.0"), std::cmp::Ordering::Greater);
        assert_eq!(compare_tags("v2.0.0", "v1.9.9"), std::cmp::Ordering::Greater);
        assert_eq!(compare_tags("0.1.0", "0.1.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_tags("v1.0.0", "1.0.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_tags("0.1.0", "0.2.0"), std::cmp::Ordering::Less);
    }

    // ---- 依赖解析测试 ----

    #[test]
    fn test_dependency_parsing() {
        let dep = "user/repo@1.0.0";
        let (repo, tag) = dep.split_once('@').unwrap();
        assert_eq!(repo, "user/repo");
        assert_eq!(tag, "1.0.0");
    }

    #[test]
    fn test_dependency_parsing_with_v() {
        let dep = "gobol-org/math@v0.3.1";
        let parts: Vec<&str> = dep.split_once('@').unwrap().0.split('/').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[1], "math");
    }

    // ---- build 参数测试 ----

    #[test]
    fn test_build_args_detection() {
        let args = vec![
            "grape".to_string(),
            "build".to_string(),
            "--release".to_string(),
            "-o".to_string(),
            "mybin".to_string(),
        ];
        let is_release = args.iter().any(|a| a == "--release");
        let out_name = args
            .iter()
            .position(|a| a == "-o")
            .and_then(|i| args.get(i + 1).cloned());
        assert!(is_release);
        assert_eq!(out_name, Some("mybin".to_string()));
    }

    #[test]
    fn test_run_args_detection() {
        let args = vec![
            "grape".to_string(),
            "run".to_string(),
            "--verbose".to_string(),
        ];
        let compile_only = args.iter().any(|a| a == "-c" || a == "--compile-only");
        assert!(!compile_only);
        let is_verbose = args.iter().any(|a| a == "--verbose");
        assert!(is_verbose);
    }
}
