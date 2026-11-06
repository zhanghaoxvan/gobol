#![allow(dead_code)]

use crate::ast::*;
use crate::ast_builder::AstBuilder;
use crate::environment::*;
use crate::error::ErrorFormatter;
use crate::lexer::Lexer;
use crate::token::{Token, TokenType};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

fn resolve_enum_payload_type(
    scrutinee: &DataType,
    declared: &DataType,
    generic_params: &[String],
) -> DataType {
    let DataType::Struct(param_name) = declared else {
        return declared.clone();
    };
    let Some(index) = generic_params.iter().position(|param| param == param_name) else {
        return declared.clone();
    };
    let DataType::Struct(enum_name) = scrutinee else {
        return declared.clone();
    };
    let Some((_, arguments)) = enum_name.split_once('<') else {
        return declared.clone();
    };
    arguments
        .trim_end_matches('>')
        .split(',')
        .nth(index)
        .map(|arg| match arg.trim() {
            "int" => DataType::Int,
            "float" => DataType::Float,
            "str" => DataType::Str,
            "bool" => DataType::Bool,
            "byte" => DataType::Byte,
            other => DataType::Struct(other.to_string()),
        })
        .unwrap_or_else(|| declared.clone())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Debug,
    Release,
}

pub struct SemanticAnalyzer {
    env: Environment,
    errors: Vec<String>,
    has_error: bool,
    error_formatter: Option<ErrorFormatter>,
    current_function: String,
    current_function_return_type: DataType,
    has_return_statement: bool,
    loop_depth: i32,
    current_module: String,
    type_stack: Vec<DataType>,
    struct_fields: HashMap<String, HashMap<String, DataType>>,
    generic_struct_params: HashMap<String, Vec<String>>,
    generic_struct_args: HashMap<String, Vec<DataType>>,
    current_impl_struct: Option<String>,
    lib_paths: Vec<String>,
    loaded_modules: HashSet<String>,
    loaded_programs: Vec<Box<Program>>,
    current_module_dir: Option<String>,
    module_aliases: HashMap<String, String>,
    /// Per-module visibility: module-name → set of module names that the
    /// module may address with a *bare* qualifier (`mod::fn`). Each loaded
    /// module only gets the modules it explicitly imports (plus the compiler
    /// auto-loaded ones: builtins/mem/trait). So `import std;` grants `std::`
    /// but NOT bare `io::`; to use `io::println` you must `import io;` (or
    /// `import std::io;`). Fully-qualified `std::io::...` remains valid via
    /// the `std` visibility + prefix fallback.
    visible_modules: HashMap<String, std::collections::HashSet<String>>,
    current_generic_params: Vec<String>,
    build_mode: BuildMode,
    /// Trait definitions: trait_name -> { name, methods, generic_params }
    trait_defs: HashMap<String, TraitDefInfo>,
    /// Pending trait impl validations: (struct_name, trait_name, items)
    pending_trait_impls: Vec<(String, String, Vec<String>)>,
    /// Structured errors for LSP: (line, col, message)
    pub structured_errors: Vec<(i32, i32, String)>,
    diagnostic_tokens: Vec<Token>,
    current_error_line: i32,
    /// External libraries to link (e.g. "C", "m") from extern "C" blocks.
    pub extern_libs: Vec<String>,
    pub extern_links: Vec<ExternLink>,
    target: String,
    /// Tracks (struct_name, method_name, param_count) triples already declared
    /// via impl blocks. Prevents duplicate method definitions when a module is
    /// loaded multiple times or when multiple modules impl the same struct,
    /// while still allowing overloaded methods (same name, different arity,
    /// e.g. `Range::new(start, end)` vs `Range::new(start, end, step)`).
    impl_methods: HashSet<(String, String, usize)>,
    /// Generic function signatures for return-type resolution at call sites:
    /// name -> (generic params, param types, return type). Populated for
    /// top-level generic functions so `var a: float = identity(3.5)` type-checks.
    generic_signatures: HashMap<String, (Vec<String>, Vec<DataType>, DataType)>,
    function_signatures: HashMap<String, DataType>,
    /// 枚举变体信息: enum_name -> (variant_names, per-variant payload types)
    enum_info: HashMap<String, (Vec<String>, Vec<Vec<DataType>>)>,
    enum_generic_params: HashMap<String, Vec<String>>,
    /// 当前 match 中绑定的变量: arm_index -> (pattern_path -> var_name)
    match_bindings: Vec<HashMap<String, DataType>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternLink {
    pub name: String,
    pub kind: String,
    pub rename: Option<String>,
    pub version: Option<String>,
    pub optional: bool,
}

/// Registered trait method signature for validation
#[derive(Debug, Clone)]
struct TraitMethodSig {
    name: String,
    param_count: usize,
    dynamic: bool,
}

/// Registered trait definition for `impl Trait for Type` validation
#[derive(Debug, Clone)]
struct TraitDefInfo {
    name: String,
    methods: Vec<TraitMethodSig>,
    generic_params: Vec<String>,
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        SemanticAnalyzer {
            env: Environment::new(),
            errors: Vec::new(),
            has_error: false,
            error_formatter: None,
            current_function: String::new(),
            current_function_return_type: DataType::None_,
            has_return_statement: false,
            loop_depth: 0,
            current_module: String::new(),
            type_stack: Vec::new(),
            struct_fields: HashMap::new(),
            generic_struct_params: HashMap::new(),
            generic_struct_args: HashMap::new(),
            current_impl_struct: None,
            lib_paths: vec!["lib".to_string()],
            loaded_modules: HashSet::new(),
            loaded_programs: Vec::new(),
            current_module_dir: None,
            module_aliases: HashMap::new(),
            visible_modules: HashMap::new(),
            current_generic_params: Vec::new(),
            build_mode: BuildMode::Debug,
            trait_defs: HashMap::new(),
            pending_trait_impls: Vec::new(),
            structured_errors: Vec::new(),
            diagnostic_tokens: Vec::new(),
            current_error_line: 0,
            extern_libs: Vec::new(),
            extern_links: Vec::new(),
            target: String::new(),
            impl_methods: HashSet::new(),
            generic_signatures: HashMap::new(),
            function_signatures: HashMap::new(),
            enum_info: HashMap::new(),
            enum_generic_params: HashMap::new(),
            match_bindings: Vec::new(),
        }
    }

    pub fn set_error_formatter(&mut self, f: ErrorFormatter) {
        self.error_formatter = Some(f);
    }

    /// Provide source tokens so semantic diagnostics can point at the
    /// offending identifier instead of falling back to line 1, column 1.
    pub fn set_source(&mut self, source: &str) {
        let mut lexer = Lexer::new(source);
        self.diagnostic_tokens.clear();
        loop {
            let token = lexer.get_next_token();
            if token.r#type == TokenType::EndOfFile {
                break;
            }
            self.diagnostic_tokens.push(token);
        }
    }

    pub fn set_lib_paths(&mut self, paths: Vec<String>) {
        self.lib_paths = paths;
    }

    pub fn set_build_mode(&mut self, mode: BuildMode) {
        self.build_mode = mode;
    }

    pub fn set_target(&mut self, target: impl Into<String>) {
        self.target = target.into();
    }

    pub fn get_extern_libs(&self) -> &Vec<String> {
        &self.extern_libs
    }

    pub fn get_extern_links(&self) -> &Vec<ExternLink> {
        &self.extern_links
    }

    fn platform_matches(&self, attrs: &[Attribute]) -> bool {
        let platforms: Vec<&str> = attrs
            .iter()
            .filter(|a| a.name == "platform")
            .flat_map(|a| {
                if a.values.is_empty() {
                    a.value.iter().map(String::as_str).collect::<Vec<_>>()
                } else {
                    a.values.iter().map(String::as_str).collect()
                }
            })
            .collect();
        if platforms.is_empty() {
            return true;
        }
        let target = self.target.to_ascii_lowercase();
        let is_windows = target.contains("windows");
        let is_macos = target.contains("apple-darwin") || target.contains("macos");
        let is_unix = target.contains("linux")
            || target.contains("darwin")
            || target.contains("bsd")
            || target.contains("solaris")
            || target.contains("unix");
        platforms
            .iter()
            .any(|platform| match platform.to_ascii_lowercase().as_str() {
                "windows" => is_windows,
                "macos" => is_macos,
                "unix" | "posix" => is_unix,
                value => target.contains(value),
            })
    }

    /// Maps an arithmetic/comparison operator to its trait method name.
    fn operator_to_method(op: &str) -> Option<&str> {
        match op {
            "+" => Some("add"),
            "-" => Some("sub"),
            "*" => Some("mul"),
            "/" => Some("div"),
            "%" => Some("rem"),
            "==" => Some("eq"),
            "!=" => Some("ne"),
            "<" => Some("lt"),
            ">" => Some("gt"),
            "<=" => Some("le"),
            ">=" => Some("ge"),
            _ => None,
        }
    }

    /// Register the standard operator/comparison traits that are built into the language.
    /// These are hardcoded so they're always available regardless of trait.gbl loading.
    fn register_std_traits(&mut self) {
        let std_traits: &[(&str, &[(&str, usize)])] = &[
            ("std::ops::Add", &[("add", 2)]), // add(self, other)
            ("std::ops::Sub", &[("sub", 2)]), // sub(self, other)
            ("std::ops::Mul", &[("mul", 2)]), // mul(self, other)
            ("std::ops::Div", &[("div", 2)]), // div(self, other)
            ("std::ops::Rem", &[("rem", 2)]), // rem(self, other)
            ("std::cmp::Eq", &[("eq", 2)]),   // eq(self, other)
            (
                "std::cmp::Cmp",
                &[("lt", 2), ("le", 2), ("gt", 2), ("ge", 2)],
            ),
            ("std::mem::Drop", &[("drop", 1)]), // drop(self)
        ];
        for (name, methods) in std_traits {
            if !self.trait_defs.contains_key(*name) {
                self.trait_defs.insert(
                    name.to_string(),
                    TraitDefInfo {
                        name: name.to_string(),
                        methods: methods
                            .iter()
                            .map(|(n, pc)| TraitMethodSig {
                                name: n.to_string(),
                                param_count: *pc,
                                dynamic: false,
                            })
                            .collect(),
                        generic_params: vec!["T".to_string()],
                    },
                );
            }
        }
    }

    /// Resolve a trait name as it appears in an `impl Trait for Type` block
    /// (e.g. `New<T>`, `ops::Add`, `cmp::Eq`, `Iterator<T>`, `std::ops::Add`)
    /// to a registered trait definition.
    ///
    /// Trait definitions are stored under a bare name (`New`, `Add`, `Eq`,
    /// ...) and, for the std built-ins, also a fully-qualified form
    /// (`std::ops::Add`).  Impl blocks may reference the trait with optional
    /// generic params or a module prefix, so we canonicalize before lookup:
    ///   • `New<T>`/`Iterator<T>`  → strip generic args → `New`/`Iterator`
    ///   • `ops::Add`  → strip the `std::` prefix → `ops::Add`, then fall
    ///     back to the bare suffix `Add`
    ///   • `std::ops::Add` → matches the reserved `std::`-form directly
    fn lookup_trait(&self, name: &str) -> Option<TraitDefInfo> {
        // 1. Strip any trailing generic arguments: `New<T>` → `New`.
        let base = match name.find('<') {
            Some(i) => &name[..i],
            None => name,
        };
        if base.is_empty() {
            return None;
        }

        let mut candidates: Vec<String> = Vec::new();
        // Prefer the exact form used by the impl block.
        candidates.push(base.to_string());
        // Then without the `std::` prefix: `std::ops::Add` → `ops::Add`.
        if let Some(rest) = base.strip_prefix("std::") {
            candidates.push(rest.to_string());
        }
        // Finally the bare trailing segment: `ops::Add` → `Add`,
        // `cmp::Eq` → `Eq`.
        if let Some((_, last)) = base.rsplit_once("::") {
            candidates.push(last.to_string());
        }

        // Try each candidate; the trait is registered under the first match.
        for candidate in &candidates {
            if let Some(info) = self.trait_defs.get(candidate) {
                return Some(info.clone());
            }
        }
        None
    }

    /// After all modules are loaded, validate that `impl Trait for Type` blocks
    /// provide all required trait methods.
    fn validate_trait_impls(&mut self) {
        let pending: Vec<_> = std::mem::take(&mut self.pending_trait_impls);
        for (struct_name, trait_name, impl_methods) in &pending {
            let trait_info = match self.lookup_trait(trait_name) {
                Some(t) => t,
                None => {
                    self.error(&format!(
                        "Trait '{}' is not defined (impl for '{}')",
                        trait_name, struct_name
                    ));
                    continue;
                }
            };

            let impl_set: HashSet<&String> = impl_methods.iter().collect();

            // Check every trait method is implemented
            for required in &trait_info.methods {
                if !impl_set.contains(&&required.name) {
                    self.error(&format!(
                        "Trait '{}' requires method '{}', but it is missing in impl for '{}'",
                        trait_name, required.name, struct_name
                    ));
                }
            }
        }
    }

    pub fn set_main_file(&mut self, file_path: &str) {
        // Derive module name from filename (e.g. "math.gbl" → "math")
        if let Some(stem) = Path::new(file_path).file_stem().and_then(|s| s.to_str()) {
            self.current_module = stem.to_string();
        } else {
            self.current_module = "main".to_string();
        }
        // Set module directory for relative imports
        if let Some(parent) = Path::new(file_path).parent() {
            self.current_module_dir = parent.to_str().map(|s| s.to_string());
        }
    }

    /// The compiler auto-loads these modules for every compilation; treat them
    /// as visible to every module so std internals (e.g. `builtins::`) keep
    /// working even where the file does not re-import them.
    fn auto_visible_modules() -> &'static [&'static str] {
        &["builtins", "mem", "trait"]
    }

    /// Ensure a visibility set exists for `module` and seed it with the
    /// auto-loaded modules.
    fn ensure_visible(&mut self, module: &str) {
        self.visible_modules.entry(module.to_string()).or_default();
        let seed: std::collections::HashSet<String> = Self::auto_visible_modules()
            .iter()
            .map(|s| s.to_string())
            .collect();
        self.visible_modules.get_mut(module).unwrap().extend(seed);
    }

    /// After `current_module` imports `mod`, register it (plus its short name
    /// and alias) as visible there.
    fn note_import(&mut self, imported: &str) {
        let cur = self.current_module.clone();
        self.ensure_visible(&cur);
        let set = self.visible_modules.get_mut(&cur).unwrap();
        set.insert(imported.to_string());
        if let Some(short) = imported.rsplit("::").next() {
            if !short.is_empty() {
                set.insert(short.to_string());
            }
        }
    }

    /// Whether `module` may be used as a *bare* qualifier (`module::fn`) from
    /// the current module. A module is visible if it is the current module
    /// itself, one of its `::` prefixes is visible (e.g. `std` for
    /// `std::io`), or its last path segment is visible (e.g. `io` when
    /// `import std::io` was done).
    fn module_visible(&self, module: &str) -> bool {
        if module.is_empty() || module == self.current_module {
            return true;
        }
        let Some(set) = self.visible_modules.get(&self.current_module) else {
            // The main program (before any import processed) — no gate.
            return true;
        };
        let segs: Vec<&str> = module.split("::").collect();
        for i in 1..=segs.len() {
            if set.contains(&segs[..i].join("::")) {
                return true;
            }
        }
        if let Some(short) = segs.last() {
            if set.contains(*short) {
                return true;
            }
        }
        false
    }

    pub fn analyze(&mut self, program: &Program) -> bool {
        // Register compiler-level builtins (panic / exit — handled by codegen).
        self.env
            .declare_function("panic", &DataType::None_, &self.current_module);
        self.env
            .declare_function("exit", &DataType::None_, &self.current_module);
        // Runtime intrinsics used by std library implementations
        self.env.declare_function(
            "gobol_array_elem_addr",
            &DataType::Int,
            &self.current_module,
        );
        // Array allocation intrinsic used by std library (e.g. Vec::push)
        self.env
            .declare_function("__new_array", &DataType::Unknown, &self.current_module);
        // Seed this program's module visibility for the bare-qualifier gate.
        let cur_module = self.current_module.clone();
        self.ensure_visible(&cur_module);
        // Register Ref as a built-in struct so Ref<T>::new resolves in std library
        if !self.struct_fields.contains_key("Ref") {
            let mut ref_fields = HashMap::new();
            ref_fields.insert("_ptr".to_string(), DataType::Unknown);
            self.struct_fields.insert("Ref".to_string(), ref_fields);
            self.env.declare_module("Ref");
        }

        // Load the builtins module (declares C runtime functions as extern "C").
        self.load_module("builtins");

        // Load mem (New / Drop traits) so std types that impl New resolve.
        self.load_module("mem");

        // Auto-load trait definitions (std::ops::Add, std::cmp::Eq, etc.)
        // so that `impl Trait for Type` validation works even without
        // explicit imports. Optional standard-library modules are loaded by
        // the injected basic prelude or by an explicit import in user code.
        self.load_module("trait");

        // Register standard traits (hardcoded fallback in case trait.gbl doesn't load)
        self.register_std_traits();

        // Validate all `impl Trait for Type` blocks after all modules loaded
        self.validate_trait_impls();

        // Pre-declare top-level signatures of the main program so that forward
        // references between sibling definitions resolve (e.g. math::trunc
        // calling math::floor which is declared further down the file). Loaded
        // modules already get this via load_module's Phase 2; the main program
        // is analysed directly by program.accept(self), so it needs its own
        // pre-pass. Re-declaration during the accept pass is idempotent.
        self.declare_program_signatures(program);

        // Clear impl_methods so that program.accept(self) can freshly register method bodies
        program.accept(self);

        if self.has_error {
            self.print_errors();
        }
        #[cfg(debug_assertions)]
        if !self.has_error {
            self.print_errors();
        }

        !self.has_error
    }

    /// Pre-declare top-level signatures (functions, structs, enums, traits,
    /// impl methods, extern functions) of a program so that forward references
    /// between sibling definitions resolve during the subsequent body-analysis
    /// pass. This mirrors the declaration phase of `load_module` but operates
    /// on the main program and skips export bookkeeping (the main program is
    /// not exported as a module).
    fn declare_program_signatures(&mut self, program: &Program) {
        for stmt in program.get_statements() {
            if let Some(func) = stmt.as_any().downcast_ref::<Function>() {
                if self.build_mode == BuildMode::Release
                    && Attribute::has_attr(func.get_attributes(), "debug")
                {
                    continue;
                }
                let prev_generic = self.current_generic_params.clone();
                let mut combined = prev_generic.clone();
                for g in func.get_generic_params() {
                    if !combined.contains(g) {
                        combined.push(g.clone());
                    }
                }
                self.current_generic_params = combined;
                let return_type = self.get_data_type_from_ast(func.get_return_type());
                self.register_function_signature(func.get_name(), func, return_type.clone());
                self.env
                    .declare_function(func.get_name(), &return_type, &self.current_module);
                // Record the generic signature so call sites can resolve the
                // concrete return type (e.g. `identity(3.5): float`).
                if !func.get_generic_params().is_empty() {
                    self.record_generic_signature(func.get_name(), func, return_type.clone());
                }
                self.current_generic_params = prev_generic;
            } else if let Some(struct_def) = stmt.as_any().downcast_ref::<StructDefinition>() {
                let prev_generic = self.current_generic_params.clone();
                self.current_generic_params = struct_def.get_generic_params().clone();
                self.generic_struct_params.insert(
                    struct_def.get_name().to_string(),
                    struct_def.get_generic_params().clone(),
                );
                let mut fields = HashMap::new();
                for field in struct_def.get_fields() {
                    let field_type = self.get_data_type_from_ast(field.field_type.as_deref());
                    fields.insert(field.name.clone(), field_type);
                }
                self.struct_fields
                    .insert(struct_def.get_name().to_string(), fields);
                self.env.declare_module(struct_def.get_name());
                self.current_generic_params = prev_generic;
            } else if let Some(enum_def) = stmt.as_any().downcast_ref::<EnumDefinition>() {
                self.visit_enum_definition(enum_def);
            } else if let Some(trait_def) = stmt.as_any().downcast_ref::<TraitDefinition>() {
                let methods: Vec<TraitMethodSig> = trait_def
                    .get_methods()
                    .iter()
                    .map(|m| TraitMethodSig {
                        name: m.name.clone(),
                        param_count: m.parameters.len(),
                        dynamic: Attribute::has_attr(&m.attributes, "dynamic"),
                    })
                    .collect();
                self.trait_defs.insert(
                    trait_def.get_name().to_string(),
                    TraitDefInfo {
                        name: trait_def.get_name().to_string(),
                        methods,
                        generic_params: trait_def.get_generic_params().clone(),
                    },
                );
            } else if let Some(impl_block) = stmt.as_any().downcast_ref::<ImplBlock>() {
                let prev_impl = self.current_impl_struct.clone();
                self.current_impl_struct = Some(
                    impl_block
                        .get_struct_name()
                        .split('<')
                        .next()
                        .unwrap_or(impl_block.get_struct_name())
                        .to_string(),
                );
                let prev_generic = self.current_generic_params.clone();
                self.current_generic_params = impl_block.get_generic_params().clone();
                for item in impl_block.get_items() {
                    match item {
                        ImplItem::Method(func) | ImplItem::Convert(func) => {
                            if self.build_mode == BuildMode::Release
                                && Attribute::has_attr(func.get_attributes(), "debug")
                            {
                                continue;
                            }
                            let struct_name = impl_block.get_struct_name().to_string();
                            let method_name = func.get_name().to_string();
                            let arity = func.get_parameters().map(|p| p.len()).unwrap_or(0);
                            if !self.impl_methods.insert((
                                struct_name.clone(),
                                method_name.clone(),
                                arity,
                            )) {
                                continue;
                            }
                            let prev_fn_generic = self.current_generic_params.clone();
                            for g in func.get_generic_params() {
                                if !self.current_generic_params.contains(g) {
                                    self.current_generic_params.push(g.clone());
                                }
                            }
                            let return_type = self.get_data_type_from_ast(func.get_return_type());
                            self.env.declare_function(
                                &method_name,
                                &return_type,
                                &self.current_module,
                            );
                            if let Some(ref struct_name) = self.current_impl_struct {
                                self.env
                                    .declare_function(&method_name, &return_type, struct_name);
                            }
                            self.current_generic_params = prev_fn_generic;
                        }
                    }
                }
                self.current_impl_struct = prev_impl;
                self.current_generic_params = prev_generic;
            } else if let Some(extern_block) = stmt.as_any().downcast_ref::<ExternBlock>() {
                for func in extern_block.get_functions() {
                    let return_type = self.get_data_type_from_ast(func.get_return_type());
                    self.env
                        .declare_function(func.get_name(), &return_type, &self.current_module);
                }
            }
        }
    }

    pub fn has_errors(&self) -> bool {
        self.has_error
    }

    pub fn get_errors(&self) -> &Vec<String> {
        &self.errors
    }

    pub fn print_errors(&self) {
        if self.errors.is_empty() {
            #[cfg(debug_assertions)]
            println!("Semantic analysis passed!");
        } else {
            eprintln!(
                "Semantic analysis failed with {} error(s):",
                self.errors.len()
            );
            for err in &self.errors {
                eprintln!("{}", err);
            }
        }
    }

    fn error(&mut self, msg: &str) {
        self.has_error = true;
        #[cfg(debug_assertions)]
        eprintln!(
            "[SEM ERROR] {} | current_module={} current_impl={:?}",
            msg, self.current_module, self.current_impl_struct
        );
        let (line, col) = self.error_position(msg);
        self.structured_errors.push((line, col, msg.to_string()));
        if let Some(ref f) = self.error_formatter {
            let formatted = f.format_error(line, col, 1, "error", msg, true);
            self.errors.push(formatted);
        } else {
            self.errors.push(format!("Error: {}", msg));
        }
    }

    fn error_position(&self, msg: &str) -> (i32, i32) {
        if self.current_error_line > 0 {
            return (self.current_error_line, 0);
        }
        let needle = msg
            .split(['\'', '"'])
            .nth(1)
            .filter(|value| !value.is_empty());
        if let Some(needle) = needle {
            if let Some(token) = self
                .diagnostic_tokens
                .iter()
                .find(|token| token.value == needle)
            {
                return (token.line, token.col);
            }
        }
        (1, 0)
    }

    /// Report a semantic error at a specific source position (for LSP).
    pub fn error_at(&mut self, line: i32, col: i32, msg: &str) {
        self.has_error = true;
        self.structured_errors.push((line, col, msg.to_string()));
        if let Some(ref f) = self.error_formatter {
            let span = 1;
            let formatted = f.format_error(line, col, span, "error", msg, true);
            self.errors.push(formatted);
        } else {
            self.errors.push(format!("Error: {}", msg));
        }
    }

    fn get_data_type_from_ast(&mut self, tp: Option<&dyn Type>) -> DataType {
        let tp = match tp {
            Some(t) => t,
            None => return DataType::None_,
        };

        // Check for GenericType: Vec<int> → Struct(Vec), legacy vec<int> → array alias
        if let Some(gt) = tp.as_type_any().downcast_ref::<GenericType>() {
            let base = gt.get_base_name();
            if base == "vec" && !gt.get_type_args().is_empty() {
                // legacy lowercase vec<int> is an alias for int[]
                let elem_type = self.get_data_type_from_ast(Some(&*gt.get_type_args()[0]));
                return elem_type; // treated as element type (array)
            }
            if base == "tuple" {
                return DataType::Tuple(
                    gt.get_type_args()
                        .iter()
                        .map(|ty| self.get_data_type_from_ast(Some(&**ty)))
                        .collect(),
                );
            }
            let args: Vec<DataType> = gt
                .get_type_args()
                .iter()
                .map(|arg| self.get_data_type_from_ast(Some(arg.as_ref())))
                .collect();
            let instance_name = format!(
                "{}<{}>",
                base,
                args.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            self.generic_struct_args.insert(instance_name.clone(), args);
            return DataType::Struct(instance_name);
        }

        // Check for NullableType
        if let Some(nullable) = tp.as_type_any().downcast_ref::<NullableType>() {
            let inner = self.get_data_type_from_ast(Some(nullable.get_inner_type()));
            return DataType::Nullable(Box::new(inner));
        }

        // Check for PointerType
        if let Some(ptr) = tp.as_type_any().downcast_ref::<PointerType>() {
            let inner = self.get_data_type_from_ast(Some(ptr.get_pointee()));
            return DataType::Pointer(Box::new(inner));
        }

        if let Some(reference) = tp.as_type_any().downcast_ref::<ReferenceType>() {
            let inner = self.get_data_type_from_ast(Some(reference.get_referent()));
            return DataType::Reference(Box::new(inner));
        }

        // Preserve callback signatures for semantic checks and indirect calls.
        if let Some(function) = tp.as_type_any().downcast_ref::<FunctionType>() {
            let parameters = function
                .get_param_types()
                .into_iter()
                .map(|parameter| self.get_data_type_from_ast(Some(parameter)))
                .collect();
            let return_type = function
                .get_return_type()
                .map(|return_type| self.get_data_type_from_ast(Some(return_type)))
                .unwrap_or(DataType::None_);
            return DataType::Function {
                parameters,
                return_type: Box::new(return_type),
            };
        }

        // Check for ArrayType via downcast
        if let Some(arr) = tp.as_type_any().downcast_ref::<ArrayType>() {
            let elem = arr.get_element_type();
            let inner = self.get_data_type_from_ast(Some(elem));
            return DataType::Array(Box::new(inner));
        }

        match tp.get_name() {
            "int" => DataType::Int,
            "float" => DataType::Float,
            "str" => DataType::Str,
            "bool" => DataType::Bool,
            // byte and unit aliases
            "byte" => DataType::Byte,
            "()" => DataType::None_,
            name => {
                // Allow current function's generic type parameters
                if self.current_generic_params.iter().any(|g| g == name) {
                    return DataType::Struct(name.to_string());
                }
                if self.struct_fields.contains_key(name) {
                    return DataType::Struct(name.to_string());
                }
                self.error(&format!("Unknown type: {}", name));
                DataType::Unknown
            }
        }
    }

    fn register_function_signature(
        &mut self,
        name: &str,
        function: &Function,
        return_type: DataType,
    ) {
        let parameters = function
            .get_parameters()
            .into_iter()
            .flatten()
            .map(|parameter| self.get_data_type_from_ast(parameter.get_type()))
            .collect();
        let signature = DataType::Function {
            parameters,
            return_type: Box::new(return_type),
        };
        self.function_signatures
            .insert(name.to_string(), signature.clone());
        if !self.current_module.is_empty() {
            self.function_signatures
                .insert(format!("{}::{}", self.current_module, name), signature);
        }
    }

    fn get_variant_index(&self, enum_name: &str, variant_name: &str) -> Option<i32> {
        if let Some((variant_names, _)) = self.enum_info.get(enum_name) {
            for (i, vname) in variant_names.iter().enumerate() {
                if vname == variant_name {
                    return Some(i as i32);
                }
            }
        }
        None
    }

    fn enum_has_variant(&self, enum_name: &str, variant_name: &str) -> bool {
        if let Some((variant_names, _)) = self.enum_info.get(enum_name) {
            variant_names.iter().any(|v| v == variant_name)
        } else {
            false
        }
    }

    /// Record the signature of a top-level generic function so later call sites
    /// can resolve a concrete return type by unifying argument types with the
    /// generic parameter types (e.g. `identity<T>(x: T) -> T` called as
    /// `identity(3.5)` resolves to `float`).
    fn record_generic_signature(&mut self, name: &str, func: &Function, return_type: DataType) {
        // Collect the declared parameter types. Generic type parameters resolve
        // to `Struct("T")` because `get_data_type_from_ast` runs under
        // `current_generic_params`, so the stored param types keep a reference
        // that later unification can map to the concrete argument type.
        let mut param_types: Vec<DataType> = Vec::new();
        if let Some(params) = func.get_parameters() {
            for p in params {
                param_types.push(self.get_data_type_from_ast(p.get_type()));
            }
        }
        self.generic_signatures.insert(
            name.to_string(),
            (func.get_generic_params().clone(), param_types, return_type),
        );
    }

    /// Resolve the concrete return type of a top-level generic call by unifying
    /// the argument types with the recorded generic signature. For a generic
    /// parameter `T`, the argument type is bound to `T` and then substituted
    /// throughout the signature's return type. Returns `None` when no generic
    /// signature matches or a type parameter cannot be determined.
    fn resolve_generic_call_type(
        &self,
        full_name: &str,
        func_name: &str,
        arg_types: &[DataType],
    ) -> Option<DataType> {
        let (generic_params, param_types, return_type) = self
            .generic_signatures
            .get(full_name)
            .or_else(|| self.generic_signatures.get(func_name))?;

        if generic_params.is_empty() {
            return None;
        }

        let mut bindings: HashMap<String, DataType> = HashMap::new();
        for (i, arg_ty) in arg_types.iter().enumerate() {
            if let Some(pattern) = param_types.get(i) {
                self.unify_generic_pattern(pattern, arg_ty, &mut bindings);
            }
        }

        // Every generic parameter must be bound to a concrete type; otherwise
        // the fallback return type is used instead.
        for g in generic_params {
            if !bindings.contains_key(g) {
                return None;
            }
        }

        Some(self.substitute_generic_types(return_type, &bindings))
    }

    /// Unify a generic parameter type (`pattern`) with an actual argument type,
    /// recording bindings for generic parameters (`Struct(name)` where the name
    /// is not a concrete struct).
    fn unify_generic_pattern(
        &self,
        pattern: &DataType,
        actual: &DataType,
        bindings: &mut HashMap<String, DataType>,
    ) {
        match pattern {
            DataType::Struct(name) => {
                // Treat a bare `Struct(name)` as a generic parameter placeholder
                // rather than a concrete struct type, and bind it to the actual
                // argument type. Unknown/None actual types don't carry usable
                // information, so leave them unbound.
                if !matches!(actual, DataType::Unknown | DataType::None_) {
                    bindings.entry(name.clone()).or_insert(actual.clone());
                }
            }
            DataType::Array(inner) => {
                // Array parameters/variables store the *element* type (the
                // analyzer pushes element types rather than the wrapped array
                // type), so the actual may arrive as either `Array(E)` (from an
                // array literal) or the bare element type `E` (from an array
                // variable). In both cases we unify the array's element pattern
                // against the element type we have.
                match actual {
                    DataType::Array(a) => self.unify_generic_pattern(inner, a, bindings),
                    other => self.unify_generic_pattern(inner, other, bindings),
                }
            }
            DataType::Nullable(inner) => {
                if let DataType::Nullable(a) = actual {
                    self.unify_generic_pattern(inner, a, bindings);
                }
            }
            _ => {}
        }
    }

    /// Substitute the recorded generic-parameter bindings throughout a type,
    /// replacing `Struct("T")` references with their concrete bound types.
    fn substitute_generic_types(
        &self,
        dt: &DataType,
        bindings: &HashMap<String, DataType>,
    ) -> DataType {
        match dt {
            DataType::Struct(name) => bindings.get(name).cloned().unwrap_or_else(|| dt.clone()),
            DataType::Array(inner) => {
                DataType::Array(Box::new(self.substitute_generic_types(inner, bindings)))
            }
            DataType::Nullable(inner) => {
                DataType::Nullable(Box::new(self.substitute_generic_types(inner, bindings)))
            }
            DataType::Reference(inner) => {
                DataType::Reference(Box::new(self.substitute_generic_types(inner, bindings)))
            }
            DataType::Pointer(inner) => {
                DataType::Pointer(Box::new(self.substitute_generic_types(inner, bindings)))
            }
            DataType::Function {
                parameters,
                return_type,
            } => DataType::Function {
                parameters: parameters
                    .iter()
                    .map(|parameter| self.substitute_generic_types(parameter, bindings))
                    .collect(),
                return_type: Box::new(self.substitute_generic_types(return_type, bindings)),
            },
            _ => dt.clone(),
        }
    }

    fn resolve_generic_method_type(
        &self,
        receiver_type: &DataType,
        method_type: &DataType,
    ) -> DataType {
        let DataType::Struct(instance_name) = receiver_type else {
            return method_type.clone();
        };
        let struct_name = instance_name
            .split('<')
            .next()
            .unwrap_or(instance_name)
            .trim();
        let (Some(params), Some(args)) = (
            self.generic_struct_params.get(struct_name),
            self.generic_struct_args.get(instance_name),
        ) else {
            return method_type.clone();
        };
        let bindings: HashMap<String, DataType> =
            params.iter().cloned().zip(args.iter().cloned()).collect();
        self.substitute_generic_types(method_type, &bindings)
    }

    fn get_current_type(&self) -> DataType {
        if self.type_stack.is_empty() {
            DataType::Unknown
        } else {
            self.type_stack[self.type_stack.len() - 1].clone()
        }
    }

    fn check_type_compatibility(
        &mut self,
        target: DataType,
        source: DataType,
        context: &str,
    ) -> bool {
        if Environment::is_type_compatible(&target, &source) {
            return true;
        }
        self.error(&format!(
            "Type mismatch in {}: expected {}, got {}",
            context,
            data_type_to_string(target),
            data_type_to_string(source)
        ));
        false
    }

    /// Resolve a C header path specified by `#[header("path")]`.
    ///
    /// Tries the following locations in order:
    /// 1. The path as-is (absolute or relative to CWD)
    /// 2. On macOS: absolute `/usr/include/<name>` under the Xcode SDK path
    ///    (retrieved via `xcrun --sdk macosx --show-sdk-path`), because
    ///    macOS Catalina+ no longer ships system headers at `/usr/include`.
    /// 3. Relative to the current module's directory
    /// 4. Relative to each configured library path
    ///
    /// Returns the resolved filesystem path together with its raw content so
    /// callers can resolve relative `#include` directives against the header's
    /// own directory.
    fn resolve_header_file(
        &self,
        path: &str,
    ) -> Result<(std::path::PathBuf, String), std::io::Error> {
        let p = Path::new(path);

        // 1. Try as-is (absolute or CWD-relative)
        if let Ok(content) = fs::read_to_string(p) {
            return Ok((p.to_path_buf(), content));
        }

        // 2. macOS SDK fallback for absolute /usr/include/<...> paths.
        //    macOS Catalina (10.15+) moved system headers into the Xcode
        //    SDK bundle; the filesystem /usr/include no longer exists even
        //    with the Command Line Tools installed. xcrun gives the active
        //    SDK path.
        if cfg!(target_os = "macos") && path.starts_with("/usr/include/") {
            if let Some(sdk) = Self::macos_sdk_path() {
                let relocated = sdk.join(path.trim_start_matches('/'));
                if let Ok(content) = fs::read_to_string(&relocated) {
                    return Ok((relocated, content));
                }
            }
        }

        // 3. Relative to the current module directory
        if let Some(ref dir) = self.current_module_dir {
            let p = Path::new(dir).join(path);
            if let Ok(content) = fs::read_to_string(&p) {
                return Ok((p, content));
            }
        }
        // 4. Relative to each lib path
        for lp in &self.lib_paths {
            let p = Path::new(lp).join(path);
            if let Ok(content) = fs::read_to_string(&p) {
                return Ok((p, content));
            }
            // Also try parent of lib path (e.g. std/../src/runtime.h)
            if let Some(parent) = Path::new(lp).parent() {
                let p2 = parent.join(path);
                if let Ok(content) = fs::read_to_string(&p2) {
                    return Ok((p2, content));
                }
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("header file '{}' not found", path),
        ))
    }

    /// Return the path to the active macOS SDK via `xcrun`. Cached per-call
    /// into a `String` so we only shell out once per header file, not per
    /// nested `#include`. Returns `None` if xcrun is unavailable or errors.
    fn macos_sdk_path() -> Option<std::path::PathBuf> {
        use std::process::Command;
        let output = Command::new("xcrun")
            .args(["--sdk", "macosx", "--show-sdk-path"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(s))
        }
    }

    /// Read a C header and recursively inline `#include "..."` directives so
    /// that function declarations living in sub-headers (e.g. `runtime/io.h`
    /// pulled in by `runtime.h`) are visible to signature validation.
    /// System includes (`#include <...>`) are left untouched.
    fn read_header_file(&self, path: &str) -> Result<String, std::io::Error> {
        let mut visited: HashSet<String> = HashSet::new();
        self.read_header_recursive(path, &mut visited)
    }

    fn read_header_recursive(
        &self,
        path: &str,
        visited: &mut HashSet<String>,
    ) -> Result<String, std::io::Error> {
        let (resolved, content) = self.resolve_header_file(path)?;
        // Guard against include cycles.
        let key = match resolved.canonicalize() {
            Ok(c) => c.to_string_lossy().into_owned(),
            Err(_) => resolved.to_string_lossy().into_owned(),
        };
        if !visited.insert(key) {
            return Ok(String::new());
        }

        // Directory where this header lives — used to resolve relative
        // `#include "..."` form.
        let base_dir = resolved.parent().map(|p| p.to_path_buf());
        // System include root for `#include <...>`: if the resolved header
        // lives under `.../usr/include/` then the system-include root is
        // `.../usr/include`.  On macOS, `<SDK>/usr/include/stdio.h` pulls
        // in `<_stdio.h>` from the same system-include root (Apple wraps
        // declarations that way).  On Linux, glibc uses sub-includes under
        // `/usr/include/<bits|features|sys>/` for the same effect — this
        // root is correct there too.
        let sysroot: Option<std::path::PathBuf> = {
            let s = resolved.to_string_lossy();
            let marker = "/usr/include/";
            s.find(marker)
                .map(|idx| std::path::PathBuf::from(&s[..idx + marker.len()]))
        };

        let mut out = String::new();
        for line in content.lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix("#include") {
                let rest = rest.trim_start();
                if rest.starts_with('"') {
                    // Local include: extract the path between the quotes and
                    // recurse relative to this header's directory.
                    let inner = &rest[1..];
                    if let Some(end) = inner.find('"') {
                        let inc_path = &inner[..end];
                        let resolved_inc = match &base_dir {
                            Some(dir) => dir.join(inc_path),
                            None => std::path::PathBuf::from(inc_path),
                        };
                        let inc_str = resolved_inc.to_string_lossy();
                        match self.read_header_recursive(&inc_str, visited) {
                            Ok(included) => out.push_str(&included),
                            Err(_) => {
                                // Fall back to searching lib paths directly.
                                match self.read_header_recursive(inc_path, visited) {
                                    Ok(included) => out.push_str(&included),
                                    Err(_) => out.push_str(line),
                                }
                            }
                        }
                        out.push('\n');
                        continue;
                    }
                } else if rest.starts_with('<') {
                    // System include `#include <foo.h>`: only resolve when we
                    // have a well-defined system include root (i.e. this
                    // header already lives inside .../usr/include/). This
                    // lets the analyzer see into Apple's `<_stdio.h>` and
                    // glibc's `<bits/libc-header-start.h>` so declarations
                    // like `printf` aren't missed just because they live
                    // in an angle-bracket child.
                    if let Some(inner_end) = rest.find('>') {
                        let inc = &rest[1..inner_end];
                        if let Some(ref root) = sysroot {
                            let child = root.join(inc);
                            let child_str = child.to_string_lossy();
                            match self.read_header_recursive(&child_str, visited) {
                                Ok(sub) => out.push_str(&sub),
                                Err(_) => out.push_str(line),
                            }
                            out.push('\n');
                            continue;
                        }
                    }
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        Ok(out)
    }

    /// Lightweight check whether a C header text contains a declaration of
    /// the given function name. Searches for `name` followed by `(` with a
    /// word boundary before it (so `printf` won't match `fprintf`).
    fn header_declares_function(header: &str, name: &str) -> bool {
        let bytes = header.as_bytes();
        let needle = name.as_bytes();
        if needle.is_empty() {
            return false;
        }
        let mut i = 0;
        while i + needle.len() <= bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                // Check word boundary before the match: preceding char must
                // not be an identifier character (letter, digit, or _).
                let prev_ok =
                    i == 0 || (!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_');
                // Check that the char after the match is `(` (allowing
                // optional whitespace — space, tab, CR, LF, form-feed,
                // vertical-tab. C declarations commonly split across lines,
                // e.g.:
                //     int
                //     printf(const char *, ...);
                // — skipping only ' ' and '\t' misses those.)
                let mut j = i + needle.len();
                while j < bytes.len() {
                    let b = bytes[j];
                    if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\x0B' | b'\x0C') {
                        j += 1;
                    } else {
                        break;
                    }
                }
                let next_ok = j < bytes.len() && bytes[j] == b'(';
                if prev_ok && next_ok {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn resolve_module_path(&self, path_parts: &[String], base_dir: Option<&str>) -> Option<String> {
        let mut module_paths = vec![path_parts.to_vec()];
        // The basic prelude is a lightweight namespace over the existing
        // standard-library module files. Prefer a real `basic/` tree when
        // present, then fall back to `std/` for the bundled layout.
        if path_parts.first().map(String::as_str) == Some("basic") {
            module_paths.push(path_parts[1..].to_vec());
        }

        // First: check relative to the importing module's directory
        if let Some(dir) = base_dir {
            for module_path in &module_paths {
                let relative = module_path.join("/") + ".gbl";
                let rel_full = format!("{}/{}", dir, relative);
                if Path::new(&rel_full).exists() {
                    return Some(rel_full);
                }
                let rel_mod = format!("{}/{}/mod.gbl", dir, module_path.join("/"));
                if Path::new(&rel_mod).exists() {
                    return Some(rel_mod);
                }
            }

            // Also check in base_dir/lib/ (local lib directory)
            for module_path in &module_paths {
                let relative = module_path.join("/") + ".gbl";
                let rel_lib = format!("{}/lib/{}", dir, relative);
                if Path::new(&rel_lib).exists() {
                    return Some(rel_lib);
                }
            }
        }

        // Second: check each lib path
        for lib_path in &self.lib_paths {
            for module_path in &module_paths {
                let relative = module_path.join("/") + ".gbl";
                // <lib_path>/<module>.gbl
                let full = format!("{}/{}", lib_path, relative);
                if Path::new(&full).exists() {
                    return Some(full);
                }
                // <lib_path>/<module>/mod.gbl  (modern module entry point)
                let mod_relative = format!("{}/mod.gbl", module_path.join("/"));
                let mod_full = format!("{}/{}", lib_path, mod_relative);
                if Path::new(&mod_full).exists() {
                    return Some(mod_full);
                }
            }
            for module_path in &module_paths {
                let relative = module_path.join("/") + ".gbl";
                // <lib_path>/src/<module>.gbl (for grape packages)
                let src_full = format!("{}/src/{}", lib_path, relative);
                if Path::new(&src_full).exists() {
                    return Some(src_full);
                }
                // <lib_path>/lib/<module>.gbl
                let lib_full = format!("{}/lib/{}", lib_path, relative);
                if Path::new(&lib_full).exists() {
                    return Some(lib_full);
                }
            }
        }
        // Third: try without lib prefix
        for module_path in &module_paths {
            let direct = format!("{}.gbl", module_path.join("/"));
            if Path::new(&direct).exists() {
                return Some(direct);
            }
            let mod_direct = format!("{}/mod.gbl", module_path.join("/"));
            if Path::new(&mod_direct).exists() {
                return Some(mod_direct);
            }
        }
        None
    }

    fn load_module(&mut self, module_name: &str) {
        if self.loaded_modules.contains(module_name) {
            return;
        }

        // Split by :: first, then by . for backward compat
        let path_parts: Vec<String> = module_name
            .split("::")
            .flat_map(|s| s.split('.'))
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let base_dir = self.current_module_dir.clone();
        let file_path = match self.resolve_module_path(&path_parts, base_dir.as_deref()) {
            Some(p) => p,
            None => {
                return;
            }
        };

        let source = match fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(_) => return,
        };

        let lexer = Lexer::new(source);
        let mut builder = AstBuilder::new(lexer);
        let prog = match builder.build() {
            Some(p) => p,
            None => return,
        };

        // Set current_module_dir for relative imports within this module
        let prev_dir = self.current_module_dir.clone();
        if let Some(parent) = Path::new(&file_path).parent() {
            self.current_module_dir = parent.to_str().map(|s| s.to_string());
        }

        self.loaded_modules.insert(module_name.to_string());

        // Save context
        let prev_module = self.current_module.clone();

        // Use the full module name (import path) for function naming,
        // so that "import lib.math as m" makes functions accessible as "lib.math.X"
        self.current_module = module_name.to_string();
        self.env.declare_module(&self.current_module);

        // ---- Phase 1: Scan for ExportStatement presence and collect items ----
        // We need to know upfront whether this module contains any export(...)
        // statement to decide between "explicit export list" mode and
        // "default export all" mode (minus #[no_export] items).
        let has_explicit_export = prog
            .get_statements()
            .iter()
            .any(|stmt| stmt.as_any().downcast_ref::<ExportStatement>().is_some());

        // Collect names of top-level defs that carry #[no_export].
        // These are excluded from the default-export path even when no explicit
        // export(...) is present.
        let mut no_export_names: HashSet<String> = HashSet::new();
        for stmt in prog.get_statements() {
            if let Some(func) = stmt.as_any().downcast_ref::<Function>() {
                if Attribute::has_attr(func.get_attributes(), "no_export") {
                    no_export_names.insert(func.get_name().to_string());
                }
            } else if let Some(struct_def) = stmt.as_any().downcast_ref::<StructDefinition>() {
                if Attribute::has_attr(struct_def.get_attributes(), "no_export") {
                    no_export_names.insert(struct_def.get_name().to_string());
                }
            } else if let Some(trait_def) = stmt.as_any().downcast_ref::<TraitDefinition>() {
                if Attribute::has_attr(trait_def.get_attributes(), "no_export") {
                    no_export_names.insert(trait_def.get_name().to_string());
                }
            } else if let Some(enum_def) = stmt.as_any().downcast_ref::<EnumDefinition>() {
                if Attribute::has_attr(enum_def.get_attributes(), "no_export") {
                    no_export_names.insert(enum_def.get_name().to_string());
                }
            }
        }

        // Track top-level names that were declared, for default-export phase.
        // (name, data_type_for_functions)
        let mut declared_funcs: Vec<(String, DataType)> = Vec::new();
        let mut declared_structs: Vec<String> = Vec::new();
        let mut declared_traits: Vec<String> = Vec::new();
        // Imported module names so `export(mod)` / default-export can
        // re-export submodules.
        let mut imported_mods: Vec<String> = Vec::new();

        // ---- Phase 2: Process declarations ----
        for stmt in prog.get_statements() {
            if let Some(import_stmt) = stmt.as_any().downcast_ref::<ImportStatement>() {
                let name = import_stmt.get_module_name();
                imported_mods.push(name.clone());
                self.load_module(&name);
                self.note_import(&name);
                if let Some(alias) = import_stmt.get_alias() {
                    self.module_aliases.insert(alias.to_string(), name.clone());
                    // Also make the alias usable as a bare qualifier.
                    self.ensure_visible(&self.current_module.clone());
                    self.visible_modules
                        .get_mut(&self.current_module)
                        .map(|s| s.insert(alias.to_string()));
                }
            } else if let Some(from_import) = stmt.as_any().downcast_ref::<FromImportStatement>() {
                // `from module import member, ...` inside a loaded module.
                // Load the referenced module, then re-export the requested
                // members as bare names in this module's namespace.
                let mod_name = from_import.get_module();
                self.load_module(mod_name);
                self.note_import(mod_name);

                // Handle wildcard import: `from module import *`
                if from_import.is_wildcard() {
                    let module_symbols = self.env.get_module_symbols(mod_name);
                    for (name, sym) in module_symbols {
                        let return_type = sym.data_type.clone();
                        self.env
                            .declare_function(&name, &return_type, &self.current_module);
                        declared_funcs.push((name, return_type));
                    }
                } else {
                    // Handle specific members with optional aliases
                    for (member_name, alias) in from_import.get_members() {
                        let qualified = format!("{}::{}", mod_name, member_name);
                        if let Some(sym) = self.env.lookup_symbol(&qualified) {
                            let return_type = sym.data_type.clone();
                            let effective_name = alias.as_ref().unwrap_or(member_name);
                            self.env.declare_function(
                                effective_name,
                                &return_type,
                                &self.current_module,
                            );
                            declared_funcs.push((effective_name.to_string(), return_type));
                        }
                    }
                }
            } else if let Some(func) = stmt.as_any().downcast_ref::<Function>() {
                if self.build_mode == BuildMode::Release
                    && Attribute::has_attr(func.get_attributes(), "debug")
                {
                    continue;
                }
                let func_name = func.get_name().to_string();
                let prev_generic = self.current_generic_params.clone();
                self.current_generic_params = func.get_generic_params().clone();
                let return_type = self.get_data_type_from_ast(func.get_return_type());
                self.register_function_signature(&func_name, func, return_type.clone());
                self.env
                    .declare_function(&func_name, &return_type, &self.current_module);
                declared_funcs.push((func_name, return_type));
                self.current_generic_params = prev_generic;
            } else if let Some(struct_def) = stmt.as_any().downcast_ref::<StructDefinition>() {
                let struct_name = struct_def.get_name().to_string();
                let prev_generic = self.current_generic_params.clone();
                self.current_generic_params = struct_def.get_generic_params().clone();
                self.generic_struct_params
                    .insert(struct_name.clone(), struct_def.get_generic_params().clone());
                let mut fields = HashMap::new();
                for field in struct_def.get_fields() {
                    let field_type = self.get_data_type_from_ast(field.field_type.as_deref());
                    fields.insert(field.name.clone(), field_type);
                }
                self.struct_fields.insert(struct_name.clone(), fields);
                self.env.declare_module(&struct_name);
                declared_structs.push(struct_name);
                self.current_generic_params = prev_generic;
            } else if let Some(impl_block) = stmt.as_any().downcast_ref::<ImplBlock>() {
                let prev_impl = self.current_impl_struct.clone();
                self.current_impl_struct = Some(
                    impl_block
                        .get_struct_name()
                        .split('<')
                        .next()
                        .unwrap_or(impl_block.get_struct_name())
                        .to_string(),
                );
                let prev_impl_generic = self.current_generic_params.clone();
                self.current_generic_params = impl_block.get_generic_params().clone();

                // If this is `impl Trait for Type`, defer validation until after all modules loaded
                if let Some(trait_name) = impl_block.get_trait_name() {
                    let method_names: Vec<String> = impl_block
                        .get_items()
                        .iter()
                        .filter_map(|item| match item {
                            ImplItem::Method(func) | ImplItem::Convert(func) => {
                                if self.build_mode == BuildMode::Release
                                    && Attribute::has_attr(func.get_attributes(), "debug")
                                {
                                    None
                                } else {
                                    Some(func.get_name().to_string())
                                }
                            }
                        })
                        .collect();
                    self.pending_trait_impls.push((
                        impl_block.get_struct_name().to_string(),
                        trait_name.to_string(),
                        method_names,
                    ));
                }

                for item in impl_block.get_items() {
                    match item {
                        ImplItem::Method(func) | ImplItem::Convert(func) => {
                            if self.build_mode == BuildMode::Release
                                && Attribute::has_attr(func.get_attributes(), "debug")
                            {
                                continue;
                            }
                            let func_name = func.get_name().to_string();
                            // Dedup: skip if this (struct, method, arity) triple was
                            // already declared. This prevents duplicate impls when a
                            // module is loaded via multiple import paths or when std
                            // re-exports a module, while still allowing overloaded
                            // methods (same name, different arity).
                            let struct_name = impl_block.get_struct_name().to_string();
                            let arity = func.get_parameters().map(|p| p.len()).unwrap_or(0);
                            if !self.impl_methods.insert((
                                struct_name.clone(),
                                func_name.clone(),
                                arity,
                            )) {
                                continue;
                            }
                            let prev_generic = self.current_generic_params.clone();
                            // Combine impl-block generics with function-level generics
                            // so methods like `func push(self, value: T)` can resolve T
                            // inherited from `impl<T> vec<T>`.
                            let mut combined = self.current_generic_params.clone();
                            for g in func.get_generic_params() {
                                if !combined.contains(g) {
                                    combined.push(g.clone());
                                }
                            }
                            self.current_generic_params = combined;
                            let return_type = self.get_data_type_from_ast(func.get_return_type());
                            self.env.declare_function(
                                &func_name,
                                &return_type,
                                &self.current_module,
                            );
                            // Also register with struct name prefix for Type::method() calls
                            if let Some(ref struct_name) = self.current_impl_struct {
                                self.env
                                    .declare_function(&func_name, &return_type, struct_name);
                            }
                            declared_funcs.push((func_name, return_type));
                            self.current_generic_params = prev_generic;
                        }
                    }
                }
                self.current_impl_struct = prev_impl;
                self.current_generic_params = prev_impl_generic;
            } else if let Some(extern_block) = stmt.as_any().downcast_ref::<ExternBlock>() {
                if let Some(lib) = extern_block.get_library() {
                    if lib != "C" && !self.extern_libs.iter().any(|l| l == lib) {
                        self.extern_libs.push(lib.to_string());
                    }
                }
                for func in extern_block.get_functions() {
                    let func_name = func.get_name().to_string();
                    let return_type = self.get_data_type_from_ast(func.get_return_type());
                    self.env
                        .declare_function(&func_name, &return_type, &self.current_module);
                    declared_funcs.push((func_name, return_type));
                }
            } else if let Some(enum_def) = stmt.as_any().downcast_ref::<EnumDefinition>() {
                self.visit_enum_definition(enum_def);
                declared_structs.push(enum_def.get_name().to_string());
            } else if let Some(trait_def) = stmt.as_any().downcast_ref::<TraitDefinition>() {
                // Register trait definition for `impl Trait for Type` validation
                let methods: Vec<TraitMethodSig> = trait_def
                    .get_methods()
                    .iter()
                    .map(|m| TraitMethodSig {
                        name: m.name.clone(),
                        param_count: m.parameters.len(),
                        dynamic: Attribute::has_attr(&m.attributes, "dynamic"),
                    })
                    .collect();
                self.trait_defs.insert(
                    trait_def.get_name().to_string(),
                    TraitDefInfo {
                        name: trait_def.get_name().to_string(),
                        methods,
                        generic_params: trait_def.get_generic_params().clone(),
                    },
                );
                declared_traits.push(trait_def.get_name().to_string());
            } else if let Some(export_stmt) = stmt.as_any().downcast_ref::<ExportStatement>() {
                for name in export_stmt.get_names() {
                    let parts: Vec<&str> = name.split('.').collect();
                    let short = parts.last().unwrap_or(&"");
                    let original_key = if parts.len() > 1 {
                        let mod_part = parts[0];
                        let resolved_mod = self
                            .module_aliases
                            .get(mod_part)
                            .map(|s| s.as_str())
                            .unwrap_or(mod_part);
                        format!("{}::{}", resolved_mod, short)
                    } else {
                        format!("{}::{}", self.current_module, name)
                    };
                    if let Some(sym) = self.env.lookup_symbol(&original_key) {
                        let return_type = sym.data_type.clone();
                        self.env
                            .declare_function(short, &return_type, &self.current_module);
                    }
                }
            }
        }

        // ---- Phase 3: Default export (if no explicit export list) ----
        //
        // When a module has NO export(...) statement, every top-level def
        // (func / struct / trait) is automatically exported — except those
        // annotated with #[no_export].
        //
        // An exported function `foo` in module `m` is re-declared also under
        // just `m::foo`'s short key for direct lookups (mirrors behaviour of
        // the explicit `export(foo)` path above).
        if !has_explicit_export {
            for (name, return_type) in &declared_funcs {
                if no_export_names.contains(name.as_str()) {
                    continue;
                }
                // Make accessible as bare `name` key too (this mirrors what
                // the explicit export loop in Phase 2 does).
                self.env
                    .declare_function(name, return_type, &self.current_module);
            }
            for name in &declared_structs {
                if no_export_names.contains(name.as_str()) {
                    continue;
                }
                // structs are registered as Module symbols in the env;
                // re-declare in current module for export.
                self.env.declare_module(name);
            }
            for name in &declared_traits {
                if no_export_names.contains(name.as_str()) {
                    continue;
                }
                // Traits don't have a specific registration mechanism
                // (they live in trait_defs map), but declaring a Module
                // symbol lets name-based resolution find them.
                self.env.declare_module(name);
            }
            // Imported submodules are also re-exported into this module's
            // namespace so `from m import ...` style patterns keep working
            // after explicit `export(...)` lists are dropped.
            for mod_name in &imported_mods {
                let short = mod_name.rsplit("::").next().unwrap_or(mod_name);
                self.env.declare_module(short);
            }
        }

        // For `import a::b` / `import std::io` etc, additionally register the
        // *short* module name as an alias for the exports, so both
        // `std::io::println(...)` and the bare `io::println(...)` resolve.
        // For the std library the legacy `io::` form is handy since the std
        // layout keeps every submodule addressable, but the canonical path
        // (`std::io::...`) always works. This mirrors how the environment
        // registers module-scoped functions.
        if module_name.contains("::") {
            let short = module_name
                .rsplit("::")
                .next()
                .unwrap_or(&module_name)
                .to_string();
            if !short.is_empty() && short != module_name {
                self.env.declare_module(&short);
                for (name, ret) in &declared_funcs {
                    // declare_function(<name>, ty, <module>) registers under
                    // `<module>::<name>`, so passing the short module name here
                    // makes `io::println` resolvable via the short alias.
                    self.env.declare_function(name, ret, &short);
                }
                for name in &declared_structs {
                    self.env.declare_module(&format!("{}::{}", short, name));
                }
                for name in &declared_traits {
                    self.env.declare_module(&format!("{}::{}", short, name));
                }
            }
        }

        // Restore context
        self.current_module = prev_module;
        self.current_module_dir = prev_dir;

        self.loaded_programs.push(prog);
    }
}

#[path = "semantic_analyzer/visitor.rs"]
mod visitor;

// These tests are for macOS and will not be used unless something went wrong with macOS
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast_builder::AstBuilder;
    use crate::lexer::Lexer;

    #[test]
    fn platform_matches_supported_target_groups() {
        let mut analyzer = SemanticAnalyzer::new();
        let attrs = vec![
            Attribute::new("platform")
                .with_value("linux")
                .with_value("unix"),
        ];

        analyzer.set_target("x86_64-unknown-linux-gnu");
        assert!(analyzer.platform_matches(&attrs));

        analyzer.set_target("x86_64-pc-windows-msvc");
        assert!(!analyzer.platform_matches(&attrs));

        let windows = vec![Attribute::new("platform").with_value("windows")];
        assert!(analyzer.platform_matches(&windows));
    }

    #[test]
    fn parses_enhanced_link_attribute_arguments() {
        let source = r#"
            #[platform("linux", "unix")]
            #[header("ffi.h")]
            #[link(name = "curl", kind = "static", rename = "libcurl.a", version = "4.8", optional = true)]
            extern "C" {
                func curl_easy_init(): *void;
            }
        "#;
        let mut builder = AstBuilder::new(Lexer::new(source.to_string()));
        let program = builder.build().expect("expected AST");
        assert!(!builder.has_error(), "{:?}", builder.get_error_message());
        let block = program
            .get_statements()
            .iter()
            .find_map(|stmt| stmt.as_any().downcast_ref::<ExternBlock>())
            .expect("expected extern block");

        let platform = block
            .get_attributes()
            .iter()
            .find(|attr| attr.name == "platform")
            .expect("expected platform attribute");
        assert_eq!(platform.values, ["linux", "unix"]);

        let link = block
            .get_attributes()
            .iter()
            .find(|attr| attr.name == "link")
            .expect("expected link attribute");
        assert_eq!(
            Attribute::get_attr_named(block.get_attributes(), "link", "name"),
            Some("curl")
        );
        assert_eq!(
            Attribute::get_attr_named(block.get_attributes(), "link", "kind"),
            Some("static")
        );
        assert_eq!(
            Attribute::get_attr_named(block.get_attributes(), "link", "rename"),
            Some("libcurl.a")
        );
        assert_eq!(
            Attribute::get_attr_named(block.get_attributes(), "link", "version"),
            Some("4.8")
        );
        assert_eq!(
            Attribute::get_attr_named(block.get_attributes(), "link", "optional"),
            Some("true")
        );
        assert!(link.values.is_empty());
    }

    #[test]
    fn resolves_methods_after_unwrapping_generic_network_results() {
        let source = r#"
            import net;
            import result;

            func response_size(): int {
                var socket = net::TcpStream::connect("localhost", 80, 1000).take();
                socket.send_str("GET / HTTP/1.1\r\n\r\n").take();
                var chunk = socket.recv_bytes(4096).take();
                var size = chunk.len();
                socket.close().take();
                size
            }
        "#;
        let mut builder = AstBuilder::new(Lexer::new(source.to_string()));
        builder.set_inject_prelude(true);
        let program = builder.build().expect("expected AST");
        assert!(!builder.has_error(), "{:?}", builder.get_error_message());

        let mut analyzer = SemanticAnalyzer::new();
        analyzer.set_main_file("tests/fixtures/stdlib/net_test.gbl");
        analyzer.set_source(source);
        analyzer.set_lib_paths(vec![format!(
            "{}/../../std",
            env!("CARGO_MANIFEST_DIR").replace('\\', "/")
        )]);
        assert!(
            analyzer.analyze(program.as_ref()),
            "semantic analysis failed: {:?}",
            analyzer.structured_errors
        );
    }

    // // Regression test for the macOS CI failure on `test_ffi_variadic`:
    // // declarations split across lines (`int\nprintf(...)`) must be detected
    // // even though `\n` sits between the name and the `(`.
    // #[test]
    // fn header_declares_function_accepts_newline_between_name_and_paren() {
    //     let hdr = "int\nprintf(const char * restrict, ...);\n";
    //     assert!(
    //         SemanticAnalyzer::header_declares_function(hdr, "printf"),
    //         "expected `printf` to be found across a line break"
    //     );
    // }

    // // `fprintf` must NOT match when searching for `printf` — word boundary.
    // #[test]
    // fn header_declares_function_respects_word_boundary() {
    //     let hdr = "int fprintf(FILE *, const char *, ...);";
    //     assert!(!SemanticAnalyzer::header_declares_function(hdr, "printf"));
    //     assert!(SemanticAnalyzer::header_declares_function(hdr, "fprintf"));
    // }

    // // macOS SDK `stdio.h` wraps declarations in angle-bracket sub-includes,
    // // e.g. `#include <_stdio.h>`. `read_header_recursive` must follow those
    // // when the resolved file lives under `.../usr/include/` so we don't
    // // miss `printf` just because its declaration is in `<_stdio.h>`.
    // #[test]
    // fn header_declares_function_finds_printf_in_angle_bracket_child() {
    //     // A synthetic header that mirrors Apple's structure: the top-level
    //     // file `.../usr/include/stdio.h` contains only `#include <_stdio.h>`,
    //     // with the actual declaration in the angle-bracket child.
    //     let tmp = std::env::temp_dir();
    //     let usr = tmp.join("ci_test_sa").join("usr").join("include");
    //     std::fs::create_dir_all(&usr).expect("mkdir");
    //     let stdio = usr.join("stdio.h");
    //     let stdio_inner = usr.join("_stdio.h");
    //     std::fs::write(&stdio, "#include <_stdio.h>\n").unwrap();
    //     std::fs::write(
    //         &stdio_inner,
    //         "int printf(const char * restrict, ...) __printflike(1,2);\n",
    //     )
    //     .unwrap();
    //     let path = stdio.to_string_lossy().to_string();
    //     let sa = SemanticAnalyzer::new();
    //     let content = sa.read_header_file(&path).expect("read_header_file");
    //     std::fs::remove_dir_all(tmp.join("ci_test_sa")).ok();

    //     assert!(
    //         SemanticAnalyzer::header_declares_function(&content, "printf"),
    //         "printf should be found after inlining angle-bracket child _stdio.h\ncontent:\n{}",
    //         &content
    //     );
    // }
}
