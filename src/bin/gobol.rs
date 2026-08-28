use gobol::ast_builder::AstBuilder;
use gobol::ast_printer::AstPrinter;
use gobol::cranelift::{CraneliftBackend, LinkOptions, host_target_string, target_is_bare_metal};
use gobol::error::ErrorFormatter;
use gobol::lexer::Lexer;
use gobol::semantic_analyzer::{SemanticAnalyzer, BuildMode};
use gobol::token;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use colored::*;

fn resolve_module_file(path_parts: &[String], lib_paths: &[String], main_file: &str) -> Option<String> {
    let relative = format!("{}.gbl", path_parts.join("/"));
    let mod_relative = format!("{}/mod.gbl", path_parts.join("/"));
    let setup_relative = format!("{}/__setup__.gbl", path_parts.join("/"));

    if let Some(parent) = Path::new(main_file).parent() {
        let p = parent.join(&relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
        let p = parent.join(&mod_relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
        let p = parent.join(&setup_relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
    }
    for lp in lib_paths {
        let p = Path::new(lp).join(&relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
        let p = Path::new(lp).join(&mod_relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
        let p = Path::new(lp).join(&setup_relative);
        if p.exists() { return p.to_str().map(|s| s.to_string()); }
    }
    if Path::new(&relative).exists() { return Some(relative); }
    if Path::new(&mod_relative).exists() { return Some(mod_relative); }
    if Path::new(&setup_relative).exists() { return Some(setup_relative); }
    None
}

fn find_runtime_c(lib_paths: &[String]) -> Option<PathBuf> {
    if let Ok(p) = env::var("GOBOL_RUNTIME") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    // Also check lib_paths for a runtime.c (e.g. when --lib-path points to
    // the project root that contains std/runtime.c).
    for lp in lib_paths {
        let p = PathBuf::from(lp).join("std").join("runtime.c");
        if p.exists() { return Some(p); }
        let p = PathBuf::from(lp).join("runtime.c");
        if p.exists() { return Some(p); }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                // Preferred: std/ next to workspace root (dev layout)
                dir.join("..").join("..").join("std").join("runtime.c"),
                dir.join("..").join("std").join("runtime.c"),
                dir.join("std").join("runtime.c"),
                // Installed layout: lib/runtime.c
                dir.join("..").join("lib").join("runtime.c"),
                dir.join("..").join("..").join("lib").join("runtime.c"),
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
    // Explicit fallback relative to CWD (works for `gobol build` invocations
    // run directly from the repository root).
    let rel = PathBuf::from("std/runtime.c");
    if rel.exists() {
        return Some(rel);
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
    println!("  gobol build <file.gbl> [--debug|--release] [-o name] [--lib-path path]  Compile to binary");
    println!("  gobol <file.gbl>                                                           Alias for 'gobol build <file> --debug'");
    println!("  gobol --version                                                            Show version information");
    println!("  gobol --help                                                               Show this help message");
    println!();
    println!("Options:");
    println!("  --debug                  Debug build (default)");
    println!("  --release                Release build (optimized)");
    println!("  -O<n>                    Optimization level 0-2 (0=none, 1=speed, 2=speed_and_size).");
    println!("                           Default: 0 for debug, 2 for release.");
    println!("  -o <name>                Output binary name");
    println!("  --verbose, -v            Enable verbose output");
    println!("  --lib-path <path>        Add a library search path (can be used multiple times)");
    println!("  --target <triple>        Cross-compile for a target triple (e.g. x86_64-pc-windows-msvc)");
    println!("  --entry-point <name>     Custom entry symbol (default: main). When set to");
    println!("                           anything else, a main() function is not required.");
    println!("  --link-script <path>     Custom linker script (bare-metal / kernel builds)");
    println!("  --no-std                 Don't link the C runtime (no_std / bare-metal)");
    println!("  --no-main                Don't require a main() function (kernel entry)");
    println!("  --link-arg <lib>         Append a library base name to the link line (repeatable).");
    println!("                           Formatted per linker: ws2_32 -> ws2_32.lib (MSVC) / -lws2_32 (cc)");
    println!();
    println!("Examples:");
    println!("  gobol main.gbl                         Debug build (alias for 'gobol build main.gbl --debug')");
    println!("  gobol build main.gbl                   Debug build");
    println!("  gobol build main.gbl --release -o myapp  Release build, output ./myapp");
    println!("  gobol build boot.gbl --target x86_64-pc-windows-msvc -o myapp");
    println!("  gobol build boot.gbl --target aarch64-unknown-none --entry-point _start --link-script kernel.ld");
}

/// Parsed cross-compilation / linking options gathered from CLI flags.
#[derive(Clone)]
struct LinkCli {
    target: Option<String>,
    entry_point: Option<String>,
    link_script: Option<String>,
    no_std: bool,
    no_main: bool,
    no_gc: bool,
    /// Extra library base names collected from repeatable `--link-arg <lib>`
    /// flags (e.g. `ws2_32`). These are appended to `LinkOptions::link_libs`
    /// and formatted per linker kind: `ws2_32.lib` for MSVC, `-lws2_32` for
    /// the cc-driver path. Lets the package manager (`grape`) inject Windows
    /// system libraries that the C runtime transitively needs.
    link_args: Vec<String>,
}

impl Default for LinkCli {
    fn default() -> Self {
        Self {
            target: None,
            entry_point: None,
            link_script: None,
            no_std: false,
            no_main: false,
            no_gc: false,
            link_args: Vec::new(),
        }
    }
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
        println!("Gobol 0.2.0");
        return;
    }

    let is_verbose = args.iter().any(|s| s == "--verbose" || s == "-v");
    let is_check_only = args.iter().any(|s| s == "--check");

    let mut build_mode = BuildMode::Debug;
    let mut out_name: Option<String> = None;
    let mut lib_paths_from_cli: Vec<String> = Vec::new();
    let mut link_cli = LinkCli::default();
    // Optimization level from `-O0`/`-O1`/`-O2` / `--opt-level N`.
    let mut opt_level: Option<usize> = None;

    let mut i = 1;
    let mut filename: Option<String> = None;
    let mut has_build_subcommand = false;

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
        } else if args[i] == "--target" && i + 1 < args.len() {
            link_cli.target = Some(args[i + 1].clone());
            i += 2;
        } else if args[i] == "--entry-point" && i + 1 < args.len() {
            link_cli.entry_point = Some(args[i + 1].clone());
            i += 2;
        } else if args[i] == "--link-script" && i + 1 < args.len() {
            link_cli.link_script = Some(args[i + 1].clone());
            i += 2;
        } else if args[i] == "--link-arg" && i + 1 < args.len() {
            // Repeatable: `--link-arg ws2_32` may appear multiple times.
            // Each value is a library base name (no `-l`/`.lib` suffix);
            // the backend formats it per linker kind.
            link_cli.link_args.push(args[i + 1].clone());
            i += 2;
        } else if args[i] == "--no-std" {
            link_cli.no_std = true;
            i += 1;
        } else if args[i] == "--no-main" {
            link_cli.no_main = true;
            i += 1;
        } else if args[i] == "--no-gc" {
            link_cli.no_gc = true;
            i += 1;
        } else if args[i] == "--release" {
            build_mode = BuildMode::Release;
            i += 1;
        } else if args[i] == "--debug" {
            build_mode = BuildMode::Debug;
            i += 1;
        } else if args[i] == "--opt-level" && i + 1 < args.len() {
            opt_level = match args[i + 1].parse::<usize>() {
                Ok(n) => Some(n),
                Err(_) => None,
            };
            i += 2;
        } else if let Some(digits) = args[i].strip_prefix("-O") {
            // -O0 / -O1 / -O2
            opt_level = digits.parse::<usize>().ok();
            i += 1;
        } else if matches!(args[i].as_str(), "--verbose" | "-v") {
            i += 1;
        } else if args[i].starts_with("-") {
            i += 1;
        } else {
            if filename.is_none() {
                if args[i] == "build" && !has_build_subcommand {
                    has_build_subcommand = true;
                    i += 1;
                } else {
                    filename = Some(args[i].clone());
                    i += 1;
                }
            } else {
                i += 1;
            }
        }
    }

    // Effective target: CLI --target, else host. no_std is implied for
    // bare-metal triples unless explicitly overridden.
    let target = link_cli.target.clone().unwrap_or_else(host_target_string);
    if target_is_bare_metal(&target) {
        link_cli.no_std = true;
        // A non-main entry point implies "don't require main()".
        if link_cli
            .entry_point
            .as_deref()
            .map(|e| e != "main")
            .unwrap_or(false)
        {
            link_cli.no_main = true;
        }
    }
    // Any explicit non-main entry point implies no_main regardless of target.
    if link_cli
        .entry_point
        .as_deref()
        .map(|e| e != "main")
        .unwrap_or(false)
        || link_cli.no_main
    {
        link_cli.no_main = true;
    }

    if !has_build_subcommand && filename.is_some() {
        build_mode = BuildMode::Debug;
    }

    let filename = match filename {
        Some(f) => f,
        None => {
            eprintln!("{}", "Error: No filename provided".red());
            process::exit(1);
        }
    };

    let source = get_source(&filename);
    // A global `--no-gc` (from `grape.toml`'s `build.no_gc` or CLI) injects
    // a synthetic file-level `#![no_gc]` attribute, which the attribute
    // system then propagates to every function — routing allocations to the
    // non-GC allocator. This reuses the Task-1 file-attribute machinery.
    let source = if link_cli.no_gc {
        format!("#![no_gc]\n{}", source)
    } else {
        source
    };
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
            // target/release/  →  target/std  (installed layout)
            if let Some(p) = exe_dir.parent().map(|d| d.join("std")).and_then(|d| d.to_str().map(|s| s.to_string())) {
                lib_paths.push(p);
                // Also add the parent so `import std;` can find std/mod.gbl
                if let Some(pp) = exe_dir.parent().and_then(|d| d.to_str().map(|s| s.to_string())) {
                    lib_paths.push(pp);
                }
                // Installed layout with lib/: ~/.gobol/bin/gobol → ~/.gobol/lib
                // (std/mod.gbl lives at <install>/lib/std/mod.gbl)
                if let Some(p) = exe_dir.parent().map(|d| d.join("lib")).and_then(|d| d.to_str().map(|s| s.to_string())) {
                    lib_paths.push(p);
                }
            }
            // target/release/  →  <workspace>/std  (dev builds)
            if let Some(workspace) = exe_dir.parent().and_then(|d| d.parent()) {
                if let Some(p) = workspace.join("std").to_str().map(|s| s.to_string()) {
                    lib_paths.push(p);
                }
                if let Some(p) = workspace.to_str().map(|s| s.to_string()) {
                    lib_paths.push(p);
                }
            }
            if let Some(p) = exe_dir.join("std").to_str().map(|s| s.to_string()) {
                lib_paths.push(p);
            }
        }
    }

    if let Ok(install_dir) = env::var("GOBOL_INSTALL_DIR") {
        // lib_paths entries must point at the *parent* of the std/ directory so
        // that `import std;` resolves to <lib_path>/std/mod.gbl. The installed
        // layout is <install>/lib/std/mod.gbl (also <install>/std/mod.gbl for
        // legacy installs). See grape.rs find_std_path() for the same rule.
        let install = Path::new(&install_dir);
        if let Some(p) = install.join("lib").to_str() {
            lib_paths.push(p.to_string());
        }
        if let Some(p) = install.to_str() {
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
    semantic_analyzer.set_build_mode(build_mode);
    let semantic_passed = semantic_analyzer.analyze(&prog);
    if !semantic_passed {
        process::exit(1);
    }

    // --check: stop after semantic analysis (parse + type-check only).
    if is_check_only {
        println!("{}", "Semantic check passed.".green());
        process::exit(0);
    }

    let mut ir_builder = gobol::ir::IRBuilder::new();
    ir_builder.set_current_file(filename.clone());
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

    fn resolve_module_file_relative(path_parts: &[String], parent_file: &str) -> Option<String> {
        let relative = format!("{}.gbl", path_parts.join("/"));
        let mod_relative = format!("{}/mod.gbl", path_parts.join("/"));
        let setup_relative = format!("{}/__setup__.gbl", path_parts.join("/"));
        if let Some(parent) = Path::new(parent_file).parent() {
            for rel in [&relative, &mod_relative, &setup_relative] {
                let p = parent.join(rel);
                if p.exists() {
                    return p.to_str().map(|s| s.to_string());
                }
            }
        }
        None
    }

    fn load_module_into_ir(
        module_name: &str,
        module_file: &str,
        lib_paths: &[String],
        error_fmt: &ErrorFormatter,
        ir: &mut gobol::ir::GobolIR,
        visited: &mut std::collections::HashSet<String>,
    ) {
        if !visited.insert(module_file.to_string()) {
            return;
        }
        let source = match fs::read_to_string(module_file) {
            Ok(s) => s,
            Err(_) => return,
        };
        let mod_lexer = gobol::lexer::Lexer::new(source);
        let mut mod_builder = gobol::ast_builder::AstBuilder::new(mod_lexer);
        mod_builder.set_error_formatter(error_fmt.clone());
        let mod_prog = match mod_builder.build() {
            Some(p) => p,
            None => return,
        };
        if mod_builder.has_error() {
            return;
        }
        let mut mod_ir_builder = gobol::ir::IRBuilder::new();
        mod_ir_builder.set_current_file(module_file);
        let mod_ir = match mod_ir_builder.build(&mod_prog) {
            Ok(ir) => ir,
            Err(_) => return,
        };

        let short_name = module_name.rsplit("::").next().unwrap_or(module_name);
        let is_builtin = short_name == "io";

        let mut existing_names: std::collections::HashSet<String> =
            ir.functions.iter().map(|f| f.name.clone()).collect();

        for f in &mod_ir.functions {
            if f.is_main || f.is_method {
                continue;
            }
            let mut f_named = f.clone();
            if is_builtin {
                f_named.body = None;
            }
            f_named.name = format!("{}::{}", module_name, f.name);
            if existing_names.insert(f_named.name.clone()) {
                ir.functions.push(f_named.clone());
            }
            if short_name != module_name {
                let mut f_short = f_named.clone();
                f_short.name = format!("{}::{}", short_name, f.name);
                if existing_names.insert(f_short.name.clone()) {
                    ir.functions.push(f_short);
                }
            }
            let mut f_bare = f_named.clone();
            f_bare.name = f.name.clone();
            if existing_names.insert(f_bare.name.clone()) {
                ir.functions.push(f_bare);
            }
        }
        for imp in &mod_ir.impls {
            // Dedup by (struct_name, method_name, param_count) — skip methods
            // that are already present in the IR. This allows overloaded
            // methods (same name, different arity) to coexist.
            let struct_name = imp.struct_name.clone();
            let existing_sigs: std::collections::HashSet<(String, usize)> = ir
                .impls
                .iter()
                .filter(|e| e.struct_name == struct_name)
                .flat_map(|e| e.methods.iter().map(|m| (m.name.clone(), m.params.len())))
                .collect();
            let new_methods: Vec<&gobol::ir::IRFunction> = imp
                .methods
                .iter()
                .filter(|m| !existing_sigs.contains(&(m.name.clone(), m.params.len())))
                .collect();
            if new_methods.is_empty() {
                continue;
            }
            // If the struct already has an impl block, merge into it;
            // otherwise create a new one.
            if let Some(existing) = ir.impls.iter_mut().find(|e| e.struct_name == struct_name) {
                for m in new_methods {
                    existing.methods.push(m.clone());
                }
            } else {
                ir.impls.push(imp.clone());
            }
        }

        // Merge struct definitions from the module into the main IR.
        // Without this, the Cranelift backend's TypeResolver has no field
        // layout information for library structs (e.g. Range), causing
        // field accesses via MemberAccess to silently fall back to
        // returning the struct pointer instead of loading the field.
        let existing_struct_names: std::collections::HashSet<String> =
            ir.structs.iter().map(|s| s.name.clone()).collect();
        for s in &mod_ir.structs {
            if !existing_struct_names.contains(&s.name) {
                ir.structs.push(s.clone());
            }
        }

        for stmt in mod_prog.get_statements() {
            if let Some(import_stmt) = stmt.as_any().downcast_ref::<gobol::ast::ImportStatement>() {
                let sub_name = import_stmt.get_module_name();
                let sub_parts: Vec<String> = sub_name
                    .split("::")
                    .flat_map(|s| s.split('.'))
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if let Some(sub_file) = resolve_module_file_relative(&sub_parts, module_file) {
                    load_module_into_ir(&sub_name, &sub_file, lib_paths, error_fmt, ir, visited);
                } else if let Some(sub_file) = resolve_module_file(&sub_parts, lib_paths, module_file) {
                    load_module_into_ir(&sub_name, &sub_file, lib_paths, error_fmt, ir, visited);
                }
            } else if let Some(from_import) = stmt.as_any().downcast_ref::<gobol::ast::FromImportStatement>() {
                // `from module import member, ...` — load the module into IR.
                // The bare-name function entries are created automatically by
                // load_module_into_ir, so from-import members are resolvable.
                let sub_name = from_import.get_module();
                let sub_parts: Vec<String> = sub_name
                    .split("::")
                    .flat_map(|s| s.split('.'))
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if let Some(sub_file) = resolve_module_file_relative(&sub_parts, module_file) {
                    load_module_into_ir(sub_name, &sub_file, lib_paths, error_fmt, ir, visited);
                } else if let Some(sub_file) = resolve_module_file(&sub_parts, lib_paths, module_file) {
                    load_module_into_ir(sub_name, &sub_file, lib_paths, error_fmt, ir, visited);
                }
            }
        }
    }

    {
        let mut visited = std::collections::HashSet::new();
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
                    load_module_into_ir(
                        &module_name,
                        &module_path,
                        &lib_paths,
                        &error_fmt,
                        &mut ir,
                        &mut visited,
                    );
                } else {
                    // Module resolution failure is non-fatal here; the
                    // semantic analyzer already reported the error.
                }
            } else if let Some(from_import) = stmt.as_any().downcast_ref::<gobol::ast::FromImportStatement>() {
                // `from module import member, ...` — load the module so its
                // functions (including the requested members) are available
                // in the IR for code generation.
                let module_name = from_import.get_module();
                let path_parts: Vec<String> = module_name
                    .split("::")
                    .flat_map(|s| s.split('.'))
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if let Some(module_path) = resolve_module_file(&path_parts, &lib_paths, &filename) {
                    load_module_into_ir(
                        module_name,
                        &module_path,
                        &lib_paths,
                        &error_fmt,
                        &mut ir,
                        &mut visited,
                    );
                }
            }
        }
    }

    let mut monomorphizer = gobol::ir::Monomorphizer::new();
    let concrete_ir = monomorphizer.monomorphize(&ir);

    let out = out_name.unwrap_or_else(|| {
        Path::new(&filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("a.out")
            .to_string()
    });

    if is_verbose {
        println!("======= Step 4: AOT Codegen (Cranelift ObjectModule) =======");
    }

    // For no_std / bare-metal targets there is no C runtime to link.
    let runtime_c = if link_cli.no_std {
        None
    } else {
        match find_runtime_c(&lib_paths) {
            Some(p) => Some(p),
            None => {
                eprintln!(
                    "{}",
                    "Error: cannot locate runtime.c for AOT linking. \
                     Set GOBOL_RUNTIME or run from the project / install tree."
                        .red()
                );
                process::exit(2);
            }
        }
    };
    if is_verbose {
        if let Some(p) = &runtime_c {
            println!("Using runtime: {}", p.display());
        } else {
            println!("no_std build: skipping C runtime");
        }
        println!("Target: {}", target);
    }

    // Effective optimization level: explicit CLI `-O`, else the build-mode
    // default (release → 2, debug → 0). Levels must be 0–2.
    let opt_level = opt_level.unwrap_or(if build_mode == BuildMode::Release { 2 } else { 0 });
    if opt_level > 2 {
        eprintln!(
            "{}",
            format!("Error: invalid optimization level {} (must be 0, 1, or 2)", opt_level).red()
        );
        process::exit(1);
    }

    let backend = match CraneliftBackend::new_for_target_with_opt(&target, opt_level) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}", format!("Target setup failed: {}", e).red());
            process::exit(2);
        }
    };
    let extern_libs = semantic_analyzer.get_extern_libs().clone();
    // Merge libraries declared via `extern "C"` blocks with any extra
    // `--link-arg <lib>` flags (e.g. `ws2_32` injected by `grape` on
    // Windows). Duplicates are deduped to avoid double-linking.
    let mut link_libs = extern_libs.clone();
    for extra in &link_cli.link_args {
        if !link_libs.iter().any(|l| l == extra) {
            link_libs.push(extra.clone());
        }
    }
    let link_opts = LinkOptions {
        target: target.clone(),
        runtime_c_path: runtime_c.as_ref().map(|p| p.to_string_lossy().into_owned()),
        link_libs,
        link_script: link_cli.link_script.clone(),
        entry_point: link_cli.entry_point.clone(),
    };
    if let Err(e) = backend.compile_to_binary(&concrete_ir, &out, &link_opts) {
        eprintln!("{}", format!("AOT compilation failed: {}", e).red());
        process::exit(2);
    }

    if is_verbose {
        println!("AOT binary written to {}", out);
    }
}
