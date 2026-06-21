use gobol::ast_builder::AstBuilder;
use gobol::ast_printer::AstPrinter;
use gobol::ccompiler::CCompiler;
use gobol::codegen_c::CodeGenC;
use gobol::error::ErrorFormatter;
use gobol::lexer::Lexer;
use gobol::semantic_analyzer::SemanticAnalyzer;
use gobol::token;
use std::env;
use std::fs;
use std::path::Path;
use std::process;
use colored::*;

fn resolve_module_file(path_parts: &[String], lib_paths: &[String], main_file: &str) -> Option<String> {
    let relative = format!("{}.gbl", path_parts.join("/"));
    // Check relative to main file's directory
    if let Some(parent) = Path::new(main_file).parent() {
        let p = parent.join(&relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
    }
    // Check lib paths
    for lp in lib_paths {
        let p = Path::new(lp).join(&relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
    }
    // Direct path
    if Path::new(&relative).exists() { return Some(relative); }
    None
}

fn get_source(file: &str) -> String {
    let source = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Cannot open file '{}': {}", file, e);
            process::exit(1);
        }
    };
    source
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let is_version = args.iter().any(|s| s == "--version");
    let is_help = args.iter().any(|s| s == "--help");
    
    if is_help {
        println!("Gobol - A statically compiled programming language");
        println!();
        println!("Usage:");
        println!("  gobol <filename> [options]                      Compile and run Gobol");
        println!("  gobol <filename> -o <out> [options]             Compile to <out> and run");
        println!("  gobol --version                                 Show version information");
        println!("  gobol --help                                    Show this help message");
        println!();
        println!("Options:");
        println!("  -o <file>                                       Output file name");
        println!("  --save-c, -s                                    Save the generated C file");
        println!("  --verbose, -v                                   Enable verbose output");
        println!("  --lib-path <path>                               Add a library search path (can be used multiple times)");
        println!();
        println!("Examples:");
        println!("  gobol main.gbl                                  Compile and run");
        println!("  gobol main.gbl -o myapp                        Compile to myapp and run");
        return;
    }
    if is_version {
        println!("Gobol 0.1.0");
        return;
    }

    let is_verbose = args.iter().any(|s| s == "--verbose" || s == "-v");
    let is_save_c = args.iter().any(|s| s == "--save-c" || s == "-s");

    // Parse --lib-path arguments (support multiple)
    let mut lib_paths_from_cli: Vec<String> = Vec::new();
    let mut i = 1;
    let mut filename = None;
    let mut out_name: Option<String> = None;

    while i < args.len() {
        if args[i] == "--lib-path" && i + 1 < args.len() {
            let paths_str = &args[i + 1];
            for p in paths_str.split(',') {
                if !p.is_empty() {
                    lib_paths_from_cli.push(p.to_string());
                }
            }
            i += 2;
        } else if args[i] == "-o" || args[i] == "--output" {
            if i + 1 < args.len() {
                out_name = Some(args[i + 1].clone());
                i += 2;
            } else {
                i += 1;
            }
        } else if args[i] == "--verbose" || args[i] == "-v" {
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

    // Default output name derived from input filename: tmp.gbl → tmp.out
    let out_name = out_name.unwrap_or_else(|| {
        let stem = Path::new(&filename).file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("a");
        let ext = if cfg!(target_os = "windows") { "exe" } else { "out" };
        format!("{}.{}", stem, ext)
    });

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

    // Build lib search paths: local script lib first, then std, then CLI paths
    let mut lib_paths = Vec::new();

    if let Some(parent) = Path::new(&filename).parent() {
        // 1. <script_dir>/lib (highest priority — local overrides)
        if let Some(p) = parent.join("lib").to_str() {
            lib_paths.push(p.to_string());
        }
        // 2. <script_dir>/../lib
        if let Some(grandparent) = parent.parent() {
            if let Some(p) = grandparent.join("lib").to_str() {
                lib_paths.push(p.to_string());
            }
        }
    }

    // 3. CLI lib paths (--lib-path arguments)
    for path in lib_paths_from_cli {
        lib_paths.push(path);
    }

    // 4. ./std (development stdlib, relative to CWD)
    lib_paths.push("std".to_string());

    // 5. <gobol_binary_dir>/../std (installed alongside binary)
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

    // 6. $GOBOL_INSTALL_DIR/std (installed stdlib, lowest priority)
    if let Ok(install_dir) = env::var("GOBOL_INSTALL_DIR") {
        let std_path = Path::new(&install_dir).join("std");
        if let Some(p) = std_path.to_str() {
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

    if is_verbose {
        println!();
        println!("======= Step 4: C Codegen =======");
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

    // Process imports: parse imported modules and merge their IR functions
    for stmt in prog.get_statements() {
        if let Some(import_stmt) = stmt.as_any().downcast_ref::<gobol::ast::ImportStatement>() {
            let module_name = import_stmt.get_module_name();
            let path_parts: Vec<String> = module_name.split('.').map(|s| s.to_string()).collect();
            // Resolve module path
            if let Some(module_path) = resolve_module_file(&path_parts, &lib_paths, &filename) {
                if let Ok(source) = fs::read_to_string(&module_path) {
                    let mod_lexer = gobol::lexer::Lexer::new(source);
                    let mut mod_builder = gobol::ast_builder::AstBuilder::new(mod_lexer);
                    mod_builder.set_error_formatter(error_fmt.clone());
                    if let Some(mod_prog) = mod_builder.build() {
                        if !mod_builder.has_error() {
                            let mod_ir_builder = gobol::ir::IRBuilder::new();
                            if let Ok(mod_ir) = mod_ir_builder.build(&mod_prog) {
                                // Merge functions — register under both full name and alias
                                let alias = import_stmt.get_alias().map(|a| a.to_string());
                                // Builtin modules (with C companions) → strip bodies
                                let is_builtin = module_name == "io";
                                for f in &mod_ir.functions {
                                    if !f.is_main && !f.is_method {
                                        let mut f = f.clone();
                                        if is_builtin { f.body = None; }
                                        // Register under alias if present (e.g. m.add)
                                        if let Some(ref a) = alias {
                                            let mut fa = f.clone();
                                            fa.name = format!("{}.{}", a, f.name);
                                            ir.functions.push(fa);
                                        }
                                        // Also register under full module name
                                        f.name = format!("{}.{}", module_name, f.name);
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

    let mut codegen = CodeGenC::new();
    let c_source = codegen.generate(&concrete_ir);

    if is_verbose {
        println!("{}", c_source);
    }

    // Collect C companion files from lib paths (std/c/*.c)
    let mut c_files: Vec<String> = Vec::new();
    let mut seen_names = std::collections::HashSet::new();
    let mut add_c_file = |path: String| {
        let basename = Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&path)
            .to_string();
        if seen_names.insert(basename) {
            c_files.push(path);
        }
    };

    for lib_path in &lib_paths {
        let c_dir = Path::new(lib_path).join("c");
        if c_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&c_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().map_or(false, |e| e == "c") {
                        if let Some(s) = p.to_str() {
                            add_c_file(s.to_string());
                        }
                    }
                }
            }
        }
    }

    // Also check for a c/ directory alongside the binary
    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let c_dir = exe_dir.join("c");
            if c_dir.is_dir() {
                if let Ok(entries) = fs::read_dir(&c_dir) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.extension().map_or(false, |e| e == "c") {
                            if let Some(s) = p.to_str() {
                                add_c_file(s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Write generated C source
    let c_file = format!("{}.c", out_name);
    if let Err(e) = fs::write(&c_file, &c_source) {
        eprintln!("{}", format!("Failed to write C file '{}': {}", c_file, e).red());
        process::exit(1);
    }

    // Cross-platform compilation
    let compiler = CCompiler::detect();
    if is_verbose {
        println!("Compiler: {}", compiler.name());
    }
    let mut sources: Vec<String> = c_files.clone();
    sources.push(c_file.clone());
    let cc_status = compiler.compile(&sources, &out_name);

    if !is_save_c {
        let _ = fs::remove_file(&c_file);
    }

    match cc_status {
        Ok(status) => {
            if !status.success() {
                process::exit(status.code().unwrap_or(1));
            }
        }
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            process::exit(1);
        }
    }

    // Run the compiled binary (unless -c / compile-only)
    let compile_only = args.iter().any(|s| s == "-c");
    if !compile_only {
        match process::Command::new(format!("./{}", out_name)).status() {
            Ok(s) => process::exit(s.code().unwrap_or(0)),
            Err(e) => {
                eprintln!("Failed to run '{}': {}", out_name, e);
                process::exit(1);
            }
        }
    }
}
