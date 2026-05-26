mod ast;
mod ast_builder;
mod ast_printer;
mod environment;
mod executor;
mod lexer;
mod semantic_analyzer;
mod token;
mod value;

use ast_builder::AstBuilder;
#[cfg(debug_assertions)]
use ast_printer::AstPrinter;
use executor::Executor;
use lexer::Lexer;
use semantic_analyzer::SemanticAnalyzer;
use std::env;
use std::fs;
use std::path::Path;

fn get_source(file: &str) -> String {
    fs::read_to_string(file).unwrap_or_else(|_| {
        eprintln!("Error: Cannot open file '{}'", file);
        String::new()
    })
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() == 1 {
        println!("Author: zhanghaoxvan in Class 18, Grade 7");
        println!("Usage:");
        println!("  {} <filename>", args[0]);
        return;
    }

    let source = get_source(&args[1]);

    #[cfg(debug_assertions)]
    {
        println!("===== Step 0: Reprint Source =====");
        println!("{}", source);
    }
    #[cfg(debug_assertions)]
    let mut lexer = Lexer::new(source);
    #[cfg(not(debug_assertions))]
    let lexer = Lexer::new(source);

    #[cfg(debug_assertions)]
    {
        let mut tk = lexer.get_next_token();
        println!("===== Step 1: Tokenize =====");
        while tk.r#type != token::TokenType::EndOfFile {
            println!(
                "Token(Type={}, Val='{}')",
                token::token_type_to_string(&tk.r#type),
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
    let prog = builder.build();
    if builder.has_error() {
        for msg in builder.get_error_message() {
            eprintln!("Builder Error: {}", msg);
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

    #[cfg(debug_assertions)]
    {
        let mut printer = AstPrinter::new();
        printer.visit(prog.as_ref());
        println!();
        println!();
        println!("======= Step 3: Semantic Analysis =======");
    }

    // Build lib search paths: relative to CWD and relative to input file
    let mut lib_paths = vec!["lib".to_string()];
    if let Some(parent) = Path::new(&args[1]).parent() {
        if let Some(p) = parent.join("lib").to_str() {
            lib_paths.push(p.to_string());
        }
        if let Some(grandparent) = parent.parent() {
            if let Some(p) = grandparent.join("lib").to_str() {
                lib_paths.push(p.to_string());
            }
        }
    }

    let mut semantic_analyzer = SemanticAnalyzer::new();
    semantic_analyzer.set_lib_paths(lib_paths.clone());
    let semantic_passed = semantic_analyzer.analyze(&prog);
    if !semantic_passed {
        std::process::exit(1);
    }

    #[cfg(debug_assertions)]
    {
        println!();
        println!("======= Step 4: Execution =======");
    }

    let mut executor = Executor::new();
    executor.set_lib_paths(lib_paths);
    match executor.execute(&prog) {
        Ok(exit_code) => {
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
        Err(errors) => {
            for msg in &errors {
                eprintln!("Runtime Error: {}", msg);
            }
            std::process::exit(1);
        }
    }
}
