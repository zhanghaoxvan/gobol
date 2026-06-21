// codegen_c.rs — C code generator. Walks the IR and emits C source.
use crate::environment::DataType;
use crate::ir::*;
use std::collections::HashMap;

#[allow(dead_code)]
pub struct CodeGenC {
    output: String,
    indent: usize,
    generated_functions: Vec<String>,
    vars: HashMap<String, DataType>,
    structs: Vec<String>,
}

impl CodeGenC {
    pub fn new() -> Self {
        CodeGenC {
            output: String::new(), indent: 0,
            generated_functions: Vec::new(),
            vars: HashMap::new(),
            structs: Vec::new(),
        }
    }

    pub fn generate(&mut self, ir: &GobolIR) -> String {
        self.emit_headers();
        self.emit_builtins();
        for s in &ir.structs { self.structs.push(s.name.clone()); self.emit_struct(s); }
        // forward-declare all user functions (including methods)
        for f in &ir.functions {
            if f.name != "main" { self.emit_forward_decl(f); }
        }
        for imp in &ir.impls {
            for m in &imp.methods { self.emit_forward_decl(m); }
        }
        self.emit_line("");
        // bodies
        for f in &ir.functions {
            if f.name != "main" { self.emit_function(f); }
        }
        for imp in &ir.impls {
            for m in &imp.methods { self.emit_function(m); }
        }
        let has_main = ir.functions.iter().any(|f| f.is_main);
        if has_main {
            for f in &ir.functions {
                if f.is_main { self.emit_main_function(f); }
            }
        } else {
            // Stub main when no main function (e.g. library modules)
            self.emit_line("int main(void) { return 0; }");
            self.emit_line("");
        }
        std::mem::take(&mut self.output)
    }

    // ── headers / builtins ──

    fn emit_headers(&mut self) {
        self.emit_line("#include <stdio.h>");
        self.emit_line("#include <stdint.h>");
        self.emit_line("#include <stdbool.h>");
        self.emit_line("#include <string.h>");
        self.emit_line("#include <stdlib.h>");
        self.emit_line("");
        self.emit_line("// std/c/__builtins__.c");
        self.emit_line("void println(const char* v);");
        self.emit_line("void print(const char* v);");
        self.emit_line("char* read(void);");
        self.emit_line("char* gobol_str_int(int64_t n);");
        self.emit_line("char* gobol_str_float(double f);");
        self.emit_line("char* gobol_str_cat(const char* a, const char* b);");
        self.emit_line("// array runtime");
        self.emit_line("typedef struct { int64_t* data; int64_t len; int64_t cap; } gobol_array_t;");
        self.emit_line("void gobol_array_add(gobol_array_t* arr, int64_t val);");
        self.emit_line("int64_t gobol_array_len(gobol_array_t* arr);");
        self.emit_line("int64_t gobol_array_get(gobol_array_t* arr, int64_t i);");
        self.emit_line("void gobol_array_set(gobol_array_t* arr, int64_t i, int64_t val);");
        self.emit_line("int64_t gobol_array_get_flat(gobol_array_t* arr, int64_t i, int64_t j);");
        self.emit_line("void gobol_array_set_flat(gobol_array_t* arr, int64_t i, int64_t j, int64_t val);");
        self.emit_line("");
    }

    fn emit_builtins(&mut self) {
        self.emit_line("void gobol_print(const char* s) { printf(\"%s\", s); }");
        self.emit_line("");
        self.emit_line("char* gobol_read(void) {");
        self.indent += 1;
        self.emit_line("static char buf[4096];");
        self.emit_line("if (fgets(buf, sizeof(buf), stdin)) {");
        self.indent += 1;
        self.emit_line("size_t len = strlen(buf);");
        self.emit_line("if (len > 0 && buf[len-1] == '\\n') buf[len-1] = '\\0';");
        self.emit_line("return buf;");
        self.indent -= 1;
        self.emit_line("}");
        self.emit_line("return \"\";");
        self.indent -= 1;
        self.emit_line("}");
        self.emit_line("");
    }

    // ── struct ──

    fn emit_struct(&mut self, s: &IRStruct) {
        self.emit(&format!("typedef struct {} {{ ", s.name));
        for f in &s.fields {
            self.emit(&format!("{} {}; ", self.c_type_name(&f.ty), f.name));
        }
        self.emit_line(&format!("}} {};", s.name));
        self.emit_line("");
    }

    // ── forward decl ──

    fn c_func_name(name: &str) -> String { name.replace('.', "_") }

    fn emit_forward_decl(&mut self, f: &IRFunction) {
        let ret = self.c_type_name(&f.return_type);
        let params: Vec<String> = f.params.iter()
            .map(|p| {
                let ct = if p.ty == DataType::Unknown { "gobol_array_t".to_string() }
                    else { self.c_type_name(&p.ty).to_string() };
                format!("{} {}", ct, p.name)
            })
            .collect();
        self.emit_line(&format!("{} {}({});", ret, Self::c_func_name(&f.name), params.join(", ")));
    }

    // ── function ──

    fn emit_function(&mut self, f: &IRFunction) {
        if self.generated_functions.contains(&f.name) { return; }
        self.generated_functions.push(f.name.clone());
        // If no body, C companion provides implementation (skip body generation)
        if f.body.is_none() { return; }
        let ret = self.c_type_name(&f.return_type);
        let params: Vec<String> = f.params.iter()
            .map(|p| {
                let ct = if p.ty == DataType::Unknown { "gobol_array_t".to_string() }
                    else { self.c_type_name(&p.ty).to_string() };
                format!("{} {}", ct, p.name)
            })
            .collect();
        let c_name = Self::c_func_name(&f.name);
        self.emit_line(&format!("{} {}({}) {{", ret, c_name, params.join(", ")));
        self.indent += 1;
        for p in &f.params {
            self.vars.insert(p.name.clone(), p.ty.clone());
        }
        if f.is_method && !f.params.iter().any(|p| p.name == "self") {
            if let Some(s) = &f.struct_name {
                let st = DataType::Struct(s.clone());
                let c_name = self.c_type_name(&st);
                self.emit_line(&format!("{} self = {{0}};", c_name));
                self.vars.insert("self".to_string(), st);
            }
        }
        if let Some(b) = &f.body { self.emit_block(b); }
        if f.return_type != DataType::None_ && f.return_type != DataType::Unknown {
            let is_struct = matches!(&f.return_type, DataType::Struct(_));
            if is_struct { self.emit_line("return self;"); }
            else { self.emit_line("return 0;"); }
        }
        self.indent -= 1;
        self.emit_line("}");
        self.emit_line("");
    }

    fn emit_main_function(&mut self, f: &IRFunction) {
        self.vars.clear();
        self.emit_line("int main(void) {");
        self.indent += 1;
        if let Some(b) = &f.body {
            for s in &b.statements {
                match s {
                    // tail expressions in main: emit as plain call (void-typed), then return 0
                    IRStmt::Return(Some(e)) => {
                        self.emit_expression(e);
                        self.emit_line(";");
                    }
                    _ => self.emit_statement(s),
                }
            }
        }
        self.emit_line("return 0;");
        self.indent -= 1;
        self.emit_line("}");
        self.emit_line("");
    }

    // ── block / stmt ──

    fn emit_block(&mut self, b: &IRBlock) {
        for s in &b.statements { self.emit_statement(s); }
    }

    fn emit_statement(&mut self, s: &IRStmt) {
        match s {
            IRStmt::Declaration { name, ty, init } => {
                let is_array_init = init.as_ref().map_or(false, |e| matches!(e, IRExpr::ArrayLiteral(_)));
                let is_array_type = *ty == DataType::Unknown; // array type like int[] or int[10]
                let is_array = is_array_init || is_array_type;
                let resolved = if *ty == DataType::None_ || *ty == DataType::Unknown {
                    init.as_ref().map(|e| self.infer_type(e)).unwrap_or(DataType::Int)
                } else { ty.clone() };
                self.vars.insert(name.clone(), if is_array { DataType::Unknown } else { resolved.clone() });
                if is_array {
                    self.emit(&format!("gobol_array_t {} = {{0}};", name));
                    // For empty array literal, just declare
                    if let Some(IRExpr::ArrayLiteral(elems)) = init {
                        for el in elems {
                            self.emit(&format!("gobol_array_add(&{}, ", name));
                            self.emit_expression(el);
                            self.emit("); ");
                        }
                    }
                    self.emit_line("");
                } else {
                    let ct = self.c_type_name(&resolved);
                    self.emit(&format!("{} {} = ", ct, name));
                    if let Some(e) = init { self.emit_expression(e); } else { self.emit("0"); }
                    self.emit_line(";");
                }
            }
            IRStmt::Expression(e) => { self.emit_expression(e); self.emit_line(";"); }
            IRStmt::Return(Some(e)) => { self.emit("return "); self.emit_expression(e); self.emit_line(";"); }
            IRStmt::Return(None) => { self.emit_line("return;"); }
            IRStmt::If { cond, then_block, else_block } => {
                self.emit("if ("); self.emit_expression(cond); self.emit_line(") {");
                self.indent += 1; self.emit_block(then_block); self.indent -= 1;
                if let Some(eb) = else_block {
                    self.emit_line("} else {"); self.indent += 1; self.emit_block(eb); self.indent -= 1;
                }
                self.emit_line("}");
            }
            IRStmt::While { cond, body } => {
                self.emit("while ("); self.emit_expression(cond); self.emit_line(") {");
                self.indent += 1; self.emit_block(body); self.indent -= 1;
                self.emit_line("}");
            }
            IRStmt::For { vars, iterable, body } => {
                let loop_var = if vars.len() >= 2 { vars[1].clone() } else { vars[0].clone() };
                let idx_var = if vars.len() >= 2 { Some(vars[0].clone()) } else { None };
                let is_range = matches!(iterable, IRExpr::Call { func, .. } if func == "range");
                let is_str_lit = matches!(iterable, IRExpr::Literal(LitValue::Str(_)));
                if is_range {
                    if let IRExpr::Call { args, .. } = &iterable {
                        self.emit(&format!("for (int64_t {} = ", loop_var));
                        if let Some(a) = args.first() { self.emit_expression(a); } else { self.emit("0"); }
                        self.emit(&format!("; {} < ", loop_var));
                        if args.len() >= 2 { self.emit_expression(&args[1]); } else { self.emit("0"); }
                        self.emit(&format!("; {}++)", loop_var));
                        self.emit_line(" {");
                        self.indent += 1;
                        self.vars.insert(loop_var, DataType::Int);
                        self.emit_block(body);
                        self.indent -= 1;
                        self.emit_line("}");
                    }
                } else if is_str_lit {
                    self.emit("for (const char* _p = ");
                    self.emit_expression(iterable);
                    self.emit_line("; *_p; _p++) {");
                    self.indent += 1;
                    self.emit_line(&format!("char {} = *_p;", loop_var));
                    self.vars.insert(loop_var, DataType::Int);
                    self.emit_block(body);
                    self.indent -= 1;
                    self.emit_line("}");
                } else {
                    let arr_name = self.expr_var_name(iterable).to_string();
                    self.emit_line(&format!("for (int64_t _i = 0; _i < gobol_array_len(&{}); _i++) {{", arr_name));
                    self.indent += 1;
                    let et = self.infer_type(iterable);
                    if let Some(iv) = &idx_var {
                        self.emit_line(&format!("int64_t {} = _i;", iv));
                        self.vars.insert(iv.clone(), DataType::Int);
                    }
                    self.emit_line(&format!("{} {} = gobol_array_get(&{}, _i);", self.c_type_name(&et), loop_var, arr_name));
                    self.vars.insert(loop_var, et);
                    self.emit_block(body);
                    self.indent -= 1;
                    self.emit_line("}");
                }
            }
            IRStmt::Break => self.emit_line("break;"),
            IRStmt::Continue => self.emit_line("continue;"),
            IRStmt::Assignment { target, value } => {
                self.emit_expression(target); self.emit(" = "); self.emit_expression(value); self.emit_line(";");
            }
            IRStmt::Call { func, args, .. } => {
                let c_name = Self::c_func_name(func);
                self.emit(&format!("{}(", c_name));
                let wrap = func == "print" || func == "println";
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    self.emit_arg(a, wrap);
                }
                self.emit_line(");");
            }
            IRStmt::MethodCall { object, method, args, .. } => {
                // Handle array methods: arr.add(x) → gobol_array_add(&arr, x)
                let is_array_method = method == "add" || method == "len" || method == "get";
                let obj_name = self.expr_var_name(object).to_string();
                let is_arr_var = matches!(self.vars.get(&obj_name), Some(DataType::Unknown))
                    || obj_name.contains("arr");
                if is_array_method && is_arr_var && !obj_name.is_empty() && obj_name != "unknown" {
                    match method.as_str() {
                        "add" => {
                            self.emit(&format!("gobol_array_add(&{}, ", obj_name));
                            for (i, a) in args.iter().enumerate() {
                                if i > 0 { self.emit(", "); }
                                self.emit_expression(a);
                            }
                            self.emit_line(");");
                        }
                        "len" => {
                            self.emit(&format!("gobol_array_len(&{})", obj_name));
                            self.emit_line(";");
                        }
                        _ => {}
                    }
                    return;
                }
                let obj_ty = self.infer_type(object);
                let struct_name = match &obj_ty {
                    DataType::Struct(n) => n.clone(),
                    _ => {
                        if let IRExpr::Variable(name) = object.as_ref() {
                            if self.structs.contains(name) { name.clone() }
                            else { String::new() }
                        } else { String::new() }
                    }
                };
                if !struct_name.is_empty() {
                    let is_type_call = matches!(object.as_ref(), IRExpr::Variable(n) if self.structs.contains(n));
                    self.emit(&format!("{}_{}(", struct_name, method));
                    let mut first = true;
                    if !is_type_call { self.emit_expression(object); first = false; }
                    let wrap = method == "print" || method == "println";
                    for a in args {
                        if !first { self.emit(", "); }
                        self.emit_arg(a, wrap);
                        first = false;
                    }
                    self.emit_line(");");
                } else {
                    let func_name = if let IRExpr::Variable(obj) = object.as_ref() {
                        let is_builtin = matches!(method.as_str(), "print" | "println" | "read");
                        if is_builtin { method.clone() }
                        else { format!("{}_{}", obj, method) }
                    } else { method.clone() };
                    self.emit(&format!("{}(", Self::c_func_name(&func_name)));
                    let wrap = method == "print" || method == "println";
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.emit_arg(a, wrap);
                    }
                    self.emit_line(");");
                }
            }
        }
    }

    // ── expression ──

    fn emit_expression(&mut self, e: &IRExpr) {
        match e {
            IRExpr::Literal(l) => match l {
                LitValue::Int(n) => self.emit(&format!("{}", n)),
                LitValue::Float(f) => self.emit(&format!("{}", f)),
                LitValue::Bool(b) => self.emit(if *b { "true" } else { "false" }),
                LitValue::Str(s) => self.emit(&format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))),
                LitValue::None => self.emit("0"),
            },
            IRExpr::Variable(name) => self.emit(name),
            IRExpr::Binary { op, left, right } => {
                if op == "+" && (self.contains_str(left) || self.contains_str(right)) {
                    self.emit("gobol_str_cat(");
                    self.emit_str_operand(left);
                    self.emit(", ");
                    self.emit_str_operand(right);
                    self.emit(")");
                } else {
                    self.emit("("); self.emit_expression(left); self.emit(&format!(" {} ", op)); self.emit_expression(right); self.emit(")");
                }
            }
            IRExpr::Unary { op, operand } => {
                self.emit(op); self.emit_expression(operand);
            }
            IRExpr::Call { func, args, .. } => {
                match func.as_str() {
                    "_print" => self.emit("gobol_print("),
                    "_read" => self.emit("gobol_read("),
                    _ => self.emit(&format!("{}(", Self::c_func_name(func))),
                }
                let wrap = func == "print" || func == "println";
                for (i, a) in args.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    self.emit_arg(a, wrap);
                }
                self.emit(")");
            }
            IRExpr::MethodCall { object, method, args, .. } => {
                // Handle array methods
                let is_array_method = method == "add" || method == "len" || method == "get";
                let obj_name = self.expr_var_name(object).to_string();
                let is_arr_var = matches!(self.vars.get(&obj_name), Some(DataType::Unknown))
                    || obj_name.contains("arr");
                if is_array_method && is_arr_var && !obj_name.is_empty() && obj_name != "unknown" {
                    match method.as_str() {
                        "add" => {
                            self.emit(&format!("gobol_array_add(&{}, ", obj_name));
                            for (i, a) in args.iter().enumerate() {
                                if i > 0 { self.emit(", "); }
                                self.emit_expression(a);
                            }
                            self.emit(")");
                        }
                        "len" => {
                            self.emit(&format!("gobol_array_len(&{})", obj_name));
                        }
                        "get" => {
                            self.emit(&format!("gobol_array_get(&{}", obj_name));
                            for a in args { self.emit(", "); self.emit_expression(a); }
                            self.emit(")");
                        }
                        _ => {}
                    }
                    return;
                }
                let obj_ty = self.infer_type(object);
                let struct_name = match &obj_ty {
                    DataType::Struct(n) => n.clone(),
                    _ => {
                        if let IRExpr::Variable(name) = object.as_ref() {
                            if self.structs.contains(name) { name.clone() }
                            else { String::new() }
                        } else { String::new() }
                    }
                };
                if !struct_name.is_empty() {
                    let is_type_call = matches!(object.as_ref(), IRExpr::Variable(n) if self.structs.contains(n));
                    self.emit(&format!("{}_{}(", struct_name, method));
                    let mut first = true;
                    if !is_type_call { self.emit_expression(object); first = false; }
                    let wrap = method == "print" || method == "println";
                    for a in args {
                        if !first { self.emit(", "); }
                        self.emit_arg(a, wrap);
                        first = false;
                    }
                    self.emit(")");
                } else {
                    // Module call: emit object_method(args) for non-builtin modules
                    let func_name = if let IRExpr::Variable(obj) = object.as_ref() {
                        let is_builtin = matches!(method.as_str(), "print" | "println" | "read");
                        if is_builtin { method.clone() }
                        else { format!("{}_{}", obj, method) }
                    } else { method.clone() };
                    self.emit(&format!("{}(", Self::c_func_name(&func_name)));
                    let wrap = method == "print" || method == "println";
                    for (i, a) in args.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.emit_arg(a, wrap);
                    }
                    self.emit(")");
                }
            }
            IRExpr::MemberAccess { object, member } => {
                self.emit_expression(object); self.emit("."); self.emit(member);
            }
            IRExpr::ArrayIndex { array, index } => {
                let arr_name = self.expr_var_name(array);
                // Handle nested indexing: arr[i][j] → gobol_array_get_flat(&arr, i, j)
                if let IRExpr::ArrayIndex { array: inner, index: inner_idx } = array.as_ref() {
                    let root_name = self.expr_var_name(inner);
                    if root_name != "unknown" {
                        self.emit(&format!("gobol_array_get_flat(&{}, ", root_name));
                        self.emit_expression(inner_idx); self.emit(", ");
                        self.emit_expression(index);
                        self.emit(")");
                        return;
                    }
                }
                if arr_name != "unknown" {
                    self.emit(&format!("gobol_array_get(&{}, ", arr_name));
                    self.emit_expression(index);
                    self.emit(")");
                } else {
                    self.emit_expression(array); self.emit("["); self.emit_expression(index); self.emit("]");
                }
            }
            IRExpr::ArrayLiteral(elems) => {
                if elems.is_empty() {
                    self.emit("(int64_t[]){0}");
                } else if let Some(first) = elems.first() {
                    let et = self.infer_type(first);
                    self.emit(&format!("({}[]){{", self.c_type_name(&et)));
                    for (i, el) in elems.iter().enumerate() {
                        if i > 0 { self.emit(", "); }
                        self.emit_expression(el);
                    }
                    self.emit("}");
                } else {
                    self.emit("(int64_t[]){0}");
                }
            }
            IRExpr::StructLiteral { name, fields } => {
                self.emit(&format!("({}){{", name));
                for (i, (fn_, fe)) in fields.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    self.emit(&format!(".{} = ", fn_));
                    self.emit_expression(fe);
                }
                self.emit("}");
            }
            IRExpr::Cast { expr, target } => {
                // Cast to str on a struct → call StructName_convert_str()
                let src_ty = self.infer_type(expr);
                if matches!(target, DataType::Str) {
                    if let DataType::Struct(name) = &src_ty {
                        self.emit(&format!("{}_convert_str(", name));
                        self.emit_expression(expr);
                        self.emit(")");
                        return;
                    }
                }
                self.emit(&format!("({})(", self.c_type_name(target)));
                self.emit_expression(expr);
                self.emit(")");
            }
            IRExpr::Assignment { target, value } => {
                // Nested array assignment: arr[i][j] = v
                if let IRExpr::ArrayIndex { array: outer_arr, index: outer_idx } = target.as_ref() {
                    if let IRExpr::ArrayIndex { array: inner_arr, index: inner_idx } = outer_arr.as_ref() {
                        let root_name = self.expr_var_name(inner_arr);
                        if root_name != "unknown" {
                            self.emit(&format!("gobol_array_set_flat(&{}, ", root_name));
                            self.emit_expression(inner_idx); self.emit(", ");
                            self.emit_expression(outer_idx); self.emit(", ");
                            self.emit_expression(value);
                            self.emit(")");
                            return;
                        }
                    }
                }
                // Single array index assignment: arr[i] = v
                if let IRExpr::ArrayIndex { array, index } = target.as_ref() {
                    let arr_name = self.expr_var_name(array);
                    if arr_name != "unknown" {
                        self.emit(&format!("gobol_array_set(&{}, ", arr_name));
                        self.emit_expression(index);
                        self.emit(", ");
                        self.emit_expression(value);
                        self.emit(")");
                        return;
                    }
                }
                self.emit_expression(target); self.emit(" = "); self.emit_expression(value);
            }
            IRExpr::None => self.emit("0"),
        }
    }

    // ── helpers ──

    fn emit(&mut self, s: &str) { self.output.push_str(s); }

    fn emit_line(&mut self, s: &str) {
        for _ in 0..self.indent { self.output.push_str("    "); }
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn c_type_name<'a>(&self, dt: &'a DataType) -> &'a str {
        match dt {
            DataType::Int => "int64_t",
            DataType::Float => "double",
            DataType::Bool => "bool",
            DataType::Str => "const char*",
            DataType::None_ => "void",
            #[allow(unused)]
            DataType::Unknown => "int64_t",
            DataType::Struct(name) if self.structs.contains(name) => name.as_str(),
            DataType::Struct(_) => "void*",
            DataType::Nullable(inner) => self.c_type_name(inner),
        }
    }

    fn emit_arg(&mut self, arg: &IRExpr, needs_wrap: bool) {
        if !needs_wrap { self.emit_expression(arg); return; }
        // If the expression already produces a string, don't wrap
        if self.contains_str(arg) { self.emit_expression(arg); return; }
        match arg {
            IRExpr::Literal(LitValue::Int(n)) => self.emit(&format!("gobol_str_int({})", n)),
            IRExpr::Literal(LitValue::Float(f)) => self.emit(&format!("gobol_str_float({})", f)),
            IRExpr::Literal(LitValue::Bool(b)) => self.emit(if *b { "\"true\"" } else { "\"false\"" }),
            IRExpr::Literal(LitValue::Str(_)) => self.emit_expression(arg),
            IRExpr::Variable(name) => {
                let is_str = matches!(self.vars.get(name), Some(DataType::Str));
                if is_str { self.emit_expression(arg); }
                else { self.emit("gobol_str_int("); self.emit_expression(arg); self.emit(")"); }
            }
            _ => { self.emit("gobol_str_int("); self.emit_expression(arg); self.emit(")"); }
        }
    }

    /// Emit an operand for gobol_str_cat — wrap non-string values
    fn expr_var_name<'a>(&self, e: &'a IRExpr) -> &'a str {
        match e { IRExpr::Variable(n) => n, _ => "unknown" }
    }

    fn emit_str_operand(&mut self, e: &IRExpr) {
        if self.contains_str(e) {
            self.emit_expression(e);
        } else {
            self.emit_arg(e, true);
        }
    }

    /// Check if expression tree contains a string literal or str-producing call
    fn contains_str(&self, e: &IRExpr) -> bool {
        match e {
            IRExpr::Literal(LitValue::Str(_)) => true,
            IRExpr::Variable(name) => matches!(self.vars.get(name), Some(DataType::Str)),
            IRExpr::Cast { target, .. } => matches!(target, DataType::Str),
            IRExpr::MethodCall { method, .. } => method.contains("str") || method == "convert_str",
            IRExpr::Call { func, .. } => {
                func == "gobol_str_cat" || func == "gobol_str_int" || func == "gobol_str_float"
                    || func.contains("str") || func.contains("convert")
            }
            IRExpr::Binary { left, right, .. } => self.contains_str(left) || self.contains_str(right),
            _ => false,
        }
    }

    fn infer_type(&self, e: &IRExpr) -> DataType {
        match e {
            IRExpr::Literal(LitValue::Int(_)) => DataType::Int,
            IRExpr::Literal(LitValue::Float(_)) => DataType::Float,
            IRExpr::Literal(LitValue::Bool(_)) => DataType::Bool,
            IRExpr::Literal(LitValue::Str(_)) => DataType::Str,
            IRExpr::Variable(name) => self.vars.get(name).cloned().unwrap_or(DataType::Int),
            IRExpr::StructLiteral { name, .. } => DataType::Struct(name.clone()),
            IRExpr::ArrayLiteral(elems) => {
                elems.first().map(|e| self.infer_type(e)).unwrap_or(DataType::Int)
            }
            IRExpr::MethodCall { object, method, .. } => {
                if method == "new" {
                    if let IRExpr::Variable(name) = object.as_ref() {
                        return DataType::Struct(name.clone());
                    }
                }
                DataType::Int
            }
            IRExpr::Call { func, .. } if func == "gobol_str_cat" => DataType::Str,
            IRExpr::Binary { left, .. } => {
                if self.contains_str(e) { DataType::Str }
                else { self.infer_type(left) }
            }
            _ => DataType::Int,
        }
    }
}
