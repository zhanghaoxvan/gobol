// cranelift.rs — AOT backend. Lowers GobolIR to native machine code via Cranelift ObjectModule.
//
// Supports the Gobol grammar: variables (var/val) with type inference, all
// primitive types (int/float/bool/str), arithmetic & comparison operators,
// control flow (if/else, while, for over range/array/string, break/continue),
// functions & method calls, structs (heap-allocated, field access), arrays
// (via a small runtime), string concatenation, and casts.
use crate::environment::DataType;
use crate::ir::*;
use cranelift_codegen::ir::{self, types, AbiParam, Inst, InstBuilder};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::{HashMap, HashSet};

// ==================== TypeResolver ====================

/// Centralised type inference and layout calculation.
///
/// Replaces the ad-hoc `infer_type` / `struct_field_offset` / `func_id_to_type`
/// methods that were previously hard-coded on `CraneliftBackend`.  All type
/// queries — expression types, struct field types/offsets, function return
/// types, and value sizes — flow through this struct so there is a single
/// source of truth.
pub struct TypeResolver {
    /// struct name -> definition
    structs: HashMap<String, IRStruct>,
    /// Struct names that opt out of GC via #[no_gc].
    no_gc_structs: HashSet<String>,
    /// IR function name -> return type (user functions + runtime imports)
    func_return_types: HashMap<String, DataType>,
    /// variable name -> type (per-function, reset between functions)
    var_types: HashMap<String, DataType>,
}

impl TypeResolver {
    pub fn new() -> Self {
        let mut r = TypeResolver {
            structs: HashMap::new(),
            no_gc_structs: HashSet::new(),
            func_return_types: HashMap::new(),
            var_types: HashMap::new(),
        };
        r.register_runtime_types();
        r
    }

    // ---- registration ----

    pub fn register_structs(&mut self, structs: &[IRStruct]) {
        for s in structs {
            if s.attributes.contains(&"no_gc".to_string()) {
                self.no_gc_structs.insert(s.name.clone());
            }
            self.structs.insert(s.name.clone(), s.clone());
        }
    }

    pub fn register_function(&mut self, name: &str, return_type: DataType) {
        self.func_return_types.insert(name.to_string(), return_type);
    }

    /// Populate return types for all C-runtime helper functions.
    fn register_runtime_types(&mut self) {
        let entries: &[(&str, DataType)] = &[
            ("gobol_print", DataType::None_),
            ("gobol_println", DataType::None_),
            ("gobol_eprint", DataType::None_),
            ("gobol_eprintln", DataType::None_),
            ("gobol_read", DataType::Str),
            ("gobol_str_int", DataType::Str),
            ("gobol_str_float", DataType::Str),
            ("gobol_str_bool", DataType::Str),
            ("gobol_str_cat", DataType::Str),
            ("gobol_str_eq", DataType::Bool),
            ("gobol_str_len", DataType::Int),
            ("gobol_str_get", DataType::Int),
            ("gobol_str_char", DataType::Str),
            ("gobol_str_contains", DataType::Bool),
            ("gobol_str_trim", DataType::Str),
            ("gobol_str_replace", DataType::Str),
            ("gobol_math_sin", DataType::Float),
            ("gobol_math_cos", DataType::Float),
            ("gobol_math_pow", DataType::Float),
            ("gobol_fs_open", DataType::Int),
            ("gobol_fs_read_all", DataType::Str),
            ("gobol_fs_write", DataType::Int),
            ("gobol_fs_close", DataType::None_),
            ("gobol_fs_exists", DataType::Bool),
            ("gobol_tcp_connect", DataType::Int),
            ("gobol_tcp_send", DataType::Int),
            ("gobol_tcp_recv", DataType::Str),
            ("gobol_tcp_close", DataType::None_),
            ("gobol_tcp_bind", DataType::Int),
            ("gobol_tcp_accept", DataType::Int),
            ("gobol_alloc", DataType::Int),
            ("gobol_array_new", DataType::Unknown),
            ("gobol_array_new_with_size", DataType::Unknown),
            ("gobol_array_new_2d", DataType::Unknown),
            ("gobol_array_add", DataType::None_),
            ("gobol_array_len", DataType::Int),
            ("gobol_array_get", DataType::Int),
            ("gobol_array_set", DataType::None_),
            ("gobol_mem_load", DataType::Int),
            ("gobol_mem_store", DataType::None_),
            ("gobol_array_elem_addr", DataType::Int),
            // GC — mark-sweep collector runtime
            ("gobol_gc_alloc", DataType::Int),
            ("gobol_gc_mark", DataType::None_),
            ("gobol_gc_sweep", DataType::None_),
            ("gobol_gc_collect", DataType::None_),
            ("gobol_gc_collect_now", DataType::None_),
            ("gobol_gc_alloc_count", DataType::Int),
            ("gobol_malloc", DataType::Int),
            ("gobol_free", DataType::None_),
        ];
        for (name, ty) in entries {
            self.func_return_types.insert(name.to_string(), ty.clone());
        }
    }

    pub fn declare_var(&mut self, name: &str, ty: DataType) {
        self.var_types.insert(name.to_string(), ty);
    }

    pub fn reset_vars(&mut self) {
        self.var_types.clear();
    }

    pub fn var_type(&self, name: &str) -> DataType {
        self.var_types.get(name).cloned().unwrap_or(DataType::Int)
    }

    pub fn has_struct(&self, name: &str) -> bool {
        self.structs.contains_key(name)
    }

    pub fn is_no_gc(&self, name: &str) -> bool {
        self.no_gc_structs.contains(name)
    }

    // ---- layout ----

    /// Byte size of a Gobol type in memory.
    /// All types are stored in 8-byte slots for simplicity (the struct is
    /// heap-allocated and fields are accessed via I64 loads/stores).
    pub fn type_size(&self, ty: &DataType) -> i64 {
        match ty {
            DataType::None_ => 0,
            DataType::Float => 8,
            DataType::Bool => 8,
            DataType::Int
            | DataType::Str
            | DataType::Unknown
            | DataType::Struct(_) => 8,
            DataType::Nullable(inner) => self.type_size(inner),
            DataType::Array(_) => 8, // array pointer
        }
    }

    /// Offset of `field` within `struct_name`, computed from actual field
    /// type sizes instead of a hard-coded stride.
    pub fn field_offset(&self, struct_name: &str, field: &str) -> Option<i64> {
        let s = self.structs.get(struct_name)?;
        let mut offset = 0i64;
        for f in &s.fields {
            if f.name == field {
                return Some(offset);
            }
            offset += self.type_size(&f.ty);
        }
        None
    }

    pub fn field_type(&self, struct_name: &str, field: &str) -> DataType {
        let s = self.structs.get(struct_name);
        s.and_then(|s| s.fields.iter().find(|f| f.name == field))
            .map(|f| f.ty.clone())
            .unwrap_or(DataType::Int)
    }

    /// Total byte size of a struct (sum of all field sizes).
    pub fn struct_size(&self, struct_name: &str) -> i64 {
        self.structs
            .get(struct_name)
            .map(|s| s.fields.iter().map(|f| self.type_size(&f.ty)).sum())
            .unwrap_or(0)
    }

    /// Return (name, type) pairs for all fields of a struct, if it exists.
    pub fn struct_fields(&self, struct_name: &str) -> Option<Vec<(String, DataType)>> {
        self.structs.get(struct_name).map(|s| {
            s.fields.iter().map(|f| (f.name.clone(), f.ty.clone())).collect()
        })
    }

    // ---- function return types ----

    /// Return type of a function by IR name.
    /// Falls back to `DataType::Int` for unknown functions (preserving
    /// previous behaviour without hard-coding specific names).
    pub fn func_return_type(&self, name: &str) -> DataType {
        if let Some(dt) = self.func_return_types.get(name) {
            return dt.clone();
        }
        // Try short name (strip :: module prefix) for cross-module calls
        let short = name.rsplit("::").next().unwrap_or(name);
        if let Some(dt) = self.func_return_types.get(short) {
            return dt.clone();
        }
        DataType::Int
    }

    /// Return type of an intrinsic arithmetic method on a primitive type.
    /// These methods (add, sub, mul, div, rem, eq, ne, lt, gt, le, ge) are the
    /// trait-based operator implementations for built-in numeric types.
    fn intrinsic_method_return_type(&self, method: &str, obj_ty: &DataType) -> Option<DataType> {
        let is_int = matches!(obj_ty, DataType::Int);
        let is_float = matches!(obj_ty, DataType::Float);
        if !is_int && !is_float {
            return None;
        }
        match method {
            "add" | "sub" | "mul" | "div" | "rem" => Some(obj_ty.clone()),
            "eq" | "ne" | "lt" | "gt" | "le" | "ge" => Some(DataType::Bool),
            _ => None,
        }
    }

    /// Return type of a builtin method on a primitive (arrays/strings).
    /// These are language-level builtins, not user-defined functions, so
    /// they are resolved here rather than via `func_return_types`.
    fn builtin_method_return_type(&self, method: &str, obj_ty: &DataType) -> Option<DataType> {
        match (obj_ty, method) {
            // Array methods
            (DataType::Unknown, "len") => Some(DataType::Int),
            (DataType::Unknown, "get") => Some(DataType::Int),
            (DataType::Unknown, "add") => Some(DataType::None_),
            // String methods
            (DataType::Str, "len") => Some(DataType::Int),
            (DataType::Str, "get") => Some(DataType::Int),
            (DataType::Str, "contains") => Some(DataType::Bool),
            (DataType::Str, "trim") => Some(DataType::Str),
            (DataType::Str, "replace") => Some(DataType::Str),
            _ => None,
        }
    }

    // ---- expression type inference ----

    pub fn infer_type(&self, e: &IRExpr) -> DataType {
        match e {
            IRExpr::Literal(LitValue::Int(_)) => DataType::Int,
            IRExpr::Literal(LitValue::Float(_)) => DataType::Float,
            IRExpr::Literal(LitValue::Bool(_)) => DataType::Bool,
            IRExpr::Literal(LitValue::Str(_)) => DataType::Str,
            IRExpr::Literal(LitValue::None) => DataType::None_,
            IRExpr::Variable(name) => self.var_type(name),
            IRExpr::StructLiteral { name, .. } => DataType::Struct(name.clone()),
            IRExpr::ArrayLiteral(elems) => {
                let _ = elems.first().map(|e| self.infer_type(e));
                DataType::Unknown
            }
            IRExpr::MethodCall { object, method, .. } => {
                let obj_ty = self.infer_type(object);
                // Struct constructor: Type::new(...)
                if method == "new" {
                    if let IRExpr::Variable(name) = object.as_ref() {
                        if self.has_struct(name) {
                            return DataType::Struct(name.clone());
                        }
                    }
                }
                // Intrinsic arithmetic methods on primitive types
                if let Some(rt) = self.intrinsic_method_return_type(method, &obj_ty) {
                    return rt;
                }
                // Builtin array / string methods
                if let Some(rt) = self.builtin_method_return_type(method, &obj_ty) {
                    return rt;
                }
                // User-defined method: StructName::method
                if let DataType::Struct(sname) = &obj_ty {
                    let full = format!("{}::{}", sname, method);
                    if self.func_return_types.contains_key(&full) {
                        return self.func_return_type(&full);
                    }
                }
                // Also try Type::method for static-like calls
                if let IRExpr::Variable(name) = object.as_ref() {
                    let full = format!("{}::{}", name, method);
                    if self.func_return_types.contains_key(&full) {
                        return self.func_return_type(&full);
                    }
                }
                DataType::Int
            }
            IRExpr::Call { func, .. } => self.func_return_type(func),
            IRExpr::Binary { op, left, right } => {
                if op == "+" && (self.contains_str(left) || self.contains_str(right)) {
                    return DataType::Str;
                }
                if matches!(op.as_str(), "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||") {
                    return DataType::Bool;
                }
                let lt = self.infer_type(left);
                let rt = self.infer_type(right);
                if matches!(lt, DataType::Float) || matches!(rt, DataType::Float) {
                    return DataType::Float;
                }
                lt
            }
            IRExpr::Unary { op, operand } => {
                if op == "!" {
                    return DataType::Bool;
                }
                self.infer_type(operand)
            }
            IRExpr::Cast { target, .. } => target.clone(),
            IRExpr::MemberAccess { object, member } => {
                if let DataType::Struct(sname) = self.infer_type(object) {
                    return self.field_type(&sname, member);
                }
                DataType::Int
            }
            IRExpr::ArrayIndex { .. } => DataType::Int,
            IRExpr::Assignment { target, .. } => self.infer_type(target),
            IRExpr::FuncRef(name) => self.func_return_type(name),
            IRExpr::IndirectCall { .. } => DataType::Int,
            IRExpr::None => DataType::None_,
        }
    }

    pub fn contains_str(&self, e: &IRExpr) -> bool {
        match e {
            IRExpr::Literal(LitValue::Str(_)) => true,
            IRExpr::Variable(name) => matches!(self.var_types.get(name), Some(DataType::Str)),
            IRExpr::Cast { target, .. } => matches!(target, DataType::Str),
            IRExpr::MethodCall { method, .. } => method.contains("str"),
            IRExpr::Call { func, .. } => {
                matches!(self.func_return_types.get(func), Some(DataType::Str))
                    || func.contains("str")
            }
            IRExpr::Binary { left, right, .. } => {
                self.contains_str(left) || self.contains_str(right)
            }
            _ => false,
        }
    }
}

// ==================== Variadic stubs ====================

/// A C wrapper stub needed for a variadic `extern "C"` call site.
///
/// C variadic functions (e.g. `printf`) cannot be called directly through a
/// single fixed-arity Cranelift signature.  For each distinct arity used at a
/// call site, the backend declares a non-variadic import symbol
/// `__gobol_va_<name>_<arity>` and generates a matching C wrapper that
/// forwards to the real variadic function.
#[derive(Debug, Clone)]
pub struct VariadicStub {
    /// The original extern "C" function name (e.g. `printf`).
    pub func_name: String,
    /// Total number of arguments at this call site (fixed + variadic).
    pub arity: usize,
    /// The Gobol type of each argument, used to pick the correct C type.
    pub param_types: Vec<DataType>,
    /// The function's return type.
    pub return_type: DataType,
}

impl VariadicStub {
    /// Symbol name used in the object file and the C stub file.
    pub fn symbol_name(&self) -> String {
        let clean = self.func_name.replace("::", "_").replace('.', "_");
        format!("__gobol_va_{}_{}", clean, self.arity)
    }

    /// Map a Gobol type to the C parameter type used in the stub.
    fn c_type(dt: &DataType) -> &'static str {
        match dt {
            DataType::Float => "double",
            DataType::Bool => "int",
            DataType::None_ => "void",
            DataType::Int
            | DataType::Str
            | DataType::Unknown
            | DataType::Struct(_)
            | DataType::Nullable(_)
            | DataType::Array(_) => "long",
        }
    }

    /// Generate the C source for this stub.
    pub fn c_source(&self) -> String {
        let sym = self.symbol_name();
        let ret_c = Self::c_type(&self.return_type);
        let params: Vec<String> = self
            .param_types
            .iter()
            .enumerate()
            .map(|(i, dt)| format!("{} a{}", Self::c_type(dt), i))
            .collect();
        let params_joined = if params.is_empty() {
            "void".to_string()
        } else {
            params.join(", ")
        };
        // Forward each argument, casting str args to const char* (Gobol str is
        // an opaque pointer; the real C function expects a string pointer).
        let forward_args: Vec<String> = (0..self.arity)
            .map(|i| {
                if matches!(self.param_types[i], DataType::Str) {
                    format!("(const char*)a{}", i)
                } else {
                    format!("a{}", i)
                }
            })
            .collect();
        let call_args = forward_args.join(", ");
        let ret_stmt = if matches!(self.return_type, DataType::None_) {
            format!("{}({});", self.func_name, call_args)
        } else {
            format!("return {}({});", self.func_name, call_args)
        };
        format!(
            "{ret_c} {sym}({params}) {{ {ret_stmt} }}\n",
            ret_c = ret_c,
            sym = sym,
            params = params_joined,
            ret_stmt = ret_stmt,
        )
    }
}

// ==================== Backend ====================

pub struct CraneliftBackend {
    module: ObjectModule,
    fn_ctx: FunctionBuilderContext,
    /// (IR function name, arity) -> symbol name.
    /// Arity is the number of IR-level parameters (including implicit `self`
    /// for methods). Including arity in the key allows overloaded methods
    /// like `Range::new(start, end)` and `Range::new(start, end, step)` to
    /// coexist with distinct symbol names.
    func_symbols: HashMap<(String, usize), String>,
    /// Per-overload linker symbol keyed by (name, arity, occurrence_index).
    /// Populated during the declare pass and consumed by the compile pass
    /// so each overload's body is compiled under the symbol that was
    /// actually declared (including disambiguator suffixes).
    func_overload_symbols: HashMap<(String, usize, usize), String>,
    /// symbol name -> FuncId (user-defined functions + runtime imports)
    func_ids: HashMap<String, cranelift_module::FuncId>,
    /// string literal text -> DataId
    string_data: HashMap<String, cranelift_module::DataId>,
    /// struct name -> set of constructor method names (e.g. "new")
    constructors: HashMap<String, bool>,
    /// centralised type inference / layout / return-type lookup
    type_resolver: TypeResolver,

    /// IR function names that are `extern "C"` variadic (declared with `...`).
    /// These are never declared directly; per-arity stubs are used instead.
    variadic_funcs: std::collections::HashSet<String>,
    /// Collected per-arity stubs, deduplicated by (name, arity).
    variadic_stubs: Vec<VariadicStub>,

    // --- per-function translation state (reset each function) ---
    variables: HashMap<String, Variable>,
    var_counter: u32,
    /// loop break/continue block targets (innermost last)
    loop_stack: Vec<(ir::Block, ir::Block)>,
    /// current function return type
    return_type: DataType,
    /// true once the current block has a terminator (return/jump/brif).
    /// Replaces the private `is_filled` API.
    diverged: bool,
    /// Attributes of the function currently being compiled. Drives codegen
    /// decisions such as `naked` (no implicit epilogue) and `no_gc`
    /// (use a non-GC allocator for heap allocations).
    current_func_attributes: Vec<String>,
}

impl CraneliftBackend {
    /// Compile the IR to JIT machine code. After this, `get_function_ptr` works.
    pub fn compile_ir(&mut self, ir: &GobolIR) -> Result<(), String> {
        // Collect struct definitions and constructor names.
        self.type_resolver.register_structs(&ir.structs);
        for imp in &ir.impls {
            for m in &imp.methods {
                if m.name == "new" || m.name.ends_with("::new") {
                    self.constructors.insert(imp.struct_name.clone(), true);
                }
                // Register method return types in the TypeResolver.
                self.type_resolver.register_function(&m.name, m.return_type.clone());
                // Also register with struct prefix for operator desugaring
                let prefixed = format!("{}::{}", imp.struct_name, m.name);
                self.type_resolver.register_function(&prefixed, m.return_type.clone());
            }
        }
        // Register user function return types.
        for f in &ir.functions {
            if !f.is_main {
                self.type_resolver.register_function(&f.name, f.return_type.clone());
            }
            // Collect variadic extern "C" functions — these use per-arity stubs
            // instead of a single fixed signature.
            if f.is_variadic {
                self.variadic_funcs.insert(f.name.clone());
            }
        }

        // Declare runtime functions (imports).
        self.declare_runtime_functions();

        // Per-(name, arity) occurrence counter, shared across declare and
        // compile passes so both iterate in identical order and assign the
        // same disambiguated symbol to the same overload.
        let mut occurrence_counter: HashMap<(String, usize), usize> = HashMap::new();
        let next_idx = |oc: &mut HashMap<_, _>, name: &str, arity: usize| -> usize {
            let key = (name.to_string(), arity);
            let v = oc.entry(key).or_insert(0);
            let idx = *v;
            *v += 1;
            idx
        };

        // First pass: declare all user functions so calls can resolve forward.
        for f in &ir.functions {
            if f.is_main {
                continue;
            }
            let idx = next_idx(&mut occurrence_counter, &f.name, f.params.len());
            self.declare_user_function(f, idx)?;
        }
        for imp in &ir.impls {
            for m in &imp.methods {
                // declare_user_function registers (m.name, arity) -> symbol.
                // m.name is already in "Struct::method" form (set by IRBuilder),
                // so no additional alias registration is needed.
                let idx = next_idx(&mut occurrence_counter, &m.name, m.params.len());
                self.declare_user_function(m, idx)?;
            }
        }

        // Reset the occurrence counter for the compile pass so it walks the
        // same sequence and resolves the same (name, arity, occurrence_idx)
        // triples used by the declare pass.
        occurrence_counter.clear();

        // Second pass: define function bodies.
        for f in &ir.functions {
            if f.is_main {
                continue;
            }
            let idx = next_idx(&mut occurrence_counter, &f.name, f.params.len());
            self.compile_function(f, idx)?;
        }
        for imp in &ir.impls {
            for m in &imp.methods {
                let idx = next_idx(&mut occurrence_counter, &m.name, m.params.len());
                self.compile_function(m, idx)?;
            }
        }

        // main function (entry point)
        for f in &ir.functions {
            if f.is_main {
                self.compile_main(f)?;
            }
        }

        Ok(())
    }

    // ==================== declaration helpers ====================

    fn declare_runtime_functions(&mut self) {
        // void print(const char*), void println(const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_print", sig);

        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_println", sig);

        // void gobol_eprint(const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_eprint", sig);

        // void gobol_eprintln(const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_eprintln", sig);

        // char* read()
        let sig = self.module.make_signature();
        self.declare_import("gobol_read", sig);

        // char* gobol_str_int(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_int", sig);

        // char* gobol_str_float(f64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_float", sig);

        // char* gobol_str_bool(i8)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I8));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_bool", sig);

        // char* gobol_str_cat(const char*, const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_cat", sig);

        // i8 gobol_str_eq(const char*, const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I8));
        self.declare_import("gobol_str_eq", sig);

        // i64 gobol_str_len(const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_len", sig);

        // i64 gobol_str_get(const char*, i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_get", sig);

        // char* gobol_str_char(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_char", sig);

        // ptr gobol_alloc(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_alloc", sig);

        // ptr gobol_gc_alloc(i64) — GC-tracked allocation (default)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_gc_alloc", sig);

        // void gobol_gc_mark(ptr)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_gc_mark", sig);

        // void gobol_gc_sweep()
        let sig = self.module.make_signature();
        self.declare_import("gobol_gc_sweep", sig);

        // void gobol_gc_collect()
        let sig = self.module.make_signature();
        self.declare_import("gobol_gc_collect", sig);

        // void gobol_gc_collect_now()
        let sig = self.module.make_signature();
        self.declare_import("gobol_gc_collect_now", sig);

        // i64 gobol_gc_alloc_count()
        let mut sig = self.module.make_signature();
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_gc_alloc_count", sig);

        // ptr gobol_array_new()
        let sig = self.module.make_signature();
        let mut sig = sig;
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_new", sig);

        // ptr gobol_array_new_with_size(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_new_with_size", sig);

        // ptr gobol_array_new_2d(i64, i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_new_2d", sig);

        // void gobol_array_add(ptr, i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_add", sig);

        // i64 gobol_array_len(ptr)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_len", sig);

        // i64 gobol_array_get(ptr, i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_get", sig);

        // void gobol_array_set(ptr, i64, i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_set", sig);

        // ---- Ref<T> runtime ----
        // i64 gobol_mem_load(i64 addr)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_mem_load", sig);

        // void gobol_mem_store(i64 addr, i64 val)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_mem_store", sig);

        // i64 gobol_array_elem_addr(ptr arr, i64 i)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_elem_addr", sig);

        // ---- string extension methods ----
        // i64 gobol_str_contains(const char*, const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_contains", sig);

        // char* gobol_str_trim(const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_trim", sig);

        // char* gobol_str_replace(const char*, const char*, const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_str_replace", sig);

        // ---- math intrinsics ----
        // f64 gobol_math_sin(f64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::F64));
        self.declare_import("gobol_math_sin", sig);

        // f64 gobol_math_cos(f64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::F64));
        self.declare_import("gobol_math_cos", sig);

        // f64 gobol_math_pow(f64, f64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::F64));
        sig.params.push(AbiParam::new(types::F64));
        sig.returns.push(AbiParam::new(types::F64));
        self.declare_import("gobol_math_pow", sig);

        // ---- fs intrinsics ----
        // i64 gobol_fs_open(const char*, const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_fs_open", sig);

        // char* gobol_fs_read_all(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_fs_read_all", sig);

        // i64 gobol_fs_write(i64, const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_fs_write", sig);

        // void gobol_fs_close(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_fs_close", sig);

        // i64 gobol_fs_exists(const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_fs_exists", sig);

        // ---- net intrinsics ----
        // i64 gobol_tcp_connect(const char*, i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_connect", sig);

        // i64 gobol_tcp_send(i64, const char*)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_send", sig);

        // char* gobol_tcp_recv(i64, i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_recv", sig);

        // void gobol_tcp_close(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_close", sig);

        // i64 gobol_tcp_bind(const char*, i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_bind", sig);

        // i64 gobol_tcp_accept(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_accept", sig);

        // ---- thread / channel concurrency runtime ----

        // i64 gobol_thread_spawn(i64 func_ptr, i64 arg)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_thread_spawn", sig);

        // i64 gobol_thread_join(i64 thread_id)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_thread_join", sig);

        // i64 gobol_chan_create()
        let mut sig = self.module.make_signature();
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_chan_create", sig);

        // i64 gobol_chan_send(i64 chan, i64 data)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_chan_send", sig);

        // i64 gobol_chan_recv(i64 chan)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_chan_recv", sig);

        // void gobol_chan_destroy(i64 chan)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_chan_destroy", sig);
    }

    fn declare_import(&mut self, name: &str, sig: ir::Signature) {
        if self.func_ids.contains_key(name) {
            return;
        }
        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .unwrap_or_else(|e| panic!("declare import {} failed: {}", name, e));
        self.func_ids.insert(name.to_string(), id);
    }

    fn declare_user_function(&mut self, f: &IRFunction, occurrence_idx: usize) -> Result<(), String> {
        let is_extern = f.attributes.contains(&"extern".to_string());
        let arity = f.params.len();
        // Variadic extern "C" functions are never declared with a fixed
        // signature — per-arity stubs are declared lazily at call sites.
        // We still register the name in `func_symbols` so call resolution
        // can find it, mapping it to the bare name as a sentinel.
        if is_extern && f.is_variadic {
            self.func_symbols.insert((f.name.clone(), arity), f.name.clone());
            return Ok(());
        }

        // Intrinsic functions (e.g., int::add, int::sub) have no body — they are
        // inlined at call sites via try_intrinsic_method. Do not declare them as
        // Export functions; the linker would complain about missing definitions.
        if f.attributes.iter().any(|a| a == "intrinsic") {
            return Ok(());
        }

        // extern "C" functions use the original C symbol name as the linker
        // symbol (no gbl_ prefix, no arity suffix, no module prefix).
        // After import processing, `f.name` may be `builtins::gobol_print`,
        // and the attribute list has either:
        //   • `extern:custom_name` → use `custom_name` verbatim, or
        //   • plain `extern` → strip the Gobol module prefix and keep the
        //     last `::`-separated segment (e.g. `builtins::gobol_print`
        //     becomes the bare C name `gobol_print`).
        //
        // BUG FIX (Windows LNK1120 / 15 unresolved externals): the prior
        // logic chained `.find(|a| *a == "extern").and_then(|a|
        // a.strip_prefix("extern:"))`, which always produced `None` because
        // a match of `"extern"` can never start with `"extern:"`. The
        // `unwrap_or(&f.name)` fallback therefore re-used the module-
        // qualified name, leading the linker to ask for `builtins::gobol_print`
        // while runtime.c only exports `gobol_print`.
        let sym = if is_extern {
            match f.attributes.iter().find_map(|a| a.strip_prefix("extern:")) {
                Some(explicit) => explicit.to_string(),
                None => f
                    .name
                    .rsplit("::")
                    .next()
                    .unwrap_or(&f.name)
                    .to_string(),
            }
        } else {
            Self::func_symbol(&f.name, arity)
        };
        self.func_symbols.insert((f.name.clone(), arity), sym.clone());

        // Extern functions may already be declared as runtime imports
        // (e.g., gobol_print is both in declare_runtime_functions and in
        // builtins.gbl's extern "C" block).  Skip duplicate declarations.
        if is_extern && self.func_ids.contains_key(&sym) {
            return Ok(());
        }

        // Overloaded methods may share the same (name, arity) — e.g.
        // `Vec::new(arr: T[])` and `Vec::new(capacity: int)` both have
        // arity 2 (self + 1 param) and thus the same base symbol
        // `gbl_Vec_new_2`.  When that happens, mint a unique symbol by
        // appending a numeric disambiguator so each overload gets its own
        // linker symbol and definition.
        let sym = if !is_extern && self.func_ids.contains_key(&sym) {
            let mut idx = 1;
            loop {
                let candidate = format!("{}_{}", sym, idx);
                if !self.func_ids.contains_key(&candidate) {
                    // Point (name, arity) at the most recently declared
                    // overload so call resolution still finds a symbol.
                    self.func_symbols.insert((f.name.clone(), arity), candidate.clone());
                    break candidate;
                }
                idx += 1;
            }
        } else {
            sym
        };

        // Remember which symbol was used for the (name, arity, occurrence_idx)
        // triple, so the compile pass can compile this function's body under
        // the exact same linker symbol.
        if !is_extern {
            self.func_overload_symbols.insert(
                (f.name.clone(), arity, occurrence_idx),
                sym.clone(),
            );
        }

        let mut sig = self.module.make_signature();
        for p in &f.params {
            sig.params.push(AbiParam::new(self.data_type_to_clif(&p.ty)?));
        }
        // void functions have no return slot
        if !matches!(f.return_type, DataType::None_) {
            sig.returns.push(AbiParam::new(self.data_type_to_clif(&f.return_type)?));
        }
        let linkage = if is_extern { Linkage::Import } else { Linkage::Export };
        let id = self
            .module
            .declare_function(&sym, linkage, &sig)
            .map_err(|e| format!("declare {} failed: {}", sym, e))?;
        self.func_ids.insert(sym, id);
        Ok(())
    }

    /// Map an IR function name + arity to a Gobol-internal symbol name.
    /// Arity is included in the symbol so overloaded methods (same name,
    /// different parameter count) get distinct linker symbols.  Also strip
    /// linker-invalid characters that generic parameters inject (`<`, `>`,
    /// `?`, `,`, whitespace) so names like `Vec<T>::iter` become valid
    /// assembler symbols.
    fn func_symbol(name: &str, arity: usize) -> String {
        let sanitized: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':'))
            .collect();
        format!(
            "gbl_{}_{}",
            sanitized.replace("::", "_").replace('.', "_"),
            arity
        )
    }

    // ==================== function compilation ====================

    fn compile_function(&mut self, ir_func: &IRFunction, occurrence_idx: usize) -> Result<(), String> {
        // Intrinsic functions (bodyless declarations backed by the C runtime)
        // are dispatched directly at call sites — never compile a body for them.
        if ir_func.attributes.iter().any(|a| a == "intrinsic") {
            return Ok(());
        }
        // Extern "C" functions are imports — they have no body to compile.
        // Their linker symbol is the bare C name (registered in
        // declare_user_function), so func_symbol() would compute a wrong key.
        if ir_func.attributes.contains(&"extern".to_string()) {
            return Ok(());
        }
        self.reset_function_state(ir_func.return_type.clone());
        self.set_current_func_attributes(&ir_func.attributes);
        // Resolve the exact symbol used during the declare pass. Overloaded
        // methods with the same (name, arity) were disambiguated with a
        // numeric suffix, so we must look up via the occurrence index.
        let arity = ir_func.params.len();
        let sym = self
            .func_overload_symbols
            .get(&(ir_func.name.clone(), arity, occurrence_idx))
            .cloned()
            .unwrap_or_else(|| Self::func_symbol(&ir_func.name, arity));
        let func_id = *self.func_ids.get(&sym).ok_or_else(|| format!("missing func {}", sym))?;

        let mut ctx = self.module.make_context();
        // rebuild signature (matches declare_user_function)
        for p in &ir_func.params {
            ctx.func.signature.params.push(AbiParam::new(self.data_type_to_clif(&p.ty)?));
        }
        if !matches!(ir_func.return_type, DataType::None_) {
            ctx.func.signature.returns.push(AbiParam::new(self.data_type_to_clif(&ir_func.return_type)?));
        }

        {
            // Move fn_ctx out so &mut self can be used alongside the builder.
            let mut fn_ctx = std::mem::take(&mut self.fn_ctx);
            {
                let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fn_ctx);
                let entry = bcx.create_block();
                bcx.switch_to_block(entry);
                self.diverged = false;
                bcx.append_block_params_for_function_params(entry);
                // Bind parameters to named variables.
                let params: Vec<ir::Value> = bcx.block_params(entry).to_vec();
                for (i, p) in ir_func.params.iter().enumerate() {
                    let ty = self.data_type_to_clif(&p.ty).unwrap_or(types::I64);
                    let var = self.declare_variable(&mut bcx, &p.name, ty, &p.ty);
                    bcx.def_var(var, params[i]);
                }

                if let Some(body) = &ir_func.body {
                    self.translate_block(&mut bcx, body)?;
                }

                // Ensure a return if the builder is still open. `#[naked]`
                // functions opt out of the implicit epilogue — the body is
                // responsible for all control flow (used for entry points
                // and interrupt handlers that must not get a synthesized
                // return).
                if !self.diverged && !self.current_func_has_attr("naked") {
                    self.emit_default_return(&mut bcx);
                }
                bcx.seal_all_blocks();
                bcx.finalize(self.module.target_config());
            }
            self.fn_ctx = fn_ctx;
        }

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| {
                eprintln!("=== Verifier error in {} ===\n{:?}", sym, ctx.func);
                format!("define {} failed: {}", sym, e)
            })?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    fn compile_main(&mut self, ir_func: &IRFunction) -> Result<(), String> {
        self.reset_function_state(DataType::Int);
        self.set_current_func_attributes(&ir_func.attributes);
        // main has no parameters in IR; give it a C-friendly i64 return.
        let sym = "gbl_main".to_string();
        self.func_symbols.insert(("main".to_string(), 0), sym.clone());
        let mut sig = self.module.make_signature();
        sig.returns.push(AbiParam::new(types::I64));
        let func_id = self
            .module
            .declare_function(&sym, Linkage::Export, &sig)
            .map_err(|e| format!("declare main failed: {}", e))?;
        self.func_ids.insert(sym, func_id);

        let mut ctx = self.module.make_context();
        ctx.func.signature.returns.push(AbiParam::new(types::I64));

        {
            // Move fn_ctx out so &mut self can be used alongside the builder.
            let mut fn_ctx = std::mem::take(&mut self.fn_ctx);
            {
                let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut fn_ctx);
                let entry = bcx.create_block();
                bcx.switch_to_block(entry);
                self.diverged = false;
                bcx.seal_block(entry);

                if let Some(body) = &ir_func.body {
                    self.translate_block(&mut bcx, body)?;
                }
                // main defaults to return 0 if no explicit return.
                if !self.diverged {
                    let zero = bcx.ins().iconst(types::I64, 0);
                    bcx.ins().return_(&[zero]);
                }
                bcx.seal_all_blocks();
                bcx.finalize(self.module.target_config());
            }
            self.fn_ctx = fn_ctx;
        }

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| {
                eprintln!("=== Verifier error in main ===\n{:?}", ctx.func);
                format!("define main failed: {}", e)
            })?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    fn reset_function_state(&mut self, return_type: DataType) {
        self.variables.clear();
        self.type_resolver.reset_vars();
        self.var_counter = 0;
        self.loop_stack.clear();
        self.return_type = return_type;
        self.diverged = false;
        self.current_func_attributes.clear();
    }

    /// Record the attributes of the function about to be compiled so codegen
    /// can consult them (e.g. `naked`, `no_gc`).
    fn set_current_func_attributes(&mut self, attrs: &[String]) {
        self.current_func_attributes = attrs.to_vec();
    }

    /// True when the function currently being compiled carries `attr`.
    fn current_func_has_attr(&self, attr: &str) -> bool {
        self.current_func_attributes.iter().any(|a| a == attr)
    }

    /// True when allocations in the current function should bypass the GC.
    /// This is the case when either the struct itself is `no_gc` or the
    /// enclosing function (possibly via a file-level `#![no_gc]`) is `no_gc`.
    fn alloc_uses_no_gc(&self, struct_name: &str) -> bool {
        self.type_resolver.is_no_gc(struct_name) || self.current_func_has_attr("no_gc")
    }

    fn emit_default_return(&mut self, bcx: &mut FunctionBuilder) {
        match &self.return_type {
            DataType::None_ => {
                bcx.ins().return_(&[]);
            }
            DataType::Float => {
                let z = bcx.ins().f64const(0.0);
                bcx.ins().return_(&[z]);
            }
            _ => {
                let z = bcx.ins().iconst(types::I64, 0);
                bcx.ins().return_(&[z]);
            }
        }
    }

    // ==================== statements ====================

    fn translate_block(
        &mut self,
        bcx: &mut FunctionBuilder,
        block: &IRBlock,
    ) -> Result<(), String> {
        for stmt in &block.statements {
            self.translate_stmt(bcx, stmt)?;
        }
        Ok(())
    }

    fn translate_stmt(&mut self, bcx: &mut FunctionBuilder, stmt: &IRStmt) -> Result<(), String> {
        match stmt {
            IRStmt::Declaration { name, ty, init } => {
                let resolved = if *ty == DataType::None_ || *ty == DataType::Unknown {
                    let inferred = init.as_ref()
                        .map(|e| self.type_resolver.infer_type(e))
                        .unwrap_or(DataType::Int);
                    // Avoid types::INVALID from DataType::None_
                    if matches!(inferred, DataType::None_) {
                        DataType::Int
                    } else {
                        inferred
                    }
                } else {
                    ty.clone()
                };
                let clif_ty = self.data_type_to_clif(&resolved).unwrap_or(types::I64);
                let var = self.declare_variable(bcx, name, clif_ty, &resolved);

                let val = if let Some(e) = init {
                    let v = self.translate_expr(bcx, e)?;
                    self.coerce(bcx, v, &self.type_resolver.infer_type(e), &resolved)?
                } else {
                    self.default_value(bcx, &resolved)
                };
                bcx.def_var(var, val);
            }
            IRStmt::Expression(e) => {
                self.translate_expr(bcx, e)?;
            }
            IRStmt::Return(None) => {
                bcx.ins().return_(&[]);
                self.diverged = true;
            }
            IRStmt::Return(Some(e)) => {
                let v = self.translate_expr(bcx, e)?;
                let v = self.coerce(bcx, v, &self.type_resolver.infer_type(e), &self.return_type)?;
                let ret_ty = self.data_type_to_clif(&self.return_type).unwrap_or(types::I64);
                let v = self.bitcast_to(bcx, v, ret_ty);
                bcx.ins().return_(&[v]);
                self.diverged = true;
            }
            IRStmt::If { cond, then_block, else_block } => {
                self.translate_if(bcx, cond, then_block, else_block.as_ref())?;
            }
            IRStmt::While { cond, body } => {
                self.translate_while(bcx, cond, body)?;
            }
            IRStmt::For { vars, iterable, body } => {
                self.translate_for(bcx, vars, iterable, body)?;
            }
            IRStmt::Break => {
                let (brk, _) = self
                    .loop_stack
                    .last()
                    .copied()
                    .ok_or_else(|| "break outside loop".to_string())?;
                bcx.ins().jump(brk, &[]);
                self.diverged = true;
            }
            IRStmt::Continue => {
                let (_, cont) = self
                    .loop_stack
                    .last()
                    .copied()
                    .ok_or_else(|| "continue outside loop".to_string())?;
                bcx.ins().jump(cont, &[]);
                self.diverged = true;
            }
            IRStmt::Assignment { target, value } => {
                self.translate_assignment(bcx, target, value)?;
            }
            IRStmt::Call { func, args, .. } => {
                self.translate_call(bcx, func, args)?;
            }
            IRStmt::MethodCall { object, method, args, .. } => {
                self.translate_method_call(bcx, object, method, args)?;
            }
        }
        Ok(())
    }

    fn translate_if(
        &mut self,
        bcx: &mut FunctionBuilder,
        cond: &IRExpr,
        then_block: &IRBlock,
        else_block: Option<&IRBlock>,
    ) -> Result<(), String> {
        let cond_val = self.translate_expr(bcx, cond)?;
        let cond_i8 = self.to_bool(bcx, cond_val, &self.type_resolver.infer_type(cond));

        let then_b = bcx.create_block();
        let else_b = bcx.create_block();
        let merge_b = bcx.create_block();

        bcx.ins().brif(cond_i8, then_b, &[], else_b, &[]);
        bcx.seal_block(then_b);
        bcx.seal_block(else_b);

        bcx.switch_to_block(then_b);
        self.diverged = false;
        self.translate_block(bcx, then_block)?;
        let then_diverged = self.diverged;
        if !then_diverged {
            bcx.ins().jump(merge_b, &[]);
        }
        bcx.switch_to_block(else_b);
        self.diverged = false;
        if let Some(eb) = else_block {
            self.translate_block(bcx, eb)?;
        }
        let else_diverged = self.diverged;
        if !else_diverged {
            bcx.ins().jump(merge_b, &[]);
        }
        bcx.seal_block(merge_b);
        // If both branches diverged (e.g. both return), the merge block is
        // unreachable — propagate divergence so the caller doesn't emit
        // dead code after the if. Otherwise switch to merge_b.
        if then_diverged && else_diverged {
            self.diverged = true;
        } else {
            bcx.switch_to_block(merge_b);
            self.diverged = false;
        }
        Ok(())
    }

    fn translate_while(
        &mut self,
        bcx: &mut FunctionBuilder,
        cond: &IRExpr,
        body: &IRBlock,
    ) -> Result<(), String> {
        let cond_b = bcx.create_block();
        let body_b = bcx.create_block();
        let end_b = bcx.create_block();

        bcx.ins().jump(cond_b, &[]);
        // NOTE: cond_b is sealed only after the body's back-edge is emitted,
        // so Cranelift knows both predecessors (entry + body) and can build
        // the correct block parameters for loop-modified variables.
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let c = self.translate_expr(bcx, cond)?;
        let c = self.to_bool(bcx, c, &self.type_resolver.infer_type(cond));
        bcx.ins().brif(c, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(body_b);
        self.loop_stack.push((end_b, cond_b));
        self.diverged = false;
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(cond_b, &[]);
        }
        bcx.seal_block(cond_b);

        bcx.switch_to_block(end_b);
        self.diverged = false;
        Ok(())
    }

    fn translate_for(
        &mut self,
        bcx: &mut FunctionBuilder,
        vars: &[String],
        iterable: &IRExpr,
        body: &IRBlock,
    ) -> Result<(), String> {
        // Range: range::new(start, end[, step])  or  Range::new(...)  or  start..end
        if let IRExpr::Call { func, args, .. } = iterable {
            if func == "range::new" || func == "Range::new" {
                return self.translate_for_range(bcx, vars, args, body);
            }
        }
        // String literal iteration: for ch in "abc"
        if let IRExpr::Literal(LitValue::Str(s)) = iterable {
            return self.translate_for_string(bcx, vars, s, body);
        }
        // Array iteration
        self.translate_for_array(bcx, vars, iterable, body)
    }

    fn translate_for_range(
        &mut self,
        bcx: &mut FunctionBuilder,
        vars: &[String],
        args: &[IRExpr],
        body: &IRBlock,
    ) -> Result<(), String> {
        let start = if let Some(a) = args.first() {
            self.translate_expr(bcx, a)?
        } else {
            bcx.ins().iconst(types::I64, 0)
        };
        let end = if args.len() >= 2 {
            self.translate_expr(bcx, &args[1])?
        } else {
            bcx.ins().iconst(types::I64, 0)
        };
        let step = if args.len() >= 3 {
            self.translate_expr(bcx, &args[2])?
        } else {
            bcx.ins().iconst(types::I64, 1)
        };

        // Determine comparison direction from compile-time step literal.
        // If step is a negative literal, loop while cur > end; else cur < end.
        let cmp_op = if args.len() >= 3 {
            match &args[2] {
                IRExpr::Literal(LitValue::Int(n)) if *n < 0 => IntCC::SignedGreaterThan,
                _ => IntCC::SignedLessThan,
            }
        } else {
            IntCC::SignedLessThan
        };

        let loop_var_name = if vars.len() >= 2 { &vars[1] } else { &vars[0] };
        let idx_var_name = if vars.len() >= 2 { Some(vars[0].as_str()) } else { None };

        let iv = self.declare_variable(bcx, loop_var_name, types::I64, &DataType::Int);
        let idx_var = match idx_var_name {
            Some(n) => Some(self.declare_variable(bcx, n, types::I64, &DataType::Int)),
            None => None,
        };

        bcx.def_var(iv, start);
        if let Some(iv2) = idx_var {
            bcx.def_var(iv2, start);
        }

        let cond_b = bcx.create_block();
        let body_b = bcx.create_block();
        let incr_b = bcx.create_block();
        let end_b = bcx.create_block();

        bcx.ins().jump(cond_b, &[]);
        // cond_b is sealed after the incr back-edge (see translate_while).
        // incr_b is sealed after the body's fall-through jump, so Cranelift
        // knows the body block as a predecessor before sealing.
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let cur = bcx.use_var(iv);
        let cmp = bcx.ins().icmp(cmp_op, cur, end);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(body_b);
        self.loop_stack.push((end_b, incr_b));
        self.diverged = false;
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(incr_b, &[]);
        }
        bcx.seal_block(incr_b);

        bcx.switch_to_block(incr_b);
        self.diverged = false;
        let cur = bcx.use_var(iv);
        let next = bcx.ins().iadd(cur, step);
        bcx.def_var(iv, next);
        if let Some(iv2) = idx_var {
            bcx.def_var(iv2, next);
        }
        bcx.ins().jump(cond_b, &[]);
        bcx.seal_block(cond_b);

        bcx.switch_to_block(end_b);
        self.diverged = false;
        Ok(())
    }

    fn translate_for_array(
        &mut self,
        bcx: &mut FunctionBuilder,
        vars: &[String],
        iterable: &IRExpr,
        body: &IRBlock,
    ) -> Result<(), String> {
        let arr_ptr = self.translate_expr(bcx, iterable)?;
        let val_name = if vars.len() >= 2 { &vars[1] } else { &vars[0] };
        let idx_name = if vars.len() >= 2 { Some(vars[0].as_str()) } else { None };

        let val_var = self.declare_variable(bcx, val_name, types::I64, &DataType::Int);
        let idx_var = self.declare_variable(bcx, &format!("__idx_{}", val_name), types::I64, &DataType::Int);
        let user_idx_var = match idx_name {
            Some(n) => Some(self.declare_variable(bcx, n, types::I64, &DataType::Int)),
            None => None,
        };

        let zero = bcx.ins().iconst(types::I64, 0);
        bcx.def_var(val_var, zero);
        bcx.def_var(idx_var, zero);
        if let Some(uiv) = user_idx_var {
            bcx.def_var(uiv, zero);
        }

        let cond_b = bcx.create_block();
        let body_b = bcx.create_block();
        let incr_b = bcx.create_block();
        let end_b = bcx.create_block();

        bcx.ins().jump(cond_b, &[]);
        // cond_b sealed after the incr back-edge (see translate_while).
        // incr_b sealed after the body's fall-through jump (see translate_while).
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let len = self.call_runtime(bcx, "gobol_array_len", &[arr_ptr]);
        let cmp = bcx.ins().icmp(IntCC::SignedLessThan, i, len);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(body_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let val = self.call_runtime(bcx, "gobol_array_get", &[arr_ptr, i]);
        bcx.def_var(val_var, val);
        if let Some(uiv) = user_idx_var {
            bcx.def_var(uiv, i);
        }
        self.loop_stack.push((end_b, incr_b));
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(incr_b, &[]);
        }
        bcx.seal_block(incr_b);

        bcx.switch_to_block(incr_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let one = bcx.ins().iconst(types::I64, 1);
        let next = bcx.ins().iadd(i, one);
        bcx.def_var(idx_var, next);
        if let Some(uiv) = user_idx_var {
            bcx.def_var(uiv, next);
        }
        bcx.ins().jump(cond_b, &[]);
        bcx.seal_block(cond_b);

        bcx.switch_to_block(end_b);
        self.diverged = false;
        Ok(())
    }

    fn translate_for_string(
        &mut self,
        bcx: &mut FunctionBuilder,
        vars: &[String],
        s: &str,
        body: &IRBlock,
    ) -> Result<(), String> {
        let str_ptr = self.intern_string(bcx, s);
        let ch_name = &vars[0];
        let ch_var = self.declare_variable(bcx, ch_name, types::I64, &DataType::Str);
        let idx_var = self.declare_variable(bcx, &format!("__idx_{}", ch_name), types::I64, &DataType::Int);
        let zero = bcx.ins().iconst(types::I64, 0);
        let empty_str = self.intern_string(bcx, "");
        bcx.def_var(ch_var, empty_str);
        bcx.def_var(idx_var, zero);

        let cond_b = bcx.create_block();
        let body_b = bcx.create_block();
        let incr_b = bcx.create_block();
        let end_b = bcx.create_block();

        bcx.ins().jump(cond_b, &[]);
        // cond_b sealed after the incr back-edge (see translate_while).
        // incr_b sealed after the body's fall-through jump (see translate_while).
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let len = self.call_runtime(bcx, "gobol_str_len", &[str_ptr]);
        let cmp = bcx.ins().icmp(IntCC::SignedLessThan, i, len);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(body_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let ch_code = self.call_runtime(bcx, "gobol_str_get", &[str_ptr, i]);
        let ch = self.call_runtime(bcx, "gobol_str_char", &[ch_code]);
        bcx.def_var(ch_var, ch);
        self.loop_stack.push((end_b, incr_b));
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(incr_b, &[]);
        }
        bcx.seal_block(incr_b);

        bcx.switch_to_block(incr_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let one = bcx.ins().iconst(types::I64, 1);
        let next = bcx.ins().iadd(i, one);
        bcx.def_var(idx_var, next);
        bcx.ins().jump(cond_b, &[]);
        bcx.seal_block(cond_b);

        bcx.switch_to_block(end_b);
        self.diverged = false;
        Ok(())
    }

    fn translate_assignment(
        &mut self,
        bcx: &mut FunctionBuilder,
        target: &IRExpr,
        value: &IRExpr,
    ) -> Result<(), String> {
        // Member assignment: obj.field = value
        if let IRExpr::MemberAccess { object, member } = target {
            let obj_ty = self.type_resolver.infer_type(object);
            if let DataType::Struct(sname) = &obj_ty {
                let obj_val = self.translate_expr(bcx, object)?;
                let val = self.translate_expr(bcx, value)?;
                if let Some(off) = self.type_resolver.field_offset(sname, member) {
                    let addr = self.field_addr(bcx, obj_val, off);
                    self.call_runtime(bcx, "gobol_mem_store", &[addr, val]);
                    return Ok(());
                }
            }
        }
        // Array index assignment: arr[i] = value
        // Degraded to the Ref<T> path: arr.index_mut(i).write(value).
        // For raw arrays: gobol_array_elem_addr(arr, i) → gobol_mem_store(addr, val).
        // For structs with index_mut: call the method, then call write on the Ref.
        if let IRExpr::ArrayIndex { array, index } = target {
            let arr_ty = self.type_resolver.infer_type(array);

            // Check if this is a nested array assignment (e.g., arr[2][2] = value)
            if let IRExpr::ArrayIndex { array: inner_array, index: inner_idx } = array.as_ref() {
                // 2D array assignment: get inner array, then store element
                let base = self.translate_expr(bcx, inner_array)?;
                let i1 = self.translate_expr(bcx, inner_idx)?;
                let inner_arr = self.call_runtime(bcx, "gobol_array_get", &[base, i1]);
                let i2 = self.translate_expr(bcx, index)?;
                let val = self.translate_expr(bcx, value)?;
                let addr = self.call_runtime(bcx, "gobol_array_elem_addr", &[inner_arr, i2]);
                self.call_runtime(bcx, "gobol_mem_store", &[addr, val]);
                return Ok(());
            }

            // Struct type with an index_mut method (e.g. vec<T>): use method dispatch.
            if let DataType::Struct(ref sname) = arr_ty {
                let full = format!("{}::index_mut", sname);
                // index_mut(self, index) → arity = 2
                if self.func_symbols.contains_key(&(full.clone(), 2)) {
                    // arr.index_mut(i) → returns a Ref<T>
                    let arr_val = self.translate_expr(bcx, array)?;
                    let idx_val = self.translate_expr(bcx, index)?;
                    let val = self.translate_expr(bcx, value)?;
                    let ref_val = self.translate_call_with_args(bcx, &full, &[arr_val, idx_val])?;
                    // ref.write(value)
                    let write_full = format!("{}::write", "Ref");
                    // Ref::write(self, value) → arity = 2
                    if self.func_symbols.contains_key(&(write_full.clone(), 2)) {
                        self.translate_call_with_args(bcx, &write_full, &[ref_val, val])?;
                    } else {
                        // Fallback: direct memory store via runtime
                        self.call_runtime(bcx, "gobol_mem_store", &[ref_val, val]);
                    }
                    return Ok(());
                }
            }

            // Raw array (DataType::Unknown or DataType::Array): use the runtime functions.
            let arr = self.translate_expr(bcx, array)?;
            let idx = self.translate_expr(bcx, index)?;
            let val = self.translate_expr(bcx, value)?;
            let addr = self.call_runtime(bcx, "gobol_array_elem_addr", &[arr, idx]);
            self.call_runtime(bcx, "gobol_mem_store", &[addr, val]);
            return Ok(());
        }
        // Simple variable assignment
        if let IRExpr::Variable(name) = target {
            let val = self.translate_expr(bcx, value)?;
            if let Some(var) = self.variables.get(name) {
                let var_ty = self.type_resolver.var_type(name);
                let v = self.coerce(bcx, val, &self.type_resolver.infer_type(value), &var_ty)?;
                bcx.def_var(*var, v);
                return Ok(());
            }
            // Undeclared: declare implicitly
            let ty = self.type_resolver.infer_type(value);
            let clif_ty = self.data_type_to_clif(&ty).unwrap_or(types::I64);
            let var = self.declare_variable(bcx, name, clif_ty, &ty);
            bcx.def_var(var, val);
            return Ok(());
        }
        // Fallback: evaluate value (side effects)
        self.translate_expr(bcx, value)?;
        Ok(())
    }

    // ==================== expressions ====================

    fn translate_expr(
        &mut self,
        bcx: &mut FunctionBuilder,
        expr: &IRExpr,
    ) -> Result<ir::Value, String> {
        match expr {
            IRExpr::Literal(lit) => Ok(self.translate_literal(bcx, lit)),
            IRExpr::Variable(name) => {
                if let Some(var) = self.variables.get(name) {
                    let v = bcx.use_var(*var);
                    Ok(v)
                } else {
                    Ok(bcx.ins().iconst(types::I64, 0))
                }
            }
            IRExpr::Binary { op, left, right } => {
                self.translate_binary(bcx, op, left, right)
            }
            IRExpr::Unary { op, operand } => self.translate_unary(bcx, op, operand),
            IRExpr::Call { func, args, .. } => self.translate_call(bcx, func, args),
            IRExpr::MethodCall { object, method, args, .. } => {
                self.translate_method_call(bcx, object, method, args)
            }
            IRExpr::MemberAccess { object, member } => {
                self.translate_member_access(bcx, object, member)
            }
            IRExpr::ArrayIndex { array, index } => {
                // Check if this is a nested array access (e.g., arr[2][2])
                if let IRExpr::ArrayIndex { array: inner_array, index: inner_idx } = array.as_ref() {
                    // 2D array access: first get the inner array, then get the element
                    let base = self.translate_expr(bcx, inner_array)?;
                    let i1 = self.translate_expr(bcx, inner_idx)?;
                    let inner_arr = self.call_runtime(bcx, "gobol_array_get", &[base, i1]);
                    let i2 = self.translate_expr(bcx, index)?;
                    Ok(self.call_runtime(bcx, "gobol_array_get", &[inner_arr, i2]))
                } else {
                    let arr = self.translate_expr(bcx, array)?;
                    let idx = self.translate_expr(bcx, index)?;
                    Ok(self.call_runtime(bcx, "gobol_array_get", &[arr, idx]))
                }
            }
            IRExpr::ArrayLiteral(elems) => {
                let arr = self.call_runtime(bcx, "gobol_array_new", &[]);
                for e in elems {
                    let v = self.translate_expr(bcx, e)?;
                    self.call_runtime(bcx, "gobol_array_add", &[arr, v]);
                }
                Ok(arr)
            }
            IRExpr::StructLiteral { name, fields } => {
                self.translate_struct_literal(bcx, name, fields)
            }
            IRExpr::Cast { expr, target } => self.translate_cast(bcx, expr, target),
            IRExpr::Assignment { target, value } => {
                self.translate_assignment(bcx, target, value)?;
                Ok(self.translate_expr(bcx, target)?)
            }
            IRExpr::FuncRef(name) => self.translate_func_ref(bcx, name),
            IRExpr::IndirectCall { callee, args } => {
                self.translate_indirect_call(bcx, callee, args)
            }
            IRExpr::None => Ok(bcx.ins().iconst(types::I64, 0)),
        }
    }

    fn translate_literal(&mut self, bcx: &mut FunctionBuilder, lit: &LitValue) -> ir::Value {
        match lit {
            LitValue::Int(n) => bcx.ins().iconst(types::I64, *n),
            LitValue::Float(f) => bcx.ins().f64const(*f),
            LitValue::Bool(b) => bcx.ins().iconst(types::I8, *b as i64),
            LitValue::Str(s) => self.intern_string(bcx, s),
            LitValue::None => bcx.ins().iconst(types::I64, 0),
        }
    }

    fn translate_binary(
        &mut self,
        bcx: &mut FunctionBuilder,
        op: &str,
        left: &IRExpr,
        right: &IRExpr,
    ) -> Result<ir::Value, String> {
        // String concatenation: "+" with a string operand.
        if op == "+" && self.contains_str(left) {
            let l = self.to_string_value(bcx, left)?;
            let r = self.to_string_value(bcx, right)?;
            return Ok(self.call_runtime(bcx, "gobol_str_cat", &[l, r]));
        }
        // String equality.
        if (op == "==" || op == "!=") && self.contains_str(left) {
            let l = self.to_string_value(bcx, left)?;
            let r = self.to_string_value(bcx, right)?;
            let eq = self.call_runtime(bcx, "gobol_str_eq", &[l, r]);
            if op == "==" {
                return Ok(eq);
            }
            let zero = bcx.ins().iconst(types::I8, 0);
            return Ok(bcx.ins().icmp(IntCC::Equal, eq, zero));
        }

        let l = self.translate_expr(bcx, left)?;
        let r = self.translate_expr(bcx, right)?;
        let lty = self.type_resolver.infer_type(left);

        if matches!(lty, DataType::Float) {
            return Ok(self.binary_float(bcx, op, l, r));
        }

        // Integer / pointer operations.
        Ok(match op {
            "+" => bcx.ins().iadd(l, r),
            "-" => bcx.ins().isub(l, r),
            "*" => bcx.ins().imul(l, r),
            "/" => bcx.ins().sdiv(l, r),
            "%" => bcx.ins().srem(l, r),
            "==" => bcx.ins().icmp(IntCC::Equal, l, r),
            "!=" => bcx.ins().icmp(IntCC::NotEqual, l, r),
            "<" => bcx.ins().icmp(IntCC::SignedLessThan, l, r),
            ">" => bcx.ins().icmp(IntCC::SignedGreaterThan, l, r),
            "<=" => bcx.ins().icmp(IntCC::SignedLessThanOrEqual, l, r),
            ">=" => bcx.ins().icmp(IntCC::SignedGreaterThanOrEqual, l, r),
            "&&" => {
                let l_b = self.to_bool(bcx, l, &lty);
                let r_b = self.to_bool(bcx, r, &self.type_resolver.infer_type(right));
                bcx.ins().band(l_b, r_b)
            }
            "||" => {
                let l_b = self.to_bool(bcx, l, &lty);
                let r_b = self.to_bool(bcx, r, &self.type_resolver.infer_type(right));
                bcx.ins().bor(l_b, r_b)
            }
            "&" => bcx.ins().band(l, r),
            "|" => bcx.ins().bor(l, r),
            "^" => bcx.ins().bxor(l, r),
            _ => {
                let _ = format!("unsupported operator: {}", op);
                bcx.ins().iconst(types::I64, 0)
            }
        })
    }

    fn binary_float(
        &self,
        bcx: &mut FunctionBuilder,
        op: &str,
        l: ir::Value,
        r: ir::Value,
    ) -> ir::Value {
        match op {
            "+" => bcx.ins().fadd(l, r),
            "-" => bcx.ins().fsub(l, r),
            "*" => bcx.ins().fmul(l, r),
            "/" => bcx.ins().fdiv(l, r),
            "==" => bcx.ins().fcmp(FloatCC::Equal, l, r),
            "!=" => bcx.ins().fcmp(FloatCC::NotEqual, l, r),
            "<" => bcx.ins().fcmp(FloatCC::LessThan, l, r),
            ">" => bcx.ins().fcmp(FloatCC::GreaterThan, l, r),
            "<=" => bcx.ins().fcmp(FloatCC::LessThanOrEqual, l, r),
            ">=" => bcx.ins().fcmp(FloatCC::GreaterThanOrEqual, l, r),
            _ => bcx.ins().f64const(0.0),
        }
    }

    fn translate_unary(
        &mut self,
        bcx: &mut FunctionBuilder,
        op: &str,
        operand: &IRExpr,
    ) -> Result<ir::Value, String> {
        let v = self.translate_expr(bcx, operand)?;
        let ty = self.type_resolver.infer_type(operand);
        Ok(match op {
            "-" => {
                if matches!(ty, DataType::Float) {
                    bcx.ins().fneg(v)
                } else {
                    let zero = bcx.ins().iconst(types::I64, 0);
                    bcx.ins().isub(zero, v)
                }
            }
            "!" => {
                let b = self.to_bool(bcx, v, &ty);
                let one = bcx.ins().iconst(types::I8, 1);
                bcx.ins().bxor(b, one)
            }
            _ => v,
        })
    }

    fn translate_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        func: &str,
        args: &[IRExpr],
    ) -> Result<ir::Value, String> {
        // Built-in IO functions — need IRExpr args for to_string_value conversion.
        if let Some(rt) = builtin_runtime(func) {
            return Ok(self.translate_runtime_call(bcx, rt, args, func));
        }
        // panic(msg)
        if func == "panic" {
            if let Some(arg) = args.first() {
                let s = self.to_string_value(bcx, arg)?;
                self.call_runtime(bcx, "gobol_println", &[s]);
            }
            return Ok(bcx.ins().iconst(types::I64, 0));
        }

        // Struct static method via :: notation: StructName::method(args)
        // The IR builder prepends `self` for methods (constructors, enum
        // variants, etc.), so we must allocate and prepend self.
        if let Some((struct_name, _method)) = func.split_once("::") {
            if self.type_resolver.has_struct(struct_name) {
                // Check that the method expects self (arity = args + 1)
                let arity = args.len() + 1;
                if self.func_symbols.contains_key(&(func.to_string(), arity)) {
                    let size = self.type_resolver.struct_size(struct_name);
                    let size_val = bcx.ins().iconst(types::I64, size);
                    let alloc_fn = if self.alloc_uses_no_gc(struct_name) {
                        "gobol_alloc"
                    } else {
                        "gobol_gc_alloc"
                    };
                    let self_ptr = self.call_runtime(bcx, alloc_fn, &[size_val]);
                    let mut all_args = vec![self_ptr];
                    all_args.append(&mut self.translate_args(bcx, args)?);
                    return self.translate_call_with_args(bcx, func, &all_args);
                }
            }
        }

        let arg_vals = self.translate_args(bcx, args)?;

        // Variadic extern "C" functions: route through a per-arity stub.
        // We check after translate_args so that arg evaluation side effects
        // still happen even if we bail, but the actual call uses the stub.
        if self.is_variadic_func(func) {
            return self.translate_variadic_call(bcx, func, args, &arg_vals);
        }

        self.translate_call_with_args(bcx, func, &arg_vals)
    }

    /// Resolve a function name to its linker symbol, looking up both the
    /// full name and the short name (for cross-module calls).
    fn resolve_func_symbol(&self, func: &str, arity: usize) -> Option<String> {
        if let Some(sym) = self.func_symbols.get(&(func.to_string(), arity)) {
            return Some(sym.clone());
        }
        let short = func.rsplit("::").next().unwrap_or(func);
        if let Some(sym) = self.func_symbols.get(&(short.to_string(), arity)) {
            return Some(sym.clone());
        }
        None
    }

    /// Translate a function reference (`FuncRef(name)`) into the function's
    /// address as an i64 value. Used to pass functions as arguments to
    /// higher-order functions.
    fn translate_func_ref(
        &mut self,
        bcx: &mut FunctionBuilder,
        name: &str,
    ) -> Result<ir::Value, String> {
        // Try arity 0..=16 to find a declared symbol for this function name.
        // Function references don't carry arity info, so probe common arities.
        for arity in 0..=32usize {
            if let Some(sym) = self.resolve_func_symbol(name, arity) {
                if let Some(fid) = self.func_ids.get(&sym) {
                    let fref = self.module.declare_func_in_func(*fid, &mut bcx.func);
                    let addr = bcx.ins().func_addr(types::I64, fref);
                    return Ok(addr);
                }
            }
        }
        Err(format!("cannot take address of unknown function '{}'", name))
    }

    /// Translate an indirect call through a function pointer value.
    /// All arguments and the return value are i64 (the universal value type
    /// in the GoBol ABI), so a single signature suffices.
    fn translate_indirect_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        callee: &IRExpr,
        args: &[IRExpr],
    ) -> Result<ir::Value, String> {
        let arg_vals = self.translate_args(bcx, args)?;
        let arity = arg_vals.len();

        // 如果 callee 是 FuncRef，根据实际参数个数解析重载
        if let IRExpr::FuncRef(name) = callee {
            if let Some(sym) = self.resolve_func_symbol(name, arity) {
                if let Some(fid) = self.func_ids.get(&sym) {
                    let fref = self.module.declare_func_in_func(*fid, &mut bcx.func);
                    let call = bcx.ins().call(fref, &arg_vals);
                    let results = bcx.inst_results(call);
                    if results.is_empty() {
                        return Ok(bcx.ins().iconst(types::I64, 0));
                    } else {
                        return Ok(results[0]);
                    }
                }
            }
        }

        let callee_val = self.translate_expr(bcx, callee)?;

        // Build a signature: all params are i64, return is i64.
        let mut sig = self.module.make_signature();
        for _ in &arg_vals {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        let sig_ref = bcx.import_signature(sig);

        let call = bcx.ins().call_indirect(sig_ref, callee_val, &arg_vals);
        let results = bcx.inst_results(call);
        if results.is_empty() {
            Ok(bcx.ins().iconst(types::I64, 0))
        } else {
            Ok(results[0])
        }
    }

    /// Check whether `func` (or its short name) is a variadic extern "C" function.
    fn is_variadic_func(&self, func: &str) -> bool {
        if self.variadic_funcs.contains(func) {
            return true;
        }
        let short = func.rsplit("::").next().unwrap_or(func);
        self.variadic_funcs.contains(short)
    }

    /// Call a variadic extern "C" function through a per-arity C stub.
    ///
    /// Each distinct (function, arity) pair gets a non-variadic import symbol
    /// `__gobol_va_<name>_<arity>` declared in the object file and a matching
    /// C wrapper generated at link time.
    fn translate_variadic_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        func: &str,
        args: &[IRExpr],
        arg_vals: &[ir::Value],
    ) -> Result<ir::Value, String> {
        // Resolve the canonical variadic function name (full or short).
        let canon_name = if self.variadic_funcs.contains(func) {
            func.to_string()
        } else {
            func.rsplit("::").next().unwrap_or(func).to_string()
        };

        let arity = arg_vals.len();
        let param_types: Vec<DataType> = args.iter().map(|a| self.type_resolver.infer_type(a)).collect();
        let return_type = self.type_resolver.func_return_type(&canon_name);

        // Deduplicate: reuse an existing stub for the same (name, arity).
        let stub = VariadicStub {
            func_name: canon_name.clone(),
            arity,
            param_types: param_types.clone(),
            return_type: return_type.clone(),
        };
        let stub_sym = stub.symbol_name();
        let already = self.variadic_stubs.iter().any(|s| {
            s.func_name == stub.func_name && s.arity == stub.arity
        });
        if !already {
            self.variadic_stubs.push(stub);
        }

        // Declare the stub import (idempotent — declare_import checks func_ids).
        let mut sig = self.module.make_signature();
        for dt in &param_types {
            sig.params.push(AbiParam::new(self.data_type_to_clif(dt)?));
        }
        if !matches!(return_type, DataType::None_) {
            sig.returns.push(AbiParam::new(self.data_type_to_clif(&return_type)?));
        }
        self.declare_import(&stub_sym, sig);

        let fid = self.func_ids[&stub_sym];
        let fref = self.module.declare_func_in_func(fid, &mut bcx.func);
        let call = bcx.ins().call(fref, arg_vals);
        Ok(self.call_result(bcx, call, return_type))
    }

    /// Call a user function with pre-translated Cranelift value arguments.
    fn translate_call_with_args(
        &mut self,
        bcx: &mut FunctionBuilder,
        func: &str,
        arg_vals: &[ir::Value],
    ) -> Result<ir::Value, String> {
        // Built-in IO functions — call runtime directly with pre-translated args.
        if let Some(rt) = builtin_runtime(func) {
            // Void-ish runtime IO helpers: call runtime but return dummy value.
            // `read` actually returns a string (i64), so forward that value.
            // For the others, just discard the call result.
            if rt == "gobol_read" {
                return Ok(self.call_runtime(bcx, rt, arg_vals));
            }
            self.call_runtime(bcx, rt, arg_vals);
            return Ok(bcx.ins().iconst(types::I64, 0));
        }
        // panic(msg)
        if func == "panic" {
            return Ok(bcx.ins().iconst(types::I64, 0));
        }

        // Struct intrinsic static methods (e.g., File::open, TcpStream::connect).
        // These have no body — intercept before user function lookup.
        if let Some((struct_name, method)) = func.split_once("::") {
            if let Some(rt) = self.struct_intrinsic_runtime(struct_name, method) {
                let fret = self.type_resolver.func_return_type(func);
                if matches!(fret, DataType::None_) {
                    self.call_runtime(bcx, rt, arg_vals);
                    return Ok(bcx.ins().iconst(types::I64, 0));
                }
                return Ok(self.call_runtime(bcx, rt, arg_vals));
            }
        }

        // User function lookup. Try (full name, arity) first, then
        // (short name, arity) for cross-module calls where the function
        // is registered under just its short name.
        let arity = arg_vals.len();
        let lookup_name = {
            if self.func_symbols.contains_key(&(func.to_string(), arity)) {
                func.to_string()
            } else {
                let short = func.rsplit("::").next().unwrap_or(func);
                if self.func_symbols.contains_key(&(short.to_string(), arity)) {
                    short.to_string()
                } else {
                    func.to_string()
                }
            }
        };
        if let Some(sym) = self.func_symbols.get(&(lookup_name.clone(), arity)) {
            if let Some(fid) = self.func_ids.get(sym) {
                let fret = self.type_resolver.func_return_type(&lookup_name);
                let fref = self.module.declare_func_in_func(*fid, &mut bcx.func);
                let call = bcx.ins().call(fref, arg_vals);
                return Ok(self.call_result(bcx, call, fret));
            }
        }
        // Unknown function: evaluate args for side effects, return 0.
        Ok(bcx.ins().iconst(types::I64, 0))
    }

    fn translate_method_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        object: &IRExpr,
        method: &str,
        args: &[IRExpr],
    ) -> Result<ir::Value, String> {
        let obj_ty = self.type_resolver.infer_type(object);

        // Struct constructor / static methods: Type::new(...) or Type::method(...)
        // Must be BEFORE the qualified function check so we can allocate self
        if let IRExpr::Variable(name) = object {
            if self.type_resolver.has_struct(name) {
                let full = format!("{}::{}", name, method);
                // For constructors (new), allocate self and pass it as the first arg
                if method == "new" {
                    let size = self.type_resolver.struct_size(name);
                    let size_val = bcx.ins().iconst(types::I64, size);
                    let alloc_fn = if self.alloc_uses_no_gc(name) {
                        "gobol_alloc"
                    } else {
                        "gobol_gc_alloc"
                    };
                    let self_ptr = self.call_runtime(bcx, alloc_fn, &[size_val]);
                    let mut all_args = vec![self_ptr];
                    all_args.append(&mut self.translate_args(bcx, args)?);
                    return self.translate_call_with_args(bcx, &full, &all_args);
                }
                return self.translate_call(bcx, &full, args);
            }
        }

        // Qualified function call via . notation (backward compat): module.func(args)
        // e.g., m.add(5, 3) where m is an alias for an imported module
        if let IRExpr::Variable(module_var) = object {
            let full = format!("{}::{}", module_var, method);
            // Module function: arity = args.len() (no implicit self)
            if self.func_symbols.contains_key(&(full.clone(), args.len())) {
                return self.translate_call(bcx, &full, args);
            }
        }

        // Intrinsic arithmetic methods on primitive types (int, float)
        if let Some(result) = self.try_intrinsic_method(bcx, object, method, args, &obj_ty)? {
            return Ok(result);
        }

        // Array methods: arr.add(x), arr.len(), arr.get(i)
        if matches!(obj_ty, DataType::Unknown) || self.is_array_var(object) {
            match method {
                "add" => {
                    let arr = self.translate_expr(bcx, object)?;
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![arr];
                    all.append(&mut vals);
                    self.call_runtime(bcx, "gobol_array_add", &all);
                    return Ok(bcx.ins().iconst(types::I64, 0));
                }
                "len" => {
                    let arr = self.translate_expr(bcx, object)?;
                    return Ok(self.call_runtime(bcx, "gobol_array_len", &[arr]));
                }
                "get" => {
                    let arr = self.translate_expr(bcx, object)?;
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![arr];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_array_get", &all));
                }
                "index" => {
                    // Index trait: arr.index(i) → gobol_array_get
                    let arr = self.translate_expr(bcx, object)?;
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![arr];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_array_get", &all));
                }
                "index_mut" => {
                    // IndexMut trait: arr.index_mut(i) → returns address as a Ref-like value
                    let arr = self.translate_expr(bcx, object)?;
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![arr];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_array_elem_addr", &all));
                }
                _ => {}
            }
        }

        // String methods: s.len(), s.contains(sub), s.trim(), s.replace(from, to)
        if matches!(obj_ty, DataType::Str) {
            let s = self.translate_expr(bcx, object)?;
            match method {
                "len" => {
                    return Ok(self.call_runtime(bcx, "gobol_str_len", &[s]));
                }
                "contains" => {
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![s];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_str_contains", &all));
                }
                "trim" => {
                    return Ok(self.call_runtime(bcx, "gobol_str_trim", &[s]));
                }
                "replace" => {
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![s];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_str_replace", &all));
                }
                _ => {}
            }
        }

        // Struct intrinsic methods: File.open/read_all/write/close,
        // TcpStream.connect/send/recv/close, TcpListener.bind/accept.
        // These are dispatched to C runtime before falling through to
        // the regular user-function lookup (which would find the bodyless
        // intrinsic declaration and return 0).
        if let DataType::Struct(sname) = &obj_ty {
            if let Some(rt) = self.struct_intrinsic_runtime(sname, method) {
                let obj_val = self.translate_expr(bcx, object)?;
                let mut vals = vec![obj_val];
                vals.append(&mut self.translate_args(bcx, args)?);
                let ret_ty = self.type_resolver.func_return_type(&format!("{}::{}", sname, method));
                if matches!(ret_ty, DataType::None_) {
                    self.call_runtime(bcx, rt, &vals);
                    return Ok(bcx.ins().iconst(types::I64, 0));
                }
                return Ok(self.call_runtime(bcx, rt, &vals));
            }
        }

        // Instance method call: obj.method(args) -> StructName_method(obj, args...)
        if let DataType::Struct(sname) = &obj_ty {
            let full = format!("{}::{}", sname, method);
            let obj_val = self.translate_expr(bcx, object)?;
            let mut vals = vec![obj_val];
            vals.append(&mut self.translate_args(bcx, args)?);
            // Instance method: arity = vals.len() (includes implicit self)
            if let Some(sym) = self.func_symbols.get(&(full.clone(), vals.len())) {
                if let Some(fid) = self.func_ids.get(sym) {
                    let fref = self.module.declare_func_in_func(*fid, &mut bcx.func);
                    let call = bcx.ins().call(fref, &vals);
                    return Ok(self.call_result(bcx, call, self.type_resolver.func_return_type(&full)));
                }
            }
        }

        // Module call: io.println(...) etc. — fall back to builtin check.
        if let Some(rt) = builtin_runtime(method) {
            return Ok(self.translate_runtime_call(bcx, rt, args, method));
        }

        // Fallback: evaluate and return 0.
        let _ = self.translate_expr(bcx, object)?;
        for a in args {
            let _ = self.translate_expr(bcx, a)?;
        }
        Ok(bcx.ins().iconst(types::I64, 0))
    }

    /// Map a (struct_name, method_name) pair to its C runtime function.
    /// Returns None for non-intrinsic methods (which use regular dispatch).
    fn struct_intrinsic_runtime(&self, struct_name: &str, method: &str) -> Option<&'static str> {
        match (struct_name, method) {
            ("File", "open") => Some("gobol_fs_open"),
            ("File", "read_all") => Some("gobol_fs_read_all"),
            ("File", "write") => Some("gobol_fs_write"),
            ("File", "close") => Some("gobol_fs_close"),
            ("TcpStream", "connect") => Some("gobol_tcp_connect"),
            ("TcpStream", "send") => Some("gobol_tcp_send"),
            ("TcpStream", "recv") => Some("gobol_tcp_recv"),
            ("TcpStream", "close") => Some("gobol_tcp_close"),
            ("TcpListener", "bind") => Some("gobol_tcp_bind"),
            ("TcpListener", "accept") => Some("gobol_tcp_accept"),
            _ => None,
        }
    }

    /// Try to handle an intrinsic arithmetic method call (add, sub, mul, etc.)
    /// on a primitive type (int, float). Returns None if not an intrinsic.
    fn try_intrinsic_method(
        &mut self,
        bcx: &mut FunctionBuilder,
        object: &IRExpr,
        method: &str,
        args: &[IRExpr],
        obj_ty: &DataType,
    ) -> Result<Option<ir::Value>, String> {
        let is_int = matches!(obj_ty, DataType::Int);
        let is_float = matches!(obj_ty, DataType::Float);
        if !is_int && !is_float {
            return Ok(None);
        }

        // Translate operands
        let l = self.translate_expr(bcx, object)?;
        let arg_vals = self.translate_args(bcx, args)?;
        if arg_vals.is_empty() {
            return Ok(Some(l)); // unary case, not expected
        }
        let r = arg_vals[0];

        if is_float {
            Ok(Some(match method {
                "add" => bcx.ins().fadd(l, r),
                "sub" => bcx.ins().fsub(l, r),
                "mul" => bcx.ins().fmul(l, r),
                "div" => bcx.ins().fdiv(l, r),
                "eq" => bcx.ins().fcmp(FloatCC::Equal, l, r),
                "ne" => bcx.ins().fcmp(FloatCC::NotEqual, l, r),
                "lt" => bcx.ins().fcmp(FloatCC::LessThan, l, r),
                "gt" => bcx.ins().fcmp(FloatCC::GreaterThan, l, r),
                "le" => bcx.ins().fcmp(FloatCC::LessThanOrEqual, l, r),
                "ge" => bcx.ins().fcmp(FloatCC::GreaterThanOrEqual, l, r),
                _ => return Ok(None),
            }))
        } else {
            Ok(Some(match method {
                "add" => bcx.ins().iadd(l, r),
                "sub" => bcx.ins().isub(l, r),
                "mul" => bcx.ins().imul(l, r),
                "div" => bcx.ins().sdiv(l, r),
                "rem" => bcx.ins().srem(l, r),
                "eq" => bcx.ins().icmp(IntCC::Equal, l, r),
                "ne" => bcx.ins().icmp(IntCC::NotEqual, l, r),
                "lt" => bcx.ins().icmp(IntCC::SignedLessThan, l, r),
                "gt" => bcx.ins().icmp(IntCC::SignedGreaterThan, l, r),
                "le" => bcx.ins().icmp(IntCC::SignedLessThanOrEqual, l, r),
                "ge" => bcx.ins().icmp(IntCC::SignedGreaterThanOrEqual, l, r),
                _ => return Ok(None),
            }))
        }
    }

    fn translate_member_access(
        &mut self,
        bcx: &mut FunctionBuilder,
        object: &IRExpr,
        member: &str,
    ) -> Result<ir::Value, String> {
        let obj_ty = self.type_resolver.infer_type(object);
        if let DataType::Struct(sname) = &obj_ty {
            let obj_val = self.translate_expr(bcx, object)?;
            if let Some(off) = self.type_resolver.field_offset(sname, member) {
                let addr = self.field_addr(bcx, obj_val, off);
                return Ok(self.call_runtime(bcx, "gobol_mem_load", &[addr]));
            }
        }
        Ok(self.translate_expr(bcx, object)?)
    }

    fn translate_struct_literal(
        &mut self,
        bcx: &mut FunctionBuilder,
        name: &str,
        fields: &[(String, IRExpr)],
    ) -> Result<ir::Value, String> {
        // Prefer calling the user-defined constructor `StructName::new`.
        if self.constructors.get(name).copied().unwrap_or(false) {
            let args: Vec<IRExpr> = fields.iter().map(|(_, e)| e.clone()).collect();
            let full = format!("{}::new", name);
            // Allocate self and prepend to constructor args
            let size = self.type_resolver.struct_size(name);
            let size_val = bcx.ins().iconst(types::I64, size);
            let alloc_fn = if self.alloc_uses_no_gc(name) {
                "gobol_alloc"
            } else {
                "gobol_gc_alloc"
            };
            let self_ptr = self.call_runtime(bcx, alloc_fn, &[size_val]);
            let mut all_args = vec![self_ptr];
            all_args.append(&mut self.translate_args(bcx, &args)?);
            return self.translate_call_with_args(bcx, &full, &all_args);
        }
        // Otherwise allocate and store fields directly.
        let size = self.type_resolver.struct_size(name);
        let size_val = bcx.ins().iconst(types::I64, size);
        let alloc_fn = if self.alloc_uses_no_gc(name) {
            "gobol_alloc"
        } else {
            "gobol_gc_alloc"
        };
        let ptr = self.call_runtime(bcx, alloc_fn, &[size_val]);
        if let Some(off) = self.type_resolver.struct_fields(name) {
            for (field_name, _field_ty) in &off {
                if let Some((_, e)) = fields.iter().find(|(n, _)| n == field_name) {
                    let v = self.translate_expr(bcx, e)?;
                    let offset = self.type_resolver.field_offset(name, field_name).unwrap_or(0);
                    let addr = self.field_addr(bcx, ptr, offset);
                    self.call_runtime(bcx, "gobol_mem_store", &[addr, v]);
                }
            }
        }
        Ok(ptr)
    }

    fn translate_cast(
        &mut self,
        bcx: &mut FunctionBuilder,
        expr: &IRExpr,
        target: &DataType,
    ) -> Result<ir::Value, String> {
        let v = self.translate_expr(bcx, expr)?;
        let src = self.type_resolver.infer_type(expr);
        // Cast struct -> str: call StructName_convert_str(self).
        if matches!(target, DataType::Str) {
            if let DataType::Struct(name) = &src {
                let full = format!("{}::convert_str", name);
                // convert_str(self) → arity = 1
                if self.func_symbols.contains_key(&(full.clone(), 1)) {
                    return self.translate_call(bcx, &full, &[expr.clone()]);
                }
            }
        }
        Ok(match (&src, target) {
            (DataType::Int, DataType::Float) => bcx.ins().fcvt_from_sint(types::F64, v),
            (DataType::Float, DataType::Int) => bcx.ins().fcvt_to_sint(types::I64, v),
            (DataType::Int, DataType::Str) => {
                self.call_runtime(bcx, "gobol_str_int", &[v])
            }
            (DataType::Float, DataType::Str) => {
                self.call_runtime(bcx, "gobol_str_float", &[v])
            }
            (DataType::Bool, DataType::Str) => {
                self.call_runtime(bcx, "gobol_str_bool", &[v])
            }
            (DataType::Str, DataType::Int) => {
                // parse via runtime not available; return 0
                let _ = v;
                bcx.ins().iconst(types::I64, 0)
            }
            _ => v,
        })
    }

    // ==================== call helpers ====================

    fn translate_args(
        &mut self,
        bcx: &mut FunctionBuilder,
        args: &[IRExpr],
    ) -> Result<Vec<ir::Value>, String> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            out.push(self.translate_expr(bcx, a)?);
        }
        Ok(out)
    }

    /// Call an io-style builtin that auto-converts its argument to a string.
    fn translate_runtime_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        rt: &str,
        args: &[IRExpr],
        _func: &str,
    ) -> ir::Value {
        match rt {
            "gobol_print" | "gobol_println" => {
                if let Some(arg) = args.first() {
                    let s = self.to_string_value(bcx, arg).unwrap_or_else(|_| bcx.ins().iconst(types::I64, 0));
                    self.call_runtime(bcx, rt, &[s]);
                }
                bcx.ins().iconst(types::I64, 0)
            }
            "gobol_read" => self.call_runtime(bcx, rt, &[]),
            _ => {
                // generic: pass args through
                let vals = self.translate_args(bcx, args).unwrap_or_default();
                self.call_runtime(bcx, rt, &vals)
            }
        }
    }

    fn call_runtime(
        &mut self,
        bcx: &mut FunctionBuilder,
        name: &str,
        args: &[ir::Value],
    ) -> ir::Value {
        let fid = self.func_ids[name];
        let fref = self.module.declare_func_in_func(fid, &mut bcx.func);
        let call = bcx.ins().call(fref, args);
        let results = bcx.inst_results(call);
        if results.is_empty() {
            // Runtime call with void return — use 0 placeholder so the caller
            // always gets a Value (the compiler only reads this placeholder
            // when the enclosing expression is in a void context anyway).
            let _ = call;
            bcx.ins().iconst(types::I64, 0)
        } else {
            results[0]
        }
    }

    #[allow(dead_code)]
    fn func_returns_void(&self, name: &str) -> bool {
        matches!(name,
            "gobol_print" | "gobol_println" | "gobol_eprint" | "gobol_eprintln"
            | "gobol_array_add" | "gobol_array_set"
            | "gobol_mem_store"
            | "gobol_fs_close" | "gobol_tcp_close"
            | "gobol_chan_destroy"
        )
    }

    fn call_result(&self, bcx: &mut FunctionBuilder, call: Inst, fret: DataType) -> ir::Value {
        if matches!(fret, DataType::None_) {
            return bcx.ins().iconst(types::I64, 0);
        }
        let results = bcx.inst_results(call);
        if results.is_empty() {
            bcx.ins().iconst(types::I64, 0)
        } else {
            results[0]
        }
    }

    // ==================== string / type helpers ====================

    fn intern_string(&mut self, bcx: &mut FunctionBuilder, s: &str) -> ir::Value {
        let key = s.to_string();
        let data_id = if let Some(id) = self.string_data.get(&key) {
            *id
        } else {
            let name = format!("gbl_str_{}", self.string_data.len());
            let data_id = self
                .module
                .declare_data(&name, Linkage::Local, false, false)
                .map_err(|e| panic!("declare string data failed: {}", e))
                .unwrap();
            let mut desc = DataDescription::new();
            let mut bytes = s.as_bytes().to_vec();
            bytes.push(0); // null terminator
            desc.define(bytes.into_boxed_slice());
            self.module
                .define_data(data_id, &desc)
                .map_err(|e| panic!("define string data failed: {}", e))
                .unwrap();
            self.string_data.insert(key, data_id);
            data_id
        };
        let global = self.module.declare_data_in_func(data_id, &mut bcx.func);
        bcx.ins().symbol_value(types::I64, global)
    }

    /// Convert any value to a string pointer (for print / concatenation).
    fn to_string_value(
        &mut self,
        bcx: &mut FunctionBuilder,
        expr: &IRExpr,
    ) -> Result<ir::Value, String> {
        let ty = self.type_resolver.infer_type(expr);
        if matches!(ty, DataType::Str) {
            return self.translate_expr(bcx, expr);
        }
        let v = self.translate_expr(bcx, expr)?;
        Ok(match ty {
            DataType::Int => self.call_runtime(bcx, "gobol_str_int", &[v]),
            DataType::Float => self.call_runtime(bcx, "gobol_str_float", &[v]),
            DataType::Bool => self.call_runtime(bcx, "gobol_str_bool", &[v]),
            _ => {
                // Best-effort: treat as int.
                self.call_runtime(bcx, "gobol_str_int", &[v])
            }
        })
    }

    fn to_bool(
        &self,
        bcx: &mut FunctionBuilder,
        v: ir::Value,
        ty: &DataType,
    ) -> ir::Value {
        if matches!(ty, DataType::Bool) {
            return v;
        }
        let zero = bcx.ins().iconst(types::I64, 0);
        bcx.ins().icmp(IntCC::NotEqual, v, zero)
    }

    /// Coerce a value of `from` type into `to` type (int<->float, etc.).
    fn coerce(
        &self,
        bcx: &mut FunctionBuilder,
        v: ir::Value,
        from: &DataType,
        to: &DataType,
    ) -> Result<ir::Value, String> {
        if from == to {
            return Ok(v);
        }
        Ok(match (from, to) {
            (DataType::Int, DataType::Float) => bcx.ins().fcvt_from_sint(types::F64, v),
            (DataType::Float, DataType::Int) => bcx.ins().fcvt_to_sint(types::I64, v),
            (DataType::Bool, DataType::Int) => bcx.ins().uextend(types::I64, v),
            (DataType::Int, DataType::Bool) => v,
            _ => v,
        })
    }

    fn bitcast_to(&self, _bcx: &mut FunctionBuilder, v: ir::Value, ty: ir::Type) -> ir::Value {
        let cur = ty;
        let _ = cur;
        v
    }

    fn default_value(&mut self, bcx: &mut FunctionBuilder, ty: &DataType) -> ir::Value {
        match ty {
            DataType::Float => bcx.ins().f64const(0.0),
            DataType::Bool => bcx.ins().iconst(types::I8, 0),
            DataType::Unknown => {
                // array: allocate an empty one
                let fid = self.func_ids["gobol_array_new"];
                let fref = self.module.declare_func_in_func(fid, &mut bcx.func);
                let call = bcx.ins().call(fref, &[]);
                bcx.inst_results(call)[0]
            }
            DataType::Array(_) => {
                // array: allocate an empty one (size info lost, caller should set init)
                let fid = self.func_ids["gobol_array_new"];
                let fref = self.module.declare_func_in_func(fid, &mut bcx.func);
                let call = bcx.ins().call(fref, &[]);
                bcx.inst_results(call)[0]
            }
            _ => bcx.ins().iconst(types::I64, 0),
        }
    }

    fn declare_variable(
        &mut self,
        bcx: &mut FunctionBuilder,
        name: &str,
        clif_ty: ir::Type,
        data_ty: &DataType,
    ) -> Variable {
        // declare_var registers the variable's type with the builder and returns
        // a fresh Variable index. def_var/use_var require this registration.
        let var = bcx.declare_var(clif_ty);
        self.variables.insert(name.to_string(), var);
        self.type_resolver.declare_var(name, data_ty.clone());
        var
    }

    fn is_array_var(&self, expr: &IRExpr) -> bool {
        if let IRExpr::Variable(name) = expr {
            matches!(self.type_resolver.var_type(name), DataType::Unknown)
        } else {
            matches!(expr, IRExpr::ArrayLiteral(_))
        }
    }

    // ==================== struct helpers ====================

    fn field_addr(
        &self,
        bcx: &mut FunctionBuilder,
        base: ir::Value,
        offset: i64,
    ) -> ir::Value {
        if offset == 0 {
            return base;
        }
        let off = bcx.ins().iconst(types::I64, offset);
        bcx.ins().iadd(base, off)
    }

    // ==================== type mapping ====================

    fn data_type_to_clif(&self, dt: &DataType) -> Result<ir::Type, String> {
        Ok(match dt {
            DataType::Int => types::I64,
            DataType::Bool => types::I8,
            DataType::Float => types::F64,
            DataType::Str => types::I64,
            DataType::None_ => types::INVALID,
            DataType::Unknown => types::I64, // array pointer
            DataType::Struct(_) => types::I64, // struct pointer
            DataType::Nullable(inner) => self.data_type_to_clif(inner)?,
            DataType::Array(_) => types::I64, // array pointer
        })
    }

    fn contains_str(&self, e: &IRExpr) -> bool {
        self.type_resolver.contains_str(e)
    }
}

impl CraneliftBackend {
    /// Create an AOT backend targeting the host platform. For cross-compilation
    /// (e.g. `aarch64-unknown-none`) use [`CraneliftBackend::new_for_target`].
    pub fn new() -> Self {
        Self::new_for_target_triple(target_lexicon::HOST.clone())
            .expect("host target must be supported by Cranelift")
    }

    /// Create an AOT backend for an arbitrary target triple string such as
    /// `x86_64-pc-windows-msvc`, `aarch64-unknown-none`, or the host triple.
    /// Returns a clear error if the triple is malformed or unsupported by
    /// the installed Cranelift.
    pub fn new_for_target(target: &str) -> Result<Self, String> {
        let triple: target_lexicon::Triple = target
            .parse()
            .map_err(|e| format!("Invalid target triple '{}': {}", target, e))?;
        Self::new_for_target_triple(triple)
    }

    fn new_for_target_triple(triple: target_lexicon::Triple) -> Result<Self, String> {
        use cranelift_codegen::isa::lookup;
        use cranelift_codegen::settings::{self, Configurable};

        let triple_str = triple.to_string();
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "true").unwrap();
        let isa_builder = lookup(triple)
            .map_err(|e| format!("Cranelift does not support target '{}': {}", triple_str, e))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| format!("ISA finish failed for '{}': {}", triple_str, e))?;

        let builder = ObjectBuilder::new(
            isa,
            "gobol_aot",
            Box::new(cranelift_module::default_libcall_names()),
        )
        .map_err(|e| format!("ObjectBuilder init failed: {}", e))?;
        let module = ObjectModule::new(builder);

        Ok(CraneliftBackend {
            module,
            fn_ctx: FunctionBuilderContext::new(),
            func_symbols: HashMap::new(),
            func_overload_symbols: HashMap::new(),
            func_ids: HashMap::new(),
            string_data: HashMap::new(),
            constructors: HashMap::new(),
            type_resolver: TypeResolver::new(),
            variadic_funcs: std::collections::HashSet::new(),
            variadic_stubs: Vec::new(),
            variables: HashMap::new(),
            var_counter: 0,
            loop_stack: Vec::new(),
            return_type: DataType::None_,
            diverged: false,
            current_func_attributes: Vec::new(),
        })
    }

    /// Compile the IR, emit an object file, and link it to produce a
    /// standalone executable at `output_path`. Linking is driven by
    /// [`LinkOptions`], which selects the right linker for the target
    /// triple (MSVC `link.exe`, MinGW `gcc.exe`, Unix `cc`, or a bare-metal
    /// `ld` with a link script).
    pub fn compile_to_binary(
        mut self,
        ir: &GobolIR,
        output_path: &str,
        opts: &LinkOptions,
    ) -> Result<(), String> {
        self.compile_ir(ir)?;

        // Collect variadic stubs needed for `extern "C" ...` call sites.
        let stubs = std::mem::take(&mut self.variadic_stubs);

        // Produce object file from the ObjectModule.
        let product = self.module.finish();
        let obj_bytes = product
            .emit()
            .map_err(|e| format!("Object emit failed: {}", e))?;

        // Object file extension differs per platform (MSVC uses .obj).
        let obj_ext = if target_is_msvc(&opts.target) { "obj" } else { "o" };
        let obj_path = format!("{}.{}", output_path, obj_ext);
        std::fs::write(&obj_path, &obj_bytes)
            .map_err(|e| format!("Failed to write object file: {}", e))?;

        // Ensure the final output name carries `.exe` on Windows targets.
        let final_output = ensure_exe_extension(&opts.target, output_path);

        // If there are variadic call sites, generate a C stub file and
        // compile it alongside the main object file.
        let stubs_src_path = format!("{}.va.c", output_path);
        let has_stubs = !stubs.is_empty();
        if has_stubs {
            let mut src = String::new();
            src.push_str("/* Auto-generated variadic stubs for gobol extern \"C\" calls. */\n");
            // Forward-declare the variadic functions the stubs call into.
            // Including the standard C headers is the safest way to get
            // correct prototypes (e.g. `int printf(const char*, ...);`).
            src.push_str("#include <stdio.h>\n#include <stdlib.h>\n#include <string.h>\n");
            for stub in &stubs {
                src.push_str(&stub.c_source());
            }
            std::fs::write(&stubs_src_path, &src)
                .map_err(|e| format!("Failed to write variadic stubs: {}", e))?;
        }

        // Select the linker appropriate for this target.
        let (linker, kind) = detect_linker(&opts.target)?;

        let result = match kind {
            LinkerKind::CcDriver => Self::link_cc_driver(
                &linker,
                &obj_path,
                &opts,
                &final_output,
                has_stubs,
                &stubs_src_path,
            ),
            LinkerKind::MsvcLink => Self::link_msvc(
                &linker,
                &obj_path,
                &opts,
                &final_output,
                has_stubs,
                &stubs_src_path,
            ),
            LinkerKind::BareLd => {
                Self::link_bare_metal(&linker, &obj_path, &opts, &final_output)
            }
        };

        // Clean up temp files.
        let _ = std::fs::remove_file(&obj_path);
        if has_stubs {
            let _ = std::fs::remove_file(&stubs_src_path);
        }

        result
    }

    /// Link using a C/C++ compiler driver (cc / gcc / gcc.exe) that compiles
    /// the runtime `.c` sources and links the final binary in one step.
    /// Used for `*-unknown-linux-*` and `*-pc-windows-gnu`.
    fn link_cc_driver(
        linker: &std::path::Path,
        obj_path: &str,
        opts: &LinkOptions,
        final_output: &str,
        has_stubs: bool,
        stubs_src_path: &str,
    ) -> Result<(), String> {
        let mut cmd = std::process::Command::new(linker);
        cmd.arg(obj_path);
        if let Some(rt) = &opts.runtime_c_path {
            cmd.arg(rt);
        }
        if has_stubs {
            cmd.arg(stubs_src_path);
        }
        cmd.args(["-o", final_output]);
        for lib in &opts.link_libs {
            cmd.arg(format!("-l{}", lib));
        }
        // libm is available on both Unix and MinGW; libpthread is Unix-only
        // (MinGW uses its own threading model).
        cmd.arg("-lm");
        if !target_is_windows(&opts.target) {
            cmd.arg("-lpthread");
        }
        // Capture stdout/stderr so the error message includes the actual
        // linker diagnostic (undefined symbol, cannot find -lfoo, etc.).
        let out = cmd
            .output()
            .map_err(|e| format!("Failed to invoke linker '{}': {}", linker.display(), e))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Err(format!(
                "Linking failed with exit code {:?}\n--- linker stdout ---\n{}\n--- linker stderr ---\n{}",
                out.status.code(), stdout, stderr
            ));
        }
        Ok(())
    }

    /// Link for the MSVC toolchain. `link.exe` links only, so the C runtime
    /// sources (and variadic stubs) are pre-compiled to `.obj` with a C
    /// compiler (`clang-cl` preferred, falling back to `cl`) before linking.
    fn link_msvc(
        link_exe: &std::path::Path,
        obj_path: &str,
        opts: &LinkOptions,
        final_output: &str,
        has_stubs: bool,
        stubs_src_path: &str,
    ) -> Result<(), String> {
        // Delegate MSVC toolchain discovery to the `cc` crate (the same
        // logic rustc's own build scripts use): it locates the Visual
        // Studio install, runs the vcvarsall-equivalent setup, and returns
        // `cl.exe` / `clang-cl.exe` plus the env overlay (PATH/INCLUDE/LIB/
        // LIBPATH) that `link.exe` needs to find `ws2_32.lib` and the CRT.
        // This replaces the hand-written `find_msvc_c_compiler` probe and
        // means we no longer require the user to run vcvarsall.bat first.
        let tool = crate::toolchain::cc_discover(&opts.target)
            .map_err(|e| format!("MSVC toolchain discovery failed: {}", e))?
            .ok_or_else(msvc_toolchain_missing_error)?;
        if !tool.is_msvc {
            return Err(format!(
                "target '{}' is not MSVC-family but the MSVC link path was selected",
                opts.target
            ));
        }
        let c_compiler = &tool.compiler;

        // Pre-link check: verify the Cranelift-produced object exists.
        if !std::path::Path::new(obj_path).exists() {
            return Err(format!(
                "link_msvc: main object file '{}' does not exist before linking",
                obj_path
            ));
        }

        // Compile the C runtime sources to .obj objects. The cc-discovered
        // env is applied to cl.exe so it finds the SDK headers (INCLUDE).
        //
        // Include-search fix (needed so the master TU `runtime.c` can do
        // `#include "runtime/platform.h"` etc.):
        //   • If runtime.c lives at `std/runtime.c`, then the parent of its
        //     parent (i.e. `./`) is not enough — `"runtime/platform.h"` is
        //     found relative to `std/`, which is runtime.c's parent dir.
        //   • We therefore add runtime.c's *parent* as `/I` AND its
        //     grandparent (defensive for installed/non-dev layouts).
        let mut rt_includes: Vec<std::path::PathBuf> = Vec::new();
        if let Some(rt) = &opts.runtime_c_path {
            let rt_p = std::path::Path::new(rt);
            if let Some(parent) = rt_p.parent() {
                rt_includes.push(parent.to_path_buf());
                if let Some(gp) = parent.parent() {
                    rt_includes.push(gp.to_path_buf());
                }
            }
        }

        let mut extra_objs: Vec<String> = Vec::new();
        if let Some(rt) = &opts.runtime_c_path {
            let rt_obj = format!("{}.runtime.obj", final_output);
            let mut c = std::process::Command::new(c_compiler);
            c.args(["/c", "/nologo"]);
            for inc in &rt_includes {
                c.arg(format!("/I{}", inc.display()));
            }
            c.arg(format!("/Fo{}", rt_obj)).arg(rt);
            for (k, v) in &tool.env {
                c.env(k, v);
            }
            // Use .output() to capture cl.exe stdout/stderr — when cl.exe
            // fails we need the diagnostic text (which file wasn't found,
            // which symbol was unresolved, etc.) in the error message.
            let out = c.output().map_err(|e| {
                format!(
                    "Failed to compile runtime with '{}': {}\n(Is cl.exe on PATH? The cc-discovered env should include it.)",
                    c_compiler.display(), e
                )
            })?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                return Err(format!(
                    "Runtime compilation failed with exit code {:?}\n--- cl.exe stdout ---\n{}\n--- cl.exe stderr ---\n{}",
                    out.status.code(), stdout, stderr
                ));
            }
            // Verify the .obj was actually produced — cl.exe can return 0
            // in edge cases where the output path was silently ignored.
            if !std::path::Path::new(&rt_obj).exists() {
                return Err(format!(
                    "Runtime compilation appeared to succeed but '{}' was not produced.\ncl.exe stdout: {}\ncl.exe stderr: {}",
                    rt_obj,
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr),
                ));
            }
            extra_objs.push(rt_obj);
        }
        if has_stubs {
            let stub_obj = format!("{}.va.obj", final_output);
            let mut c = std::process::Command::new(c_compiler);
            c.args(["/c", "/nologo"]);
            for inc in &rt_includes {
                c.arg(format!("/I{}", inc.display()));
            }
            c.arg(format!("/Fo{}", stub_obj)).arg(stubs_src_path);
            for (k, v) in &tool.env {
                c.env(k, v);
            }
            let out = c.output().map_err(|e| {
                format!("Failed to compile stubs with '{}': {}", c_compiler.display(), e)
            })?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                return Err(format!(
                    "Stubs compilation failed with exit code {:?}\n--- cl.exe stdout ---\n{}\n--- cl.exe stderr ---\n{}",
                    out.status.code(), stdout, stderr
                ));
            }
            if !std::path::Path::new(&stub_obj).exists() {
                return Err(format!(
                    "Stubs compilation appeared to succeed but '{}' was not produced.\ncl.exe stdout: {}\ncl.exe stderr: {}",
                    stub_obj,
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr),
                ));
            }
            extra_objs.push(stub_obj);
        }

        // ---- Build the link.exe command ----
        let mut cmd = std::process::Command::new(link_exe);
        // Apply the MSVC env so link.exe resolves import libs (LIB) and
        // its own DLLs (PATH) — this is what makes ws2_32.lib findable
        // without a pre-set vcvarsall environment.
        for (k, v) in &tool.env {
            cmd.env(k, v);
        }
        cmd.arg("/NOLOGO")
            .arg(format!("/OUT:{}", final_output))
            .arg(obj_path);
        for obj in &extra_objs {
            cmd.arg(obj);
        }
        // Export the entry point symbol so the linker accepts `-e`-style
        // custom entry functions (e.g. `_start` for kernels).
        if let Some(ep) = &opts.entry_point {
            if ep != "main" {
                cmd.arg(format!("/ENTRY:{}", ep));
                cmd.arg("/SUBSYSTEM:CONSOLE");
            }
        }
        for lib in &opts.link_libs {
            cmd.arg(format!("{}.lib", lib));
        }
        // Add Rust toolchain lib paths so common system libs resolve.
        if let Ok(sysroot) = rust_sysroot() {
            let libroot = sysroot.join("lib");
            if libroot.exists() {
                cmd.arg(format!("/LIBPATH:{}", libroot.display()));
            }
            let rustlib = sysroot.join("lib").join("rustlib").join(&opts.target).join("lib");
            if rustlib.exists() {
                cmd.arg(format!("/LIBPATH:{}", rustlib.display()));
            }
        }
        // ---- LNK1181 fix: explicitly add /LIBPATH: for every directory
        // in the cc env's LIB variable. On some Windows CI setups the
        // `LIB` env var propagation via `cmd.env("LIB", …)` is silently
        // insufficient (the cc crate is build-script-oriented and may not
        // populate LIB at runtime). Passing `/LIBPATH:` on the command
        // line is the belt-and-suspenders approach — link.exe always
        // honours these, so `ws2_32.lib` and the CRT libs resolve even
        // when the env var is missing or wrong.
        for (k, v) in &tool.env {
            if k.to_ascii_lowercase() == "lib" {
                let lib_str = v.to_string_lossy();
                for dir in lib_str.split(';') {
                    let trimmed = dir.trim();
                    if !trimmed.is_empty() && std::path::Path::new(trimmed).is_dir() {
                        cmd.arg(format!("/LIBPATH:{}", trimmed));
                    }
                }
            }
        }

        // Use .output() to capture link.exe stdout/stderr. LNK1181 /
        // LNK2019 / LNK2001 diagnostics include the *name of the missing
        // file or symbol* — without capturing them, the user only sees
        // the raw exit code (e.g. "Some(1181)") and has no way to know
        // which file link.exe couldn't find.
        let out = cmd
            .output()
            .map_err(|e| format!("Failed to invoke link.exe '{}': {}", link_exe.display(), e))?;

        // Clean up the temporary .obj files.
        for obj in &extra_objs {
            let _ = std::fs::remove_file(obj);
        }
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Err(format!(
                "Linking failed with exit code {:?}\n--- link.exe stdout ---\n{}\n--- link.exe stderr ---\n{}",
                out.status.code(), stdout, stderr
            ));
        }
        Ok(())
    }

    /// Link for bare-metal / `no_std` targets (e.g. `aarch64-unknown-none`,
    /// `riscv64gc-unknown-none`). Uses a raw `ld` (or cross-`ld`) with a
    /// link script and a custom entry point — no C runtime is linked.
    fn link_bare_metal(
        ld: &std::path::Path,
        obj_path: &str,
        opts: &LinkOptions,
        final_output: &str,
    ) -> Result<(), String> {
        let mut cmd = std::process::Command::new(ld);
        cmd.arg("-nostdlib")
            .arg("-static")
            .args(["-o", final_output]);
        if let Some(script) = &opts.link_script {
            cmd.args(["-T", script]);
        }
        if let Some(ep) = &opts.entry_point {
            if ep != "main" {
                cmd.arg("-e").arg(ep);
            }
        }
        for lib in &opts.link_libs {
            cmd.arg(format!("-l{}", lib));
        }
        cmd.arg(obj_path);
        let output = cmd
            .output()
            .map_err(|e| format!("Failed to invoke linker '{}': {}", ld.display(), e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!(
                "Linking failed with exit code {:?}\n\n\
                 stdout:\n{}\n\n\
                 stderr:\n{}",
                output.status.code(),
                stdout,
                stderr
            ));
        }
        Ok(())
    }
}

// ==================== Link options & linker detection ====================

/// Options driving the final link step. Selects the right linker for the
/// target triple and carries any custom link script / entry point needed
/// for bare-metal builds.
#[derive(Clone)]
pub struct LinkOptions {
    /// Target triple, e.g. `x86_64-pc-windows-msvc`, `aarch64-unknown-none`.
    pub target: String,
    /// Path to the Gobol C runtime master unit (`std/runtime.c`). `None` for
    /// `no_std` / bare-metal targets that have no runtime.
    pub runtime_c_path: Option<String>,
    /// Extra libraries to link (from `extern "C"` blocks).
    pub link_libs: Vec<String>,
    /// Custom linker script (`-T`/`/LIBPATH`-less bare-metal path). Used with
    /// `grape.toml`'s `build.link_script`.
    pub link_script: Option<String>,
    /// Custom entry symbol (e.g. `_start`). When set to anything other than
    /// `"main"`, the linker is told to use it as the entry point and the
    /// semantic checker is told not to require a `main` function.
    pub entry_point: Option<String>,
}

impl LinkOptions {
    /// Defaults for a hosted build on the current host platform.
    pub fn host(runtime_c_path: impl Into<String>, link_libs: Vec<String>) -> Self {
        Self {
            target: host_target_string(),
            runtime_c_path: Some(runtime_c_path.into()),
            link_libs,
            link_script: None,
            entry_point: None,
        }
    }
}

/// The host target triple as a string (e.g. `x86_64-unknown-linux-gnu`).
pub fn host_target_string() -> String {
    target_lexicon::HOST.to_string()
}

/// True for any Windows target triple.
pub fn target_is_windows(target: &str) -> bool {
    target.contains("windows")
}

/// True for the MSVC ABI (Windows) target triple.
pub fn target_is_msvc(target: &str) -> bool {
    target.contains("windows-msvc")
}

/// True for `no_std` / bare-metal targets (triple ends in `none`).
pub fn target_is_bare_metal(target: &str) -> bool {
    target.ends_with("-none") || target.contains("unknown-none")
}

/// Append `.exe` to `name` for Windows targets when it isn't already present.
pub fn ensure_exe_extension(target: &str, name: &str) -> String {
    if !target_is_windows(target) {
        return name.to_string();
    }
    if name.to_ascii_lowercase().ends_with(".exe") {
        return name.to_string();
    }
    format!("{}.exe", name)
}

/// Kind of linker selected for a target triple.
enum LinkerKind {
    /// A C/C++ compiler driver (cc, gcc, gcc.exe) that both compiles `.c`
    /// runtime sources and links the final binary in one step.
    CcDriver,
    /// MSVC `link.exe` — links only; C sources must be pre-compiled.
    MsvcLink,
    /// A raw linker for bare-metal targets (`ld` / cross-`ld`), used with a
    /// link script and no C runtime.
    BareLd,
}

/// Run `rustc --print sysroot` to locate the Rust installation root. Produces
/// a clear, actionable error message when Rust isn't installed.
pub fn rust_sysroot() -> Result<std::path::PathBuf, String> {
    let output = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .map_err(|e| format!("Failed to run 'rustc --print sysroot': {}", e))?;
    if !output.status.success() {
        return Err(rust_not_installed_error());
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        return Err(rust_not_installed_error());
    }
    Ok(std::path::PathBuf::from(s))
}

/// The friendly error shown when the Rust toolchain (and thus a linker)
/// can't be located.
pub fn rust_not_installed_error() -> String {
    "linker not found\n\
     Rust toolchain not found. Please ensure Rust is installed:\n  \
     `https://rustup.rs/`\n\n\
     For MSVC toolchain:\n  \
     rustup default stable-msvc\n\n\
     For GNU toolchain (MinGW):\n  \
     rustup default stable-gnu".to_string()
}

fn msvc_toolchain_missing_error() -> String {
    "MSVC C compiler not found. The MSVC link path requires a C compiler\n\
     (cl.exe or clang-cl.exe) to compile the Gobol runtime.\n\
     Install Visual Studio Build Tools (C++ workload) or run:\n  \
     rustup default stable-gnu\n\
     to use the MinGW (GNU) toolchain instead, which bundles gcc.exe."
        .to_string()
}

/// Locate an executable on `PATH`. A minimal reimplementation of `which`.
fn which(exe: &str) -> Option<std::path::PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Locate an executable by searching the `PATH` entry found inside a
/// `cc::Tool`-style env overlay. Used to resolve `link.exe` from the MSVC
/// environment that `cc_discover` returns, so we find the VS linker even
/// when the parent shell hasn't run vcvarsall.bat (the cc env carries the
/// post-vcvarsall `PATH`).
fn which_in_env(
    exe: &str,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Option<std::path::PathBuf> {
    let path_val = env
        .iter()
        .find(|(k, _)| k == std::ffi::OsStr::new("PATH"))
        .map(|(_, v)| v)?;
    for dir in std::env::split_paths(path_val) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Detect the linker for a target triple string. Returns the linker path and
/// the kind of link command to build.
///
/// | Target                          | Linker        | Lookup              |
/// |---------------------------------|---------------|---------------------|
/// | `x86_64-pc-windows-msvc`        | `link.exe`    | sysroot/bin, PATH   |
/// | `x86_64-pc-windows-gnu`         | `gcc.exe`     | sysroot/bin, PATH   |
/// | `x86_64-unknown-linux-gnu`      | `cc` / `gcc`  | system PATH         |
/// | `aarch64-unknown-none`          | `ld` / cross | system PATH         |
/// | `riscv64gc-unknown-none`        | `ld` / cross | system PATH         |
fn detect_linker(target: &str) -> Result<(std::path::PathBuf, LinkerKind), String> {
    if target_is_bare_metal(target) {
        // Prefer a target-prefixed cross ld (e.g. aarch64-linux-gnu-ld),
        // falling back to the plain `ld`.
        let arch = target.split('-').next().unwrap_or("");
        let cross_ld = format!("{}-linux-gnu-ld", arch);
        if let Some(p) = which(&cross_ld) {
            return Ok((p, LinkerKind::BareLd));
        }
        if let Some(p) = which("ld") {
            return Ok((p, LinkerKind::BareLd));
        }
        return Err(format!(
            "linker not found for bare-metal target '{}': no `ld` or `{}-linux-gnu-ld` on PATH",
            target, arch
        ));
    }

    let sysroot = rust_sysroot()?;
    let bin_dir = sysroot.join("bin");

    if target_is_msvc(target) {
        // Use the `cc` crate to discover the MSVC environment (the same
        // discovery rustc uses), then resolve `link.exe` within that
        // environment's PATH — this finds the VS linker even when the
        // current shell hasn't run vcvarsall.bat. Fall back to the
        // Rust-bundled `lld-link` and a bare PATH lookup.
        if let Ok(Some(tool)) = crate::toolchain::cc_discover(target) {
            if let Some(p) = which_in_env("link.exe", &tool.env) {
                return Ok((p, LinkerKind::MsvcLink));
            }
        }
        // lld-link is the linker Rust itself ships for MSVC; accept it too.
        let lld_link = bin_dir.join("lld-link.exe");
        if lld_link.exists() {
            return Ok((lld_link, LinkerKind::MsvcLink));
        }
        if let Some(p) = which("link.exe") {
            return Ok((p, LinkerKind::MsvcLink));
        }
        return Err(format!(
            "linker not found for MSVC target '{}': the `cc` crate could not\n\
             locate the VS toolchain and no link.exe / lld-link.exe is on PATH.\n\
             Install Visual Studio with the 'Desktop development with C++' workload.",
            target
        ));
    }

    if target_is_windows(target) {
        // Windows GNU (MinGW): gcc.exe from the Rust sysroot, then PATH.
        let candidate = bin_dir.join("gcc.exe");
        if candidate.exists() {
            return Ok((candidate, LinkerKind::CcDriver));
        }
        if let Some(p) = which("gcc.exe") {
            return Ok((p, LinkerKind::CcDriver));
        }
        return Err(format!(
            "linker not found for GNU target '{}': no gcc.exe in {} or on PATH",
            target,
            bin_dir.display()
        ));
    }

    // Unix / other hosted: prefer cc, then gcc, from PATH.
    if let Some(p) = which("cc") {
        return Ok((p, LinkerKind::CcDriver));
    }
    if let Some(p) = which("gcc") {
        return Ok((p, LinkerKind::CcDriver));
    }
    Err(format!(
        "linker not found for target '{}': neither cc nor gcc on PATH",
        target
    ))
}

/// Map a Gobol builtin call name to its runtime function.
fn builtin_runtime(name: &str) -> Option<&'static str> {
    // Strip any :: namespace prefix and match on the function name.
    // Call sites sometimes use bare names (`print`) or sometimes the
    // module-qualified C name (`builtins::gobol_print`).
    let short = name.rsplit("::").next().unwrap_or(name);
    match short {
        "print" | "_print" | "gobol_print" => Some("gobol_print"),
        "println" | "_println" | "gobol_println" => Some("gobol_println"),
        "eprint" | "_eprint" | "gobol_eprint" => Some("gobol_eprint"),
        "eprintln" | "_eprintln" | "gobol_eprintln" => Some("gobol_eprintln"),
        "read" | "_read" | "gobol_read" => Some("gobol_read"),
        // math intrinsics
        "sin" => Some("gobol_math_sin"),
        "cos" => Some("gobol_math_cos"),
        "pow" => Some("gobol_math_pow"),
        // fs intrinsics (standalone functions)
        "exists" => Some("gobol_fs_exists"),
        // array intrinsics
        "gobol_array_new" => Some("gobol_array_new"),
        "gobol_array_new_with_size" => Some("gobol_array_new_with_size"),
        "gobol_array_new_2d" => Some("gobol_array_new_2d"),
        // GC intrinsics
        "gobol_gc_alloc" => Some("gobol_gc_alloc"),
        "gobol_gc_mark" => Some("gobol_gc_mark"),
        "gobol_gc_sweep" => Some("gobol_gc_sweep"),
        "gobol_gc_collect" => Some("gobol_gc_collect"),
        "gobol_gc_collect_now" => Some("gobol_gc_collect_now"),
        "gobol_gc_alloc_count" => Some("gobol_gc_alloc_count"),
        // thread / channel runtime
        "gobol_thread_spawn" => Some("gobol_thread_spawn"),
        "gobol_thread_join" => Some("gobol_thread_join"),
        "gobol_chan_create" => Some("gobol_chan_create"),
        "gobol_chan_send" => Some("gobol_chan_send"),
        "gobol_chan_recv" => Some("gobol_chan_recv"),
        "gobol_chan_destroy" => Some("gobol_chan_destroy"),
        _ => None,
    }
}
