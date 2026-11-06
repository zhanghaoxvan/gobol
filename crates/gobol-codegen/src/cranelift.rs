// cranelift.rs — AOT backend. Lowers GobolIR to native machine code via Cranelift ObjectModule.
//
// Supports the Gobol grammar: variables (var/val) with type inference, all
// primitive types (int/float/bool/str), arithmetic & comparison operators,
// control flow (if/else, while, for over range/array/string, break/continue),
// functions & method calls, structs (heap-allocated, field access), arrays
// (via a small runtime), string concatenation, and casts.
use crate::environment::DataType;
use crate::ir::*;
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    self, AbiParam, BlockArg, Inst, InstBuilder, MemFlagsData, StackSlot, StackSlotData,
    StackSlotKind, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::{HashMap, HashSet};

/// How a net struct-intrinsic should be lowered: either building a
/// `Result<_, str>` value in code, exposing a raw loaded payload value, or
/// calling the C function directly as a plain primitive-returning intrinsic.
#[derive(Clone, Copy, PartialEq)]
enum NetIntrinsicKind {
    /// Returns a `Result<OkPayload, str>` to be constructed here.
    Result_(OkPayload),
    /// Returns a raw value that is a struct instance (used when the gobol
    /// signature already returns the struct directly rather than a Result).
    ValuePayload(OkPayload),
    /// Plain primitive return (int/str/bool) — call the C func directly.
    Plain,
}

/// The type of the Ok payload inside a net-returned `Result<_, str>`.
#[derive(Clone, Copy, PartialEq)]
enum OkPayload {
    /// A TcpStream/TcpListener handle (an i64 file descriptor).
    Struct,
    /// A string / byte buffer pointer.
    Str,
    /// A byte buffer that must be converted to Gobol's array representation.
    Bytes,
    /// An integer count (bytes written, etc.).
    Int,
    /// Unit (no payload) — `Result<(), str>`.
    Unit,
}

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
    /// IR function name -> first-class callable signature.
    func_signatures: HashMap<String, DataType>,
    /// variable name -> type (per-function, reset between functions)
    var_types: HashMap<String, DataType>,
}

impl TypeResolver {
    pub fn new() -> Self {
        let mut r = TypeResolver {
            structs: HashMap::new(),
            no_gc_structs: HashSet::new(),
            func_return_types: HashMap::new(),
            func_signatures: HashMap::new(),
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

    pub fn register_function_signature(&mut self, name: &str, function: &IRFunction) {
        self.func_signatures.insert(
            name.to_string(),
            DataType::Function {
                parameters: function
                    .params
                    .iter()
                    .map(|parameter| parameter.ty.clone())
                    .collect(),
                return_type: Box::new(function.return_type.clone()),
            },
        );
    }

    /// Populate return types for all C-runtime helper functions.
    fn register_runtime_types(&mut self) {
        let entries: &[(&str, DataType)] = &[
            ("gobol_print", DataType::None_),
            ("gobol_println", DataType::None_),
            ("gobol_eprint", DataType::None_),
            ("gobol_eprintln", DataType::None_),
            ("gobol_read", DataType::Str),
            ("gobol_args_count", DataType::Int),
            ("gobol_args_get", DataType::Str),
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
        self.struct_definition(name).is_some()
    }

    pub fn is_no_gc(&self, name: &str) -> bool {
        self.no_gc_structs.contains(name)
            || self
                .no_gc_structs
                .contains(name.split('<').next().unwrap_or(name))
    }

    fn is_enum(&self, name: &str) -> bool {
        self.struct_definition(name)
            .is_some_and(|definition| definition.attributes.iter().any(|a| a == "__enum"))
    }

    fn struct_definition(&self, name: &str) -> Option<&IRStruct> {
        self.structs.get(name).or_else(|| {
            let base_name = name.split('<').next().unwrap_or(name);
            self.structs.get(base_name)
        })
    }

    // ---- layout ----

    /// Byte size of a Gobol type in memory.
    /// All types are stored in 8-byte slots for simplicity (the struct is
    /// heap-allocated and fields are accessed via I64 loads/stores).
    pub fn type_size(&self, ty: &DataType) -> i64 {
        match ty {
            DataType::None_ => 0,
            DataType::Float => 8,
            DataType::Bool => 1,
            DataType::Byte => 1,
            DataType::Int | DataType::Str | DataType::Unknown | DataType::Struct(_) => 8,
            DataType::Nullable(inner) => self.type_size(inner),
            DataType::Array(_) => 8, // array pointer
            DataType::Tuple(_) => 8, // tuple array pointer
            DataType::Reference(_) => 8,
            DataType::Pointer(_) => 8,
            DataType::Function { .. } => 8,
        }
    }

    /// Offset of `field` within `struct_name`, computed from actual field
    /// type sizes instead of a hard-coded stride.
    pub fn field_offset(&self, struct_name: &str, field: &str) -> Option<i64> {
        let s = self.struct_definition(struct_name)?;
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
        let s = self.struct_definition(struct_name);
        s.and_then(|s| s.fields.iter().find(|f| f.name == field))
            .map(|f| f.ty.clone())
            .unwrap_or(DataType::Int)
    }

    /// Total byte size of a struct (sum of all field sizes).
    pub fn struct_size(&self, struct_name: &str) -> i64 {
        self.struct_definition(struct_name)
            .map(|s| s.fields.iter().map(|f| self.type_size(&f.ty)).sum())
            .unwrap_or(0)
    }

    /// Return (name, type) pairs for all fields of a struct, if it exists.
    pub fn struct_fields(&self, struct_name: &str) -> Option<Vec<(String, DataType)>> {
        self.struct_definition(struct_name).map(|s| {
            s.fields
                .iter()
                .map(|f| (f.name.clone(), f.ty.clone()))
                .collect()
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

    fn func_signature(&self, name: &str) -> Option<DataType> {
        self.func_signatures.get(name).cloned().or_else(|| {
            let short = name.rsplit("::").next().unwrap_or(name);
            self.func_signatures.get(short).cloned()
        })
    }

    /// Return type of an intrinsic arithmetic method on a primitive type.
    /// These methods (add, sub, mul, div, rem, eq, ne, lt, gt, le, ge) are the
    /// trait-based operator implementations for built-in numeric types.
    fn intrinsic_method_return_type(&self, method: &str, obj_ty: &DataType) -> Option<DataType> {
        let value_ty = match obj_ty {
            DataType::Reference(inner) => inner.as_ref(),
            _ => obj_ty,
        };
        let is_int = matches!(value_ty, DataType::Int | DataType::Byte);
        let is_float = matches!(value_ty, DataType::Float);
        if !is_int && !is_float {
            return None;
        }
        match method {
            "add" | "sub" | "mul" | "div" | "rem" => Some(value_ty.clone()),
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
            (DataType::Array(_), "len") => Some(DataType::Int),
            (DataType::Array(element), "get") => Some((**element).clone()),
            (DataType::Array(_), "add") => Some(DataType::None_),
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
            IRExpr::Literal(LitValue::Byte(_)) => DataType::Byte,
            IRExpr::Variable(name) => self.var_type(name),
            IRExpr::StructLiteral { name, .. } => DataType::Struct(name.clone()),
            IRExpr::ArrayLiteral(elems) => {
                let _ = elems.first().map(|e| self.infer_type(e));
                DataType::Unknown
            }
            IRExpr::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                let obj_ty = self.infer_type(object);
                if method == "take" {
                    if let Some(payload_ty) = self.net_result_payload_type(object) {
                        return payload_ty;
                    }
                }
                if method == "add"
                    && (self.contains_str(object) || args.iter().any(|arg| self.contains_str(arg)))
                {
                    return DataType::Str;
                }
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
                if matches!(
                    op.as_str(),
                    "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||"
                ) {
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
                let operand_type = self.infer_type(operand);
                match op.as_str() {
                    "&" => DataType::Reference(Box::new(operand_type)),
                    "*" => match operand_type {
                        DataType::Pointer(inner) | DataType::Reference(inner) => *inner,
                        _ => DataType::Unknown,
                    },
                    _ => operand_type,
                }
            }
            IRExpr::Cast { target, .. } | IRExpr::Bitcast { target, .. } => target.clone(),
            IRExpr::MemberAccess { object, member } => {
                match self.infer_type(object) {
                    DataType::Struct(sname) => return self.field_type(&sname, member),
                    DataType::Tuple(elements) => {
                        if let Ok(index) = member.parse::<usize>() {
                            return elements.get(index).cloned().unwrap_or(DataType::Unknown);
                        }
                    }
                    _ => {}
                }
                DataType::Int
            }
            IRExpr::ArrayIndex { array, .. } => match self.infer_type(array) {
                DataType::Str => DataType::Str,
                DataType::Array(element) => *element,
                _ => DataType::Int,
            },
            IRExpr::Assignment { target, .. } => self.infer_type(target),
            IRExpr::FuncRef(name) => self.func_signature(name).unwrap_or(DataType::Unknown),
            IRExpr::IndirectCall { callee, .. } => match self.infer_type(callee) {
                DataType::Function { return_type, .. } => *return_type,
                _ => DataType::Unknown,
            },
            IRExpr::None => DataType::None_,
        }
    }

    fn net_result_payload_type(&self, expr: &IRExpr) -> Option<DataType> {
        match expr {
            IRExpr::Call { func, .. } => {
                match func.rsplit("::").take(2).collect::<Vec<_>>().as_slice() {
                    ["connect", "TcpStream"] => Some(DataType::Struct("TcpStream".to_string())),
                    ["bind", "TcpListener"] => Some(DataType::Struct("TcpListener".to_string())),
                    _ => None,
                }
            }
            IRExpr::MethodCall { method, .. } => match method.as_str() {
                "recv_bytes" | "recv_exact" => Some(DataType::Array(Box::new(DataType::Byte))),
                "send_str" | "send_bytes" => Some(DataType::Int),
                "local_addr" | "remote_addr" => Some(DataType::Str),
                "close" | "set_read_timeout" | "set_write_timeout" | "set_keepalive" => {
                    Some(DataType::None_)
                }
                "accept" => Some(DataType::Struct("TcpStream".to_string())),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn contains_str(&self, e: &IRExpr) -> bool {
        match e {
            IRExpr::Literal(LitValue::Str(_)) => true,
            IRExpr::Variable(name) => match self.var_types.get(name) {
                Some(DataType::Str) => true,
                Some(DataType::Reference(inner)) => matches!(inner.as_ref(), DataType::Str),
                _ => false,
            },
            IRExpr::Cast { target, expr } | IRExpr::Bitcast { target, expr } => {
                matches!(target, DataType::Str) || self.contains_str(expr)
            }
            IRExpr::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                method.contains("str")
                    || (method == "add"
                        && (self.contains_str(object)
                            || args.iter().any(|arg| self.contains_str(arg))))
            }
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
    ///
    /// `long` is NOT used for string parameters: on Windows (LLP64) MSVC's
    /// `long` is 32-bit while a pointer is 64-bit, so `(const char*)`-casting a
    /// `long` stub argument would truncate the pointer. Strings are declared
    /// directly as `const char*` so no cast is needed in the forwarding call.
    fn c_param_type(dt: &DataType) -> &'static str {
        if matches!(dt, DataType::Str) {
            "const char*"
        } else {
            Self::c_type(dt)
        }
    }

    /// Map a Gobol type to the C parameter type used in the stub.
    fn c_type(dt: &DataType) -> &'static str {
        match dt {
            DataType::Float => "double",
            DataType::Bool => "int",
            DataType::None_ => "void",
            DataType::Byte => "unsigned char",
            DataType::Int
            | DataType::Str
            | DataType::Unknown
            | DataType::Struct(_)
            | DataType::Nullable(_)
            | DataType::Pointer(_)
            | DataType::Reference(_)
            | DataType::Array(_)
            | DataType::Tuple(_)
            | DataType::Function { .. } => "long",
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
            .map(|(i, dt)| format!("{} a{}", Self::c_param_type(dt), i))
            .collect();
        let params_joined = if params.is_empty() {
            "void".to_string()
        } else {
            params.join(", ")
        };
        // Forward each argument. String parameters are already declared as
        // `const char*` (see `c_param_type`), so no cast is required here.
        // This avoids truncating pointers on Windows/LLP64, where `long` is
        // 32-bit and pointers are 64-bit.
        let forward_args: Vec<String> = (0..self.arity).map(|i| format!("a{}", i)).collect();
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
    /// Module-level mutable `var` globals. name -> (data slot, type, initializer).
    /// Each global owns a shared 8-byte data slot so all functions see the
    /// same storage (fixed `var` mutation across function calls).
    global_vars: HashMap<String, (cranelift_module::DataId, DataType, Option<IRExpr>)>,
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
    addressable_slots: HashMap<String, StackSlot>,
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
    /// Struct whose `new` method is currently being compiled, if any.
    current_constructor: Option<String>,
    /// Custom linker entry symbol, emitted without the usual Gobol prefix.
    entry_point: Option<String>,
    debugger_enabled: bool,
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
                self.type_resolver
                    .register_function(&m.name, m.return_type.clone());
                self.type_resolver.register_function_signature(&m.name, m);
                // Also register with struct prefix for operator desugaring
                let prefixed = format!("{}::{}", imp.struct_name, m.name);
                self.type_resolver
                    .register_function(&prefixed, m.return_type.clone());
                self.type_resolver.register_function_signature(&prefixed, m);
            }
        }
        // Register user function return types.
        for f in &ir.functions {
            if !f.is_main {
                self.type_resolver
                    .register_function(&f.name, f.return_type.clone());
            }
            self.type_resolver.register_function_signature(&f.name, f);
            // Collect variadic extern "C" functions — these use per-arity stubs
            // instead of a single fixed signature.
            if f.is_variadic {
                self.variadic_funcs.insert(f.name.clone());
            }
        }

        // Declare runtime functions (imports).
        self.declare_runtime_functions();

        // Declare shared storage for module-level mutable `var` globals so
        // every function (including main) reads/writes the same slot.
        self.declare_global_slots(ir);

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
        let mut sig = self.module.make_signature();
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_read", sig);

        // i64 gobol_args_count()
        let mut sig = self.module.make_signature();
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_args_count", sig);

        // char* gobol_args_get(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_args_get", sig);

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

        // void gobol_debug_hook(i64 source_line)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        self.declare_import("gobol_debug_hook", sig);

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
        // i64 gobol_tcp_bind(const char*, i64, i64, i8, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I8));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_bind", sig);

        // i64 gobol_tcp_connect(const char*, i64, i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_connect", sig);

        // i64 gobol_tcp_send_str(i64, const char*, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_send_str", sig);

        // i64 gobol_tcp_send_bytes(i64, const unsigned char*, i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_send_bytes", sig);

        // unsigned char* gobol_tcp_recv_bytes(i64, i64, i64*, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_recv_bytes", sig);

        // unsigned char* gobol_tcp_recv_exact(i64, i64, i64*, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_recv_exact", sig);

        // i64 gobol_tcp_close(i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_close", sig);

        // i64 gobol_tcp_set_read_timeout(i64, i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_set_read_timeout", sig);

        // i64 gobol_tcp_set_write_timeout(i64, i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_set_write_timeout", sig);

        // i64 gobol_tcp_set_keepalive(i64, i64, i64, i64, i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_set_keepalive", sig);

        // char* gobol_tcp_local_addr(i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_local_addr", sig);

        // char* gobol_tcp_remote_addr(i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_remote_addr", sig);

        // i64 gobol_tcp_is_alive(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_is_alive", sig);

        // i64 gobol_tcp_listener_close(i64, char**)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_tcp_listener_close", sig);

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

    fn declare_user_function(
        &mut self,
        f: &IRFunction,
        occurrence_idx: usize,
    ) -> Result<(), String> {
        let is_extern = f.attributes.contains(&"extern".to_string());
        let arity = f.params.len();
        // Variadic extern "C" functions are never declared with a fixed
        // signature — per-arity stubs are declared lazily at call sites.
        // We still register the name in `func_symbols` so call resolution
        // can find it, mapping it to the bare name as a sentinel.
        if is_extern && f.is_variadic {
            self.func_symbols
                .insert((f.name.clone(), arity), f.name.clone());
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
                None => f.name.rsplit("::").next().unwrap_or(&f.name).to_string(),
            }
        } else if self.entry_point.as_deref() == Some(f.name.as_str()) {
            f.name.clone()
        } else {
            Self::func_symbol(&f.name, arity)
        };
        self.func_symbols
            .insert((f.name.clone(), arity), sym.clone());

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
                    self.func_symbols
                        .insert((f.name.clone(), arity), candidate.clone());
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
            self.func_overload_symbols
                .insert((f.name.clone(), arity, occurrence_idx), sym.clone());
        }

        let mut sig = self.module.make_signature();
        for p in &f.params {
            sig.params
                .push(AbiParam::new(self.data_type_to_clif(&p.ty)?));
        }
        // void functions have no return slot
        if !matches!(f.return_type, DataType::None_) {
            sig.returns
                .push(AbiParam::new(self.data_type_to_clif(&f.return_type)?));
        }
        let linkage = if is_extern {
            Linkage::Import
        } else {
            Linkage::Export
        };
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

    fn compile_function(
        &mut self,
        ir_func: &IRFunction,
        occurrence_idx: usize,
    ) -> Result<(), String> {
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
        self.current_constructor = ir_func
            .name
            .rsplit_once("::")
            .filter(|(_, method)| *method == "new")
            .map(|(struct_name, _)| {
                struct_name
                    .split('<')
                    .next()
                    .unwrap_or(struct_name)
                    .to_string()
            });
        // Resolve the exact symbol used during the declare pass. Overloaded
        // methods with the same (name, arity) were disambiguated with a
        // numeric suffix, so we must look up via the occurrence index.
        let arity = ir_func.params.len();
        let sym = self
            .func_overload_symbols
            .get(&(ir_func.name.clone(), arity, occurrence_idx))
            .cloned()
            .unwrap_or_else(|| Self::func_symbol(&ir_func.name, arity));
        let func_id = *self
            .func_ids
            .get(&sym)
            .ok_or_else(|| format!("missing func {}", sym))?;

        let mut ctx = self.module.make_context();
        // rebuild signature (matches declare_user_function)
        for p in &ir_func.params {
            ctx.func
                .signature
                .params
                .push(AbiParam::new(self.data_type_to_clif(&p.ty)?));
        }
        if !matches!(ir_func.return_type, DataType::None_) {
            ctx.func
                .signature
                .returns
                .push(AbiParam::new(self.data_type_to_clif(&ir_func.return_type)?));
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
                let current_block = bcx.current_block();
                let last_inst = current_block.and_then(|block| bcx.func.layout.last_inst(block));
                let current_terminated = last_inst.map_or(false, |inst| {
                    let opcode = bcx.func.dfg.insts[inst].opcode();
                    opcode.is_terminator() || format!("{:?}", opcode).contains("return")
                });
                if !self.diverged
                    && !current_terminated
                    && !bcx.is_unreachable()
                    && !self.current_func_has_attr("naked")
                {
                    self.emit_default_return(&mut bcx);
                }
                bcx.seal_all_blocks();
                bcx.finalize(self.module.target_config());
            }
            self.fn_ctx = fn_ctx;
        }

        if let Err(e) = ctx.verify(self.module.isa().flags()) {
            eprintln!("=== Detailed verifier error in {} ===\n{}", sym, e);
        }
        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| {
                eprintln!("=== Verifier error in {} ===\n{}\n{:?}", sym, e, ctx.func);
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
        self.func_symbols
            .insert(("main".to_string(), 0), sym.clone());
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

                // Initialize module-level mutable `var` globals before running
                // any user code in main.
                self.translate_global_inits(&mut bcx)?;
                self.diverged = false;

                if let Some(body) = &ir_func.body {
                    self.translate_block(&mut bcx, body)?;
                }
                // main defaults to return 0 if no explicit return.
                let current_block = bcx.current_block();
                let current_terminated = current_block
                    .and_then(|block| bcx.func.layout.last_inst(block))
                    .map_or(false, |inst| {
                        bcx.func.dfg.insts[inst].opcode().is_terminator()
                    });
                if current_block.is_some() && !current_terminated {
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
        self.addressable_slots.clear();
        self.type_resolver.reset_vars();
        self.var_counter = 0;
        self.loop_stack.clear();
        self.return_type = return_type;
        self.diverged = false;
        self.current_func_attributes.clear();
        self.current_constructor = None;
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
            DataType::Bool => {
                let z = bcx.ins().iconst(types::I8, 0);
                bcx.ins().return_(&[z]);
            }
            _ => {
                let z = bcx.ins().iconst(types::I64, 0);
                bcx.ins().return_(&[z]);
            }
        }
    }
}

#[path = "cranelift/expressions.rs"]
mod expressions;
#[path = "cranelift/statements.rs"]
mod statements;

impl CraneliftBackend {
    fn translate_member_access(
        &mut self,
        bcx: &mut FunctionBuilder,
        object: &IRExpr,
        member: &str,
    ) -> Result<ir::Value, String> {
        let obj_ty = self.type_resolver.infer_type(object);
        if member == "_tag" || member.starts_with('_') && member[1..].parse::<i64>().is_ok() {
            let obj_val = self.translate_expr(bcx, object)?;
            let offset = if member == "_tag" {
                0
            } else {
                member[1..].parse::<i64>().unwrap_or(0) * 8 + 8
            };
            let addr = self.field_addr(bcx, obj_val, offset);
            return Ok(self.call_runtime(bcx, "gobol_mem_load", &[addr]));
        }
        if let Ok(index) = member.parse::<i64>() {
            let arr = self.translate_expr(bcx, object)?;
            let idx = bcx.ins().iconst(types::I64, index);
            return Ok(self.call_runtime(bcx, "gobol_array_get", &[arr, idx]));
        }
        if let DataType::Struct(sname) = &obj_ty {
            let obj_val = self.translate_expr(bcx, object)?;
            if let Some(off) = self.type_resolver.field_offset(sname, member) {
                let addr = self.field_addr(bcx, obj_val, off);
                let value = self.call_runtime(bcx, "gobol_mem_load", &[addr]);
                return Ok(match self.type_resolver.field_type(sname, member) {
                    DataType::Float => bcx.ins().bitcast(types::F64, MemFlagsData::new(), value),
                    DataType::Bool | DataType::Byte => bcx.ins().ireduce(types::I8, value),
                    _ => value,
                });
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
        if self.current_constructor.as_deref() != Some(name)
            && self.constructors.get(name).copied().unwrap_or(false)
        {
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
                    let offset = self
                        .type_resolver
                        .field_offset(name, field_name)
                        .unwrap_or(0);
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
        if matches!((&src, target), (DataType::Array(inner), DataType::Str) if **inner == DataType::Byte)
        {
            return Ok(self.byte_array_to_string(bcx, v));
        }
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
            (DataType::Int, DataType::Str) => self.call_runtime(bcx, "gobol_str_int", &[v]),
            (DataType::Float, DataType::Str) => self.call_runtime(bcx, "gobol_str_float", &[v]),
            (DataType::Bool, DataType::Str) => self.call_runtime(bcx, "gobol_str_bool", &[v]),
            (DataType::Str, DataType::Int) => self.translate_str_to_int(bcx, v),
            _ => v,
        })
    }

    fn translate_str_to_int(&mut self, bcx: &mut FunctionBuilder, value: ir::Value) -> ir::Value {
        let index_var = bcx.declare_var(types::I64);
        let magnitude_var = bcx.declare_var(types::I64);
        let negative_var = bcx.declare_var(types::I64);
        let digit_count_var = bcx.declare_var(types::I64);

        let zero = bcx.ins().iconst(types::I64, 0);
        bcx.def_var(index_var, zero);
        bcx.def_var(magnitude_var, zero);
        bcx.def_var(negative_var, zero);
        bcx.def_var(digit_count_var, zero);

        let negative_sign = bcx.create_block();
        let sign_check = bcx.create_block();
        let positive_sign = bcx.create_block();
        let loop_block = bcx.create_block();
        let non_null = bcx.create_block();
        let digit_block = bcx.create_block();
        let overflow_check = bcx.create_block();
        let digit_limit_check = bcx.create_block();
        let accumulate = bcx.create_block();
        let end_block = bcx.create_block();
        let return_block = bcx.create_block();
        let trap_invalid = bcx.create_block();
        let trap_overflow = bcx.create_block();

        let first_byte = bcx.ins().load(types::I8, MemFlagsData::trusted(), value, 0);
        let first_byte = bcx.ins().uextend(types::I64, first_byte);
        let is_negative = bcx.ins().icmp_imm_u(IntCC::Equal, first_byte, b'-' as i64);
        bcx.ins()
            .brif(is_negative, negative_sign, &[], sign_check, &[]);

        bcx.switch_to_block(negative_sign);
        let one = bcx.ins().iconst(types::I64, 1);
        bcx.def_var(index_var, one);
        bcx.def_var(negative_var, one);
        bcx.ins().jump(loop_block, &[]);

        bcx.switch_to_block(sign_check);
        let is_positive = bcx.ins().icmp_imm_u(IntCC::Equal, first_byte, b'+' as i64);
        bcx.ins()
            .brif(is_positive, positive_sign, &[], loop_block, &[]);

        bcx.switch_to_block(positive_sign);
        let one = bcx.ins().iconst(types::I64, 1);
        bcx.def_var(index_var, one);
        bcx.ins().jump(loop_block, &[]);

        bcx.switch_to_block(loop_block);
        let index = bcx.use_var(index_var);
        let address = bcx.ins().iadd(value, index);
        let byte = bcx
            .ins()
            .load(types::I8, MemFlagsData::trusted(), address, 0);
        let byte = bcx.ins().uextend(types::I64, byte);
        let is_terminator = bcx.ins().icmp_imm_u(IntCC::Equal, byte, 0);
        bcx.ins().brif(is_terminator, end_block, &[], non_null, &[]);

        bcx.switch_to_block(non_null);
        let at_least_zero = bcx
            .ins()
            .icmp_imm_u(IntCC::UnsignedGreaterThanOrEqual, byte, 48);
        let at_most_nine = bcx
            .ins()
            .icmp_imm_u(IntCC::UnsignedLessThanOrEqual, byte, 57);
        let is_digit = bcx.ins().band(at_least_zero, at_most_nine);
        bcx.ins()
            .brif(is_digit, digit_block, &[], trap_invalid, &[]);

        bcx.switch_to_block(digit_block);
        let ascii_zero = bcx.ins().iconst(types::I64, 48);
        let digit = bcx.ins().isub(byte, ascii_zero);
        let magnitude = bcx.use_var(magnitude_var);
        let above_maximum_prefix =
            bcx.ins()
                .icmp_imm_u(IntCC::UnsignedGreaterThan, magnitude, 922337203685477580);
        bcx.ins().brif(
            above_maximum_prefix,
            trap_overflow,
            &[],
            overflow_check,
            &[],
        );

        bcx.switch_to_block(overflow_check);
        let at_maximum_prefix = bcx
            .ins()
            .icmp_imm_u(IntCC::Equal, magnitude, 922337203685477580);
        bcx.ins()
            .brif(at_maximum_prefix, digit_limit_check, &[], accumulate, &[]);

        bcx.switch_to_block(digit_limit_check);
        let negative = bcx.use_var(negative_var);
        let negative_limit = bcx.ins().icmp_imm_u(IntCC::NotEqual, negative, 0);
        let negative_maximum = bcx.ins().iconst(types::I64, 8);
        let positive_maximum = bcx.ins().iconst(types::I64, 7);
        let maximum_digit = bcx
            .ins()
            .select(negative_limit, negative_maximum, positive_maximum);
        let digit_exceeds_limit = bcx
            .ins()
            .icmp(IntCC::UnsignedGreaterThan, digit, maximum_digit);
        bcx.ins()
            .brif(digit_exceeds_limit, trap_overflow, &[], accumulate, &[]);

        bcx.switch_to_block(accumulate);
        let magnitude = bcx.use_var(magnitude_var);
        let scaled = bcx.ins().imul_imm_s(magnitude, 10);
        let magnitude = bcx.ins().iadd(scaled, digit);
        bcx.def_var(magnitude_var, magnitude);
        let index = bcx.use_var(index_var);
        let index = bcx.ins().iadd_imm_s(index, 1);
        bcx.def_var(index_var, index);
        let one = bcx.ins().iconst(types::I64, 1);
        bcx.def_var(digit_count_var, one);
        bcx.ins().jump(loop_block, &[]);

        bcx.switch_to_block(end_block);
        let digit_count = bcx.use_var(digit_count_var);
        let has_digits = bcx.ins().icmp_imm_u(IntCC::NotEqual, digit_count, 0);
        bcx.ins()
            .brif(has_digits, return_block, &[], trap_invalid, &[]);

        bcx.switch_to_block(trap_invalid);
        bcx.ins().trap(ir::TrapCode::BAD_CONVERSION_TO_INTEGER);

        bcx.switch_to_block(trap_overflow);
        bcx.ins().trap(ir::TrapCode::INTEGER_OVERFLOW);

        bcx.switch_to_block(return_block);
        let magnitude = bcx.use_var(magnitude_var);
        let negative = bcx.use_var(negative_var);
        let is_negative = bcx.ins().icmp_imm_u(IntCC::NotEqual, negative, 0);
        let zero = bcx.ins().iconst(types::I64, 0);
        let negated = bcx.ins().isub(zero, magnitude);
        let result = bcx.ins().select(is_negative, negated, magnitude);

        bcx.seal_block(negative_sign);
        bcx.seal_block(sign_check);
        bcx.seal_block(positive_sign);
        bcx.seal_block(loop_block);
        bcx.seal_block(non_null);
        bcx.seal_block(digit_block);
        bcx.seal_block(overflow_check);
        bcx.seal_block(digit_limit_check);
        bcx.seal_block(accumulate);
        bcx.seal_block(end_block);
        bcx.seal_block(return_block);
        bcx.seal_block(trap_invalid);
        bcx.seal_block(trap_overflow);

        result
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
                    let s = self
                        .to_string_value(bcx, arg)
                        .unwrap_or_else(|_| bcx.ins().iconst(types::I64, 0));
                    self.call_runtime(bcx, rt, &[s]);
                }
                bcx.ins().iconst(types::I64, 0)
            }
            "gobol_read" => self.call_runtime(bcx, rt, &[]),
            _ => {
                // generic: pass args through
                let mut vals = self.translate_args(bcx, args).unwrap_or_default();
                match rt {
                    "gobol_array_add" if args.len() > 1 && vals.len() > 1 => {
                        vals[1] = self.to_storage_word(bcx, vals[1], &args[1]);
                    }
                    "gobol_array_set" if args.len() > 2 && vals.len() > 2 => {
                        vals[2] = self.to_storage_word(bcx, vals[2], &args[2]);
                    }
                    "gobol_mem_store" if args.len() > 1 && vals.len() > 1 => {
                        vals[1] = self.to_storage_word(bcx, vals[1], &args[1]);
                    }
                    _ => {}
                }
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
        let mut word_args = Vec::with_capacity(args.len());
        for (index, &arg) in args.iter().enumerate() {
            let expects_i8 = (name == "gobol_tcp_bind" && index == 3)
                || (name == "gobol_str_bool" && index == 0);
            if !expects_i8 && bcx.func.dfg.value_type(arg) == types::I8 {
                word_args.push(bcx.ins().uextend(types::I64, arg));
            } else {
                word_args.push(arg);
            }
        }
        let call = bcx.ins().call(fref, &word_args);
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
        matches!(
            name,
            "gobol_print"
                | "gobol_println"
                | "gobol_eprint"
                | "gobol_eprintln"
                | "gobol_array_add"
                | "gobol_array_set"
                | "gobol_mem_store"
                | "gobol_fs_close"
                | "gobol_tcp_close"
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

    // ==================== global variables ====================
    //
    // Module-level mutable `var` declarations are lowered to a shared 8-byte
    // data slot so all functions read/write the same storage. This makes
    // `var counter` mutated inside one function visible to every other
    // function (previously each function treated it as its own local).

    /// Declare a zero-initialized 8-byte data slot for each module-level
    /// mutable `var` and remember (slot, type, initializer). Called once at
    /// compile time; the slot address is shared by every function.
    fn declare_global_slots(&mut self, ir: &GobolIR) {
        for (i, (name, ty, init)) in ir.globals.iter().enumerate() {
            let data_name = format!("gbl_global_{}_{}", i, name.replace("::", "_"));
            // writable=true so the slot lands in the writable `.data` segment —
            // globals are mutated at runtime (unlike string literals, which are
            // stored in read-only data and never written).
            let data_id = match self
                .module
                .declare_data(&data_name, Linkage::Local, true, false)
            {
                Ok(id) => id,
                Err(e) => {
                    eprintln!("declare global data failed: {}", e);
                    continue;
                }
            };
            let mut desc = DataDescription::new();
            // 8 zero bytes: holds i64 / f64 / a pointer, zero-initialized.
            desc.define(vec![0u8; 8].into_boxed_slice());
            if let Err(e) = self.module.define_data(data_id, &desc) {
                eprintln!("define global data failed: {}", e);
                continue;
            }
            self.global_vars
                .insert(name.clone(), (data_id, ty.clone(), init.clone()));
        }
    }

    /// Runtime address (i64 pointer) of a global variable's data slot.
    fn global_addr(&mut self, bcx: &mut FunctionBuilder, name: &str) -> ir::Value {
        let data_id = self.global_vars.get(name).map(|g| g.0).unwrap_or_else(|| {
            // Shouldn't happen: force a dummy slot so compilation continues.
            eprintln!("internal: missing global slot for {}", name);
            let dummy = self
                .module
                .declare_data("gbl_missing", Linkage::Local, false, false)
                .map_err(|e| eprintln!("{}", e))
                .unwrap();
            dummy
        });
        let global = self.module.declare_data_in_func(data_id, &mut bcx.func);
        bcx.ins().symbol_value(types::I64, global)
    }

    fn global_type(&self, name: &str) -> DataType {
        self.global_vars
            .get(name)
            .map(|g| g.1.clone())
            .unwrap_or(DataType::Int)
    }

    /// Read a global variable, producing a value of its declared type.
    fn translate_global_read(&mut self, bcx: &mut FunctionBuilder, name: &str) -> ir::Value {
        let ty = self.global_type(name);
        let clif_ty = self.data_type_to_clif(&ty).unwrap_or(types::I64);
        let addr = self.global_addr(bcx, name);
        let flags = cranelift_codegen::ir::MemFlagsData::trusted();
        bcx.ins().load(clif_ty, flags, addr, 0)
    }

    /// Store a value into a global variable, coercing `value` (whose source
    /// type is `from`) to the global's declared type.
    fn translate_global_write(
        &mut self,
        bcx: &mut FunctionBuilder,
        name: &str,
        value: ir::Value,
        from: &DataType,
    ) -> Result<(), String> {
        let ty = self.global_type(name);
        let v = self.coerce(bcx, value, from, &ty)?;
        let addr = self.global_addr(bcx, name);
        let flags = cranelift_codegen::ir::MemFlagsData::trusted();
        bcx.ins().store(flags, v, addr, 0);
        Ok(())
    }

    /// Evaluate and store every global's initializer. Called at the top of
    /// main so globals are initialized before user code runs. Globals without
    /// an initializer keep their zero-filled slot.
    fn translate_global_inits(&mut self, bcx: &mut FunctionBuilder) -> Result<(), String> {
        let names: Vec<String> = self.global_vars.keys().cloned().collect();
        for name in names {
            if let Some(init) = self.global_vars.get(&name).and_then(|g| g.2.clone()) {
                let from = self.type_resolver.infer_type(&init);
                let v = self.translate_expr(bcx, &init)?;
                self.translate_global_write(bcx, &name, v, &from)?;
            }
        }
        Ok(())
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

    fn to_bool(&self, bcx: &mut FunctionBuilder, v: ir::Value, ty: &DataType) -> ir::Value {
        if matches!(ty, DataType::Bool) {
            return v;
        }
        let value_type = bcx.func.dfg.value_type(v);
        match value_type {
            types::F32 => {
                let zero = bcx.ins().f32const(0.0);
                bcx.ins().fcmp(FloatCC::NotEqual, v, zero)
            }
            types::F64 => {
                let zero = bcx.ins().f64const(0.0);
                bcx.ins().fcmp(FloatCC::NotEqual, v, zero)
            }
            _ => {
                let zero = bcx.ins().iconst(value_type, 0);
                bcx.ins().icmp(IntCC::NotEqual, v, zero)
            }
        }
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
            (DataType::Byte, DataType::Int) => bcx.ins().uextend(types::I64, v),
            (DataType::Int, DataType::Byte) => bcx.ins().ireduce(types::I8, v),
            (DataType::Byte, DataType::Bool) => v,
            (DataType::Bool, DataType::Byte) => v,
            (DataType::Int, DataType::Float) => bcx.ins().fcvt_from_sint(types::F64, v),
            (DataType::Float, DataType::Int) => bcx.ins().fcvt_to_sint(types::I64, v),
            (DataType::Bool, DataType::Int) => bcx.ins().uextend(types::I64, v),
            (DataType::Int, DataType::Bool) => bcx.ins().ireduce(types::I8, v),
            _ => v,
        })
    }

    fn bitcast_to(&self, bcx: &mut FunctionBuilder, v: ir::Value, ty: ir::Type) -> ir::Value {
        let from = bcx.func.dfg.value_type(v);
        if from == ty {
            return v;
        }
        if from.is_int() && ty.is_int() {
            return if from.bits() > ty.bits() {
                bcx.ins().ireduce(ty, v)
            } else {
                bcx.ins().uextend(ty, v)
            };
        }
        v
    }

    fn default_value(&mut self, bcx: &mut FunctionBuilder, ty: &DataType) -> ir::Value {
        match ty {
            DataType::Float => bcx.ins().f64const(0.0),
            DataType::Bool => bcx.ins().iconst(types::I8, 0),
            DataType::Byte => bcx.ins().iconst(types::I8, 0),
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
            matches!(
                self.type_resolver.var_type(name),
                DataType::Unknown | DataType::Array(_)
            )
        } else {
            matches!(expr, IRExpr::ArrayLiteral(_))
        }
    }

    // ==================== struct helpers ====================

    fn field_addr(&self, bcx: &mut FunctionBuilder, base: ir::Value, offset: i64) -> ir::Value {
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
            DataType::Byte => types::I8,
            DataType::None_ => types::INVALID,
            DataType::Unknown => types::I64,   // array pointer
            DataType::Struct(_) => types::I64, // struct pointer
            DataType::Nullable(inner) => self.data_type_to_clif(inner)?,
            DataType::Array(_) => types::I64, // array pointer
            DataType::Tuple(_) => types::I64, // tuple array pointer
            DataType::Pointer(_) => types::I64,
            DataType::Reference(_) => types::I64,
            DataType::Function { .. } => types::I64,
        })
    }

    pub(super) fn to_storage_word(
        &self,
        bcx: &mut FunctionBuilder,
        value: ir::Value,
        expr: &IRExpr,
    ) -> ir::Value {
        match self.type_resolver.infer_type(expr) {
            DataType::Float => bcx.ins().bitcast(types::I64, MemFlagsData::new(), value),
            DataType::Bool | DataType::Byte if bcx.func.dfg.value_type(value) == types::I8 => {
                bcx.ins().uextend(types::I64, value)
            }
            _ => value,
        }
    }

    fn contains_str(&self, e: &IRExpr) -> bool {
        self.type_resolver.contains_str(e)
    }
}

impl CraneliftBackend {
    /// Create an AOT backend targeting the host platform. For cross-compilation
    /// (e.g. `aarch64-unknown-none`) use [`CraneliftBackend::new_for_target`].
    pub fn new() -> Self {
        Self::new_for_target_triple(target_lexicon::HOST.clone(), 0)
            .expect("host target must be supported by Cranelift")
    }

    /// Create an AOT backend for an arbitrary target triple string such as
    /// `x86_64-pc-windows-msvc`, `aarch64-unknown-none`, or the host triple.
    /// Returns a clear error if the triple is malformed or unsupported by
    /// the installed Cranelift. Uses optimization level 0 (none); use
    /// [`CraneliftBackend::new_for_target_with_opt`] to request a higher level.
    pub fn new_for_target(target: &str) -> Result<Self, String> {
        Self::new_for_target_with_opt(target, 0)
    }

    /// Like [`CraneliftBackend::new_for_target`] but with an explicit
    /// optimization level. `opt_level` is 0 (none), 1 (speed), or ≥2 (treated
    /// as speed_and_size, the highest Cranelift level).
    pub fn new_for_target_with_opt(target: &str, opt_level: usize) -> Result<Self, String> {
        let triple: target_lexicon::Triple = target
            .parse()
            .map_err(|e| format!("Invalid target triple '{}': {}", target, e))?;
        Self::new_for_target_triple(triple, opt_level)
    }

    fn new_for_target_triple(
        mut triple: target_lexicon::Triple,
        opt_level: usize,
    ) -> Result<Self, String> {
        use cranelift_codegen::isa::lookup;
        use cranelift_codegen::settings::{self, Configurable};

        if target_is_bare_metal(&triple.to_string())
            && triple.binary_format == target_lexicon::BinaryFormat::Unknown
        {
            triple.binary_format = target_lexicon::BinaryFormat::Elf;
        }

        let triple_str = triple.to_string();
        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "true").unwrap();
        // Map the numeric level to Cranelift's three-supported opt levels.
        let opt = match opt_level {
            0 => "none",
            1 => "speed",
            _ => "speed_and_size",
        };
        flag_builder.set("opt_level", opt).unwrap();
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
            global_vars: HashMap::new(),
            constructors: HashMap::new(),
            type_resolver: TypeResolver::new(),
            variadic_funcs: std::collections::HashSet::new(),
            variadic_stubs: Vec::new(),
            variables: HashMap::new(),
            addressable_slots: HashMap::new(),
            var_counter: 0,
            loop_stack: Vec::new(),
            return_type: DataType::None_,
            diverged: false,
            current_func_attributes: Vec::new(),
            current_constructor: None,
            entry_point: None,
            debugger_enabled: false,
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
        self.debugger_enabled = opts.debugger;
        self.entry_point = opts
            .entry_point
            .as_deref()
            .filter(|entry| *entry != "main")
            .map(str::to_owned);
        self.compile_ir(ir)?;

        // Collect variadic stubs needed for `extern "C" ...` call sites.
        let stubs = std::mem::take(&mut self.variadic_stubs);

        // Produce object file from the ObjectModule.
        let product = self.module.finish();
        let obj_bytes = product
            .emit()
            .map_err(|e| format!("Object emit failed: {}", e))?;

        // Object file extension differs per platform (MSVC uses .obj).
        let obj_ext = if target_is_msvc(&opts.target) {
            "obj"
        } else {
            "o"
        };
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
            LinkerKind::BareLd => Self::link_bare_metal(&linker, &obj_path, &opts, &final_output),
        };

        // The object file (and the C stubs source) are deliberately KEPT:
        // like Cargo's intermediates, they live in the project root
        // `target/` directory next to the final binary and form the
        // compilation cache. `grape clean` (which removes `target/`)
        // reclaims them.

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
        for spec in &opts.link_specs {
            if spec.optional && !optional_link_available(spec, &opts.target) {
                continue;
            }
            for arg in link_spec_args(spec, &opts.target, false) {
                cmd.arg(arg);
            }
        }
        for spec in &opts.link_specs {
            for arg in link_spec_args(spec, &opts.target, true) {
                cmd.arg(arg);
            }
        }
        for lib in &opts.link_libs {
            cmd.arg(format!("-l{}", lib));
        }
        // libm is available on both Unix and MinGW; libpthread is Unix-only
        // (MinGW uses its own threading model).
        cmd.arg("-lm");
        if !target_is_windows(&opts.target) {
            cmd.arg("-lpthread");
        }
        // MinGW: the C runtime's `net` module reference Winsock transitively.
        // Link `ws2_32` whenever the C runtime is present (dedup with any
        // `--link-arg ws2_32` already injected by `grape`).
        if opts.runtime_c_path.is_some()
            && target_is_windows(&opts.target)
            && !opts
                .link_libs
                .iter()
                .any(|l| l.eq_ignore_ascii_case("ws2_32"))
        {
            cmd.arg("-lws2_32");
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
                out.status.code(),
                stdout,
                stderr
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
        // The cc-discovered MSVC C compiler (cl.exe). We deliberately do NOT
        // reach for clang-cl: a plain MSVC Developer Prompt / VS install may
        // not ship it. clang-cl is optional and unreliable to assume.
        let c_compiler_candidates: Vec<std::path::PathBuf> = { vec![tool.compiler.clone()] };

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
            // Resolve each include dir to ABSOLUTE, same rationale as the
            // source path in `compile_to_obj`: cl.exe runs with CWD set to
            // `build_dir`, so a relative include root like `std` would be
            // searched *there* and silently miss the SDK headers under the
            // real (source-file) directory. Absolute dirs are resolved from
            // the filesystem root and immune to that coordinate mismatch.
            let abs_of = |p: &std::path::Path| -> std::path::PathBuf {
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    std::env::current_dir().unwrap_or_default().join(p)
                }
            };
            if let Some(parent) = rt_p.parent() {
                // 如果 parent 非空，才加进去
                if !parent.as_os_str().is_empty() {
                    rt_includes.push(abs_of(parent));
                }
                if let Some(gp) = parent.parent() {
                    // 如果 grandparent 也是非空的，才加进去
                    if !gp.as_os_str().is_empty() {
                        rt_includes.push(abs_of(gp));
                    }
                }
            }
        }

        let mut extra_objs: Vec<String> = Vec::new();
        // Object files for the C runtime and variadic stubs are written to a
        // dedicated, short build dir next to the final output instead of
        // being suffixed onto `final_output`. This keeps the path
        // absolute/short and avoids the `~` short-path (RUNNER~1) problems
        // cl.exe hit when output landed directly under %TEMP% with a long,
        // dashed stem. `process::id()` guards against concurrent builds
        // clobbering each other's .obj files.
        let build_dir = std::env::current_dir()
            .map_err(|e| format!("Failed to get current dir: {}", e))?
            .join("target")
            .join("gobol_build");
        std::fs::create_dir_all(&build_dir).map_err(|e| {
            format!(
                "Failed to create build dir '{}': {}",
                build_dir.display(),
                e
            )
        })?;
        let pid = std::process::id();
        let rt_name = |suffix: &str| build_dir.join(format!("runtime_{}_{}.obj", pid, suffix));
        let stub_name = || build_dir.join(format!("stubs_{}.obj", pid));

        // Compile a C source to an .obj in `build_dir` and force/remap it to
        // the requested absolute output path. Runs the compiler with CWD set
        // to `build_dir` and emits under a SHORT relative name — cl.exe is
        // known to ghost-compile (read source, exit 0, write nothing) on
        // long absolute `/Fo` paths. The single candidate (cl.exe) is used;
        let compile_to_obj = |src: &str,
                              final_obj: &std::path::Path,
                              tag: &str|
         -> std::result::Result<(), String> {
            let obj_file_name = final_obj
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| format!("Invalid obj name '{}'", final_obj.display()))?;

            // Resolve the source path to ABSOLUTE before handing it to
            // cl.exe. The compiler runs with CWD set to `build_dir`, but a
            // relative source like `std/runtime.c` (the CWD-relative form
            // returned by find_runtime_c) is resolved *against that build
            // dir* — the file isn't there, so cl.exe reports C1083 "Cannot
            // open source file". Absolute paths are interpreted from the
            // filesystem root and are immune to this "coordinate mismatch".
            // (For `/Fo` we deliberately keep the short RELATIVE name to
            // dodge cl.exe's long-absolute-path ghost-compile.)
            let src_path = std::path::Path::new(src);
            let abs_src = if src_path.is_absolute() {
                src_path.to_path_buf()
            } else {
                std::env::current_dir()
                    .map_err(|e| format!("Failed to get current dir: {}", e))?
                    .join(src_path)
            };
            let abs_src = abs_src.to_string_lossy().into_owned();

            let mut last_diag = String::new();
            for compiler in &c_compiler_candidates {
                let mut c = std::process::Command::new(compiler);
                c.args(["/c", "/nologo", "/utf-8"]);
                for inc in &rt_includes {
                    c.arg(format!("/I{}", inc.display()));
                }
                c.arg(format!("/Fo{}", obj_file_name)).arg(&abs_src);
                for (k, v) in &tool.env {
                    c.env(k, v);
                }
                c.current_dir(&build_dir);

                let out = c.output().map_err(|e| {
                    format!(
                        "Failed to spawn {} for {} ({}): {}",
                        compiler.display(),
                        src,
                        tag,
                        e
                    )
                })?;
                let produced = build_dir.join(obj_file_name);

                if out.status.success() && produced.exists() {
                    // Force / re-map output to the requested absolute path.
                    std::fs::rename(&produced, final_obj).map_err(|e| {
                        format!(
                            "Failed to re-map '{}' -> '{}': {}\nstdout: {}\nstderr: {}",
                            produced.display(),
                            final_obj.display(),
                            e,
                            String::from_utf8_lossy(&out.stdout),
                            String::from_utf8_lossy(&out.stderr),
                        )
                    })?;
                    return Ok(());
                }

                // Record the diagnostic for this candidate and try the next.
                last_diag = if !out.status.success() {
                    format!(
                        "[{}] failed with {:?}\nstdout: {}\nstderr: {}",
                        compiler.display(),
                        out.status.code(),
                        String::from_utf8_lossy(&out.stdout),
                        String::from_utf8_lossy(&out.stderr),
                    )
                } else {
                    format!(
                        "[{}] exited 0 but produced no .obj at '{}'\nstdout: {}\nstderr: {}",
                        compiler.display(),
                        produced.display(),
                        String::from_utf8_lossy(&out.stdout),
                        String::from_utf8_lossy(&out.stderr),
                    )
                };
                let _ = std::fs::remove_file(&produced);
            }

            Err(format!(
                "Compiling {} ({}) failed with all compiler candidates.\n{}",
                src, tag, last_diag
            ))
        };

        if let Some(rt) = &opts.runtime_c_path {
            let rt_obj = rt_name("rt");
            compile_to_obj(rt, &rt_obj, "runtime")?;
            extra_objs.push(rt_obj.to_string_lossy().into_owned());
        }
        if has_stubs {
            let stub_obj = stub_name();
            compile_to_obj(stubs_src_path, &stub_obj, "stubs")?;
            extra_objs.push(stub_obj.to_string_lossy().into_owned());
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
        for spec in &opts.link_specs {
            if spec.optional && !optional_link_available(spec, &opts.target) {
                continue;
            }
            if spec.optional && !optional_link_available(spec, &opts.target) {
                continue;
            }
            for arg in link_spec_args(spec, &opts.target, false) {
                cmd.arg(arg);
            }
        }
        for lib in &opts.link_libs {
            cmd.arg(format!("{}.lib", lib));
        }
        // MSVC: the C runtime's `net` module references Winsock (socket,
        // connect, accept, …) which on Windows lives in `ws2_32.lib`, not
        // the standard C library. link.exe must be told to load it, or the
        // link fails with LNK2019 "unresolved external symbol __imp_accept"
        // etc. (≈12 symbols). Link it whenever the C runtime is present,
        // dedup with any `--link-arg ws2_32` already injected by grape —
        // same guard as the MinGW path in `link_cc_driver`.
        if opts.runtime_c_path.is_some()
            && target_is_windows(&opts.target)
            && !opts
                .link_libs
                .iter()
                .any(|l| l.eq_ignore_ascii_case("ws2_32"))
        {
            cmd.arg("ws2_32.lib");
        }
        // Add Rust toolchain lib paths so common system libs resolve.
        if let Ok(sysroot) = rust_sysroot() {
            let libroot = sysroot.join("lib");
            if libroot.exists() {
                cmd.arg(format!("/LIBPATH:{}", libroot.display()));
            }
            let rustlib = sysroot
                .join("lib")
                .join("rustlib")
                .join(&opts.target)
                .join("lib");
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

        // Keep the intermediate runtime/stub .obj files: they live in the
        // `target/` dir and form the compilation cache (grape clean reclaims
        // them). There is no failure-path cleanup on purpose — left-overs
        // under target/ are harmless and scanable via grape clean.
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Err(format!(
                "Linking failed with exit code {:?}\n--- link.exe stdout ---\n{}\n--- link.exe stderr ---\n{}",
                out.status.code(),
                stdout,
                stderr
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
    /// Structured libraries declared with `#[link(...)]`.
    pub link_specs: Vec<LinkSpec>,
    /// Custom linker script (`-T`/`/LIBPATH`-less bare-metal path). Used with
    /// `grape.toml`'s `build.link_script`.
    pub link_script: Option<String>,
    /// Custom entry symbol (e.g. `_start`). When set to anything other than
    /// `"main"`, the linker is told to use it as the entry point and the
    /// semantic checker is told not to require a `main` function.
    pub entry_point: Option<String>,
    /// Instrument executable statements with the interactive debugger hook.
    pub debugger: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkSpec {
    pub name: String,
    pub kind: String,
    pub rename: Option<String>,
    pub version: Option<String>,
    pub optional: bool,
}

impl LinkOptions {
    /// Defaults for a hosted build on the current host platform.
    pub fn host(runtime_c_path: impl Into<String>, link_libs: Vec<String>) -> Self {
        Self {
            target: host_target_string(),
            runtime_c_path: Some(runtime_c_path.into()),
            link_libs,
            link_specs: Vec::new(),
            link_script: None,
            entry_point: None,
            debugger: false,
        }
    }
}

fn link_spec_args(spec: &LinkSpec, target: &str, msvc: bool) -> Vec<String> {
    let name = spec.rename.as_deref().unwrap_or(&spec.name);
    if msvc {
        return vec![if name.to_ascii_lowercase().ends_with(".lib") {
            name.to_string()
        } else {
            format!("{}.lib", name)
        }];
    }
    if spec.kind == "framework" {
        return vec!["-framework".to_string(), spec.name.clone()];
    }
    let mut library = name.to_string();
    if target.contains("darwin") && library.starts_with("lib") {
        library = library[3..].to_string();
    }
    if spec.rename.is_none() {
        if let Some(version) = &spec.version {
            if target.contains("darwin") {
                library.push('.');
                library.push_str(version.split('.').next().unwrap_or(version));
            } else if !target.contains("windows") {
                library.push('.');
                library.push_str(version);
            }
        }
    }
    if spec.kind == "static" && library.ends_with(".a") {
        return vec![format!("-l:{}", library)];
    }
    vec![format!("-l{}", library)]
}

fn optional_link_available(spec: &LinkSpec, target: &str) -> bool {
    let requested = spec.rename.as_deref().unwrap_or(&spec.name);
    let mut candidates = Vec::new();
    if std::path::Path::new(requested).is_absolute() {
        candidates.push(std::path::PathBuf::from(requested));
    }
    for variable in ["LIBRARY_PATH", "LIB", "LD_LIBRARY_PATH"] {
        if let Ok(paths) = std::env::var(variable) {
            candidates.extend(std::env::split_paths(&paths).map(|path| path.join(requested)));
        }
    }
    if target.contains("windows") {
        candidates.push(std::path::PathBuf::from(format!("{}.lib", requested)));
    } else {
        candidates.push(std::path::PathBuf::from(format!("lib{}.so", requested)));
        candidates.push(std::path::PathBuf::from(format!("lib{}.dylib", requested)));
        candidates.push(std::path::PathBuf::from(format!("lib{}.a", requested)));
        candidates.push(std::path::PathBuf::from(requested));
    }
    candidates.iter().any(|path| path.is_file())
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

#[cfg(test)]
mod link_tests {
    use super::*;

    #[test]
    fn generic_struct_types_reuse_base_layout() {
        let mut resolver = TypeResolver::new();
        resolver.register_structs(&[IRStruct {
            name: "Vec".to_string(),
            generic_params: vec!["T".to_string()],
            fields: vec![
                IRField {
                    name: "_data".to_string(),
                    ty: DataType::Struct("T".to_string()),
                },
                IRField {
                    name: "_len".to_string(),
                    ty: DataType::Int,
                },
                IRField {
                    name: "_cap".to_string(),
                    ty: DataType::Int,
                },
            ],
            attributes: Vec::new(),
        }]);

        assert!(resolver.has_struct("Vec<T>"));
        assert_eq!(resolver.field_offset("Vec<T>", "_len"), Some(8));
        assert_eq!(resolver.field_type("Vec<T>", "_len"), DataType::Int);
        assert_eq!(resolver.struct_size("Vec<T>"), 24);
    }

    #[test]
    fn network_result_take_preserves_byte_array_type() {
        let resolver = TypeResolver::new();
        let receive = IRExpr::MethodCall {
            object: Box::new(IRExpr::Variable("socket".to_string())),
            method: "recv_bytes".to_string(),
            args: vec![],
            generic_args: vec![],
        };
        let take = IRExpr::MethodCall {
            object: Box::new(receive),
            method: "take".to_string(),
            args: vec![],
            generic_args: vec![],
        };
        assert_eq!(
            resolver.infer_type(&take),
            DataType::Array(Box::new(DataType::Byte))
        );
        let len = IRExpr::MethodCall {
            object: Box::new(take),
            method: "len".to_string(),
            args: vec![],
            generic_args: vec![],
        };
        assert_eq!(resolver.infer_type(&len), DataType::Int);
    }

    #[test]
    fn msvc_linker_is_resolved_beside_its_compiler() {
        let root =
            std::env::temp_dir().join(format!("gobol-msvc-linker-test-{}", std::process::id()));
        let compiler_dir = root.join("VC").join("bin");
        let decoy_dir = root.join("msys").join("usr").join("bin");
        std::fs::create_dir_all(&compiler_dir).unwrap();
        std::fs::create_dir_all(&decoy_dir).unwrap();
        let compiler = compiler_dir.join("cl.exe");
        let linker = compiler_dir.join("link.exe");
        std::fs::write(&compiler, []).unwrap();
        std::fs::write(&linker, []).unwrap();
        std::fs::write(decoy_dir.join("link.exe"), []).unwrap();
        let path = std::env::join_paths([decoy_dir]).unwrap();
        let tool = crate::toolchain::CcTool {
            compiler,
            is_msvc: true,
            env: vec![(std::ffi::OsString::from("PATH"), path)],
        };

        assert_eq!(msvc_linker_for_tool(&tool), Some(linker));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn renders_link_kinds_per_target() {
        let static_lib = LinkSpec {
            name: "curl".to_string(),
            kind: "static".to_string(),
            rename: Some("libcurl.a".to_string()),
            version: Some("4.8".to_string()),
            optional: false,
        };
        assert_eq!(
            link_spec_args(&static_lib, "x86_64-unknown-linux-gnu", false),
            vec!["-l:libcurl.a"]
        );

        let framework = LinkSpec {
            name: "CoreGraphics".to_string(),
            kind: "framework".to_string(),
            rename: None,
            version: None,
            optional: false,
        };
        assert_eq!(
            link_spec_args(&framework, "aarch64-apple-darwin", false),
            vec!["-framework", "CoreGraphics"]
        );

        let windows = LinkSpec {
            name: "curl".to_string(),
            kind: "static".to_string(),
            rename: Some("curl_static.lib".to_string()),
            version: Some("4.8".to_string()),
            optional: false,
        };
        assert_eq!(
            link_spec_args(&windows, "x86_64-pc-windows-msvc", true),
            vec!["curl_static.lib"]
        );
    }

    #[test]
    fn link_options_keep_structured_specs() {
        let options = LinkOptions::host("runtime.c", vec![]);
        assert!(options.link_specs.is_empty());
    }

    #[test]
    fn custom_entry_point_keeps_the_linker_symbol_name() {
        let function = |name: &str| IRFunction {
            name: name.to_string(),
            generic_params: Vec::new(),
            params: Vec::new(),
            return_type: DataType::None_,
            body: Some(IRBlock {
                statements: vec![IRStmt::Return(None)],
            }),
            is_main: false,
            is_method: false,
            struct_name: None,
            attributes: Vec::new(),
            is_variadic: false,
        };
        let ir = GobolIR {
            functions: vec![function("_start"), function("helper")],
            structs: Vec::new(),
            impls: Vec::new(),
            main_function: None,
            globals: Vec::new(),
        };
        let mut backend = CraneliftBackend::new();
        backend.entry_point = Some("_start".to_string());

        backend.compile_ir(&ir).unwrap();

        assert!(backend.func_ids.contains_key("_start"));
        assert!(backend.func_ids.contains_key("gbl_helper_0"));
    }

    #[test]
    #[cfg(target_arch = "x86_64")]
    fn bare_metal_x86_64_uses_elf_object_format() {
        CraneliftBackend::new_for_target("x86_64-unknown-none").unwrap();
    }
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
     rustup default stable-gnu"
        .to_string()
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
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{}.exe", exe));
            if candidate.is_file() {
                return Some(candidate);
            }
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

fn msvc_linker_for_tool(tool: &crate::toolchain::CcTool) -> Option<std::path::PathBuf> {
    if !tool.is_msvc {
        return None;
    }

    let compiler = if tool.compiler.is_file() {
        tool.compiler.clone()
    } else {
        let compiler_name = tool.compiler.file_name()?.to_str()?;
        which_in_env(compiler_name, &tool.env)?
    };
    let linker = compiler.with_file_name("link.exe");
    linker.is_file().then_some(linker)
}

/// Detect the linker for a target triple string. Returns the linker path and
/// the kind of link command to build.
///
/// | Target                          | Linker        | Lookup              |
/// |---------------------------------|---------------|---------------------|
/// | `x86_64-pc-windows-msvc`        | `link.exe`    | beside cl.exe, lld  |
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
        if let Some(p) = which("ld.lld") {
            return Ok((p, LinkerKind::BareLd));
        }
        if let Some(p) = which("ld") {
            return Ok((p, LinkerKind::BareLd));
        }
        return Err(format!(
            "linker not found for bare-metal target '{}': no `ld`, `ld.lld`, or `{}-linux-gnu-ld` on PATH",
            target, arch
        ));
    }

    let sysroot = rust_sysroot()?;
    let bin_dir = sysroot.join("bin");

    if target_is_msvc(target) {
        // Prefer the linker beside the discovered MSVC compiler. A bare
        // PATH search can select GNU's unrelated `link` utility (for
        // example from an MSYS/Git installation), which also exposes a
        // `link.exe` name on Windows.
        if let Ok(Some(tool)) = crate::toolchain::cc_discover(target) {
            if let Some(p) = msvc_linker_for_tool(&tool) {
                return Ok((p, LinkerKind::MsvcLink));
            }
        }
        // Fall back to the Rust toolchain's MSVC-compatible linker.
        let lld_link = bin_dir.join("lld-link.exe");
        if lld_link.exists() {
            return Ok((lld_link, LinkerKind::MsvcLink));
        }
        return Err(format!(
            "linker not found for MSVC target '{}': no compatible link.exe was found\n\
             beside the MSVC compiler and no lld-link.exe is available in the Rust sysroot.\n\
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
    // Specializations from generic monomorphization look like
    // `debug::println__str` / `io::println__int`. Strip the module prefix and
    // any generated type suffix (`__str`, `__int`, ...) before matching.
    let short = name.rsplit("::").next().unwrap_or(name);
    let base = short
        .split_once("__")
        .filter(|(prefix, _)| !prefix.is_empty())
        .map_or(short, |(prefix, _)| prefix);
    match base {
        "print" | "_print" | "gobol_print" => Some("gobol_print"),
        "println" | "_println" | "gobol_println" => Some("gobol_println"),
        "eprint" | "_eprint" | "gobol_eprint" => Some("gobol_eprint"),
        "eprintln" | "_eprintln" | "gobol_eprintln" => Some("gobol_eprintln"),
        "read" | "_read" | "gobol_read" => Some("gobol_read"),
        "__new_array" => Some("gobol_array_new_with_size"),
        "gobol_args_count" => Some("gobol_args_count"),
        "gobol_args_get" => Some("gobol_args_get"),
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
        "gobol_array_add" => Some("gobol_array_add"),
        "gobol_array_len" => Some("gobol_array_len"),
        "gobol_array_get" => Some("gobol_array_get"),
        "gobol_array_set" => Some("gobol_array_set"),
        "gobol_array_elem_addr" => Some("gobol_array_elem_addr"),
        "gobol_mem_load" => Some("gobol_mem_load"),
        "gobol_mem_store" => Some("gobol_mem_store"),
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
