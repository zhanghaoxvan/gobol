use gobol::ast_builder::AstBuilder;
use gobol::ast_printer::AstPrinter;
use gobol::codegen_c::CodeGenC;
use gobol::error::ErrorFormatter;
use gobol::executor::Executor;
use gobol::lexer::Lexer;
use gobol::semantic_analyzer::SemanticAnalyzer;
use gobol::token;
use std::env;
use std::fs;
use std::path::Path;

fn get_source(file: &str) -> String {
    let source = match fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: Cannot open file '{}': {}", file, e);
            std::process::exit(1);
        }
    };
    source
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let is_version = args.iter().any(|s| s == "--version");
    let is_help = args.iter().any(|s| s == "--help");
    
    if is_help {
        println!("Gobol - A test programming language");
        println!();
        println!("Usage:");
        println!("  gobol <filename> [--verbose] [--lib-path <path>...]    Run a Gobol script");
        println!("  gobol <filename> --compile [-o <out>]                  Compile to native binary via C");
        println!("  gobol --version                                        Show version information");
        println!("  gobol --help                                           Show this help message");
        println!();
        println!("Options:");
        println!("  --verbose, -v                                          Enable verbose output");
        println!("  --compile, -c                                          Compile to native binary (via C)");
        println!("  -o <file>                                              Output file name (with --compile)");
        println!("  --lib-path <path>                                      Add a library search path (can be used multiple times)");
        return;
    }
    if is_version {
        println!("gobol 0.1.0");
        return;
    }

    let is_verbose = args.iter().any(|s| s == "--verbose" || s == "-v");
    let is_compile = args.iter().any(|s| s == "--compile" || s == "-c");
    let out_name = args.iter().position(|s| s == "-o")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| "a.out".to_string());
    
    // Parse --lib-path arguments (support multiple)
    let mut lib_paths_from_cli: Vec<String> = Vec::new();
    let mut i = 1;
    let mut filename = None;

    while i < args.len() {
        if args[i] == "--lib-path" && i + 1 < args.len() {
            // Split by comma if multiple paths are joined
            let paths_str = &args[i + 1];
            for p in paths_str.split(',') {
                if !p.is_empty() {
                    lib_paths_from_cli.push(p.to_string());
                }
            }
            i += 2;
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
            eprintln!("Error: No filename provided");
            std::process::exit(1);
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
            eprintln!("{}", msg);
        }
        std::process::exit(1);
    }

    let prog = match prog {
        Some(p) => p,
        None => {
            eprintln!("Failed to build AST");
            std::process::exit(1);
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

    // 4. ./std (development stdlib, before installed)
    lib_paths.push("std".to_string());

    // 5. $GOBOL_INSTALL_DIR/std (installed stdlib, lowest priority)
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
        std::process::exit(1);
    }

    if is_compile {
        if is_verbose {
            println!();
            println!("======= Step 4: C Codegen =======");
        }
        let mut codegen = CodeGenC::new();
        let c_source = codegen.generate(&prog);
        if is_verbose {
            println!("{}", c_source);
        }
        match CodeGenC::compile(&c_source, &out_name) {
            Ok(exit_code) => std::process::exit(exit_code),
            Err(e) => {
                eprintln!("Compilation failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    if is_verbose {
        println!();
        println!("======= Step 4: Execution =======");
    }

    let mut executor = Executor::new();
    executor.set_main_file(&filename);
    executor.set_lib_paths(lib_paths);
    executor.set_error_formatter(error_fmt);
    match executor.execute(&prog) {
        Ok(exit_code) => {
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
        Err(errors) => {
            eprintln!("Runtime execution failed with {} error(s):", errors.len());
            for msg in &errors {
                eprintln!("{}", msg);
            }
            std::process::exit(1);
        }
    }
}
