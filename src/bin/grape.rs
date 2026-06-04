use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use git2::{Repository, ResetType};

/// # Description
///
/// grape is a package manager & running tool for Gobol
///
/// # File
///
/// Use TOML to store dependencies
///
/// ## Example
/// ```toml
/// [project]
/// name = "grape-test"
/// version = "0.1.0"
/// entry = "main.gbl"
///
/// [dependencies]
/// math = { repo = "gobol-org/mathematics", tag = "0.1.0" }
/// ```
///
/// # Commands
///
/// ```bash
/// grape add user/repo@version   # add dependency
/// grape run                     # run the program
/// ```
fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    match args[1].as_str() {
        "init" => cmd_init(),
        "add" => cmd_add(&args[2..]),
        "run" => cmd_run(&args[2..]),
        "help" | "--help" => print_help(),
        "version" | "--version" => {
            println!("Grape version: 0.1.0, binding with Gobol 0.1.0");
            return;
        }
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            println!();
            print_help();
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!("Grape - Package Manager for Gobol");
    println!();
    println!("Usage:");
    println!("  grape init              Initialize a new Gobol project");
    println!("  grape add <dep>         Add a dependency (format: user/repo@tag)");
    println!("  grape run [--verbose]   Run the Gobol program");
    println!("  grape version           Show the version");
    println!("  grape help              Show this help message");
    println!();
    println!("Examples:");
    println!("  grape add gobol-org/math@0.1.0");
    println!("  grape run --verbose");
}

// ============ 数据结构 ============

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GrapeToml {
    pub project: Project,
    pub dependencies: HashMap<String, DependencySpec>,  // key = 本地文件夹名（变量名）
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Project {
    pub name: String,
    pub version: String,
    pub entry: String,
}

/// 依赖配置
/// math = { repo = "gobol-org/mathematics", tag = "0.1.0" }
/// - 变量名 "math" 作为本地文件夹名
/// - repo 指向 GitHub 仓库 "gobol-org/mathematics"
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DependencySpec {
    pub repo: String,   // GitHub 仓库 "user/repo"
    pub tag: String,
}

impl DependencySpec {
    fn git_url(&self) -> String {
        format!("https://github.com/{}.git", self.repo)
    }
}

// ============ 命令实现 ============

fn cmd_init() {
    if Path::new("grape.toml").exists() {
        eprintln!("grape.toml already exists");
        return;
    }

    println!("Initializing new Gobol project...");

    let current_dir = std::env::current_dir().unwrap();
    let project_name = current_dir
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let config = GrapeToml {
        project: Project {
            name: project_name,
            version: "0.1.0".to_string(),
            entry: "main.gbl".to_string(),
        },
        dependencies: HashMap::new(),
    };

    let toml_str = toml::to_string_pretty(&config).unwrap();
    fs::write("grape.toml", toml_str).unwrap();
    fs::create_dir_all(".grape/packages").unwrap();

    println!("Project initialized successfully");

    if !Path::new("main.gbl").exists() {
        fs::write("main.gbl", "// Your Gobol code here\n").unwrap();
        println!("Created example entry file: main.gbl");
    }
}

fn cmd_add(deps: &[String]) {
    if deps.is_empty() {
        eprintln!("Error: No dependency specified");
        eprintln!("Usage: grape add <user/repo@tag>");
        eprintln!("Example: grape add gobol-org/math@0.1.0");
        std::process::exit(1);
    }

    let mut config = match read_grape_toml() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading grape.toml: {}", e);
            eprintln!("Run 'grape init' first.");
            std::process::exit(1);
        }
    };

    for dep in deps {
        println!("Adding dependency: {}", dep);

        // 解析格式: user/repo@tag
        let (repo, tag) = match dep.split_once('@') {
            Some((r, t)) => (r, t),
            None => {
                eprintln!("Error: Invalid format '{}'", dep);
                eprintln!("Expected: user/repo@tag");
                std::process::exit(1);
            }
        };

        // 验证 repo 格式
        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() != 2 {
            eprintln!("Error: Invalid repo format '{}'", repo);
            eprintln!("Expected: user/repo");
            std::process::exit(1);
        }

        // 变量名 = repo 的最后一部分（如 "gobol-org/math" -> "math"）
        let var_name = parts[1].to_string();

        let dep_spec = DependencySpec {
            repo: repo.to_string(),
            tag: tag.to_string(),
        };

        // 本地路径 = .grape/packages/{变量名}/
        let local_path = format!(".grape/packages/{}", var_name);

        if download_package_with_git2(&dep_spec.git_url(), tag, &local_path) {
            println!("Downloaded {} to {}", dep, local_path);

            config.dependencies.insert(var_name, dep_spec);
        } else {
            eprintln!("Failed to download dependency: {}", dep);
            std::process::exit(1);
        }
    }

    save_grape_toml(&config);
    println!("Dependencies added successfully");
}

fn cmd_run(args: &[String]) {
    let is_verbose = args.iter().any(|a| a == "--verbose");

    println!("Running Gobol program...");

    if !Path::new("grape.toml").exists() {
        eprintln!("Error: grape.toml not found. Run 'grape init' first.");
        std::process::exit(1);
    }

    let config = match read_grape_toml() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading grape.toml: {}", e);
            std::process::exit(1);
        }
    };

    let entry_file = &config.project.entry;
    if !Path::new(entry_file).exists() {
        eprintln!("Error: Entry file '{}' not found.", entry_file);
        std::process::exit(1);
    }

    let lib_paths = build_lib_paths(&config);

    if is_verbose {
        println!("Project: {}", config.project.name);
        println!("Entry: {}", entry_file);
        println!("Dependencies: {:?}", config.dependencies.keys().collect::<Vec<_>>());
        println!("Lib paths: {:?}", lib_paths);
    }

    let mut cmd = std::process::Command::new("gobol");

    for path in &lib_paths {
        cmd.arg("--lib-path").arg(path);
    }
    cmd.arg(entry_file);

    if is_verbose {
        println!("Running: {:?}", cmd);
    }

    let status = cmd.status().expect("Failed to run gobol. Make sure gobol is installed.");

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
}

// ============ 辅助函数 ============

fn read_grape_toml() -> Result<GrapeToml, Box<dyn std::error::Error>> {
    let contents = fs::read_to_string("grape.toml")?;
    let config: GrapeToml = toml::from_str(&contents)?;
    Ok(config)
}

fn save_grape_toml(config: &GrapeToml) {
    let toml_str = toml::to_string_pretty(config).unwrap();
    fs::write("grape.toml", toml_str).unwrap();
}

fn download_package_with_git2(git_url: &str, tag: &str, target_dir: &str) -> bool {
    if Path::new(target_dir).exists() {
        println!("Package already exists at {}, skipping", target_dir);
        return true;
    }

    if let Some(parent) = Path::new(target_dir).parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("Failed to create directory: {}", e);
            return false;
        }
    }

    println!("Cloning {} (tag: {})", git_url, tag);

    match clone_shallow_with_branch(git_url, target_dir, tag) {
        Ok(_) => {
            println!("Successfully cloned");
            true
        }
        Err(e) => {
            eprintln!("Shallow clone failed: {}", e);
            eprintln!("Retrying with full clone...");
            match clone_full_with_branch(git_url, target_dir, tag) {
                Ok(_) => {
                    println!("Successfully cloned with full clone");
                    true
                }
                Err(e2) => {
                    eprintln!("Full clone failed: {}", e2);
                    false
                }
            }
        }
    }
}

fn clone_shallow_with_branch(git_url: &str, target_dir: &str, tag: &str) -> Result<Repository, git2::Error> {
    let mut fetch_options = git2::FetchOptions::new();
    fetch_options.depth(1);

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_options);
    builder.branch(tag);

    builder.clone(git_url, Path::new(target_dir))
}

fn clone_full_with_branch(git_url: &str, target_dir: &str, tag: &str) -> Result<Repository, git2::Error> {
    let repo = Repository::clone(git_url, Path::new(target_dir))?;

    let tag_ref = format!("refs/tags/{}", tag);
    if let Ok(tag_ref_obj) = repo.find_reference(&tag_ref) {
        let annotated = repo.reference_to_annotated_commit(&tag_ref_obj)?;
        let commit = repo.find_commit(annotated.id())?;
        repo.reset(&commit.as_object(), ResetType::Hard, None)?;
        return Ok(repo);
    }

    let branch_ref = format!("refs/heads/{}", tag);
    if let Ok(branch_ref_obj) = repo.find_reference(&branch_ref) {
        let commit = branch_ref_obj.peel_to_commit()?;
        repo.reset(&commit.as_object(), ResetType::Hard, None)?;
        return Ok(repo);
    }

    Err(git2::Error::from_str(&format!("Tag/branch '{}' not found", tag)))
}

fn build_lib_paths(config: &GrapeToml) -> Vec<String> {
    let mut paths = Vec::new();

    for (var_name, _spec) in &config.dependencies {
        let dep_path = format!(".grape/packages/{}", var_name);

        let src_path = format!("{}/src", dep_path);
        if Path::new(&src_path).exists() {
            paths.push(src_path);
        }

        if Path::new(&dep_path).exists() {
            paths.push(dep_path);
        }
    }

    paths
}
