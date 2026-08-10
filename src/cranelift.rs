// cranelift.rs — JIT backend. Lowers GobolIR to native machine code via Cranelift.
//
// Supports the Gobol grammar: variables (var/val) with type inference, all
// primitive types (int/float/bool/str), arithmetic & comparison operators,
// control flow (if/else, while, for over range/array/string, break/continue),
// functions & method calls, structs (heap-allocated, field access), arrays
// (via a small runtime), string concatenation, and casts.
use crate::environment::DataType;
use crate::ir::*;
use cranelift_codegen::ir::{self, types, AbiParam, Inst, InstBuilder, MemFlagsData};
use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// ==================== Runtime ====================
//
// These extern "C" functions are compiled into the host process and registered
// with the JIT so generated code can call them. They mirror std/c/__builtins__.c
// so that the JIT backend is self-contained.

#[repr(C)]
struct GobolArray {
    data: *mut i64,
    len: i64,
    cap: i64,
}

extern "C" fn gobol_print(s: *const c_char) {
    if !s.is_null() {
        unsafe {
            let bytes = CStr::from_ptr(s).to_bytes();
            use std::io::Write;
            let _ = std::io::stdout().write_all(bytes);
        }
    }
}

extern "C" fn gobol_println(s: *const c_char) {
    if !s.is_null() {
        unsafe {
            let bytes = CStr::from_ptr(s).to_bytes();
            use std::io::Write;
            let _ = std::io::stdout().write_all(bytes);
            let _ = std::io::stdout().write_all(b"\n");
        }
    }
}

extern "C" fn gobol_read() -> *mut c_char {
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    CString::new(line).unwrap_or_else(|_| CString::new("").unwrap()).into_raw()
}

extern "C" fn gobol_str_int(n: i64) -> *mut c_char {
    CString::new(n.to_string()).unwrap_or_else(|_| CString::new("").unwrap()).into_raw()
}

extern "C" fn gobol_str_float(f: f64) -> *mut c_char {
    CString::new(format!("{}", f)).unwrap_or_else(|_| CString::new("").unwrap()).into_raw()
}

extern "C" fn gobol_str_bool(b: i8) -> *mut c_char {
    let s = if b != 0 { "true" } else { "false" };
    CString::new(s).unwrap().into_raw()
}

extern "C" fn gobol_str_cat(a: *const c_char, b: *const c_char) -> *mut c_char {
    let a = if a.is_null() { "" } else { unsafe { CStr::from_ptr(a).to_str().unwrap_or("") } };
    let b = if b.is_null() { "" } else { unsafe { CStr::from_ptr(b).to_str().unwrap_or("") } };
    CString::new(format!("{}{}", a, b)).unwrap_or_else(|_| CString::new("").unwrap()).into_raw()
}

extern "C" fn gobol_str_eq(a: *const c_char, b: *const c_char) -> i8 {
    let a = if a.is_null() { "" } else { unsafe { CStr::from_ptr(a).to_str().unwrap_or("") } };
    let b = if b.is_null() { "" } else { unsafe { CStr::from_ptr(b).to_str().unwrap_or("") } };
    if a == b { 1 } else { 0 }
}

extern "C" fn gobol_alloc(size: i64) -> *mut u8 {
    let len = size.max(0) as usize;
    let mut v = vec![0u8; len];
    let ptr = v.as_mut_ptr();
    std::mem::forget(v);
    ptr
}

extern "C" fn gobol_array_new() -> *mut GobolArray {
    Box::into_raw(Box::new(GobolArray { data: std::ptr::null_mut(), len: 0, cap: 0 }))
}

extern "C" fn gobol_array_add(arr: *mut GobolArray, val: i64) {
    if arr.is_null() {
        return;
    }
    unsafe {
        let arr = &mut *arr;
        if arr.len >= arr.cap {
            let new_cap = if arr.cap == 0 { 8 } else { arr.cap * 2 };
            let mut new_data = vec![0i64; new_cap as usize];
            if !arr.data.is_null() {
                std::ptr::copy_nonoverlapping(arr.data, new_data.as_mut_ptr(), arr.len as usize);
            }
            arr.data = new_data.as_mut_ptr();
            std::mem::forget(new_data);
            arr.cap = new_cap;
        }
        arr.data.add(arr.len as usize).write(val);
        arr.len += 1;
    }
}

extern "C" fn gobol_array_len(arr: *mut GobolArray) -> i64 {
    if arr.is_null() { 0 } else { unsafe { (*arr).len } }
}

extern "C" fn gobol_array_get(arr: *mut GobolArray, i: i64) -> i64 {
    if arr.is_null() {
        return 0;
    }
    unsafe {
        let arr = &*arr;
        if i < 0 || i >= arr.len {
            0
        } else {
            arr.data.add(i as usize).read()
        }
    }
}

extern "C" fn gobol_array_set(arr: *mut GobolArray, i: i64, val: i64) {
    if arr.is_null() {
        return;
    }
    unsafe {
        let arr = &mut *arr;
        if i >= 0 && i < arr.len {
            arr.data.add(i as usize).write(val);
        }
    }
}

extern "C" fn gobol_str_len(s: *const c_char) -> i64 {
    if s.is_null() { 0 } else { unsafe { CStr::from_ptr(s).to_bytes().len() as i64 } }
}

extern "C" fn gobol_str_get(s: *const c_char, i: i64) -> i64 {
    if s.is_null() {
        return 0;
    }
    unsafe {
        let bytes = CStr::from_ptr(s).to_bytes();
        if i < 0 || i >= bytes.len() as i64 { 0 } else { bytes[i as usize] as i64 }
    }
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
    /// IR function name -> return type (user functions + runtime imports)
    func_return_types: HashMap<String, DataType>,
    /// variable name -> type (per-function, reset between functions)
    var_types: HashMap<String, DataType>,
}

impl TypeResolver {
    pub fn new() -> Self {
        let mut r = TypeResolver {
            structs: HashMap::new(),
            func_return_types: HashMap::new(),
            var_types: HashMap::new(),
        };
        r.register_runtime_types();
        r
    }

    // ---- registration ----

    pub fn register_structs(&mut self, structs: &[IRStruct]) {
        for s in structs {
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
            ("gobol_read", DataType::Str),
            ("gobol_str_int", DataType::Str),
            ("gobol_str_float", DataType::Str),
            ("gobol_str_bool", DataType::Str),
            ("gobol_str_cat", DataType::Str),
            ("gobol_str_eq", DataType::Bool),
            ("gobol_str_len", DataType::Int),
            ("gobol_str_get", DataType::Int),
            ("gobol_alloc", DataType::Int),
            ("gobol_array_new", DataType::Unknown),
            ("gobol_array_add", DataType::None_),
            ("gobol_array_len", DataType::Int),
            ("gobol_array_get", DataType::Int),
            ("gobol_array_set", DataType::None_),
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
        self.func_return_types
            .get(name)
            .cloned()
            .unwrap_or(DataType::Int)
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

// ==================== Backend ====================

pub struct CraneliftBackend<M: Module> {
    module: M,
    fn_ctx: FunctionBuilderContext,
    /// IR function name -> symbol name (e.g. "Point::new" -> "gbl_Point_new")
    func_symbols: HashMap<String, String>,
    /// symbol name -> FuncId (user-defined functions + runtime imports)
    func_ids: HashMap<String, cranelift_module::FuncId>,
    /// string literal text -> DataId
    string_data: HashMap<String, cranelift_module::DataId>,
    /// struct name -> set of constructor method names (e.g. "new")
    constructors: HashMap<String, bool>,
    /// centralised type inference / layout / return-type lookup
    type_resolver: TypeResolver,

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
}

impl<M: Module> CraneliftBackend<M> {
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
            }
        }
        // Register user function return types.
        for f in &ir.functions {
            if !f.is_main {
                self.type_resolver.register_function(&f.name, f.return_type.clone());
            }
        }

        // Declare runtime functions (imports).
        self.declare_runtime_functions();

        // First pass: declare all user functions so calls can resolve forward.
        for f in &ir.functions {
            if f.is_main {
                continue;
            }
            self.declare_user_function(f)?;
        }
        for imp in &ir.impls {
            for m in &imp.methods {
                self.declare_user_function(m)?;
            }
        }

        // Second pass: define function bodies.
        for f in &ir.functions {
            if f.is_main {
                continue;
            }
            self.compile_function(f)?;
        }
        for imp in &ir.impls {
            for m in &imp.methods {
                self.compile_function(m)?;
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

        // ptr gobol_alloc(i64)
        let mut sig = self.module.make_signature();
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_alloc", sig);

        // ptr gobol_array_new()
        let sig = self.module.make_signature();
        let mut sig = sig;
        sig.returns.push(AbiParam::new(types::I64));
        self.declare_import("gobol_array_new", sig);

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

    fn declare_user_function(&mut self, f: &IRFunction) -> Result<(), String> {
        let sym = Self::func_symbol(&f.name);
        self.func_symbols.insert(f.name.clone(), sym.clone());
        let mut sig = self.module.make_signature();
        for p in &f.params {
            sig.params.push(AbiParam::new(self.data_type_to_clif(&p.ty)?));
        }
        // void functions have no return slot
        if !matches!(f.return_type, DataType::None_) {
            sig.returns.push(AbiParam::new(self.data_type_to_clif(&f.return_type)?));
        }
        let id = self
            .module
            .declare_function(&sym, Linkage::Export, &sig)
            .map_err(|e| format!("declare {} failed: {}", sym, e))?;
        self.func_ids.insert(sym, id);
        Ok(())
    }

    /// Map an IR function name to a JIT symbol name.
    fn func_symbol(name: &str) -> String {
        format!("gbl_{}", name.replace("::", "_").replace('.', "_"))
    }

    // ==================== function compilation ====================

    fn compile_function(&mut self, ir_func: &IRFunction) -> Result<(), String> {
        self.reset_function_state(ir_func.return_type.clone());
        let sym = Self::func_symbol(&ir_func.name);
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

                // Ensure a return if the builder is still open.
                if !self.diverged {
                    self.emit_default_return(&mut bcx);
                }
                bcx.seal_all_blocks();
                bcx.finalize(self.module.target_config());
            }
            self.fn_ctx = fn_ctx;
        }

        self.module
            .define_function(func_id, &mut ctx)
            .map_err(|e| format!("define {} failed: {}", sym, e))?;
        self.module.clear_context(&mut ctx);
        Ok(())
    }

    fn compile_main(&mut self, ir_func: &IRFunction) -> Result<(), String> {
        self.reset_function_state(DataType::Int);
        // main has no parameters in IR; give it a C-friendly i64 return.
        let sym = "gbl_main".to_string();
        self.func_symbols.insert("main".to_string(), sym.clone());
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
            .map_err(|e| format!("define main failed: {}", e))?;
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
        if !self.diverged {
            bcx.ins().jump(merge_b, &[]);
        }
        bcx.switch_to_block(else_b);
        self.diverged = false;
        if let Some(eb) = else_block {
            self.translate_block(bcx, eb)?;
        }
        if !self.diverged {
            bcx.ins().jump(merge_b, &[]);
        }
        bcx.seal_block(merge_b);
        bcx.switch_to_block(merge_b);
        self.diverged = false;
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
        // Range: range(start, end[, step])  or  start..end
        if let IRExpr::Call { func, args, .. } = iterable {
            if func == "range" {
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
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let cur = bcx.use_var(iv);
        let cmp = bcx.ins().icmp(IntCC::SignedLessThan, cur, end);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(incr_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(body_b);
        self.loop_stack.push((end_b, incr_b));
        self.diverged = false;
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(incr_b, &[]);
        }

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
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let len = self.call_runtime(bcx, "gobol_array_len", &[arr_ptr]);
        let cmp = bcx.ins().icmp(IntCC::SignedLessThan, i, len);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(incr_b);
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
        let ch_var = self.declare_variable(bcx, ch_name, types::I64, &DataType::Int);
        let idx_var = self.declare_variable(bcx, &format!("__idx_{}", ch_name), types::I64, &DataType::Int);
        let zero = bcx.ins().iconst(types::I64, 0);
        bcx.def_var(ch_var, zero);
        bcx.def_var(idx_var, zero);

        let cond_b = bcx.create_block();
        let body_b = bcx.create_block();
        let incr_b = bcx.create_block();
        let end_b = bcx.create_block();

        bcx.ins().jump(cond_b, &[]);
        // cond_b sealed after the incr back-edge (see translate_while).
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let len = self.call_runtime(bcx, "gobol_str_len", &[str_ptr]);
        let cmp = bcx.ins().icmp(IntCC::SignedLessThan, i, len);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(incr_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(body_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let ch = self.call_runtime(bcx, "gobol_str_get", &[str_ptr, i]);
        bcx.def_var(ch_var, ch);
        self.loop_stack.push((end_b, incr_b));
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(incr_b, &[]);
        }

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
                    let val_ty = self.data_type_to_clif(&self.type_resolver.infer_type(value)).unwrap_or(types::I64);
                    bcx.ins().store(MemFlagsData::trusted(), val, addr, 0);
                    let _ = val_ty;
                    return Ok(());
                }
            }
        }
        // Array index assignment: arr[i] = value
        if let IRExpr::ArrayIndex { array, index } = target {
            let arr = self.translate_expr(bcx, array)?;
            let idx = self.translate_expr(bcx, index)?;
            let val = self.translate_expr(bcx, value)?;
            self.call_runtime(bcx, "gobol_array_set", &[arr, idx, val]);
            return Ok(());
        }
        // Simple variable assignment
        if let IRExpr::Variable(name) = target {
            let val = self.translate_expr(bcx, value)?;
            if let Some(var) = self.variables.get(name) {
                let v = self.coerce(bcx, val, &self.type_resolver.infer_type(value), &self.type_resolver.var_type(name))?;
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
                    Ok(bcx.use_var(*var))
                } else {
                    // Unknown variable — return 0. This keeps code generation
                    // robust against unresolved names (e.g. forward references).
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
                let arr = self.translate_expr(bcx, array)?;
                let idx = self.translate_expr(bcx, index)?;
                Ok(self.call_runtime(bcx, "gobol_array_get", &[arr, idx]))
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

        let arg_vals = self.translate_args(bcx, args)?;
        self.translate_call_with_args(bcx, func, &arg_vals)
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
            if rt == "gobol_print" || rt == "gobol_println" {
                self.call_runtime(bcx, rt, arg_vals);
                return Ok(bcx.ins().iconst(types::I64, 0));
            }
            let _rt = rt;
            return Ok(bcx.ins().iconst(types::I64, 0));
        }
        // panic(msg)
        if func == "panic" {
            return Ok(bcx.ins().iconst(types::I64, 0));
        }

        // User function lookup.
        if let Some(sym) = self.func_symbols.get(func) {
            if let Some(fid) = self.func_ids.get(sym) {
                let fret = self.type_resolver.func_return_type(func);
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
                    let self_ptr = self.call_runtime(bcx, "gobol_alloc", &[size_val]);
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
            if self.func_symbols.contains_key(&full) {
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
                _ => {}
            }
        }

        // Instance method call: obj.method(args) -> StructName_method(obj, args...)
        if let DataType::Struct(sname) = &obj_ty {
            let full = format!("{}::{}", sname, method);
            let obj_val = self.translate_expr(bcx, object)?;
            let mut vals = vec![obj_val];
            vals.append(&mut self.translate_args(bcx, args)?);
            if let Some(sym) = self.func_symbols.get(&full) {
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
                let field_ty = self.type_resolver.field_type(sname, member);
                let clif_ty = self.data_type_to_clif(&field_ty).unwrap_or(types::I64);
                let loaded = bcx.ins().load(clif_ty, MemFlagsData::trusted(), addr, 0);
                let _ = loaded;
                return Ok(self.bitcast_to(bcx, loaded, types::I64));
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
            let self_ptr = self.call_runtime(bcx, "gobol_alloc", &[size_val]);
            let mut all_args = vec![self_ptr];
            all_args.append(&mut self.translate_args(bcx, &args)?);
            return self.translate_call_with_args(bcx, &full, &all_args);
        }
        // Otherwise allocate and store fields directly.
        let size = self.type_resolver.struct_size(name);
        let size_val = bcx.ins().iconst(types::I64, size);
        let ptr = self.call_runtime(bcx, "gobol_alloc", &[size_val]);
        if let Some(off) = self.type_resolver.struct_fields(name) {
            for (field_name, field_ty) in &off {
                if let Some((_, e)) = fields.iter().find(|(n, _)| n == field_name) {
                    let v = self.translate_expr(bcx, e)?;
                    let offset = self.type_resolver.field_offset(name, field_name).unwrap_or(0);
                    let addr = self.field_addr(bcx, ptr, offset);
                    let _ = field_ty;
                    bcx.ins().store(MemFlagsData::trusted(), v, addr, 0);
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
                if self.func_symbols.contains_key(&full) {
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
        if self.func_returns_void(name) {
            let _ = call;
            bcx.ins().iconst(types::I64, 0)
        } else {
            bcx.inst_results(call)[0]
        }
    }

    fn func_returns_void(&self, name: &str) -> bool {
        matches!(name, "gobol_print" | "gobol_println" | "gobol_array_add" | "gobol_array_set")
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
        })
    }

    fn contains_str(&self, e: &IRExpr) -> bool {
        self.type_resolver.contains_str(e)
    }
}

impl CraneliftBackend<JITModule> {
    pub fn new() -> Self {
        let mut builder =
            JITBuilder::new(Box::new(cranelift_module::default_libcall_names()))
                .expect("failed to create JIT builder");
        // Register runtime symbols so generated code can call them.
        builder.symbols([
            ("gobol_print", gobol_print as *const u8),
            ("gobol_println", gobol_println as *const u8),
            ("gobol_read", gobol_read as *const u8),
            ("gobol_str_int", gobol_str_int as *const u8),
            ("gobol_str_float", gobol_str_float as *const u8),
            ("gobol_str_bool", gobol_str_bool as *const u8),
            ("gobol_str_cat", gobol_str_cat as *const u8),
            ("gobol_str_eq", gobol_str_eq as *const u8),
            ("gobol_str_len", gobol_str_len as *const u8),
            ("gobol_str_get", gobol_str_get as *const u8),
            ("gobol_alloc", gobol_alloc as *const u8),
            ("gobol_array_new", gobol_array_new as *const u8),
            ("gobol_array_add", gobol_array_add as *const u8),
            ("gobol_array_len", gobol_array_len as *const u8),
            ("gobol_array_get", gobol_array_get as *const u8),
            ("gobol_array_set", gobol_array_set as *const u8),
        ]);
        let module = JITModule::new(builder);
        CraneliftBackend {
            module,
            fn_ctx: FunctionBuilderContext::new(),
            func_symbols: HashMap::new(),
            func_ids: HashMap::new(),
            string_data: HashMap::new(),
            constructors: HashMap::new(),
            type_resolver: TypeResolver::new(),
            variables: HashMap::new(),
            var_counter: 0,
            loop_stack: Vec::new(),
            return_type: DataType::None_,
            diverged: false,
        }
    }

    /// Compile the IR, finalize JIT definitions, then run `main`.
    pub fn compile_and_run(&mut self, ir: &GobolIR) -> Result<i64, String> {
        self.compile_ir(ir)?;
        self.finalize()?;
        self.run()
    }

    /// Finalize all pending JIT definitions so function pointers become valid.
    /// Must be called after `compile_ir` and before `run` / `get_function_ptr`.
    pub fn finalize(&mut self) -> Result<(), String> {
        self.module
            .finalize_definitions()
            .map_err(|e| format!("Finalize error: {}", e))
    }

    /// Get a pointer to a compiled top-level function (e.g. "main").
    pub fn get_function_ptr(&self, name: &str) -> Option<fn() -> i64> {
        let sym = self.func_symbols.get(name)?;
        let func_id = self.func_ids.get(sym)?;
        unsafe {
            let ptr = self.module.get_finalized_function(*func_id);
            Some(std::mem::transmute(ptr))
        }
    }

    /// Run the program's main function and return its exit code.
    pub fn run(&self) -> Result<i64, String> {
        match self.get_function_ptr("main") {
            Some(f) => Ok(f()),
            None => Err("no main function found".to_string()),
        }
    }
}

impl CraneliftBackend<ObjectModule> {
    /// Create an AOT backend that produces object files for linking.
    pub fn new_aot() -> Self {
        use cranelift_codegen::isa::lookup;
        use cranelift_codegen::settings::{self, Configurable};

        let mut flag_builder = settings::builder();
        flag_builder.set("use_colocated_libcalls", "false").unwrap();
        flag_builder.set("is_pic", "true").unwrap();
        let isa_builder = lookup(target_lexicon::HOST).unwrap();
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .unwrap();

        let builder = ObjectBuilder::new(
            isa,
            "gobol_aot",
            Box::new(cranelift_module::default_libcall_names()),
        )
        .unwrap();
        let module = ObjectModule::new(builder);

        CraneliftBackend {
            module,
            fn_ctx: FunctionBuilderContext::new(),
            func_symbols: HashMap::new(),
            func_ids: HashMap::new(),
            string_data: HashMap::new(),
            constructors: HashMap::new(),
            type_resolver: TypeResolver::new(),
            variables: HashMap::new(),
            var_counter: 0,
            loop_stack: Vec::new(),
            return_type: DataType::None_,
            diverged: false,
        }
    }

    /// Compile the IR, emit an object file, and link it with the C runtime
    /// to produce a standalone executable at `output_path`.
    pub fn compile_to_binary(
        mut self,
        ir: &GobolIR,
        output_path: &str,
        runtime_c_path: &str,
    ) -> Result<(), String> {
        self.compile_ir(ir)?;

        // Produce object file from the ObjectModule.
        let product = self.module.finish();
        let obj_bytes = product
            .emit()
            .map_err(|e| format!("Object emit failed: {}", e))?;

        // Write object file to a temp path.
        let obj_path = format!("{}.o", output_path);
        std::fs::write(&obj_path, &obj_bytes)
            .map_err(|e| format!("Failed to write object file: {}", e))?;

        // Link with the C runtime using the system C compiler.
        let status = std::process::Command::new("cc")
            .args([&obj_path, runtime_c_path, "-o", output_path])
            .status()
            .map_err(|e| format!("Failed to invoke cc: {}", e))?;

        // Clean up the object file.
        let _ = std::fs::remove_file(&obj_path);

        if !status.success() {
            return Err(format!("Linking failed with exit code {:?}", status.code()));
        }
        Ok(())
    }
}

/// Map a Gobol builtin call name to its runtime function.
fn builtin_runtime(name: &str) -> Option<&'static str> {
    // Strip any :: namespace prefix and match on the function name
    let short = name.rsplit("::").next().unwrap_or(name);
    match short {
        "print" | "_print" => Some("gobol_print"),
        "println" | "_println" => Some("gobol_println"),
        "read" | "_read" => Some("gobol_read"),
        _ => None,
    }
}
