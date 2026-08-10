use gobol::ast_builder::AstBuilder;
use gobol::ast_printer::AstPrinter;
use gobol::cranelift::CraneliftBackend;
use gobol::error::ErrorFormatter;
use gobol::lexer::Lexer;
use gobol::semantic_analyzer::SemanticAnalyzer;
use gobol::token;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use colored::*;

/// Compilation / execution mode selected from the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// JIT: compile in-memory and run immediately (default).
    Jit,
    /// AOT: emit a standalone binary via Cranelift ObjectModule, then run it.
    AotRun,
    /// AOT: emit a standalone binary, do not run (--release / -c).
    AotNoRun,
}

fn resolve_module_file(path_parts: &[String], lib_paths: &[String], main_file: &str) -> Option<String> {
    let relative = format!("{}.gbl", path_parts.join("/"));
    if let Some(parent) = Path::new(main_file).parent() {
        let p = parent.join(&relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
    }
    for lp in lib_paths {
        let p = Path::new(lp).join(&relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
    }
    if Path::new(&relative).exists() { return Some(relative); }
    None
}

/// Locate the C runtime (`runtime.c`) used for AOT linking.
///
/// Search order:
/// 1. `$GOBOL_RUNTIME` env var (explicit override).
/// 2. `<exe_dir>/../lib/runtime.c` (installed layout: ~/.gobol/bin + ~/.gobol/lib).
/// 3. `<exe_dir>/../src/runtime.c` (cargo target/release -> src).
/// 4. `<exe_dir>/runtime.c` (alongside the binary).
fn find_runtime_c() -> Option<PathBuf> {
    if let Ok(p) = env::var("GOBOL_RUNTIME") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                dir.join("..").join("lib").join("runtime.c"),
                dir.join("..").join("..").join("src").join("runtime.c"),
                dir.join("..").join("src").join("runtime.c"),
                dir.join("runtime.c"),
            ];
            for c in &candidates {
                if c.exists() {
                    return Some(c.to_path_buf());
                }
            }
        }
    }
    None
}

fn get_source(file: &str) -> String {
    match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Cannot open file '{}': {}", file, e);
            process::exit(1);
        }
    }
}

fn print_help() {
    println!("Gobol - A statically compiled programming language");
    println!();
    println!("Usage:");
    println!("  gobol <filename> [options]              Compile and run via JIT (Cranelift, default)");
    println!("  gobol <filename> --release [-o name]    AOT: emit a standalone binary (no run)");
    println!("  gobol <filename> --aot-run [-o name]    AOT: emit binary then run it");
    println!("  gobol <filename> -c [-o name]           AOT compile only (alias for --release)");
    println!("  gobol --version                          Show version information");
    println!("  gobol --help                             Show this help message");
    println!();
    println!("Options:");
    println!("  --release                AOT mode: produce an optimized binary on disk");
    println!("  --aot-run                AOT mode: produce binary and run it");
    println!("  --jit, --debug           Explicit JIT mode (default)");
    println!("  -c                        Compile only (AOT, do not run)");
    println!("  -o <name>                Output binary name (AOT mode only)");
    println!("  --verbose, -v            Enable verbose output");
    println!("  --lib-path <path>        Add a library search path (can be used multiple times)");
    println!();
    println!("Examples:");
    println!("  gobol main.gbl                         Run via JIT");
    println!("  gobol main.gbl --verbose               Run with verbose output");
    println!("  gobol main.gbl --release -o myapp      Produce ./myapp via AOT");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let is_version = args.iter().any(|s| s == "--version");
    let is_help = args.iter().any(|s| s == "--help");

    if is_help {
        print_help();
        return;
    }
    if is_version {
        println!("Gobol 0.1.0");
        return;
    }

    let is_verbose = args.iter().any(|s| s == "--verbose" || s == "-v");

    // Mode selection: --release / -c => AOT (no run); --aot-run => AOT + run;
    // --jit / --debug => JIT (default).
    let is_aot_run = args.iter().any(|s| s == "--aot-run");
    let is_aot_norun = !is_aot_run && args.iter().any(|s| s == "--release" || s == "-c");
    let is_jit_explicit = args.iter().any(|s| s == "--jit" || s == "--debug");
    let mode = if is_aot_run {
        Mode::AotRun
    } else if is_aot_norun {
        Mode::AotNoRun
    } else {
        // --jit/--debug or default => JIT (run immediately).
        let _ = is_jit_explicit;
        Mode::Jit
    };

    // Parse -o <name> (AOT output name).
    let mut out_name: Option<String> = None;

    // Parse --lib-path arguments (support multiple)
    let mut lib_paths_from_cli: Vec<String> = Vec::new();
    let mut i = 1;
    let mut filename = None;

    while i < args.len() {
        if args[i] == "--lib-path" && i + 1 < args.len() {
            let paths_str = &args[i + 1];
            for p in paths_str.split(',') {
                if !p.is_empty() {
                    lib_paths_from_cli.push(p.to_string());
                }
            }
            i += 2;
        } else if args[i] == "-o" && i + 1 < args.len() {
            out_name = Some(args[i + 1].clone());
            i += 2;
        } else if matches!(args[i].as_str(),
            "--verbose" | "-v" | "--jit" | "--debug" | "--release" | "-c" | "--aot-run"
        ) {
            i += 1;
        } else if args[i].starts_with("-") {
            i += 1;
        } else {
            if filename.is_none() {
                filename = Some(args[i].clone());
            }
            i += 1;
        }
    }

    let filename = match filename {
        Some(f) => f,
        None => {
            eprintln!("{}", "Error: No filename provided".red());
            process::exit(1);
        }
    };

    let source = get_source(&filename);
    let source_for_errors = source.clone();

    if is_verbose {
        println!("===== Step 0: Reprint Source =====");
        println!("{}", source);
    }
    let error_fmt = ErrorFormatter::new(filename.clone(), source_for_errors);

    let mut lexer = Lexer::new(source);
    if is_verbose {
        let mut tk = lexer.get_next_token();
        println!("===== Step 1: Tokenize =====");
        while tk.r#type != token::TokenType::EndOfFile {
            println!(
                "Token(Type={}, Val='{}')",
                tk.r#type,
                if tk.value == "\n" { "\\n".to_string() } else { tk.value.clone() }
            );
            tk = lexer.get_next_token();
        }
        println!();
        println!();
        println!("======= Step 2: AST =======");
        lexer.reset_position();
    }

    let mut builder = AstBuilder::new(lexer);
    builder.set_error_formatter(error_fmt.clone());
    let prog = builder.build();
    if builder.has_error() {
        for msg in builder.get_error_message() {
            eprintln!("{}", msg.red());
        }
        process::exit(1);
    }

    let prog = match prog {
        Some(p) => p,
        None => {
            eprintln!("{}", "Failed to build AST".red());
            process::exit(1);
        }
    };

    if is_verbose {
        let mut printer = AstPrinter::new();
        printer.visit(prog.as_ref());
        println!();
        println!();
        println!("======= Step 3: Semantic Analysis =======");
    }

    // Build lib search paths
    let mut lib_paths = Vec::new();

    if let Some(parent) = Path::new(&filename).parent() {
        if let Some(p) = parent.join("lib").to_str() {
            lib_paths.push(p.to_string());
        }
        if let Some(grandparent) = parent.parent() {
            if let Some(p) = grandparent.join("lib").to_str() {
                lib_paths.push(p.to_string());
            }
        }
    }

    for path in lib_paths_from_cli {
        lib_paths.push(path);
    }

    lib_paths.push("std".to_string());

    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            if let Some(p) = exe_dir.parent().map(|d| d.join("std")).and_then(|d| d.to_str().map(|s| s.to_string())) {
                lib_paths.push(p);
            }
            if let Some(p) = exe_dir.join("std").to_str().map(|s| s.to_string()) {
                lib_paths.push(p);
            }
        }
    }

    if let Ok(install_dir) = env::var("GOBOL_INSTALL_DIR") {
        let std_path = Path::new(&install_dir).join("lib").join("std");
        if let Some(p) = std_path.to_str() {
            lib_paths.push(p.to_string());
        }
        let alt = Path::new(&install_dir).join("std");
        if let Some(p) = alt.to_str() {
            lib_paths.push(p.to_string());
        }
    }

    if is_verbose {
        println!("Library paths: {:?}", lib_paths);
    }

    let mut semantic_analyzer = SemanticAnalyzer::new();
    semantic_analyzer.set_main_file(&filename);
    semantic_analyzer.set_lib_paths(lib_paths.clone());
    semantic_analyzer.set_error_formatter(error_fmt.clone());
    let semantic_passed = semantic_analyzer.analyze(&prog);
    if !semantic_passed {
        process::exit(1);
    }

    // Build IR from AST
    let ir_builder = gobol::ir::IRBuilder::new();
    let mut ir = match ir_builder.build(&prog) {
        Ok(ir) => ir,
        Err(errors) => {
            eprintln!("{}", "IR build failed:".red());
            for msg in &errors {
                eprintln!("{}", msg.red());
            }
            process::exit(1);
        }
    };

    // Process imports
    for stmt in prog.get_statements() {
        if let Some(import_stmt) = stmt.as_any().downcast_ref::<gobol::ast::ImportStatement>() {
            let module_name = import_stmt.get_module_name();
            let path_parts: Vec<String> = module_name
                .split("::")
                .flat_map(|s| s.split('.'))
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if let Some(module_path) = resolve_module_file(&path_parts, &lib_paths, &filename) {
                if let Ok(source) = fs::read_to_string(&module_path) {
                    let mod_lexer = gobol::lexer::Lexer::new(source);
                    let mut mod_builder = gobol::ast_builder::AstBuilder::new(mod_lexer);
                    mod_builder.set_error_formatter(error_fmt.clone());
                    if let Some(mod_prog) = mod_builder.build() {
                        if !mod_builder.has_error() {
                            let mod_ir_builder = gobol::ir::IRBuilder::new();
                            if let Ok(mod_ir) = mod_ir_builder.build(&mod_prog) {
                                let alias = import_stmt.get_alias().map(|a| a.to_string());
                                let is_builtin = module_name == "io" || module_name.ends_with("::io");
                                for f in &mod_ir.functions {
                                    if !f.is_main && !f.is_method {
                                        let mut f = f.clone();
                                        if is_builtin { f.body = None; }
                                        if let Some(ref a) = alias {
                                            let mut fa = f.clone();
                                            fa.name = format!("{}::{}", a, f.name);
                                            ir.functions.push(fa);
                                        }
                                        f.name = format!("{}::{}", module_name, f.name);
                                        ir.functions.push(f);
                                    }
                                }
                                for imp in &mod_ir.impls {
                                    ir.impls.push(imp.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Monomorphize (expand generics)
    let mut monomorphizer = gobol::ir::Monomorphizer::new();
    let concrete_ir = monomorphizer.monomorphize(&ir);

    match mode {
        Mode::Jit => run_jit(&concrete_ir, is_verbose),
        Mode::AotRun | Mode::AotNoRun => {
            // Default output name derives from the source file stem.
            let out = out_name.unwrap_or_else(|| {
                Path::new(&filename)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("a.out")
                    .to_string()
            });
            let run_after = matches!(mode, Mode::AotRun);
            run_aot(&concrete_ir, &out, is_verbose, run_after)
        }
    }
}

/// JIT: compile in-memory with Cranelift and execute `main` immediately.
fn run_jit(concrete_ir: &gobol::ir::GobolIR, is_verbose: bool) {
    if is_verbose {
        println!("======= Step 4: JIT Codegen (Cranelift) =======");
    }
    let mut backend = CraneliftBackend::new();
    if let Err(e) = backend.compile_ir(concrete_ir) {
        eprintln!("{}", format!("JIT compilation failed: {}", e).red());
        process::exit(2);
    }
    if let Err(e) = backend.finalize() {
        eprintln!("{}", format!("JIT finalize failed: {}", e).red());
        process::exit(2);
    }
    match backend.run() {
        Ok(code) => process::exit(code as i32),
        Err(e) => {
            eprintln!("{}", format!("JIT run failed: {}", e).red());
            process::exit(2);
        }
    }
}

/// AOT: emit a standalone binary via Cranelift ObjectModule + C runtime linker.
/// If `run_after` is true, execute the produced binary after linking.
fn run_aot(
    concrete_ir: &gobol::ir::GobolIR,
    out_name: &str,
    is_verbose: bool,
    run_after: bool,
) {
    if is_verbose {
        println!("======= Step 4: AOT Codegen (Cranelift ObjectModule) =======");
    }

    let runtime_c = match find_runtime_c() {
        Some(p) => p,
        None => {
            eprintln!(
                "{}",
                "Error: cannot locate runtime.c for AOT linking. \
                 Set GOBOL_RUNTIME or run from the project / install tree."
                    .red()
            );
            process::exit(2);
        }
    };
    if is_verbose {
        println!("Using runtime: {}", runtime_c.display());
    }

    let backend = CraneliftBackend::new_aot();
    if let Err(e) = backend.compile_to_binary(concrete_ir, out_name, runtime_c.to_str().unwrap()) {
        eprintln!("{}", format!("AOT compilation failed: {}", e).red());
        process::exit(2);
    }

    if is_verbose {
        println!("AOT binary written to {}", out_name);
    }

    if run_after {
        // Bare names need a `./` prefix to execute from the cwd; paths
        // (absolute or containing a separator) are executed as-is.
        let run_path = if out_name.contains(std::path::MAIN_SEPARATOR) || out_name.starts_with("./") {
            out_name.to_string()
        } else {
            format!("./{}", out_name)
        };
        let status = process::Command::new(&run_path)
            .status()
            .map_err(|e| {
                eprintln!("{}", format!("Failed to run {}: {}", run_path, e).red());
                process::exit(2);
            })
            .unwrap();
        process::exit(status.code().unwrap_or(1));
    }
}
