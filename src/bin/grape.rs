use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{self, Write};
use colored::*;
use git2::{Repository, ResetType};

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
        }
    }
}

impl std::error::Error for GrapeError {}

type Result<T> = std::result::Result<T, GrapeError>;

// ============ 数据结构 ============

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GrapeToml {
    pub project: Project,
    pub dependencies: HashMap<String, DependencySpec>,
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

impl DependencySpec {
    fn git_url(&self) -> String {
        format!("https://github.com/{}.git", self.repo)
    }
    
    fn local_name(&self) -> String {
        self.repo.split('/').last().unwrap().to_string()
    }
    
    fn local_path(&self) -> PathBuf {
        PathBuf::from(format!(".grape/packages/{}", self.local_name()))
    }
}

// ============ 主函数 ============

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_help();
        return;
    }

    if let Err(e) = run_command(&args) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
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
        "help" | "--help" => {
            print_help();
            Ok(())
        }
        "version" | "--version" => {
            println!("Grape version: 0.2.0, binding with Gobol 0.1.0");
            Ok(())
        }
        _ => Err(GrapeError::NotFound(format!("Unknown command: {}", args[1]))),
    }
}

fn print_help() {
    println!("Grape - Package Manager for Gobol");
    println!();
    println!("Usage:");
    println!("  grape init               Initialize a new Gobol project");
    println!("  grape add <dep>          Add a dependency (format: user/repo@tag)");
    println!("  grape add <dep> --optional  Add as optional dependency");
    println!("  grape remove <name>      Remove a dependency");
    println!("  grape update [name]      Update dependencies");
    println!("  grape list               List all dependencies");
    println!("  grape run [--verbose]    Run the Gobol program");
    println!("  grape clean              Clean cached packages");
    println!("  grape version            Show the version");
    println!("  grape help               Show this help message");
    println!();
    println!("Examples:");
    println!("  grape add gobol-org/math@0.1.0");
    println!("  grape add gobol-org/test@0.2.0 --optional");
    println!("  grape remove math");
    println!("  grape update");
    println!("  grape run --verbose");
}

// ============ 命令实现 ============

fn cmd_init() -> Result<()> {
    if Path::new("grape.toml").exists() {
        return Err(GrapeError::AlreadyExists("grape.toml already exists".to_string()));
    }

    println!("Initializing new Gobol project...");

    let current_dir = std::env::current_dir().map_err(GrapeError::Io)?;
    let project_name = current_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project")
        .to_string();

    // 询问用户信息
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
    };

    let toml_str = toml::to_string_pretty(&config).map_err(|e| GrapeError::Toml(e.to_string()))?;
    fs::write("grape.toml", toml_str).map_err(GrapeError::Io)?;
    fs::create_dir_all(".grape/packages").map_err(GrapeError::Io)?;

    println!("✓ Project initialized successfully");

    if !Path::new("main.gbl").exists() {
        let content = r#"// Your Gobol code here
// Run with: grape run

func main(): int {
    io.print("Hello, World!\n")
    return 0
}
"#;
        fs::write("main.gbl", content).map_err(GrapeError::Io)?;
        println!("✓ Created example entry file: main.gbl");
    }

    println!("\nNext steps:");
    println!("  1. Edit grape.toml to configure your project");
    println!("  2. Add dependencies: grape add user/repo@tag");
    println!("  3. Run your program: grape run");
    
    Ok(())
}

fn cmd_add(deps: &[String]) -> Result<()> {
    if deps.is_empty() {
        return Err(GrapeError::InvalidDependency(
            "No dependency specified. Usage: grape add <user/repo@tag>".to_string()
        ));
    }

    let mut config = read_grape_toml()?;
    let is_optional = deps.iter().any(|a| a == "--optional");
    let deps: Vec<&String> = deps.iter().filter(|d| !d.starts_with('-')).collect();

    if deps.is_empty() {
        return Err(GrapeError::InvalidDependency(
            "No dependency specified. Usage: grape add <user/repo@tag>".to_string()
        ));
    }

    for dep in deps {
        println!("📦 Adding dependency: {}", dep);

        let (repo, tag) = dep.split_once('@').ok_or_else(|| {
            GrapeError::InvalidDependency(format!("Invalid format '{}'. Expected: user/repo@tag", dep))
        })?;

        let parts: Vec<&str> = repo.split('/').collect();
        if parts.len() != 2 {
            return Err(GrapeError::InvalidDependency(
                format!("Invalid repo format '{}'. Expected: user/repo", repo)
            ));
        }

        let var_name = parts[1].to_string();
        
        // 检查是否已存在
        if config.dependencies.contains_key(&var_name) {
            return Err(GrapeError::AlreadyExists(
                format!("Dependency '{}' already exists. Use 'grape update {}' to update", var_name, var_name)
            ));
        }

        let dep_spec = DependencySpec {
            repo: repo.to_string(),
            tag: tag.to_string(),
            optional: if is_optional { Some(true) } else { None },
        };

        let local_path = dep_spec.local_path();

        // 下载依赖
        println!("  Downloading from {}", dep_spec.git_url());
        download_package(&dep_spec)?;
        println!("  ✓ Downloaded to {}", local_path.display());

        config.dependencies.insert(var_name.clone(), dep_spec);
        println!("  ✓ Added dependency: {}", var_name);
    }

    save_grape_toml(&config)?;
    update_lock_file(&config)?;
    
    println!("✓ Dependencies added successfully");
    Ok(())
}

fn cmd_remove(deps: &[String]) -> Result<()> {
    if deps.is_empty() {
        return Err(GrapeError::InvalidDependency(
            "No dependency specified. Usage: grape remove <name>".to_string()
        ));
    }

    let mut config = read_grape_toml()?;
    
    for dep in deps {
        if config.dependencies.remove(dep).is_some() {
            println!("✓ Removed dependency: {}", dep);
            
            // 删除本地文件
            let local_path = PathBuf::from(format!(".grape/packages/{}", dep));
            if local_path.exists() {
                fs::remove_dir_all(&local_path).map_err(GrapeError::Io)?;
                println!("  Removed local files: {}", local_path.display());
            }
        } else {
            println!("⚠ Dependency not found: {}", dep);
        }
    }
    
    save_grape_toml(&config)?;
    update_lock_file(&config)?;
    
    println!("✓ Dependencies removed successfully");
    Ok(())
}

fn cmd_update(args: &[String]) -> Result<()> {
    let config = read_grape_toml()?;
    let specific_dep = args.first();
    
    println!("🔄 Updating dependencies...");
    
    let deps_to_update: Vec<(String, DependencySpec)> = if let Some(name) = specific_dep {
        if let Some(spec) = config.dependencies.get(name).cloned() {
            vec![(name.clone(), spec)]
        } else {
            return Err(GrapeError::NotFound(format!("Dependency '{}' not found", name)));
        }
    } else {
        config.dependencies.clone().into_iter().collect()
    };
    
    for (name, spec) in deps_to_update {
        println!("  Updating {}@{}", name, spec.tag);
        
        // 重新下载
        let local_path = spec.local_path();
        if local_path.exists() {
            fs::remove_dir_all(&local_path).map_err(GrapeError::Io)?;
        }
        
        download_package(&spec)?;
        println!("  ✓ Updated {}", name);
    }
    
    save_grape_toml(&config)?;
    update_lock_file(&config)?;
    
    println!("✓ Dependencies updated successfully");
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
        let optional = if spec.optional.unwrap_or(false) { " (optional)" } else { "" };
        println!("     {} = {}{}", name, spec.repo, optional);
        println!("       tag: {}", spec.tag);
        
        let local_path = spec.local_path();
        if local_path.exists() {
            println!("       ✓ Downloaded");
        } else {
            println!("       ✗ Not downloaded (run 'grape update' to download)");
        }
    }
    
    Ok(())
}

fn cmd_run(args: &[String]) -> Result<()> {
    let is_verbose = args.iter().any(|a| a == "--verbose");
    let no_check = args.iter().any(|a| a == "--no-check");

    println!("{}", " Running Gobol program...".bold().green());

    if !Path::new("grape.toml").exists() {
        return Err(GrapeError::NotFound(
            "grape.toml not found. Run 'grape init' first.".to_string()
        ));
    }

    let config = read_grape_toml()?;
    
    // 检查锁文件
    if !no_check && !Path::new("grape.lock").exists() {
        println!("grape.lock not found, generating...");
        update_lock_file(&config)?;
    } else if !no_check {
        verify_lock_file(&config)?;
    }

    let entry_file = &config.project.entry;
    if !Path::new(entry_file).exists() {
        return Err(GrapeError::NotFound(
            format!("Entry file '{}' not found.", entry_file)
        ));
    }

    // 确保所有依赖都已下载
    ensure_dependencies_downloaded(&config)?;

    let lib_paths = build_lib_paths(&config);

    if is_verbose {
        println!("Project: {}", config.project.name);
        println!("Version: {}", config.project.version);
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

    let status = cmd.status().map_err(|_| {
        GrapeError::CommandFailed("Failed to run gobol. Make sure gobol is installed.".to_string())
    })?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    
    Ok(())
}

fn cmd_clean() -> Result<()> {
    println!("🧹 Cleaning cached packages...");
    
    let packages_dir = Path::new(".grape/packages");
    if packages_dir.exists() {
        for entry in fs::read_dir(packages_dir).map_err(GrapeError::Io)? {
            let entry = entry.map_err(GrapeError::Io)?;
            let path = entry.path();
            if path.is_dir() {
                fs::remove_dir_all(&path).map_err(GrapeError::Io)?;
                println!("  Removed: {}", path.display());
            }
        }
    }
    
    // 清理锁文件（可选）
    if Path::new("grape.lock").exists() {
        fs::remove_file("grape.lock").map_err(GrapeError::Io)?;
        println!("  Removed: grape.lock");
    }
    
    println!("✓ Clean completed");
    Ok(())
}

// ============ 辅助函数 ============

fn read_grape_toml() -> Result<GrapeToml> {
    let contents = fs::read_to_string("grape.toml").map_err(GrapeError::Io)?;
    let config: GrapeToml = toml::from_str(&contents).map_err(|e| GrapeError::Toml(e.to_string()))?;
    Ok(config)
}

fn save_grape_toml(config: &GrapeToml) -> Result<()> {
    let toml_str = toml::to_string_pretty(config).map_err(|e| GrapeError::Toml(e.to_string()))?;
    fs::write("grape.toml", toml_str).map_err(GrapeError::Io)?;
    Ok(())
}

fn read_lock_file() -> Result<GrapeLock> {
    if !Path::new("grape.lock").exists() {
        return Ok(GrapeLock {
            version: 1,
            packages: HashMap::new(),
        });
    }
    
    let contents = fs::read_to_string("grape.lock").map_err(GrapeError::Io)?;
    let lock: GrapeLock = toml::from_str(&contents).map_err(|e| GrapeError::Toml(e.to_string()))?;
    Ok(lock)
}

fn save_lock_file(lock: &GrapeLock) -> Result<()> {
    let toml_str = toml::to_string_pretty(lock).map_err(|e| GrapeError::Toml(e.to_string()))?;
    fs::write("grape.lock", toml_str).map_err(GrapeError::Io)?;
    Ok(())
}

fn update_lock_file(config: &GrapeToml) -> Result<()> {
    let mut lock = read_lock_file()?;
    
    for (name, spec) in &config.dependencies {
        // 获取当前 commit hash
        let local_path = spec.local_path();
        if local_path.exists() {
            if let Ok(commit) = get_current_commit(&local_path) {
                lock.packages.insert(name.clone(), LockedPackage {
                    repo: spec.repo.clone(),
                    tag: spec.tag.clone(),
                    commit,
                });
            }
        }
    }
    
    save_lock_file(&lock)?;
    Ok(())
}

fn verify_lock_file(config: &GrapeToml) -> Result<()> {
    let lock = read_lock_file()?;
    
    for (name, spec) in &config.dependencies {
        if let Some(locked) = lock.packages.get(name) {
            if locked.tag != spec.tag {
                println!("⚠ Warning: {} version mismatch (grape.toml: {}, grape.lock: {})", 
                         name, spec.tag, locked.tag);
                println!("  Run 'grape update' to sync");
            }
        }
    }
    
    Ok(())
}

fn ensure_dependencies_downloaded(config: &GrapeToml) -> Result<()> {
    for (name, spec) in &config.dependencies {
        let local_path = spec.local_path();
        if !local_path.exists() {
            println!("📦 Downloading missing dependency: {}", name);
            download_package(spec)?;
        }
    }
    Ok(())
}

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

    // 尝试浅克隆
    match clone_tag_shallow(&git_url, tag, &target_dir) {
        Ok(_) => {
            println!("  ✓ Successfully cloned");
            Ok(())
        }
        Err(e) => {
            println!("  ⚠ Shallow clone failed: {}, retrying with full clone...", e);
            clone_tag_full(&git_url, tag, &target_dir)?;
            println!("  ✓ Successfully cloned with full clone");
            Ok(())
        }
    }
}

fn clone_tag_shallow(git_url: &str, tag: &str, target_dir: &Path) -> Result<()> {
    // 使用命令行进行浅克隆（git2 对浅克隆支持有限）
    let status = std::process::Command::new("git")
        .args(&["clone", "--depth", "1", "--branch", tag, git_url, target_dir.to_str().unwrap()])
        .status()
        .map_err(|_| GrapeError::CommandFailed("git not found".to_string()))?;
    
    if status.success() {
        Ok(())
    } else {
        Err(GrapeError::CommandFailed(format!("Failed to clone tag {}", tag)))
    }
}

fn clone_tag_full(git_url: &str, tag: &str, target_dir: &Path) -> Result<()> {
    let repo = Repository::clone(git_url, target_dir).map_err(GrapeError::Git)?;
    
    // 查找 tag
    let tag_ref_name = format!("refs/tags/{}", tag);
    let branch_ref_name = format!("refs/heads/{}", tag);
    
    let commit_id = {
        if let Ok(reference) = repo.find_reference(&tag_ref_name) {
            let annotated = repo.reference_to_annotated_commit(&reference)
                .map_err(GrapeError::Git)?;
            annotated.id()
        }
        else if let Ok(reference) = repo.find_reference(&branch_ref_name) {
            let annotated = repo.reference_to_annotated_commit(&reference)
                .map_err(GrapeError::Git)?;
            annotated.id()
        } else {
            return Err(GrapeError::NotFound(format!("Tag/branch '{}' not found", tag)));
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
    let commit_id = head.target().ok_or_else(|| {
        GrapeError::NotFound("No commit found".to_string())
    })?;
    Ok(commit_id.to_string())
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

// ============ 测试 ============

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_dependency_spec() {
        let spec = DependencySpec {
            repo: "test/repo".to_string(),
            tag: "1.0.0".to_string(),
            optional: None,
        };
        
        assert_eq!(spec.git_url(), "https://github.com/test/repo.git");
        assert_eq!(spec.local_name(), "repo");
        assert_eq!(spec.local_path(), PathBuf::from(".grape/packages/repo"));
    }
    
    #[test]
    fn test_dependency_parsing() {
        let dep = "user/repo@1.0.0";
        let (repo, tag) = dep.split_once('@').unwrap();
        assert_eq!(repo, "user/repo");
        assert_eq!(tag, "1.0.0");
    }
}
