#![allow(dead_code)]

use crate::ast::*;
use crate::ast_builder::AstBuilder;
use crate::environment::*;
use crate::error::ErrorFormatter;
use crate::lexer::Lexer;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

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
    current_impl_struct: Option<String>,
    lib_paths: Vec<String>,
    loaded_modules: HashSet<String>,
    loaded_programs: Vec<Box<Program>>,
    current_module_dir: Option<String>,
    module_aliases: HashMap<String, String>,
    current_generic_params: Vec<String>,
    build_mode: BuildMode,
    /// Trait definitions: trait_name -> { name, methods, generic_params }
    trait_defs: HashMap<String, TraitDefInfo>,
    /// Pending trait impl validations: (struct_name, trait_name, items)
    pending_trait_impls: Vec<(String, String, Vec<String>)>,
    /// Structured errors for LSP: (line, col, message)
    pub structured_errors: Vec<(i32, i32, String)>,
    /// External libraries to link (e.g. "C", "m") from extern "C" blocks.
    pub extern_libs: Vec<String>,
    /// Tracks (struct_name, method_name, param_count) triples already declared
    /// via impl blocks. Prevents duplicate method definitions when a module is
    /// loaded multiple times or when multiple modules impl the same struct,
    /// while still allowing overloaded methods (same name, different arity,
    /// e.g. `Range::new(start, end)` vs `Range::new(start, end, step)`).
    impl_methods: HashSet<(String, String, usize)>,
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
            current_impl_struct: None,
            lib_paths: vec!["lib".to_string()],
            loaded_modules: HashSet::new(),
            loaded_programs: Vec::new(),
            current_module_dir: None,
            module_aliases: HashMap::new(),
            current_generic_params: Vec::new(),
            build_mode: BuildMode::Debug,
            trait_defs: HashMap::new(),
            pending_trait_impls: Vec::new(),
            structured_errors: Vec::new(),
            extern_libs: Vec::new(),
            impl_methods: HashSet::new(),
        }
    }

    pub fn set_error_formatter(&mut self, f: ErrorFormatter) {
        self.error_formatter = Some(f);
    }

    pub fn set_lib_paths(&mut self, paths: Vec<String>) {
        self.lib_paths = paths;
    }

    pub fn set_build_mode(&mut self, mode: BuildMode) {
        self.build_mode = mode;
    }

    pub fn get_extern_libs(&self) -> &Vec<String> {
        &self.extern_libs
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
            ("std::ops::Add", &[("add", 2)]),       // add(self, other)
            ("std::ops::Sub", &[("sub", 2)]),       // sub(self, other)
            ("std::ops::Mul", &[("mul", 2)]),       // mul(self, other)
            ("std::ops::Div", &[("div", 2)]),       // div(self, other)
            ("std::ops::Rem", &[("rem", 2)]),       // rem(self, other)
            ("std::cmp::Eq", &[("eq", 2)]),         // eq(self, other)
            ("std::cmp::Cmp", &[
                ("lt", 2), ("le", 2), ("gt", 2), ("ge", 2),
            ]),
            ("std::mem::Drop", &[("drop", 1)]),     // drop(self)
        ];
        for (name, methods) in std_traits {
            if !self.trait_defs.contains_key(*name) {
                self.trait_defs.insert(name.to_string(), TraitDefInfo {
                    name: name.to_string(),
                    methods: methods.iter().map(|(n, pc)| TraitMethodSig {
                        name: n.to_string(),
                        param_count: *pc,
                        dynamic: false,
                    }).collect(),
                    generic_params: vec!["T".to_string()],
                });
            }
        }
    }

    /// After all modules are loaded, validate that `impl Trait for Type` blocks
    /// provide all required trait methods.
    fn validate_trait_impls(&mut self) {
        let pending: Vec<_> = std::mem::take(&mut self.pending_trait_impls);
        for (struct_name, trait_name, impl_methods) in &pending {
            let trait_info = match self.trait_defs.get(trait_name).cloned() {
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

    pub fn analyze(&mut self, program: &Program) -> bool {
        // Register compiler-level builtins (panic / exit — handled by codegen).
        self.env.declare_function("panic", &DataType::None_, &self.current_module);
        self.env.declare_function("exit", &DataType::None_, &self.current_module);
        // Runtime intrinsics used by std library implementations
        self.env.declare_function("gobol_array_elem_addr", &DataType::Int, &self.current_module);
        // Array allocation intrinsic used by std library (e.g. Vec::push)
        self.env.declare_function("__new_array", &DataType::Unknown, &self.current_module);
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
        // explicit `import std;`.  This is a compiler-internal fallback;
        // user code still needs `import std;` for io/range/etc.
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
                self.env.declare_function(func.get_name(), &return_type, &self.current_module);
                self.current_generic_params = prev_generic;
            } else if let Some(struct_def) = stmt.as_any().downcast_ref::<StructDefinition>() {
                let prev_generic = self.current_generic_params.clone();
                self.current_generic_params = struct_def.get_generic_params().clone();
                let mut fields = HashMap::new();
                for field in struct_def.get_fields() {
                    let field_type = self.get_data_type_from_ast(field.field_type.as_deref());
                    fields.insert(field.name.clone(), field_type);
                }
                self.struct_fields.insert(struct_def.get_name().to_string(), fields);
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
                            if !self
                                .impl_methods
                                .insert((struct_name.clone(), method_name.clone(), arity))
                            {
                                continue;
                            }
                            let prev_fn_generic = self.current_generic_params.clone();
                            for g in func.get_generic_params() {
                                if !self.current_generic_params.contains(g) {
                                    self.current_generic_params.push(g.clone());
                                }
                            }
                            let return_type = self.get_data_type_from_ast(func.get_return_type());
                            self.env
                                .declare_function(&method_name, &return_type, &self.current_module);
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
            eprintln!("Semantic analysis failed with {} error(s):", self.errors.len());
            for err in &self.errors {
                eprintln!("{}", err);
            }
        }
    }

    fn error(&mut self, msg: &str) {
        self.has_error = true;
        #[cfg(debug_assertions)]
        eprintln!("[SEM ERROR] {} | current_module={} current_impl={:?}", msg, self.current_module, self.current_impl_struct);
        self.structured_errors.push((0, 0, msg.to_string()));
        if let Some(ref f) = self.error_formatter {
            let formatted = f.format_error(0, 0, 0, "error", msg, true);
            self.errors.push(formatted);
        } else {
            self.errors.push(format!("Error: {}", msg));
        }
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
            // PascalCase generic types (Vec<T>, Result<T, E>, etc.) — treat as struct
            return DataType::Struct(base.to_string());
        }

        // Check for NullableType
        if let Some(nullable) = tp.as_type_any().downcast_ref::<NullableType>() {
            let inner = self.get_data_type_from_ast(Some(nullable.get_inner_type()));
            return DataType::Nullable(Box::new(inner));
        }

        // Check for FunctionType (e.g. `func(T): U` callback parameters).
        // The analyser does not track precise function signatures — collapse
        // to Unknown so call sites on the parameter resolve without error.
        if tp.as_type_any().downcast_ref::<FunctionType>().is_some() {
            return DataType::Unknown;
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

    fn get_current_type(&self) -> DataType {
        if self.type_stack.is_empty() {
            DataType::Unknown
        } else {
            self.type_stack[self.type_stack.len() - 1].clone()
        }
    }

    fn check_type_compatibility(&mut self, target: DataType, source: DataType, context: &str) -> bool {
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
    fn resolve_header_file(&self, path: &str) -> Result<(std::path::PathBuf, String), std::io::Error> {
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
        if s.is_empty() { None } else { Some(std::path::PathBuf::from(s)) }
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
                let prev_ok = i == 0
                    || (!bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_');
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
        let relative = path_parts.join("/") + ".gbl";

        // First: check relative to the importing module's directory
        if let Some(dir) = base_dir {
            let rel_full = format!("{}/{}", dir, relative);
            if Path::new(&rel_full).exists() {
                return Some(rel_full);
            }
            let rel_mod = format!("{}/{}/mod.gbl", dir, path_parts.join("/"));
            if Path::new(&rel_mod).exists() {
                return Some(rel_mod);
            }
            let rel_setup = format!("{}/{}/__setup__.gbl", dir, path_parts.join("/"));
            if Path::new(&rel_setup).exists() {
                return Some(rel_setup);
            }
            // Also check in base_dir/lib/ (local lib directory)
            let rel_lib = format!("{}/lib/{}", dir, relative);
            if Path::new(&rel_lib).exists() {
                return Some(rel_lib);
            }
        }

        // Second: check each lib path
        for lib_path in &self.lib_paths {
            // <lib_path>/<module>.gbl
            let full = format!("{}/{}", lib_path, relative);
            if Path::new(&full).exists() {
                return Some(full);
            }
            // <lib_path>/<module>/mod.gbl  (modern module entry point)
            let mod_relative = format!("{}/mod.gbl", path_parts.join("/"));
            let mod_full = format!("{}/{}", lib_path, mod_relative);
            if Path::new(&mod_full).exists() {
                return Some(mod_full);
            }
            // <lib_path>/<module>/__setup__.gbl (legacy)
            let setup_relative = format!("{}/__setup__.gbl", path_parts.join("/"));
            let setup_full = format!("{}/{}", lib_path, setup_relative);
            if Path::new(&setup_full).exists() {
                return Some(setup_full);
            }
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
        // Third: try without lib prefix
        let direct = format!("{}.gbl", path_parts.join("/"));
        if Path::new(&direct).exists() {
            return Some(direct);
        }
        let mod_direct = format!("{}/mod.gbl", path_parts.join("/"));
        if Path::new(&mod_direct).exists() {
            return Some(mod_direct);
        }
        let setup_direct = format!("{}/__setup__.gbl", path_parts.join("/"));
        if Path::new(&setup_direct).exists() {
            return Some(setup_direct);
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
        let has_explicit_export = prog.get_statements().iter().any(|stmt| {
            stmt.as_any().downcast_ref::<ExportStatement>().is_some()
        });

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
        // Imported module names so `export(mod)` can re-export modules.
        let mut imported_mods: Vec<String> = Vec::new();

        // ---- Phase 2: Process declarations ----
        for stmt in prog.get_statements() {
            if let Some(import_stmt) = stmt.as_any().downcast_ref::<ImportStatement>() {
                let name = import_stmt.get_module_name();
                imported_mods.push(name.clone());
                self.load_module(&name);
                if let Some(alias) = import_stmt.get_alias() {
                    self.module_aliases.insert(alias.to_string(), name);
                }
            } else if let Some(from_import) = stmt.as_any().downcast_ref::<FromImportStatement>() {
                // `from module import member, ...` inside a loaded module.
                // Load the referenced module, then re-export the requested
                // members as bare names in this module's namespace.
                let mod_name = from_import.get_module();
                self.load_module(mod_name);

                // Handle wildcard import: `from module import *`
                if from_import.is_wildcard() {
                    let module_symbols = self.env.get_module_symbols(mod_name);
                    for (name, sym) in module_symbols {
                        let return_type = sym.data_type.clone();
                        self.env.declare_function(&name, &return_type, &self.current_module);
                        declared_funcs.push((name, return_type));
                    }
                } else {
                    // Handle specific members with optional aliases
                    for (member_name, alias) in from_import.get_members() {
                        let qualified = format!("{}::{}", mod_name, member_name);
                        if let Some(sym) = self.env.lookup_symbol(&qualified) {
                            let return_type = sym.data_type.clone();
                            let effective_name = alias.as_ref().unwrap_or(member_name);
                            self.env.declare_function(effective_name, &return_type, &self.current_module);
                            declared_funcs.push((effective_name.to_string(), return_type));
                        }
                    }
                }
            } else if let Some(func) = stmt.as_any().downcast_ref::<Function>() {
                if self.build_mode == BuildMode::Release && Attribute::has_attr(func.get_attributes(), "debug") {
                    continue;
                }
                let func_name = func.get_name().to_string();
                let prev_generic = self.current_generic_params.clone();
                self.current_generic_params = func.get_generic_params().clone();
                let return_type = self.get_data_type_from_ast(func.get_return_type());
                self.env.declare_function(&func_name, &return_type, &self.current_module);
                declared_funcs.push((func_name, return_type));
                self.current_generic_params = prev_generic;
            } else if let Some(struct_def) = stmt.as_any().downcast_ref::<StructDefinition>() {
                let struct_name = struct_def.get_name().to_string();
                let prev_generic = self.current_generic_params.clone();
                self.current_generic_params = struct_def.get_generic_params().clone();
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
                self.current_impl_struct = Some(impl_block.get_struct_name().split('<').next().unwrap_or(impl_block.get_struct_name()).to_string());
                let prev_impl_generic = self.current_generic_params.clone();
                self.current_generic_params = impl_block.get_generic_params().clone();

                // If this is `impl Trait for Type`, defer validation until after all modules loaded
                if let Some(trait_name) = impl_block.get_trait_name() {
                    let method_names: Vec<String> = impl_block.get_items().iter().filter_map(|item| {
                        match item {
                            ImplItem::Method(func) | ImplItem::Convert(func) => {
                                if self.build_mode == BuildMode::Release && Attribute::has_attr(func.get_attributes(), "debug") {
                                    None
                                } else {
                                    Some(func.get_name().to_string())
                                }
                            }
                        }
                    }).collect();
                    self.pending_trait_impls.push((
                        impl_block.get_struct_name().to_string(),
                        trait_name.to_string(),
                        method_names,
                    ));
                }

                for item in impl_block.get_items() {
                    match item {
                        ImplItem::Method(func) | ImplItem::Convert(func) => {
                            if self.build_mode == BuildMode::Release && Attribute::has_attr(func.get_attributes(), "debug") {
                                continue;
                            }
                            let func_name = func.get_name().to_string();
                            // Dedup: skip if this (struct, method, arity) triple was
                            // already declared. This prevents duplicate impls when a
                            // module is loaded via multiple import paths or when std
                            // re-exports a module, while still allowing overloaded
                            // methods (same name, different arity).
                            let struct_name = impl_block.get_struct_name().to_string();
                            let arity = func
                                .get_parameters()
                                .map(|p| p.len())
                                .unwrap_or(0);
                            if !self.impl_methods.insert((struct_name.clone(), func_name.clone(), arity)) {
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
                            self.env.declare_function(&func_name, &return_type, &self.current_module);
                            // Also register with struct name prefix for Type::method() calls
                            if let Some(ref struct_name) = self.current_impl_struct {
                                self.env.declare_function(&func_name, &return_type, struct_name);
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
                    self.env.declare_function(&func_name, &return_type, &self.current_module);
                    declared_funcs.push((func_name, return_type));
                }
            } else if let Some(enum_def) = stmt.as_any().downcast_ref::<EnumDefinition>() {
                self.visit_enum_definition(enum_def);
                declared_structs.push(enum_def.get_name().to_string());
            } else if let Some(trait_def) = stmt.as_any().downcast_ref::<TraitDefinition>() {
                // Register trait definition for `impl Trait for Type` validation
                let methods: Vec<TraitMethodSig> = trait_def.get_methods().iter().map(|m| {
                    TraitMethodSig {
                        name: m.name.clone(),
                        param_count: m.parameters.len(),
                        dynamic: Attribute::has_attr(&m.attributes, "dynamic"),
                    }
                }).collect();
                self.trait_defs.insert(trait_def.get_name().to_string(), TraitDefInfo {
                    name: trait_def.get_name().to_string(),
                    methods,
                    generic_params: trait_def.get_generic_params().clone(),
                });
                declared_traits.push(trait_def.get_name().to_string());
            } else if let Some(export_stmt) = stmt.as_any().downcast_ref::<ExportStatement>() {
                for name in export_stmt.get_names() {
                    let parts: Vec<&str> = name.split('.').collect();
                    let short = parts.last().unwrap_or(&"");
                    let original_key = if parts.len() > 1 {
                        let mod_part = parts[0];
                        let resolved_mod = self.module_aliases.get(mod_part).map(|s| s.as_str()).unwrap_or(mod_part);
                        format!("{}::{}", resolved_mod, short)
                    } else {
                        format!("{}::{}", self.current_module, name)
                    };
                    if let Some(sym) = self.env.lookup_symbol(&original_key) {
                        let return_type = sym.data_type.clone();
                        self.env.declare_function(short, &return_type, &self.current_module);
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
                self.env.declare_function(name, return_type, &self.current_module);
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
            // Also re-export every imported module into this module's
            // namespace so `from m import ...` style patterns keep working
            // after we drop explicit `export(...)` lists.
            for mod_name in &imported_mods {
                let short = mod_name.rsplit("::").next().unwrap_or(mod_name);
                self.env.declare_module(short);
            }
        }

        // Restore context
        self.current_module = prev_module;
        self.current_module_dir = prev_dir;

        self.loaded_programs.push(prog);
    }
}

impl AstVisitor for SemanticAnalyzer {
    fn visit_ast_node(&mut self, _node: &dyn AstNode) {}

    fn visit_statement(&mut self, _node: &dyn Statement) {}

    fn visit_expression(&mut self, _node: &dyn Expression) {}

    fn visit_program(&mut self, node: &Program) {
        for stmt in node.get_statements() {
            stmt.accept(self);
        }
    }

    fn visit_struct_definition(&mut self, node: &StructDefinition) {
        let struct_name = node.get_name().to_string();
        #[cfg(debug_assertions)]
        println!("  Struct definition: {}", struct_name);


        // Push generic scope so that type params like T are resolved in field types
        let prev_generic = self.current_generic_params.clone();
        self.current_generic_params = node.get_generic_params().clone();
        let mut fields = HashMap::new();
        for field in node.get_fields() {
            let field_type = self.get_data_type_from_ast(field.field_type.as_deref());
            fields.insert(field.name.clone(), field_type);
        }
        self.struct_fields.insert(struct_name.clone(), fields);

        self.env.declare_module(&struct_name);
        self.current_generic_params = prev_generic;
    }

    fn visit_enum_definition(&mut self, node: &EnumDefinition) {
        let enum_name = node.get_name().to_string();
        let generic_params = node.get_generic_params().clone();
        #[cfg(debug_assertions)]
        println!("  Enum definition: {} ({} variants)", enum_name, node.get_variants().len());

        // Push generic scope so that type params like T, E are resolved
        let prev_generic = self.current_generic_params.clone();
        self.current_generic_params = generic_params.clone();

        // Lower enum to a tagged struct:
        //   struct EnumName { _tag: int, _0: Payload0, _1: Payload1, ... }
        let mut fields = HashMap::new();
        fields.insert("_tag".to_string(), DataType::Int);

        let mut variant_idx = 0i32;
        for variant in node.get_variants() {
            if let Some(ref payload) = variant.payload_type {
                let field_ty = self.get_data_type_from_ast(Some(payload.as_ref()));
                fields.insert(format!("_{}", variant_idx), field_ty);
            }
            variant_idx += 1;
        }
        self.struct_fields.insert(enum_name.clone(), fields);

        // Declare enum name as a module so `EnumName::Variant` resolution works.
        self.env.declare_module(&enum_name);

        // Register each variant as a constructor function in the enum's module.
        for variant in node.get_variants() {
            let ctor_name = variant.name.clone();
            let return_type = DataType::Struct(enum_name.clone());

            // Declare in the enum's namespace: EnumName::VariantName
            // Also register as `enum_name::VariantName` for general lookup
            self.env.declare_function(&ctor_name, &return_type, &enum_name);
            self.env.declare_function(&format!("{}::{}", enum_name, ctor_name), &return_type, &self.current_module);
        }

        self.current_generic_params = prev_generic;
    }

    fn visit_impl_block(&mut self, node: &ImplBlock) {
        #[cfg(debug_assertions)]
        println!("  Impl block for: {}", node.get_struct_name());

        // If this is `impl Trait for Type`, only validate if the trait has
        // already been loaded. Otherwise defer to validate_trait_impls()
        // which runs after all modules are loaded (the first pass already
        // queued this block in pending_trait_impls).
        if let Some(trait_name) = node.get_trait_name() {
            if let Some(trait_info) = self.trait_defs.get(trait_name).cloned() {
                let impl_methods: HashMap<String, Vec<usize>> = node.get_items().iter()
                    .filter_map(|item| {
                        match item {
                            ImplItem::Method(func) | ImplItem::Convert(func) => {
                                if self.build_mode == BuildMode::Release && Attribute::has_attr(func.get_attributes(), "debug") {
                                    None
                                } else {
                                    let name = func.get_name().to_string();
                                    let param_count = func.get_parameters().iter().len();
                                    Some((name, param_count))
                                }
                            }
                        }
                    })
                    .fold(HashMap::new(), |mut map, (name, count)| {
                        map.entry(name).or_insert_with(Vec::new).push(count);
                        map
                    });

                for required in &trait_info.methods {
                    if !impl_methods.contains_key(&required.name) {
                        self.error(&format!(
                            "Trait '{}' requires method '{}', but it is missing in impl for '{}'",
                            trait_name, required.name, node.get_struct_name()
                        ));
                        // #[dynamic] methods can have any signature — skip param check
                        if required.dynamic {
                            continue;
                        }
                    }
                }
            }
            // Trait not loaded yet — validate_trait_impls() will handle it.
        }

        let prev_impl = self.current_impl_struct.clone();
        let prev_generic = self.current_generic_params.clone();
        self.current_impl_struct = Some(node.get_struct_name().split('<').next().unwrap_or(node.get_struct_name()).to_string());
        self.current_generic_params = node.get_generic_params().clone();

        // === Pass 1: Register all method names (without analyzing bodies) ===
        // This ensures that methods defined in later impl blocks are available
        // for lookup when analyzing earlier impl blocks (e.g. Vec::push is needed
        // by New::new which is in a different impl block).
        for item in node.get_items() {
            match item {
                ImplItem::Method(func) | ImplItem::Convert(func) => {
                    if self.build_mode == BuildMode::Release && Attribute::has_attr(func.get_attributes(), "debug") {
                        continue;
                    }
                    let struct_name = node.get_struct_name().to_string();
                    let method_name = func.get_name().to_string();
                    let arity = func
                        .get_parameters()
                        .map(|p| p.len())
                        .unwrap_or(0);
                    if !self.impl_methods.insert((struct_name.clone(), method_name.clone(), arity)) {
                        continue;
                    }
                    let prev_fn_generic = self.current_generic_params.clone();
                    for g in func.get_generic_params() {
                        if !self.current_generic_params.contains(g) {
                            self.current_generic_params.push(g.clone());
                        }
                    }
                    let return_type = self.get_data_type_from_ast(func.get_return_type());
                    self.env.declare_function(&method_name, &return_type, &self.current_module);
                    if let Some(ref struct_name) = self.current_impl_struct {
                        self.env.declare_function(&method_name, &return_type, struct_name);
                    }
                    self.current_generic_params = prev_fn_generic;
                }
            }
        }

        // === Pass 2: Analyze all method bodies ===
        for item in node.get_items() {
            match item {
                ImplItem::Method(func) | ImplItem::Convert(func) => {
                    if self.build_mode == BuildMode::Release && Attribute::has_attr(func.get_attributes(), "debug") {
                        continue;
                    }
                    let struct_name = node.get_struct_name().to_string();
                    let method_name = func.get_name().to_string();
                    let arity = func
                        .get_parameters()
                        .map(|p| p.len())
                        .unwrap_or(0);
                    // Skip if not registered in pass 1 (dedup)
                    if !self.impl_methods.contains(&(struct_name, method_name, arity)) {
                        continue;
                    }
                    let prev_fn_generic = self.current_generic_params.clone();
                    for g in func.get_generic_params() {
                        if !self.current_generic_params.contains(g) {
                            self.current_generic_params.push(g.clone());
                        }
                    }
                    func.accept(self);
                    self.current_generic_params = prev_fn_generic;
                }
            }
        }

        self.current_impl_struct = prev_impl;
        self.current_generic_params = prev_generic;
    }

    fn visit_trait_definition(&mut self, node: &TraitDefinition) {
        #[cfg(debug_assertions)]
        println!("  Trait definition: {}", node.get_name());

        let methods: Vec<TraitMethodSig> = node.get_methods().iter().map(|m| {
            TraitMethodSig {
                name: m.name.clone(),
                param_count: m.parameters.len(),
                dynamic: Attribute::has_attr(&m.attributes, "dynamic"),
            }
        }).collect();
        self.trait_defs.insert(node.get_name().to_string(), TraitDefInfo {
            name: node.get_name().to_string(),
            methods,
            generic_params: node.get_generic_params().clone(),
        });
    }

    fn visit_export_statement(&mut self, _node: &ExportStatement) {
        // Export is a compile-time concept; no runtime effect
        // TODO: validate exported names are declared in current module
    }

    fn visit_extern_block(&mut self, node: &ExternBlock) {
        #[cfg(debug_assertions)]
        println!("  Extern block: lib={:?}", node.get_library());

        // Every extern "C" block MUST specify a #[header("path")] attribute
        // pointing to the C header that declares these functions. The header
        // is used for signature validation only — symbols are NOT auto-imported;
        // the user must explicitly list each function in the block.
        let header_path = match Attribute::get_attr_value(node.get_attributes(), "header") {
            Some(path) => path.to_string(),
            None => {
                self.error(
                    "extern \"C\" block requires a #[header(\"path/to/header.h\")] attribute \
                     specifying the C header that provides these functions",
                );
                return;
            }
        };

        // Validate that the header file exists and contains declarations
        // for every function in the block.
        let header_content = match self.read_header_file(&header_path) {
            Ok(content) => content,
            Err(e) => {
                self.error(&format!(
                    "Cannot read C header '{}' specified by #[header]: {}",
                    header_path, e
                ));
                return;
            }
        };

        if let Some(lib) = node.get_library() {
            // "C" is an ABI specifier, not a library name — don't pass -lC.
            // Only collect actual library names (e.g. "m" → -lm).
            if lib != "C" && !self.extern_libs.iter().any(|l| l == lib) {
                self.extern_libs.push(lib.to_string());
            }
        }

        for func in node.get_functions() {
            let func_name = func.get_name().to_string();

            // Validate that the function is declared in the specified header.
            // We do a lightweight text search for the function name appearing
            // as a function declaration (preceded by a non-identifier char to
            // avoid matching substrings of other names).
            if !Self::header_declares_function(&header_content, &func_name) {
                self.error(&format!(
                    "Function '{}' is not declared in header '{}'",
                    func_name, header_path
                ));
            }

            let return_type = self.get_data_type_from_ast(func.get_return_type());
            // Register extern "C" functions in the current module so that
            // call sites (e.g. `printf(...)`) resolve to `<module>::printf`.
            self.env.declare_function(&func_name, &return_type, &self.current_module);
            func.accept(self);
        }
    }

    fn visit_import_statement(&mut self, node: &ImportStatement) {
        let module_name = node.get_module_name();
        #[cfg(debug_assertions)]
        println!("  Import module: {} (alias: {:?})", module_name, node.get_alias());

        self.load_module(&module_name);
        if let Some(alias) = node.get_alias() {
            self.module_aliases.insert(alias.to_string(), module_name);
        }
    }

    fn visit_from_import_statement(&mut self, node: &FromImportStatement) {
        let module_name = node.get_module();
        #[cfg(debug_assertions)]
        println!("  From-import: module={}, wildcard={}, members={:?}", module_name, node.is_wildcard(), node.get_members());

        // First, ensure the module is loaded (this registers all its
        // public symbols under `module::member` keys).
        self.load_module(module_name);

        // Handle wildcard import: `from module import *`
        if node.is_wildcard() {
            let module_symbols = self.env.get_module_symbols(module_name);
            for (name, sym) in module_symbols {
                let return_type = sym.data_type.clone();
                self.env.declare_function(&name, &return_type, &self.current_module);
            }
            return;
        }

        // Then, re-declare each requested member as a bare name in the
        // current module so it can be called without the `module::` qualifier.
        for (member_name, alias) in node.get_members() {
            let qualified = format!("{}::{}", module_name, member_name);
            if let Some(sym) = self.env.lookup_symbol(&qualified) {
                let return_type = sym.data_type.clone();
                // Use alias if provided, otherwise use original name
                let effective_name = alias.as_ref().unwrap_or(member_name);
                self.env.declare_function(effective_name, &return_type, &self.current_module);
            } else {
                self.error(&format!(
                    "Cannot import '{}' from module '{}': member not found",
                    member_name, module_name
                ));
            }
        }
    }

    fn visit_function(&mut self, node: &Function) {
        if self.build_mode == BuildMode::Release && Attribute::has_attr(node.get_attributes(), "debug") {
            return;
        }
        let func_name = node.get_name().to_string();
        #[cfg(debug_assertions)]
        println!("  Function: {}", func_name);

        // Save impl context for self parameter handling
        let prev_impl = self.current_impl_struct.clone();
        // If this is a top-level function with `self: StructType` parameter,
        // set current_impl_struct so that field access (_data, _cap, _len) works.
        if self.current_impl_struct.is_none() {
            if let Some(params) = node.get_parameters() {
                if let Some(first_param) = params.first() {
                    if first_param.get_name() == "self" {
                        if let Some(tp) = first_param.get_type() {
                            let type_name = tp.get_name();
                            if self.struct_fields.contains_key(type_name) {
                                self.current_impl_struct = Some(type_name.to_string());
                            }
                        }
                    }
                }
            }
        }
        // Set generic params BEFORE type resolution
        // Merge function-level generics with existing (e.g., impl-block-level) generics
        let prev_generic_params = self.current_generic_params.clone();
        let mut combined = prev_generic_params.clone();
        for g in node.get_generic_params() {
            if !combined.contains(g) {
                combined.push(g.clone());
            }
        }
        self.current_generic_params = combined;

        let return_type = self.get_data_type_from_ast(node.get_return_type());

        if !self.env.declare_function(&func_name, &return_type, &self.current_module) {
            self.error(&format!(
                "Failed to declare function '{}.{}'",
                self.current_module, func_name
            ));
            self.current_impl_struct = prev_impl;
            self.current_generic_params = prev_generic_params;
            return;
        }

        // Also register with struct name prefix for Type.method() calls
        if let Some(ref struct_name) = self.current_impl_struct {
            self.env.declare_function(&func_name, &return_type, struct_name);
        }

        // Save context
        let prev_function = self.current_function.clone();
        let prev_return_type = self.current_function_return_type.clone();
        let prev_has_return = self.has_return_statement;

        self.current_function = func_name;
        self.current_function_return_type = return_type.clone();
        self.has_return_statement = false;

        self.env.enter_scope();

        // Parameters
        if let Some(params) = node.get_parameters() {
            for param in params {
                param.accept(self);
            }
        }

        // Body
        let mut has_tail_expr = false;
        if let Some(body) = node.get_body() {
            body.accept(self);
            // Check if last statement is a tail expression (implicit return)
            let stmts = body.get_statements();
            if let Some(last) = stmts.last() {
                if let Some(es) = last.as_any().downcast_ref::<ExpressionStatement>() {
                    if es.tail {
                        has_tail_expr = true;
                    }
                }
                // Also treat if/match/block expressions as implicit returns
                if last.as_any().downcast_ref::<IfStatement>().is_some()
                    || last.as_any().downcast_ref::<MatchExpression>().is_some()
                {
                    has_tail_expr = true;
                }
            }
        }

        if !Attribute::has_attr(node.get_attributes(), "intrinsic") {
            if return_type != DataType::None_ && !self.has_return_statement && !has_tail_expr {
                self.error(&format!(
                    "Function '{}' must return a value of type {}",
                    self.current_function,
                    data_type_to_string(return_type)
                ));
            }
        }

        self.env.exit_scope();

        self.current_function = prev_function;
        self.current_function_return_type = prev_return_type;
        self.has_return_statement = prev_has_return;
        self.current_generic_params = prev_generic_params;
    }

    fn visit_lambda(&mut self, node: &Lambda) {
        // Lambda is analyzed in the context of a function call (inline).
        // Set up generic params, a new scope for parameters, and analyze the body.
        let prev_generic_params = self.current_generic_params.clone();
        let mut combined = prev_generic_params.clone();
        for g in node.get_generic_params() {
            if !combined.contains(g) {
                combined.push(g.clone());
            }
        }
        self.current_generic_params = combined;

        let return_type = self.get_data_type_from_ast(node.get_return_type());

        let prev_function = self.current_function.clone();
        let prev_return_type = self.current_function_return_type.clone();
        let prev_has_return = self.has_return_statement;

        self.current_function = "__lambda".to_string();
        self.current_function_return_type = return_type.clone();
        self.has_return_statement = false;

        self.env.enter_scope();

        // Parameters
        if let Some(params) = node.get_parameters() {
            for param in params {
                param.accept(self);
            }
        }

        // Body
        node.get_body().accept(self);

        self.env.exit_scope();

        self.current_function = prev_function;
        self.current_function_return_type = prev_return_type;
        self.has_return_statement = prev_has_return;
        self.current_generic_params = prev_generic_params;

        // The lambda expression's type is its return type (used when the
        // lambda is immediately invoked).
        self.type_stack.push(return_type);
    }

    fn visit_parameter(&mut self, node: &Parameter) {
        let param_name = node.get_name();
        let param_type = self.get_data_type_from_ast(node.get_type());

        // Array parameters are always mutable (reference type)
        let is_array = node.get_type().map_or(false, |t| t.as_type_any().downcast_ref::<ArrayType>().is_some());
        // For array parameters, store the element type (unwrap Array wrapper)
        let stored_type = if is_array {
            if let DataType::Array(elem) = &param_type {
                (**elem).clone()
            } else {
                param_type.clone()
            }
        } else {
            param_type.clone()
        };
        self.env.declare_variable(param_name, &stored_type, is_array);
        // Mark as array if the parameter type is an array
        if is_array {
            if let Some(sym) = self.env.lookup_symbol_mut(param_name) {
                sym.is_array = true;
            }
        }
        #[cfg(debug_assertions)]
        println!("    Parameter: {} : {}", param_name, data_type_to_string(param_type));
    }

    fn visit_block(&mut self, node: &Block) {
        self.env.enter_scope();
        #[cfg(debug_assertions)]
        println!("    Block (scope {})", self.env.get_current_scope());

        for stmt in node.get_statements() {
            stmt.accept(self);
        }

        self.env.exit_scope();
    }

    fn visit_declaration(&mut self, node: &Declaration) {
        let var_name = node.get_name().to_string();
        let is_mut = node.get_keyword() == "var";

        // Check for array type
        if let Some(tp) = node.get_type() {
            if let Some(_array_type) = tp.as_type_any().downcast_ref::<ArrayType>() {
                let mut constant_sizes: Vec<i32> = Vec::new();
                let mut expr_sizes: Vec<Box<dyn Expression>> = Vec::new();
                let mut all_constant = true;

                // Walk array dimensions
                let mut current: Option<&dyn Type> = Some(tp);
                let mut innermost: Option<&dyn Type> = None;

                while let Some(c) = current {
                    if let Some(arr) = c.as_type_any().downcast_ref::<ArrayType>() {
                        if let Some(size) = arr.get_size() {
                            size.accept(self);
                            let size_type = self.get_current_type();
                            self.type_stack.pop();

                            if size_type != DataType::Int {
                                self.error("Array size must be integer");
                                return;
                            }

                            // Check if size is constant
                            if let Some(num) = (size as &dyn AstNode).as_any().downcast_ref::<NumberLiteral>() {
                                constant_sizes.push(num.get_value() as i32);
                                expr_sizes.push(Box::new(NumberLiteral::new(0.0))); // placeholder
                            } else {
                                all_constant = false;
                                constant_sizes.push(0);
                                // Can't easily clone trait objects, skip expr
                            }
                        }
                        current = Some(arr.get_element_type());
                    } else {
                        innermost = Some(c);
                        break;
                    }
                }

                let elem_type = self.get_data_type_from_ast(innermost);

                if all_constant {
                    self.env.declare_array_constant(&var_name, &elem_type, &constant_sizes, is_mut);
                } else {
                    // For non-constant arrays
                    self.env.declare_variable(&var_name, &elem_type, is_mut);
                }

                return;
            }
        }

        // Normal variable
        let declared_type = self.get_data_type_from_ast(node.get_type());
        let actual_type = if declared_type == DataType::None_ {
            // Infer type from initializer
            if let Some(init) = node.get_initializer() {
                init.accept(self);
                let init_type = self.get_current_type();
                self.env.declare_variable(&var_name, &init_type, is_mut);
                return;
            } else {
                self.env.declare_variable(&var_name, &declared_type, is_mut);
                return;
            }
        } else {
            self.env.declare_variable(&var_name, &declared_type, is_mut);
            declared_type
        };

        if let Some(init) = node.get_initializer() {
            init.accept(self);
            let init_type = self.get_current_type();
            self.check_type_compatibility(
                actual_type,
                init_type,
                &format!("variable '{}' initialization", var_name),
            );
        }
    }

    fn visit_if_statement(&mut self, node: &IfStatement) {
        #[cfg(debug_assertions)]
        println!("    IfStatement");

        if let Some(cond) = node.get_condition() {
            cond.accept(self);
            let cond_type = self.get_current_type();
            self.type_stack.pop();  // Pop condition type after use

            if cond_type != DataType::Bool && !Environment::is_numeric_type(&cond_type) {
                self.error("If condition must be boolean or numeric type");
            }
        }

        if let Some(then_branch) = node.get_then_branch() {
            then_branch.accept(self);
        }

        if let Some(else_branch) = node.get_else_branch() {
            else_branch.accept(self);
        }
    }

    fn visit_while_statement(&mut self, node: &WhileStatement) {
        #[cfg(debug_assertions)]
        println!("    WhileStatement");

        if let Some(cond) = node.get_condition() {
            cond.accept(self);
            let cond_type = self.get_current_type();
            self.type_stack.pop();  // Pop condition type after use

            if cond_type != DataType::Bool && !Environment::is_numeric_type(&cond_type) {
                self.error("While condition must be boolean or numeric type");
            }
        }

        self.loop_depth += 1;

        if let Some(body) = node.get_body() {
            body.accept(self);
        }

        self.loop_depth -= 1;
    }

    fn visit_for_statement(&mut self, node: &ForStatement) {
        let loop_vars = node.get_loop_variables().clone();

        self.env.enter_scope();

        // Declare index variable (first) as Int
        self.env.declare_variable(&loop_vars[0], &DataType::Int, false);

        if let Some(iter) = node.get_iterable() {
            iter.accept(self);
            let iter_type = self.get_current_type();
            self.type_stack.pop();  // Pop iterable type after use
            // Accept Range, Str, array types, generic type params (like T in T[]),
            // Unknown, and variables that are arrays
            let iter_is_valid = matches!(iter_type, DataType::Int)
                || matches!(&iter_type, DataType::Struct(s) if s == "Range")
                || matches!(iter_type, DataType::Str)
                || matches!(iter_type, DataType::Unknown)
                || matches!(&iter_type, DataType::Struct(s) if self.current_generic_params.contains(&s.to_string()))
                || matches!(iter_type, DataType::Array(_))
                || matches!(&iter_type, DataType::Struct(s) if s == "Vec");
            if !iter_is_valid {
                // Check if the iterable is a variable that is an array
                let is_array_var = if let Some(iter_expr) = node.get_iterable() {
                    if let Some(id) = iter_expr.as_any().downcast_ref::<crate::ast::Identifier>() {
                        self.env.lookup_symbol(id.get_name()).map_or(false, |s| s.is_array)
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !is_array_var {
                    self.error("For loop iterable must be range, string, or array");
                }
            }

            // Declare value variable (second) with appropriate type
            if loop_vars.len() >= 2 {
                let val_type = match &iter_type {
                    DataType::Struct(s) if s == "Range" => DataType::Int,
                    DataType::Str => DataType::Str,
                    DataType::Array(elem) => (**elem).clone(),
                    _ => DataType::Int, // default for arrays
                };
                self.env.declare_variable(&loop_vars[1], &val_type, false);
            }
        }

        self.loop_depth += 1;

        if let Some(body) = node.get_body() {
            body.accept(self);
        }

        self.loop_depth -= 1;
        self.env.exit_scope();
    }

    fn visit_return_statement(&mut self, node: &ReturnStatement) {
        self.has_return_statement = true;

        if self.current_function.is_empty() {
            self.error("Return statement outside function");
            return;
        }

        if node.get_value().is_none() {
            if self.current_function_return_type != DataType::None_ {
                let expected = self.current_function_return_type.clone();
                self.error(&format!(
                    "Function '{}' expects return type {}, but got none",
                    self.current_function,
                    data_type_to_string(expected)
                ));
            }
            return;
        }

        if let Some(val) = node.get_value() {
            val.accept(self);
            let return_type = self.get_current_type();
            let expected = self.current_function_return_type.clone();

            self.check_type_compatibility(
                expected,
                return_type,
                &format!("function '{}' return", self.current_function),
            );
        }
    }

    fn visit_break_statement(&mut self, _node: &BreakStatement) {
        if self.loop_depth == 0 {
            self.error("Break statement outside loop");
        }
    }

    fn visit_continue_statement(&mut self, _node: &ContinueStatement) {
        if self.loop_depth == 0 {
            self.error("Continue statement outside loop");
        }
    }

    fn visit_expression_statement(&mut self, node: &ExpressionStatement) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
    }

    fn visit_identifier(&mut self, node: &Identifier) {
        let name = node.get_name();

        // Check for 'self' inside an impl block
        if name == "self" {
            if let Some(ref struct_name) = self.current_impl_struct {
                self.type_stack.push(DataType::Struct(struct_name.clone()));
                return;
            }
            self.error("'self' used outside of impl block");
            self.type_stack.push(DataType::Unknown);
            return;
        }

        // Check for bare struct field access inside an impl block
        if let Some(ref struct_name) = self.current_impl_struct {
            if let Some(fields) = self.struct_fields.get(struct_name) {
                if let Some(field_type) = fields.get(name) {
                    self.type_stack.push(field_type.clone());
                    return;
                }
            }
        }

        // Look up: bare name first (local vars/params shadow module-level),
        // then module-prefixed name (for module-level functions).
        // This ensures local parameters like `index: int` are found before
        // module-level methods like `vec::index` (Vec::index).
        let sym = self.env.lookup_symbol(name)
            .or_else(|| {
                let full_name = format!("{}::{}", self.current_module, name);
                self.env.lookup_symbol(&full_name)
            });

        match sym {
            Some(s) => self.type_stack.push(s.data_type.clone()),
            None => {
                self.error(&format!("Undeclared identifier: '{}'", name));
                self.type_stack.push(DataType::Unknown);
            }
        }
    }

    fn visit_number_literal(&mut self, node: &NumberLiteral) {
        // Source-level form decides int vs float (e.g. `3.0` is float, `3` is int).
        if node.is_float_literal() {
            self.type_stack.push(DataType::Float);
        } else {
            self.type_stack.push(DataType::Int);
        }
    }

    fn visit_string_literal(&mut self, _node: &StringLiteral) {
        self.type_stack.push(DataType::Str);
    }

    fn visit_null_literal(&mut self, _node: &NullLiteral) {
        self.type_stack.push(DataType::None_);
    }

    fn visit_array_literal(&mut self, node: &ArrayLiteral) {
        let mut first = true;
        let mut elem_type = DataType::Unknown;
        for elem in node.get_elements() {
            elem.accept(self);
            let et = self.get_current_type();
            self.type_stack.pop();
            if first {
                elem_type = et;
                first = false;
            } else if !Environment::is_type_compatible(&elem_type, &et) {
                self.error(&format!(
                    "Mixed types in array literal: {} and {}",
                    data_type_to_string(elem_type.clone()),
                    data_type_to_string(et)
                ));
            }
        }
        self.type_stack.push(DataType::Array(Box::new(elem_type)));
    }

    fn visit_boolean_literal(&mut self, _node: &BooleanLiteral) {
        self.type_stack.push(DataType::Bool);
    }

    fn visit_format_string(&mut self, node: &FormatString) {
        for var in node.get_variables() {
            if let Some(ref val) = var.value {
                val.accept(self);
                self.type_stack.pop();
            } else {
                self.error("Invalid expression in format string");
            }
        }
        self.type_stack.push(DataType::Str);
    }

    fn visit_binary_expression(&mut self, node: &BinaryExpression) {
        if let Some(left) = node.get_left() {
            left.accept(self);
        }
        let left_type = self.get_current_type();
        self.type_stack.pop();

        if let Some(right) = node.get_right() {
            right.accept(self);
        }
        let right_type = self.get_current_type();
        self.type_stack.pop();

        let op = node.get_operator();

        if op == "=" {
            let mut is_assignable = false;

            if let Some(left) = node.get_left() {
                if let Some(id) = left.as_any().downcast_ref::<Identifier>() {
                    let name = id.get_name();
                    // Check if it's a struct field in the current impl block
                    if let Some(ref struct_name) = self.current_impl_struct {
                        if let Some(fields) = self.struct_fields.get(struct_name) {
                            if fields.contains_key(name) {
                                is_assignable = true;
                            }
                        }
                    }
                    if !is_assignable {
                        if let Some(sym) = self.env.lookup_symbol(name) {
                            if sym.is_mut {
                                is_assignable = true;
                            } else {
                                self.error(&format!("Cannot assign to constant variable '{}'", name));
                            }
                        }
                    }
                } else if let Some(member) = left.as_any().downcast_ref::<MemberAccess>() {
                    // Check for self.field assignment inside impl block
                    if let Some(obj) = member.get_object() {
                        if let Some(obj_id) = obj.as_any().downcast_ref::<Identifier>() {
                            if obj_id.get_name() == "self" {
                                if let Some(ref struct_name) = self.current_impl_struct {
                                    if let Some(fields) = self.struct_fields.get(struct_name) {
                                        if fields.contains_key(member.get_member()) {
                                            is_assignable = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if let Some(arr_idx) = left.as_any().downcast_ref::<ArrayIndex>() {
                    // Walk nested array indices
                    let mut array: &dyn Expression = arr_idx as &dyn Expression;
                    while let Some(nested) = array.as_any().downcast_ref::<ArrayIndex>() {
                        if let Some(a) = nested.get_array() {
                            array = a;
                        } else {
                            break;
                        }
                    }

                    // Check if the base is a struct field (e.g., _data in impl block)
                    let is_struct_field_array = if let Some(arr_id) = array.as_any().downcast_ref::<Identifier>() {
                        self.current_impl_struct.as_ref().map_or(false, |s| {
                            self.struct_fields.get(s).map_or(false, |f| f.contains_key(arr_id.get_name()))
                        })
                    } else {
                        false
                    };

                    if is_struct_field_array {
                        is_assignable = true;
                    } else if let Some(arr_id) = array.as_any().downcast_ref::<Identifier>() {
                        if let Some(sym) = self.env.lookup_symbol(arr_id.get_name()) {
                            if !sym.is_mut {
                                self.error(&format!("Cannot assign to constant array '{}'", arr_id.get_name()));
                            } else if !sym.is_array {
                                self.error(&format!("Cannot index non-array variable '{}'", arr_id.get_name()));
                            } else {
                                is_assignable = true;
                            }
                        }
                    }
                }
            }

            if !is_assignable {
                self.error("Left side of assignment must be a mutable variable or array element");
                self.type_stack.push(DataType::Unknown);
                return;
            }

            // `__new_array` is a compiler intrinsic produced by the parser for
            // `new T[size]?` / `new T[size]` array allocations. The element
            // type is determined at codegen from the allocation site, so the
            // intrinsic's static type is `Unknown`. Accept the assignment
            // against any array / nullable-array LHS without flagging a type
            // mismatch (the actual element type is enforced by the surrounding
            // `var x: T[] = new T[n]` annotation, which the ArrayType path
            // already type-checks).
            let rhs_is_new_array = node.get_right().map_or(false, |r| {
                r.as_any()
                    .downcast_ref::<Identifier>()
                    .map_or(false, |id| id.get_name() == "__new_array")
            });
            if rhs_is_new_array {
                self.type_stack.push(left_type);
                return;
            }

            // `null` assigned to a generic element slot (e.g. `_data[i] = null`
            // inside `Vec<T>::pop`). The slot type is the generic `T`, which
            // may or may not be nullable — at this point we cannot know. Treat
            // `null` as compatible with a generic slot to allow optional
            // post-pop cleanup without forcing every `T` to be nullable.
            let rhs_is_null = node.get_right().map_or(false, |r| {
                r.as_any().downcast_ref::<NullLiteral>().is_some()
            });
            if rhs_is_null && right_type == DataType::None_ {
                let slot_is_generic = match &left_type {
                    DataType::Struct(name) => self.current_generic_params.contains(name),
                    _ => false,
                };
                if slot_is_generic {
                    self.type_stack.push(left_type);
                    return;
                }
            }

            if !Environment::is_type_compatible(&left_type, &right_type) {
                self.error(&format!(
                    "Cannot assign {} to {}",
                    data_type_to_string(right_type),
                    data_type_to_string(left_type)
                ));
                self.type_stack.push(DataType::Unknown);
                return;
            }

            self.type_stack.push(left_type);
            return;
        }

        if op == "+" || op == "-" || op == "*" || op == "/" || op == "%" {
            if op == "+" && (left_type == DataType::Str || right_type == DataType::Str) {
                self.type_stack.push(DataType::Str);
                return;
            }

            // Numeric operands: hardcoded path
            if Environment::is_numeric_type(&left_type) && Environment::is_numeric_type(&right_type) {
                if left_type == DataType::Float || right_type == DataType::Float {
                    self.type_stack.push(DataType::Float);
                } else {
                    self.type_stack.push(DataType::Int);
                }
                return;
            }

            // User struct types: desugar operator to trait method call
            if let DataType::Struct(ref struct_name) = left_type {
                if let Some(method_name) = Self::operator_to_method(op) {
                    let full_method = format!("{}::{}", struct_name, method_name);
                    if let Some(sym) = self.env.lookup_symbol(&full_method) {
                        // Process arguments (right operand has already been type-checked)
                        self.type_stack.push(sym.data_type.clone());
                        return;
                    }
                    self.error(&format!(
                        "Type '{}' does not implement operator '{}' (method '{}::{}' not found)",
                        struct_name, op, struct_name, method_name
                    ));
                    self.type_stack.push(DataType::Unknown);
                    return;
                }
            }

            self.error(&format!("Operator '{}' requires numeric operands", op));
            self.type_stack.push(DataType::Unknown);
            return;
        }

        // 处理复合赋值运算符 +=, -=, *=, /=
        if op == "+=" || op == "-=" || op == "*=" || op == "/=" {
            let mut is_assignable = false;
            let mut var_name = String::new();

            if let Some(left) = node.get_left() {
                if let Some(id) = left.as_any().downcast_ref::<Identifier>() {
                    var_name = id.get_name().to_string();
                    // Check if it's a bare struct field first
                    let is_field = self.current_impl_struct.as_ref().map_or(false, |s| {
                        self.struct_fields.get(s).map_or(false, |f| f.contains_key(&var_name))
                    });
                    if is_field {
                        is_assignable = true;
                    } else if let Some(sym) = self.env.lookup_symbol(&var_name) {
                        if sym.is_mut {
                            is_assignable = true;
                        } else {
                            self.error(&format!("Cannot assign to constant variable '{}'", var_name));
                        }
                    } else {
                        self.error(&format!("Undeclared variable '{}'", var_name));
                    }
                } else {
                    self.error("Left side of compound assignment must be a variable");
                    self.type_stack.push(DataType::Unknown);
                    return;
                }
            }

            if !is_assignable {
                if var_name.is_empty() {
                    self.error("Left side of compound assignment must be a mutable variable");
                }
                self.type_stack.push(DataType::Unknown);
                return;
            }

            // 检查类型兼容性
            let base_op = &op[0..1];
            
            // 字符串拼接
            if base_op == "+" && (left_type == DataType::Str || right_type == DataType::Str) {
                self.type_stack.push(DataType::Str);
                return;
            }

            // 数值运算
            if !Environment::is_numeric_type(&left_type) || !Environment::is_numeric_type(&right_type) {
                self.error(&format!("Operator '{}' requires numeric operands", op));
                self.type_stack.push(DataType::Unknown);
                return;
            }

            self.type_stack.push(left_type);
            return;
        }

        if op == "==" || op == "!=" || op == "<" || op == ">" || op == "<=" || op == ">=" {
            // Nullable and None_ comparisons: e.g., opt == null, opt != null
            let is_nullable = matches!(left_type, DataType::Nullable(_)) || matches!(right_type, DataType::Nullable(_));
            let has_none = left_type == DataType::None_ || right_type == DataType::None_;
            if is_nullable || has_none {
                if op == "==" || op == "!=" {
                    self.type_stack.push(DataType::Bool);
                    return;
                }
                self.error(&format!(
                    "Cannot compare {} and {} with '{}'",
                    data_type_to_string(left_type),
                    data_type_to_string(right_type),
                    op
                ));
                self.type_stack.push(DataType::Unknown);
                return;
            }

            // Numeric/bool comparison: hardcoded path
            if Environment::is_numeric_type(&left_type) || left_type == DataType::Bool {
                if !Environment::is_type_compatible(&left_type, &right_type)
                    && !Environment::is_type_compatible(&right_type, &left_type)
                {
                    self.error(&format!(
                        "Cannot compare {} and {}",
                        data_type_to_string(left_type),
                        data_type_to_string(right_type)
                    ));
                }
                self.type_stack.push(DataType::Bool);
                return;
            }

            // User struct types: desugar comparison to trait method call
            if let DataType::Struct(ref struct_name) = left_type {
                if let Some(method_name) = Self::operator_to_method(op) {
                    let full_method = format!("{}::{}", struct_name, method_name);
                    if let Some(sym) = self.env.lookup_symbol(&full_method) {
                        self.type_stack.push(sym.data_type.clone());
                        return;
                    }
                    self.error(&format!(
                        "Type '{}' does not implement operator '{}' (method '{}::{}' not found)",
                        struct_name, op, struct_name, method_name
                    ));
                    self.type_stack.push(DataType::Unknown);
                    return;
                }
            }

            self.error(&format!(
                "Cannot compare {} and {}",
                data_type_to_string(left_type),
                data_type_to_string(right_type)
            ));
            self.type_stack.push(DataType::Bool);
            return;
        }

        if op == "&&" || op == "||" {
            if left_type != DataType::Bool || right_type != DataType::Bool {
                self.error("Logical operators require boolean operands");
            }
            self.type_stack.push(DataType::Bool);
            return;
        }

        if op == ".." {
            // Range literal: a..b → Range struct. Ensure Range is declared.
            if !self.struct_fields.contains_key("Range") {
                self.struct_fields.insert("Range".to_string(), HashMap::new());
                self.env.declare_module("Range");
            }
            self.type_stack.push(DataType::Struct("Range".to_string()));
            return;
        }

        self.error(&format!("Unknown operator: {}", op));
        self.type_stack.push(DataType::Unknown);
    }

    fn visit_cast_expression(&mut self, node: &CastExpression) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
        let target_type = self.get_data_type_from_ast(Some(node.get_target_type()));
        self.type_stack.push(target_type);
    }

    fn visit_unary_expression(&mut self, node: &UnaryExpression) {
        let op = node.get_operator();

        // `&name` — address-of operator for function references.
        // The operand is a function name; yield a function-pointer type.
        if op == "&" {
            if let Some(operand) = node.get_operand() {
                operand.accept(self);
            }
            // Consume the operand type and push Unknown (function pointer).
            let _ = self.get_current_type();
            self.type_stack.push(DataType::Unknown);
            return;
        }

        if let Some(operand) = node.get_operand() {
            operand.accept(self);
        }
        let operand_type = self.get_current_type();

        if op == "-" || op == "+" {
            if !Environment::is_numeric_type(&operand_type) {
                self.error(&format!("Unary operator '{}' requires numeric operand", op));
            }
            self.type_stack.push(operand_type);
        } else if op == "!" {
            if operand_type != DataType::Bool {
                self.error("Logical not '!' requires boolean operand");
            }
            self.type_stack.push(DataType::Bool);
        } else {
            self.error(&format!("Unknown unary operator: {}", op));
            self.type_stack.push(DataType::Unknown);
        }
    }

    fn visit_function_call(&mut self, node: &FunctionCall) {
        // Lambda immediate invocation: lambda(params): ret { body }(args)
        if let Some(callee) = node.get_callee() {
            if let Some(lambda) = callee.as_any().downcast_ref::<Lambda>() {
                // Analyze the lambda (pushes its return type onto type_stack)
                lambda.accept(self);
                let return_type = self.type_stack.pop().unwrap_or(DataType::Unknown);

                // Analyze and consume argument types
                if let Some(args) = node.get_arguments() {
                    for arg in args {
                        arg.accept(self);
                        self.type_stack.pop();
                    }
                }

                self.type_stack.push(return_type);
                return;
            }
        }

        let mut func_name = String::new();
        let mut module_name = self.current_module.clone();

        if let Some(callee) = node.get_callee() {
            if let Some(id) = callee.as_any().downcast_ref::<Identifier>() {
                func_name = id.get_name().to_string();
            } else if let Some(path_access) = callee.as_any().downcast_ref::<PathAccess>() {
                // :: namespace path: std::io::println
                // path = ["std", "io"], member = "println" → module = "std::io", func = "println"
                module_name = path_access.get_path().join("::");
                func_name = path_access.get_member().to_string();
            } else if let Some(member) = callee.as_any().downcast_ref::<MemberAccess>() {
                if let Some(obj) = member.get_object() {
                    if let Some(obj_id) = obj.as_any().downcast_ref::<Identifier>() {
                        let obj_name = obj_id.get_name().to_string();
                        let member_name = member.get_member().to_string();

                        // Reject dot notation for module access (e.g., io.println → io::println)
                        let is_module = self.env.lookup_symbol(&obj_name)
                            .map_or(false, |s| s.symbol_type == SymbolType::Module);
                        if is_module && obj_name != "self" {
                            self.error(&format!(
                                "Use '::' for module access: '{0}::{1}' instead of '{0}.{1}'",
                                obj_name, member_name
                            ));
                            func_name = member_name;
                        } else {
                            // If obj is "self", use current_impl_struct as module for method lookup
                            if obj_name == "self" {
                                if let Some(ref struct_name) = self.current_impl_struct {
                                    module_name = struct_name.clone();
                                } else {
                                    module_name = obj_name;
                                }
                            } else {
                                module_name = obj_name;
                            }
                            func_name = member_name;
                        }
                    } else {
                        // Chained method call: obj is not a simple Identifier
                        // (e.g. `arr.index_mut(i).write(v)` — the outer call's
                        // obj is a FunctionCall). Visit the obj expression to
                        // discover its DataType, then dispatch via its struct
                        // name so that trait/struct methods like `Ref::write`
                        // are resolved correctly.
                        obj.accept(self);
                        let obj_dt = self.get_current_type();
                        self.type_stack.pop();
                        let member_name = member.get_member().to_string();
                        if let DataType::Struct(struct_name) = obj_dt {
                            module_name = struct_name;
                        }
                        func_name = member_name;
                    }
                }
            }
        }

        // Check if it's a struct constructor call (e.g. Point(1, 2) or vec::VecIterator(...))
        // Also check bare identifier that matches a struct name (e.g., VecIterator(...))
        // Strip generic parameters from func_name (e.g., "VecIterator<T>" → "VecIterator")
        let func_name_bare = func_name.split('<').next().unwrap_or(&func_name).to_string();
        let constructor_name = func_name_bare.split("::").last().unwrap_or(&func_name_bare).to_string();
        let is_struct_ctor = self.struct_fields.contains_key(&func_name_bare)
            || self.struct_fields.contains_key(&constructor_name);
        if is_struct_ctor {
            let resolved_ctor = if self.struct_fields.contains_key(&func_name_bare) {
                func_name_bare.clone()
            } else {
                constructor_name
            };
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            self.type_stack.push(DataType::Struct(resolved_ctor));
            return;
        }

        // Static-method constructor form: `TypeName::new(args...)` — returns
        // `TypeName` as the expression type. This enables automatic type
        // inference for declarations such as `var x = Range::new(0, 10);`
        // without requiring a type annotation.
        // Also handle `new T[n]` where T is a generic type parameter.
        let module_name_bare = module_name.split('<').next().unwrap_or(&module_name).to_string();
        if func_name == "new" && (self.struct_fields.contains_key(&module_name_bare)
            || self.current_generic_params.contains(&module_name_bare))
        {
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            self.type_stack.push(DataType::Struct(module_name_bare.clone()));
            return;
        }

        // Range constructor: `range::new(a, b)` / `Range::new(a, b)` produces a Range struct.
        // The `..` operator is desugared to `range::new(start, end)` by the parser, and
        // `new Range(a, b)` desugars to `Range::new(a, b)`. Both must be recognised even
        // when std/range.gbl is not loaded.
        if func_name == "new" && (module_name == "range" || module_name == "Range") {
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            if !self.struct_fields.contains_key("Range") {
                self.struct_fields.insert("Range".to_string(), HashMap::new());
                self.env.declare_module("range");
            }
            self.type_stack.push(DataType::Struct("Range".to_string()));
            return;
        }

        // Compile-time macros `file()` and `line()` are valid inside #[expand]
        // functions. They are folded to literals by the IR builder; here we
        // only need to give them a type so semantic analysis passes.
        if (func_name == "file" || func_name == "line")
            && module_name == self.current_module
        {
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            self.type_stack.push(if func_name == "file" {
                DataType::Str
            } else {
                DataType::Int
            });
            return;
        }

        // Resolve module aliases (e.g., "import lib.math as m" → "m" maps to "lib.math")
        let resolved_module = self.module_aliases.get(&module_name)
            .cloned()
            .unwrap_or_else(|| module_name.clone());

        // Strip generic parameter suffixes from the module half of the lookup
        // name so that `Result<U, E>::Ok` resolves the same way `Result::Ok`
        // does. Variant constructors and methods are registered under the
        // bare type name, never the monomorphised form.
        let resolved_module_bare = resolved_module.split('<').next().unwrap_or(&resolved_module).to_string();

        // Build lookup name. For method calls (obj.method), resolve via struct type
        let full_name = if resolved_module_bare != self.current_module {
            // Check if resolved_module is a variable (not a module) → method dispatch
            let is_var = self.env.lookup_symbol(&resolved_module_bare)
                .map_or(false, |s| s.symbol_type != SymbolType::Module);
            if is_var {
                // For struct types, look up method in current module
                // For arrays, the method is handled by the executor
                format!("{}::{}", self.current_module, func_name)
            } else {
                format!("{}::{}", resolved_module_bare, func_name)
            }
        } else {
            format!("{}::{}", resolved_module_bare, func_name)
        };

        // Try qualified lookup, then short-name lookup
        let sym_data_type = self.env.lookup_symbol(&full_name)
            .or_else(|| self.env.lookup_symbol(&func_name))
            .map(|s| s.data_type.clone());

        // If not found but it's a method call on a variable (e.g. arr.len, arr.add),
        // allow it with sensible return types
        if sym_data_type.is_none() && module_name != self.current_module {
            if let Some(var_sym) = self.env.lookup_symbol(&module_name) {
                if var_sym.is_array {
                    // Array methods: len() -> Int, add() -> None_
                    if func_name == "len" {
                        // Process explicit arguments (none expected)
                        if let Some(args) = node.get_arguments() {
                            for arg in args {
                                arg.accept(self);
                                self.type_stack.pop();
                            }
                        }
                        self.type_stack.push(DataType::Int);
                        return;
                    }
                    if func_name == "add" {
                        if let Some(args) = node.get_arguments() {
                            for arg in args {
                                arg.accept(self);
                                self.type_stack.pop();
                            }
                        }
                        self.type_stack.push(DataType::None_);
                        return;
                    }
                }
                // String methods: s.len() -> Int, s.contains(sub) -> Bool,
                // s.trim() -> Str, s.replace(from, to) -> Str
                if matches!(var_sym.data_type, DataType::Str) {
                    let ret = match func_name.as_str() {
                        "len" => Some(DataType::Int),
                        "contains" => Some(DataType::Bool),
                        "trim" => Some(DataType::Str),
                        "replace" => Some(DataType::Str),
                        _ => None,
                    };
                    if let Some(rt) = ret {
                        if let Some(args) = node.get_arguments() {
                            for arg in args {
                                arg.accept(self);
                                self.type_stack.pop();
                            }
                        }
                        self.type_stack.push(rt);
                        return;
                    }
                }
            }
        }

        // Struct method dispatch: if the callee is obj.method() and obj is
        // a variable of type DataType::Struct(S), look up S::method in the
        // symbol table. This enables method calls on imported struct types
        // (e.g. Result::take, Channel::send) without requiring the method
        // to be in the current module namespace.
        if sym_data_type.is_none() {
            // Special case: if module_name is "self", use current_impl_struct directly
            let dispatch_struct = if module_name == "self" {
                self.current_impl_struct.clone()
            } else {
                self.env.lookup_symbol(&module_name)
                    .and_then(|s| if let DataType::Struct(ref sn) = s.data_type {
                        Some(sn.clone())
                    } else {
                        None
                    })
            };
            if let Some(struct_name) = dispatch_struct {
                let struct_method = format!("{}::{}", struct_name, func_name);
                let method_type = self.env.lookup_symbol(&struct_method)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = method_type {
                    if let Some(args) = node.get_arguments() {
                        for arg in args {
                            arg.accept(self);
                            self.type_stack.pop();
                        }
                    }
                    self.type_stack.push(dt);
                    return;
                }
                // Also try without struct prefix (bare method name) for generic impls
                let bare_method_type = self.env.lookup_symbol(&func_name)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = bare_method_type {
                    if let Some(args) = node.get_arguments() {
                        for arg in args {
                            arg.accept(self);
                            self.type_stack.pop();
                        }
                    }
                    self.type_stack.push(dt);
                    return;
                }
                // Try looking up in current module with struct name prefix (e.g., vec::push)
                let current_module_method = format!("{}::{}", self.current_module, func_name);
                let current_method_type = self.env.lookup_symbol(&current_module_method)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = current_method_type {
                    if let Some(args) = node.get_arguments() {
                        for arg in args {
                            arg.accept(self);
                            self.type_stack.pop();
                        }
                    }
                    self.type_stack.push(dt);
                    return;
                }
                // Also try with bare struct name (e.g., "Vec" instead of "Vec<T>")
                let bare_struct_method = format!("{}::{}", struct_name.split('<').next().unwrap_or(&struct_name), func_name);
                let bare_struct_method_type = self.env.lookup_symbol(&bare_struct_method)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = bare_struct_method_type {
                    if let Some(args) = node.get_arguments() {
                        for arg in args {
                            arg.accept(self);
                            self.type_stack.pop();
                        }
                    }
                    self.type_stack.push(dt);
                    return;
                }
            }
        }

        match sym_data_type {
            Some(dt) => {
                // Process arguments
                if let Some(args) = node.get_arguments() {
                    for arg in args {
                        arg.accept(self);
                        self.type_stack.pop();
                    }
                }
                self.type_stack.push(dt);
            }
            None => {
                self.error(&format!("Undeclared function: '{}'", full_name));
                self.type_stack.push(DataType::Unknown);
            }
        }
    }

    fn visit_match_expression(&mut self, node: &MatchExpression) {
        // Type-check scrutinee
        if let Some(scrut) = node.get_scrutinee() {
            scrut.accept(self);
            let scrut_type = self.get_current_type();
            self.type_stack.pop();

            // For each arm, type-check the body and collect return types
            let mut result_type: Option<DataType> = None;
            for arm in node.get_arms() {
                // For variable patterns, declare the variable in a scope
                if let MatchPattern::Variable(ref name) = arm.pattern {
                    self.env.enter_scope();
                    self.env.declare_variable(name, &scrut_type, false);
                }

                if let Some(ref body) = arm.body {
                    body.accept(self);
                    let arm_type = self.get_current_type();
                    self.type_stack.pop();

                    match &result_type {
                        None => result_type = Some(arm_type.clone()),
                        Some(existing) => {
                            if !Environment::is_type_compatible(existing, &arm_type)
                                && !Environment::is_type_compatible(&arm_type, existing)
                                && arm_type != DataType::Unknown
                                && *existing != DataType::Unknown
                            {
                                self.error("Match arms have incompatible types");
                            }
                        }
                    }
                }

                if let MatchPattern::Variable(_) = arm.pattern {
                    self.env.exit_scope();
                }
            }

            if let Some(rt) = result_type {
                self.type_stack.push(rt);
            } else {
                self.type_stack.push(DataType::None_);
            }
        }
    }

    fn visit_try_operator(&mut self, node: &TryOperator) {
        // Visit the inner expression to get its type
        if let Some(inner) = node.get_inner() {
            inner.accept(self);
            let inner_type = self.get_current_type();
            self.type_stack.pop();

            // The ? operator works on Result<T, E>.
            // For now, since Result is a struct, we check if the type is
            // DataType::Struct("Result"). The result type of expr? is T
            // (the inner value type). Since all values are i64 at the
            // Cranelift level, we can safely return Int as the type.
            if let DataType::Struct(ref sname) = inner_type {
                if sname == "Result" {
                    // Result<T, E> ? → T (represented as Int at runtime)
                    self.type_stack.push(DataType::Int);
                    return;
                }
            }

            // For non-Result types, just pass through the type.
            // This allows the ? operator to be used loosely in tests.
            self.type_stack.push(inner_type);
        } else {
            self.type_stack.push(DataType::Unknown);
        }
    }

    fn visit_struct_literal(&mut self, node: &StructLiteral) {
        let type_name = node.get_type_name().to_string();

        // Look up struct definition
        let struct_fields = match self.struct_fields.get(&type_name) {
            Some(fields) => fields.clone(),
            None => {
                self.error(&format!("Unknown struct type: '{}'", type_name));
                self.type_stack.push(DataType::Unknown);
                return;
            }
        };

        let field_names: Vec<String> = struct_fields.keys().cloned().collect();
        let mut named_assigned: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut covered: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut positional_index = 0;

        for field_init in node.get_fields() {
            match field_init {
                StructFieldInit::Named { name, value } => {
                    // Verify field exists
                    if !struct_fields.contains_key(name) {
                        self.error(&format!(
                            "Struct '{}' has no field '{}'",
                            type_name, name
                        ));
                        self.type_stack.push(DataType::Unknown);
                        return;
                    }
                    // Only error on duplicate NAMED assignments (overriding spread/positional is OK)
                    if named_assigned.contains(name) {
                        self.error(&format!(
                            "Field '{}' assigned multiple times in struct literal",
                            name
                        ));
                        self.type_stack.push(DataType::Unknown);
                        return;
                    }
                    named_assigned.insert(name.clone());
                    covered.insert(name.clone());

                    // Type-check the value
                    value.accept(self);
                    let value_type = self.get_current_type();
                    self.type_stack.pop();

                    let expected_type = struct_fields.get(name).cloned().unwrap_or(DataType::Unknown);
                    if expected_type != DataType::Unknown && value_type != DataType::Unknown {
                        self.check_type_compatibility(
                            expected_type,
                            value_type,
                            &format!("struct literal field '{}'", name),
                        );
                    }
                }
                StructFieldInit::Positional(value) => {
                    // Check if it's a spread (identifier of same struct type)
                    let mut is_spread = false;
                    if let Some(id) = value.as_any().downcast_ref::<Identifier>() {
                        let id_name = id.get_name();
                        let full_name = format!("{}::{}", self.current_module, id_name);
                        if let Some(sym) = self.env.lookup_symbol(&full_name)
                            .or_else(|| self.env.lookup_symbol(id_name))
                        {
                            if let DataType::Struct(ref s) = sym.data_type {
                                if s == &type_name {
                                    is_spread = true;
                                }
                            }
                        }
                    }

                    if is_spread {
                        // Spread: all unassigned fields are filled from this struct
                        value.accept(self);
                        let _spread_type = self.get_current_type();
                        self.type_stack.pop();
                        // Mark all fields as covered (but NOT named_assigned, so named inits can override)
                        for fname in &field_names {
                            covered.insert(fname.clone());
                        }
                    } else {
                        // Positional: match to next uncovered field
                        value.accept(self);
                        let value_type = self.get_current_type();
                        self.type_stack.pop();

                        // Skip fields already covered by spread or previous positional
                        while positional_index < field_names.len()
                            && covered.contains(&field_names[positional_index])
                        {
                            positional_index += 1;
                        }

                        if positional_index >= field_names.len() {
                            self.error(&format!(
                                "Too many positional fields in struct literal for '{}'",
                                type_name
                            ));
                            self.type_stack.push(DataType::Unknown);
                            return;
                        }

                        let field_name = &field_names[positional_index];
                        covered.insert(field_name.clone());
                        positional_index += 1;

                        let expected_type = struct_fields.get(field_name).cloned().unwrap_or(DataType::Unknown);
                        if expected_type != DataType::Unknown && value_type != DataType::Unknown {
                            self.check_type_compatibility(
                                expected_type,
                                value_type,
                                &format!("struct literal field '{}'", field_name),
                            );
                        }
                    }
                }
            }
        }

        self.type_stack.push(DataType::Struct(type_name));
    }

    fn visit_member_access(&mut self, node: &MemberAccess) {
        if let Some(obj) = node.get_object() {
            obj.accept(self);
        }
        let obj_type = self.get_current_type();
        self.type_stack.pop();

        if let Some(obj) = node.get_object() {
            if let Some(id) = obj.as_any().downcast_ref::<Identifier>() {
                let obj_name = id.get_name();
                let member = node.get_member();

                // Handle 'self.field' inside an impl block
                if obj_name == "self" {
                    if let Some(ref struct_name) = self.current_impl_struct {
                        if let Some(fields) = self.struct_fields.get(struct_name) {
                            if let Some(field_type) = fields.get(member) {
                                self.type_stack.push(field_type.clone());
                                return;
                            }
                            self.error(&format!("Struct '{}' has no field '{}'", struct_name, member));
                            self.type_stack.push(DataType::Unknown);
                            return;
                        }
                    }
                    self.error("'self' used outside of impl block");
                    self.type_stack.push(DataType::Unknown);
                    return;
                }

                // Check module-level lookup via dot (deprecated — use ::)
                let full_name = format!("{}::{}", obj_name, member);
                let module_sym_type = self.env.lookup_symbol(&full_name)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = module_sym_type {
                    self.error(&format!(
                        "Use '::' for module access: '{0}::{1}' instead of '{0}.{1}'",
                        obj_name, member
                    ));
                    self.type_stack.push(dt);
                    return;
                }

                // Check struct field access on a variable
                if let DataType::Struct(ref struct_name) = obj_type {
                    if let Some(fields) = self.struct_fields.get(struct_name) {
                        if let Some(field_type) = fields.get(member) {
                            // _-prefixed fields are private to the struct's impl blocks
                            if member.starts_with('_') {
                                let in_own_impl = self.current_impl_struct.as_deref() == Some(struct_name.as_str());
                                if !in_own_impl {
                                    self.error(&format!(
                                        "Private field '{}' of struct '{}' is not accessible here",
                                        member, struct_name
                                    ));
                                    self.type_stack.push(DataType::Unknown);
                                    return;
                                }
                            }
                            self.type_stack.push(field_type.clone());
                            return;
                        }
                        self.error(&format!("Struct '{}' has no field '{}'", struct_name, member));
                        self.type_stack.push(DataType::Unknown);
                        return;
                    }
                }

                self.error(&format!("Module '{}' has no member '{}'", obj_name, member));
                self.type_stack.push(DataType::Unknown);
                return;
            }
        }

        self.error("Member access left side must be an identifier");
        self.type_stack.push(DataType::Unknown);
    }

    fn visit_range_expression(&mut self, node: &RangeExpression) {
        for arg in node.get_arguments() {
            arg.accept(self);
            let arg_type = self.get_current_type();
            self.type_stack.pop();

            if !Environment::is_numeric_type(&arg_type) {
                self.error("Range arguments must be numeric");
            }
        }
        // Range literal a..b produces a `Range` struct value, not an Int.
        if !self.struct_fields.contains_key("Range") {
            self.struct_fields.insert("Range".to_string(), HashMap::new());
            self.env.declare_module("Range");
        }
        self.type_stack.push(DataType::Struct("Range".to_string()));
    }

    fn visit_grouped_expression(&mut self, node: &GroupedExpression) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
    }

    fn visit_basic_type(&mut self, _node: &BasicType) {}
    fn visit_type(&mut self, _node: &dyn Type) {}

    fn visit_array_type(&mut self, node: &ArrayType) {
        if let Some(size) = node.get_size() {
            size.accept(self);
            let size_type = self.get_current_type();
            self.type_stack.pop();

            if size_type != DataType::Int {
                self.error("Array size must be integer");
            }
        }
    }

    fn visit_array_index(&mut self, node: &ArrayIndex) {
        if let Some(arr) = node.get_array() {
            arr.accept(self);
        }
        let array_type = self.get_current_type();
        self.type_stack.pop();

        if let Some(idx) = node.get_index() {
            idx.accept(self);
        }
        let index_type = self.get_current_type();
        self.type_stack.pop();

        if index_type != DataType::Int {
            self.error("Array index must be integer");
            self.type_stack.push(DataType::Unknown);
            return;
        }

        if let Some(arr) = node.get_array() {
            if let Some(id) = arr.as_any().downcast_ref::<Identifier>() {
                let id_name = id.get_name();
                // Check if it's a struct field (e.g., _data in impl block) — type already on stack
                let is_struct_field = self.current_impl_struct.as_ref().map_or(false, |s| {
                    self.struct_fields.get(s).map_or(false, |f| f.contains_key(id_name))
                });
                if is_struct_field {
                    // Unwrap Nullable if the struct field is a nullable array (T[]? → T)
                    let elem_type = match &array_type {
                        DataType::Nullable(inner) => *inner.clone(),
                        _ => array_type.clone(),
                    };
                    self.type_stack.push(elem_type);
                    return;
                }
                // Otherwise look up in environment (local variables)
                if let Some(sym) = self.env.lookup_symbol(id_name) {
                    if !sym.is_array {
                        self.error(&format!("Variable '{}' is not an array", id_name));
                        self.type_stack.push(DataType::Unknown);
                        return;
                    }
                    self.type_stack.push(sym.data_type.clone());
                    return;
                }
                self.error(&format!("Array variable '{}' not declared", id_name));
                self.type_stack.push(DataType::Unknown);
                return;
            }
        }

        self.type_stack.push(array_type);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regression test for the macOS CI failure on `test_ffi_variadic`:
    // declarations split across lines (`int\nprintf(...)`) must be detected
    // even though `\n` sits between the name and the `(`.
    #[test]
    fn header_declares_function_accepts_newline_between_name_and_paren() {
        let hdr = "int\nprintf(const char * restrict, ...);\n";
        assert!(
            SemanticAnalyzer::header_declares_function(hdr, "printf"),
            "expected `printf` to be found across a line break"
        );
    }

    // `fprintf` must NOT match when searching for `printf` — word boundary.
    #[test]
    fn header_declares_function_respects_word_boundary() {
        let hdr = "int fprintf(FILE *, const char *, ...);";
        assert!(!SemanticAnalyzer::header_declares_function(hdr, "printf"));
        assert!(SemanticAnalyzer::header_declares_function(hdr, "fprintf"));
    }

    // macOS SDK `stdio.h` wraps declarations in angle-bracket sub-includes,
    // e.g. `#include <_stdio.h>`. `read_header_recursive` must follow those
    // when the resolved file lives under `.../usr/include/` so we don't
    // miss `printf` just because its declaration is in `<_stdio.h>`.
    #[test]
    fn header_declares_function_finds_printf_in_angle_bracket_child() {
        // A synthetic header that mirrors Apple's structure: the top-level
        // file `.../usr/include/stdio.h` contains only `#include <_stdio.h>`,
        // with the actual declaration in the angle-bracket child.
        let tmp = std::env::temp_dir();
        let usr = tmp.join("ci_test_sa").join("usr").join("include");
        std::fs::create_dir_all(&usr).expect("mkdir");
        let stdio = usr.join("stdio.h");
        let stdio_inner = usr.join("_stdio.h");
        std::fs::write(&stdio, "#include <_stdio.h>\n").unwrap();
        std::fs::write(
            &stdio_inner,
            "int printf(const char * restrict, ...) __printflike(1,2);\n",
        )
        .unwrap();
        let path = stdio.to_string_lossy().to_string();
        let sa = SemanticAnalyzer::new();
        let content = sa.read_header_file(&path).expect("read_header_file");
        std::fs::remove_dir_all(tmp.join("ci_test_sa")).ok();

        assert!(
            SemanticAnalyzer::header_declares_function(&content, "printf"),
            "printf should be found after inlining angle-bracket child _stdio.h\ncontent:\n{}",
            &content
        );
    }
}

