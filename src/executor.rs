use crate::ast::*;
use crate::ast_builder::AstBuilder;
use crate::error::ErrorFormatter;
use crate::lexer::Lexer;
use crate::value::RtValue;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

// ==================== Runtime Environment ====================

pub struct RuntimeEnv {
    scopes: Vec<HashMap<String, RtValue>>,
}

impl RuntimeEnv {
    pub fn new() -> Self {
        let mut env = RuntimeEnv { scopes: Vec::new() };
        env.scopes.push(HashMap::new());
        env
    }

    pub fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn exit_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    pub fn declare(&mut self, name: &str, value: RtValue) {
        let scope = self.scopes.last_mut().unwrap();
        scope.insert(name.to_string(), value);
    }

    pub fn lookup(&self, name: &str) -> Option<&RtValue> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v);
            }
        }
        None
    }

    pub fn assign(&mut self, name: &str, value: RtValue) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(v) = scope.get_mut(name) {
                *v = value;
                return true;
            }
        }
        false
    }
}

// ==================== Built-in Functions ====================

type BuiltinFn = fn(&[RtValue]) -> Result<RtValue, String>;

pub struct Builtins {
    functions: HashMap<String, BuiltinFn>,
}

impl Builtins {
    pub fn new() -> Self {
        let mut functions: HashMap<String, BuiltinFn> = HashMap::new();
        functions.insert("__builtins__._print".to_string(), builtin_print);
        functions.insert("__builtins__._read".to_string(), builtin_read);
        functions.insert("__builtins__.panic".to_string(), builtin_panic);
        Builtins { functions }
    }

    pub fn call(&self, name: &str, args: &[RtValue]) -> Result<RtValue, String> {
        match self.functions.get(name) {
            Some(f) => f(args),
            None => Err(format!("Undefined function: '{}'", name)),
        }
    }
}

fn builtin_print(args: &[RtValue]) -> Result<RtValue, String> {
    for arg in args {
        print!("{}", arg);
    }
    Ok(RtValue::None_)
}

fn builtin_read(_args: &[RtValue]) -> Result<RtValue, String> {
    let mut input = String::new();
    match std::io::stdin().read_line(&mut input) {
        Ok(_) => {
            // Trim trailing newline
            if input.ends_with('\n') {
                input.pop();
            }
            if input.ends_with('\r') {
                input.pop();
            }
            Ok(RtValue::Str(input))
        }
        Err(e) => Err(format!("Failed to read input: {}", e)),
    }
}


fn builtin_panic(args: &[RtValue]) -> Result<RtValue, String> {
    let msg = if args.is_empty() {
        "panic called".to_string()
    } else {
        args[0].to_string_val()
    };
    Err(format!("Panic: {}", msg))
}

// ==================== Executor ====================

pub struct Executor {
    env: RuntimeEnv,
    pub builtins: Builtins,
    value_stack: Vec<RtValue>,
    returning: bool,
    return_value: RtValue,
    breaking: bool,
    continuing: bool,
    errors: Vec<String>,
    error_formatter: Option<ErrorFormatter>,
    expression_depth: i32,
    current_impl_struct: Option<String>,
    user_functions: HashMap<String, *const Function>,
    struct_definitions: HashMap<String, Vec<String>>,
    lib_paths: Vec<String>,
    loaded_modules: HashSet<String>,
    loaded_programs: Vec<Box<Program>>,
    current_module_dir: Option<String>,
    current_module: String,
    module_aliases: HashMap<String, String>,
}

impl Executor {
    pub fn new() -> Self {
        Executor {
            env: RuntimeEnv::new(),
            builtins: Builtins::new(),
            value_stack: Vec::new(),
            returning: false,
            return_value: RtValue::None_,
            breaking: false,
            continuing: false,
            errors: Vec::new(),
            error_formatter: None,
            expression_depth: 0,
            current_impl_struct: None,
            user_functions: HashMap::new(),
            struct_definitions: HashMap::new(),
            lib_paths: vec!["lib".to_string()],
            loaded_modules: HashSet::new(),
            loaded_programs: Vec::new(),
            current_module_dir: None,
            current_module: String::new(),
            module_aliases: HashMap::new(),
        }
    }

    pub fn set_lib_paths(&mut self, paths: Vec<String>) {
        self.lib_paths = paths;
    }

    pub fn set_error_formatter(&mut self, f: ErrorFormatter) {
        self.error_formatter = Some(f);
    }

    pub fn set_main_file(&mut self, file_path: &str) {
        if let Some(stem) = Path::new(file_path).file_stem().and_then(|s| s.to_str()) {
            self.current_module = stem.to_string();
        } else {
            self.current_module = "main".to_string();
        }
    }

    pub fn execute(&mut self, program: &Program) -> Result<i32, Vec<String>> {
        program.accept(self);

        if !self.errors.is_empty() {
            return Err(self.errors.clone());
        }

        match &self.return_value {
            RtValue::Int(n) => Ok(*n as i32),
            _ => Ok(0),
        }
    }

    fn error(&mut self, msg: String) {
        if let Some(ref f) = self.error_formatter {
            let formatted = f.format_error(0, 0, 0, "runtime error", &msg, true);
            self.errors.push(formatted);
        } else {
            self.errors.push(format!("Runtime Error: {}", msg));
        }
    }

    fn push(&mut self, v: RtValue) {
        self.value_stack.push(v);
    }

    fn pop(&mut self) -> RtValue {
        self.value_stack.pop().unwrap_or(RtValue::None_)
    }

    #[allow(dead_code)]
    fn call_user_function(&mut self, func: &Function, args: &[RtValue]) {
        self.call_user_function_with_copyback(func, args, &[], None)
    }

    fn call_user_function_with_copyback(&mut self, func: &Function, args: &[RtValue], copyback: &[(usize, String)], impl_struct: Option<&str>) {
        let prev_impl = self.current_impl_struct.clone();
        if let Some(s) = impl_struct {
            self.current_impl_struct = Some(s.to_string());
        }
        self.env.enter_scope();

        // Bind parameters to arguments
        if let Some(params) = func.get_parameters() {
            for (i, param) in params.iter().enumerate() {
                let value = if i < args.len() {
                    args[i].clone()
                } else {
                    RtValue::None_
                };
                self.env.declare(param.get_name(), value);
            }
        }

        // Execute body
        let prev_returning = self.returning;
        let prev_return_value = self.return_value.clone();
        self.returning = false;
        self.return_value = RtValue::None_;

        if let Some(body) = func.get_body() {
            body.accept(self);
        }

        let result = if self.returning {
            self.return_value.clone()
        } else {
            RtValue::None_
        };

        // Capture modified array param values before exiting scope
        let mut copyback_values: Vec<(String, RtValue)> = Vec::new();
        if let Some(params) = func.get_parameters() {
            for (idx, caller_var) in copyback {
                if let Some(param) = params.get(*idx) {
                    if let Some(val) = self.env.lookup(param.get_name()) {
                        if matches!(val, RtValue::Array(_)) {
                            copyback_values.push((caller_var.clone(), val.clone()));
                        }
                    }
                }
            }
        }

        self.returning = prev_returning;
        self.return_value = prev_return_value;

        self.env.exit_scope();

        // Now update caller's variables (after function scope is gone)
        for (caller_var, val) in copyback_values {
            self.env.assign(&caller_var, val);
        }

        self.current_impl_struct = prev_impl;
        self.push(result);
    }
}

impl AstVisitor for Executor {
    fn visit_program(&mut self, node: &Program) {
        // Register built-in modules
        self.env.declare("__builtins__", RtValue::None_);
        self.env.declare("io", RtValue::None_);

        // Auto-import __setup__ which provides io, range, etc.
        self.load_module("__setup__");

        // First pass: collect all function declarations and struct/impl definitions
        for stmt in node.get_statements() {
            if stmt.as_any().downcast_ref::<Function>().is_some()
                || stmt.as_any().downcast_ref::<StructDefinition>().is_some()
                || stmt.as_any().downcast_ref::<ImplBlock>().is_some()
            {
                stmt.accept(self);
            }
        }

        // Second pass: handle import statements (they set up context)
        for stmt in node.get_statements() {
            if stmt.as_any().downcast_ref::<ImportStatement>().is_some() {
                stmt.accept(self);
            }
        }

        // Find and execute 'main' function
        for stmt in node.get_statements() {
            if let Some(func) = stmt.as_any().downcast_ref::<Function>() {
                if func.get_name() == "main" {
                    self.env.enter_scope();
                    if let Some(body) = func.get_body() {
                        body.accept(self);
                    }
                    self.env.exit_scope();
                    break;
                }
            }
        }
    }

    fn visit_import_statement(&mut self, node: &ImportStatement) {
        let module_name = node.get_module_name();
        self.load_module(&module_name);
    }

    fn visit_struct_definition(&mut self, node: &StructDefinition) {
        // Store struct metadata for later instantiation
        let struct_name = node.get_name().to_string();
        let field_names: Vec<String> = node.get_fields().iter().map(|f| f.name.clone()).collect();
        self.struct_definitions.insert(struct_name.clone(), field_names);
        self.env.declare(&struct_name, RtValue::Struct(
            struct_name.clone(),
            std::collections::HashMap::new(),
        ));
    }

    fn visit_impl_block(&mut self, node: &ImplBlock) {
        // Track current impl struct for bare field assignment
        let prev_impl = self.current_impl_struct.clone();
        self.current_impl_struct = Some(node.get_struct_name().to_string());
        // Process each item in the impl block
        for item in node.get_items() {
            match item {
                ImplItem::Constructor(func) | ImplItem::Method(func) | ImplItem::Convert(func) => {
                    // Store method for later dispatch
                    func.accept(self);
                }
            }
        }
        self.current_impl_struct = prev_impl;
    }

    fn visit_export_statement(&mut self, _node: &ExportStatement) {
        // Exports are a compile-time concept; no runtime effect
    }

    fn visit_block(&mut self, node: &Block) {
        let was_expr = self.expression_depth > 0;
        self.expression_depth += 1;
        self.env.enter_scope();
        let stmts = node.get_statements();
        let len = stmts.len();
        for (i, stmt) in stmts.iter().enumerate() {
            if self.returning || self.breaking || self.continuing || !self.errors.is_empty() {
                break;
            }
            let is_last = i == len - 1;
            if is_last && was_expr {
                // Last statement in expression context: keep value on stack
                self.expression_depth -= 1;
                stmt.accept(self);
                self.expression_depth += 1;
            } else {
                stmt.accept(self);
            }
        }
        self.env.exit_scope();
        self.expression_depth -= 1;
    }

    fn visit_function(&mut self, node: &Function) {
        // Store function pointer with module-qualified name
        let qualified = if self.current_module.is_empty() {
            node.get_name().to_string()
        } else {
            format!("{}.{}", self.current_module, node.get_name())
        };
        self.user_functions.insert(
            qualified.clone(),
            node as *const Function,
        );
        // Also register under short name for unqualified calls within the same module
        self.user_functions.insert(
            node.get_name().to_string(),
            node as *const Function,
        );
        self.env.declare(node.get_name(), RtValue::None_);
    }

    fn visit_declaration(&mut self, node: &Declaration) {
        let name = node.get_name();
        let initializer = node.get_initializer();

        let value = if let Some(init) = initializer {
            init.accept(self);
            self.pop()
        } else {
            // Default values based on type
            if let Some(tp) = node.get_type() {
                if let Some(arr) = tp.as_type_any().downcast_ref::<ArrayType>() {
                    // Create default array
                    return self.declare_array(name, arr, &node.get_keyword());
                }
            }
            RtValue::None_
        };

        self.env.declare(name, value);
    }

    fn visit_expression_statement(&mut self, node: &ExpressionStatement) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
            if self.expression_depth == 0 {
                self.pop(); // discard result in statement context
            }
        }
    }

    fn visit_return_statement(&mut self, node: &ReturnStatement) {
        if let Some(val) = node.get_value() {
            val.accept(self);
            self.return_value = self.pop();
        } else {
            self.return_value = RtValue::None_;
        }
        self.returning = true;
    }

    fn visit_if_statement(&mut self, node: &IfStatement) {
        if let Some(cond) = node.get_condition() {
            cond.accept(self);
            let condition = self.pop();

            let was_expr_context = self.expression_depth > 0;
            self.expression_depth += 1;

            if condition.is_truthy() {
                if let Some(then_branch) = node.get_then_branch() {
                    then_branch.accept(self);
                }
            } else if let Some(else_branch) = node.get_else_branch() {
                else_branch.accept(self);
            } else if was_expr_context {
                // No else branch in expression context: push None_
                self.push(RtValue::None_);
            }

            self.expression_depth -= 1;

            // If used as statement, discard any value left by the branch
            if !was_expr_context && !self.value_stack.is_empty() {
                // Check if the branch actually pushed something
                // (for safety, we don't pop here to avoid removing unpredictably)
                let _ = self.pop();
            }
        }
    }

    fn visit_while_statement(&mut self, node: &WhileStatement) {
        loop {
            if let Some(cond) = node.get_condition() {
                cond.accept(self);
                let condition = self.pop();

                if !condition.is_truthy() {
                    break;
                }
            } else {
                break;
            }

            if let Some(body) = node.get_body() {
                body.accept(self);

                if self.returning {
                    break;
                }
                if self.continuing {
                    self.continuing = false;
                    continue;
                }
                if self.breaking {
                    self.breaking = false;
                    break;
                }
            }
        }
    }

    fn visit_for_statement(&mut self, node: &ForStatement) {
        let loop_vars = node.get_loop_variables().clone();
        let has_idx_val = loop_vars.len() >= 2;

        if let Some(iter) = node.get_iterable() {
            iter.accept(self);
            let iterable = self.pop();

            match iterable {
                RtValue::Array(elems) => {
                    self.env.enter_scope();
                    for (i, elem) in elems.iter().enumerate() {
                        self.env.declare(&loop_vars[0], if has_idx_val { RtValue::Int(i as i64) } else { elem.clone() });
                        if has_idx_val {
                            self.env.declare(&loop_vars[1], elem.clone());
                        }
                        if let Some(body) = node.get_body() {
                            body.accept(self);
                        }
                        if self.returning { break; }
                        if self.continuing { self.continuing = false; continue; }
                        if self.breaking { self.breaking = false; break; }
                    }
                    self.env.exit_scope();
                }
                RtValue::Str(s) => {
                    self.env.enter_scope();
                    let chars: Vec<char> = s.chars().collect();
                    for (i, ch) in chars.iter().enumerate() {
                        let ch_str = RtValue::Str(ch.to_string());
                        self.env.declare(&loop_vars[0], if has_idx_val { RtValue::Int(i as i64) } else { ch_str.clone() });
                        if has_idx_val {
                            self.env.declare(&loop_vars[1], ch_str.clone());
                        }
                        if let Some(body) = node.get_body() {
                            body.accept(self);
                        }
                        if self.returning { break; }
                        if self.continuing { self.continuing = false; continue; }
                        if self.breaking { self.breaking = false; break; }
                    }
                    self.env.exit_scope();
                }
                RtValue::Struct(ref name, ref fields) if name == "range" => {
                    let get_int = |key: &str, default: i64| {
                        fields.get(key)
                            .and_then(|v| if let RtValue::Int(n) = v { Some(*n) } else { None })
                            .unwrap_or(default)
                    };
                    let start = get_int("_start", 0);
                    let end = get_int("_end", 0);
                    let step = get_int("_step", 1);

                    self.env.enter_scope();
                    let mut i = start;
                    if step > 0 {
                        while i < end {
                            self.env.declare(&loop_vars[0], RtValue::Int(i));
                            if has_idx_val {
                                self.env.declare(&loop_vars[1], RtValue::Int(i));
                            }
                            if let Some(body) = node.get_body() {
                                body.accept(self);
                            }
                            if self.returning { break; }
                            if self.continuing { self.continuing = false; i += step; continue; }
                            if self.breaking { self.breaking = false; break; }
                            i += step;
                        }
                    } else if step < 0 {
                        while i > end {
                            self.env.declare(&loop_vars[0], RtValue::Int(i));
                            if has_idx_val {
                                self.env.declare(&loop_vars[1], RtValue::Int(i));
                            }
                            if let Some(body) = node.get_body() {
                                body.accept(self);
                            }
                            if self.returning { break; }
                            if self.continuing { self.continuing = false; i += step; continue; }
                            if self.breaking { self.breaking = false; break; }
                            i += step;
                        }
                    }
                    self.env.exit_scope();
                }
                other => {
                    self.error(format!(
                        "For loop iterable must be an array, string, or range, got {}",
                        other.type_name()
                    ));
                }
            }
        }
    }

    fn visit_break_statement(&mut self, _node: &BreakStatement) {
        self.breaking = true;
    }

    fn visit_continue_statement(&mut self, _node: &ContinueStatement) {
        self.continuing = true;
    }

    // ==================== Expressions ====================

    fn visit_binary_expression(&mut self, node: &BinaryExpression) {
        let op = node.get_operator();

        // Assignment and compound assignment
        if op == "=" || op == "+=" || op == "-=" || op == "*=" || op == "/=" {
            if let Some(right) = node.get_right() {
                right.accept(self);
                let mut value = self.pop();

                // For compound assignments, compute left + operation
                if op != "=" {
                    // Evaluate left side value
                    if let Some(left) = node.get_left() {
                        if let Some(id) = left.as_any().downcast_ref::<Identifier>() {
                            let name = id.get_name();
                            let left_val = self.env.lookup(name).cloned().or_else(|| {
                                // Check struct field
                                self.current_impl_struct.as_ref().and_then(|_s| {
                                    self.env.lookup("self").and_then(|sv| {
                                        if let RtValue::Struct(_, fields) = sv {
                                            fields.get(name).cloned()
                                        } else { None }
                                    })
                                })
                            });
                            if let Some(left_val) = left_val {
                                let arith_op: &str = &op[..1]; // "+=" -> "+"
                                value = match arith_op {
                                    "+" => match (&left_val, &value) {
                                        (RtValue::Int(a), RtValue::Int(b)) => RtValue::Int(a + b),
                                        (RtValue::Float(a), RtValue::Float(b)) => RtValue::Float(a + b),
                                        (RtValue::Int(a), RtValue::Float(b)) => RtValue::Float(*a as f64 + b),
                                        (RtValue::Float(a), RtValue::Int(b)) => RtValue::Float(a + *b as f64),
                                        _ => {
                                            self.error(format!("Cannot += on non-numeric types"));
                                            RtValue::None_
                                        }
                                    },
                                    "-" => binary_arith(&left_val, &value, |a, b| a - b),
                                    "*" => binary_arith(&left_val, &value, |a, b| a * b),
                                    "/" => match (&left_val, &value) {
                                        (RtValue::Int(a), RtValue::Int(b)) => {
                                            if *b == 0 {
                                                self.error("Division by zero".to_string());
                                                RtValue::None_
                                            } else {
                                                RtValue::Int(a / b)
                                            }
                                        }
                                        (a, b) => {
                                            if b.to_number() == 0.0 {
                                                self.error("Division by zero".to_string());
                                                RtValue::None_
                                            } else {
                                                binary_arith(a, b, |x, y| x / y)
                                            }
                                        }
                                    },
                                    _ => {
                                        self.error(format!("Unknown compound operator: {}", op));
                                        RtValue::None_
                                    }
                                };
                            }
                        }
                    }
                }

                // Left side must be an identifier or array index
                if let Some(left) = node.get_left() {
                    if let Some(id) = left.as_any().downcast_ref::<Identifier>() {
                        let name = id.get_name();
                        // Check if it's a struct field in the current impl block
                        let is_field = self.current_impl_struct.as_ref().map_or(false, |s| {
                            self.struct_definitions.get(s).map_or(false, |fields| fields.contains(&name.to_string()))
                        });
                        if is_field {
                            // Field assignment via self — update the self struct in env
                            if let Some(self_val) = self.env.lookup("self") {
                                if let RtValue::Struct(type_name, fields) = self_val {
                                    let mut new_fields = fields.clone();
                                    new_fields.insert(name.to_string(), value.clone());
                                    self.env.assign("self", RtValue::Struct(type_name.clone(), new_fields));
                                }
                            }
                            self.push(value);
                            return;
                        }
                        if !self.env.assign(name, value.clone()) {
                            self.error(format!("Cannot assign to undeclared variable '{}'", name));
                        }
                        self.push(value);
                        return;
                    }

                    if let Some(arr_idx) = left.as_any().downcast_ref::<ArrayIndex>() {
                        // Evaluate array and index
                        self.assign_array_element(arr_idx, value.clone());
                        self.push(value);
                        return;
                    }
                }

                self.error("Invalid assignment target".to_string());
                self.push(RtValue::None_);
            }
            return;
        }

        // Logical short-circuit
        if op == "&&" {
            if let Some(left) = node.get_left() {
                left.accept(self);
            }
            let left_val = self.pop();
            if !left_val.is_truthy() {
                self.push(RtValue::Bool(false));
                return;
            }
            if let Some(right) = node.get_right() {
                right.accept(self);
            }
            let right_val = self.pop();
            self.push(RtValue::Bool(right_val.is_truthy()));
            return;
        }

        if op == "||" {
            if let Some(left) = node.get_left() {
                left.accept(self);
            }
            let left_val = self.pop();
            if left_val.is_truthy() {
                self.push(RtValue::Bool(true));
                return;
            }
            if let Some(right) = node.get_right() {
                right.accept(self);
            }
            let right_val = self.pop();
            self.push(RtValue::Bool(right_val.is_truthy()));
            return;
        }

        // Evaluate both sides
        if let Some(left) = node.get_left() {
            left.accept(self);
        }
        let left_val = self.pop();
        if let Some(right) = node.get_right() {
            right.accept(self);
        }
        let right_val = self.pop();

        let result = match op {
            "+" => match (&left_val, &right_val) {
                (RtValue::Str(a), RtValue::Str(b)) => RtValue::Str(format!("{}{}", a, b)),
                (RtValue::Str(a), b) => RtValue::Str(format!("{}{}", a, b.to_string_val())),
                (a, RtValue::Str(b)) => RtValue::Str(format!("{}{}", a.to_string_val(), b)),
                (a, b) => {
                    let (la, lb) = (a.to_number(), b.to_number());
                    if matches!(a, RtValue::Float(_)) || matches!(b, RtValue::Float(_)) {
                        RtValue::Float(la + lb)
                    } else {
                        RtValue::Int((la + lb) as i64)
                    }
                }
            },
            "-" => binary_arith(&left_val, &right_val, |a, b| a - b),
            "*" => binary_arith(&left_val, &right_val, |a, b| a * b),
            "/" => match (&left_val, &right_val) {
                (RtValue::Int(a), RtValue::Int(b)) => {
                    if *b == 0 {
                        self.error("Division by zero".to_string());
                        RtValue::None_
                    } else {
                        RtValue::Int(a / b)
                    }
                }
                (a, b) => {
                    let rhs = b.to_number();
                    if rhs == 0.0 {
                        self.error("Division by zero".to_string());
                        RtValue::None_
                    } else {
                        binary_arith(a, b, |x, y| x / y)
                    }
                }
            },
            "%" => match (&left_val, &right_val) {
                (RtValue::Int(a), RtValue::Int(b)) => {
                    if *b == 0 {
                        self.error("Modulo by zero".to_string());
                        RtValue::None_
                    } else {
                        RtValue::Int(a % b)
                    }
                }
                (a, b) => {
                    let rhs = b.to_number();
                    if rhs == 0.0 {
                        self.error("Modulo by zero".to_string());
                        RtValue::None_
                    } else {
                        binary_arith(a, b, |x, y| x % y)
                    }
                }
            }
            "==" => RtValue::Bool(values_equal(&left_val, &right_val)),
            "!=" => RtValue::Bool(!values_equal(&left_val, &right_val)),
            "<" => RtValue::Bool(left_val.to_number() < right_val.to_number()),
            ">" => RtValue::Bool(left_val.to_number() > right_val.to_number()),
            "<=" => RtValue::Bool(left_val.to_number() <= right_val.to_number()),
            ">=" => RtValue::Bool(left_val.to_number() >= right_val.to_number()),
            _ => {
                self.error(format!("Unknown operator: {}", op));
                RtValue::None_
            }
        };
        self.push(result);
    }

    fn visit_cast_expression(&mut self, node: &CastExpression) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
        let value = self.pop();
        let target = node.get_target_type().get_name();

        let result = match target {
            "int" => match &value {
                RtValue::Int(_) => value,
                RtValue::Float(f) => RtValue::Int(*f as i64),
                RtValue::Bool(b) => RtValue::Int(if *b { 1 } else { 0 }),
                RtValue::Str(s) => {
                    match s.parse::<i64>() {
                        Ok(n) => RtValue::Int(n),
                        Err(_) => {
                            self.error(format!("Cannot cast '{}' to int", s));
                            RtValue::None_
                        }
                    }
                }
                _ => {
                    self.error(format!("Cannot cast {} to int", value.type_name()));
                    RtValue::None_
                }
            },
            "float" => match &value {
                RtValue::Float(_) => value,
                RtValue::Int(n) => RtValue::Float(*n as f64),
                RtValue::Str(s) => {
                    match s.parse::<f64>() {
                        Ok(f) => RtValue::Float(f),
                        Err(_) => {
                            self.error(format!("Cannot cast '{}' to float", s));
                            RtValue::None_
                        }
                    }
                }
                _ => {
                    self.error(format!("Cannot cast {} to float", value.type_name()));
                    RtValue::None_
                }
            },
            "str" => RtValue::Str(value.to_string_val()),
            "bool" => RtValue::Bool(value.is_truthy()),
            _ => {
                self.error(format!("Cannot cast to unknown type '{}'", target));
                RtValue::None_
            }
        };
        self.push(result);
    }

    fn visit_unary_expression(&mut self, node: &UnaryExpression) {
        if let Some(operand) = node.get_operand() {
            operand.accept(self);
        }
        let val = self.pop();
        let op = node.get_operator();

        let result = match op {
            "-" => match val {
                RtValue::Int(n) => RtValue::Int(-n),
                RtValue::Float(f) => RtValue::Float(-f),
                _ => {
                    self.error(format!("Cannot negate type {}", val.type_name()));
                    RtValue::None_
                }
            },
            "+" => val,
            "!" => RtValue::Bool(!val.is_truthy()),
            _ => {
                self.error(format!("Unknown unary operator: {}", op));
                RtValue::None_
            }
        };
        self.push(result);
    }

    fn visit_identifier(&mut self, node: &Identifier) {
        let name = node.get_name();
        // Check if it's a bare struct field access inside an impl method
        if self.current_impl_struct.is_some() {
            if let Some(self_val) = self.env.lookup("self") {
                if let RtValue::Struct(_, fields) = self_val {
                    if fields.contains_key(name) {
                        self.push(fields[name].clone());
                        return;
                    }
                }
            }
        }
        match self.env.lookup(name) {
            Some(v) => self.push(v.clone()),
            None => {
                self.error(format!("Undefined variable: '{}'", name));
                self.push(RtValue::None_);
            }
        }
    }

    fn visit_number_literal(&mut self, node: &NumberLiteral) {
        let val = node.get_value();
        if val == (val as i64) as f64 {
            self.push(RtValue::Int(val as i64));
        } else {
            self.push(RtValue::Float(val));
        }
    }

    fn visit_string_literal(&mut self, node: &StringLiteral) {
        self.push(RtValue::Str(node.get_value().to_string()));
    }

    fn visit_null_literal(&mut self, _node: &NullLiteral) {
        self.push(RtValue::None_);
    }

    fn visit_array_literal(&mut self, node: &ArrayLiteral) {
        let mut elems: Vec<RtValue> = Vec::new();
        for elem in node.get_elements() {
            elem.accept(self);
            elems.push(self.pop());
        }
        self.push(RtValue::Array(elems));
    }

    fn visit_boolean_literal(&mut self, node: &BooleanLiteral) {
        self.push(RtValue::Bool(node.get_value()));
    }

    fn visit_format_string(&mut self, node: &FormatString) {
        let template = node.get_value().to_string();
        let vars = node.get_variables();

        // Build a position-to-variable map
        let mut var_map: std::collections::HashMap<usize, &VariablePosition> = std::collections::HashMap::new();
        for var in vars {
            var_map.insert(var.pos_in_value as usize, var);
        }

        let mut result = String::new();
        let chars: Vec<char> = template.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '{' && var_map.contains_key(&i) {
                // Found a variable to substitute
                if let Some(ref expr) = var_map[&i].value {
                    expr.accept(self);
                    let val = self.pop();
                    result.push_str(&val.to_string_val());
                }
                // Skip to after the closing brace
                while i < chars.len() && chars[i] != '}' {
                    i += 1;
                }
                if i < chars.len() {
                    i += 1; // skip '}'
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        self.push(RtValue::Str(result));
    }

    fn visit_function_call(&mut self, node: &FunctionCall) {
        let func_name = self.resolve_function_name(node);

        // Collect arguments. For method calls (obj.method), prepend self
        let mut args: Vec<RtValue> = Vec::new();
        let mut method_target: Option<String> = None;
        let mut array_arg_vars: Vec<(usize, String)> = Vec::new();

        // Check if this is a method call — prepend self, track var name
        if let Some(callee) = node.get_callee() {
            if let Some(member) = callee.as_any().downcast_ref::<MemberAccess>() {
                if let Some(obj) = member.get_object() {
                    if let Some(obj_id) = obj.as_any().downcast_ref::<Identifier>() {
                        if self.env.lookup(obj_id.get_name()).map_or(false, |v| !matches!(v, RtValue::None_)) {
                            method_target = Some(obj_id.get_name().to_string());
                            obj.accept(self);
                            args.push(self.pop());
                        }
                    }
                }
            }
        }

        // Evaluate explicit arguments, tracking array variable names for copy-back
        if let Some(arg_list) = node.get_arguments() {
            for (i, arg) in arg_list.iter().enumerate() {
                // Track if this argument is a simple variable (for copy-back)
                if let Some(id) = arg.as_any().downcast_ref::<Identifier>() {
                    let var_name = id.get_name();
                    if self.env.lookup(var_name).map_or(false, |v| matches!(v, RtValue::Array(_))) {
                        array_arg_vars.push((i, var_name.to_string()));
                    }
                }
                arg.accept(self);
                args.push(self.pop());
            }
        }

        // Handle array methods (len, add) directly
        if args.len() >= 1 && matches!(&args[0], RtValue::Array(_)) {
            let method = if let Some(dot) = func_name.rfind('.') {
                &func_name[dot + 1..]
            } else {
                &func_name
            };
            match method {
                "len" => {
                    if let RtValue::Array(elems) = &args[0] {
                        self.push(RtValue::Int(elems.len() as i64));
                        return;
                    }
                }
                "add" => {
                    if args.len() >= 2 {
                        let val = args.remove(1);
                        if let RtValue::Array(mut elems) = args.remove(0) {
                            elems.push(val);
                            // Update the variable in env
                            if let Some(ref var_name) = method_target {
                                self.env.assign(var_name, RtValue::Array(elems.clone()));
                            }
                            self.push(RtValue::Array(elems));
                            return;
                        }
                    }
                }
                _ => {}
            }
        }

        // Check if it's a struct constructor call (e.g. Point(1, 2))
        let short_name = if let Some(dot) = func_name.rfind('.') {
            &func_name[dot + 1..]
        } else {
            &func_name
        };

        if let Some(field_names) = self.struct_definitions.get(short_name) {
            // Direct struct creation by positional field matching
            let mut fields = HashMap::new();
            for (i, field_name) in field_names.iter().enumerate() {
                let value = if i < args.len() {
                    args[i].clone()
                } else {
                    RtValue::None_
                };
                fields.insert(field_name.clone(), value);
            }
            self.push(RtValue::Struct(short_name.to_string(), fields));
            return;
        }

        // Try built-in
        match self.builtins.call(&func_name, &args) {
            Ok(val) => {
                self.push(val);
                return;
            }
            Err(_) => {}
        }

        // Extract impl struct from qualified name (e.g. "range.constructor" → "range")
        let impl_struct = func_name.rfind('.').map(|i| &func_name[..i]);

        // Try user-defined function (qualified name first, then short name)
        if let Some(&func_ptr) = self.user_functions.get(&func_name) {
            let func = unsafe { &*func_ptr };
            self.call_user_function_with_copyback(func, &args, &array_arg_vars, impl_struct);
            return;
        }
        if let Some(&func_ptr) = self.user_functions.get(short_name) {
            let func = unsafe { &*func_ptr };
            self.call_user_function_with_copyback(func, &args, &array_arg_vars, impl_struct);
            return;
        }

        self.error(format!("Undefined function: '{}'", func_name));
        self.push(RtValue::None_);
    }

    fn visit_match_expression(&mut self, node: &MatchExpression) {
        let was_expr = self.expression_depth > 0;
        self.expression_depth += 1;

        // Evaluate scrutinee
        if let Some(scrut) = node.get_scrutinee() {
            scrut.accept(self);
        }
        let scrut_val = self.pop();

        // Try each arm
        let mut matched = false;
        for arm in node.get_arms() {
            let matches = match &arm.pattern {
                MatchPattern::Wildcard => true,
                MatchPattern::Variable(name) => {
                    self.env.enter_scope();
                    self.env.declare(name, scrut_val.clone());
                    true
                }
                MatchPattern::Literal(lit) => {
                    let lit_val = match lit {
                        RtValueSimple::Int(n) => RtValue::Int(*n),
                        RtValueSimple::FloatStr(s) => {
                            if let Ok(f) = s.parse::<f64>() { RtValue::Float(f) } else { RtValue::Str(s.clone()) }
                        }
                        RtValueSimple::Str(s) => RtValue::Str(s.clone()),
                        RtValueSimple::Bool(b) => RtValue::Bool(*b),
                    };
                    values_equal(&scrut_val, &lit_val)
                }
            };

            if matches {
                matched = true;
                if let Some(ref body) = arm.body {
                    body.accept(self);
                }
                if let MatchPattern::Variable(_) = &arm.pattern {
                    self.env.exit_scope();
                }
                break;
            }

            if let MatchPattern::Variable(_) = &arm.pattern {
                self.env.exit_scope();
            }
        }

        if !matched {
            self.error(format!("Match error: no arm matched value {}", scrut_val));
            self.push(RtValue::None_);
        }

        self.expression_depth -= 1;
        if !was_expr && !self.value_stack.is_empty() {
            let _ = self.pop();
        }
    }

    fn visit_struct_literal(&mut self, node: &StructLiteral) {
        let type_name = node.get_type_name().to_string();

        // Get struct field definitions
        let field_names = match self.struct_definitions.get(&type_name) {
            Some(names) => names.clone(),
            None => {
                self.error(format!("Unknown struct type: '{}'", type_name));
                self.push(RtValue::None_);
                return;
            }
        };

        // Initialize all fields to default values
        let mut fields: HashMap<String, RtValue> = HashMap::new();
        for fname in &field_names {
            fields.insert(fname.clone(), RtValue::Int(0));
        }

        // Track which fields have been explicitly set (for positional matching)
        let mut assigned: HashSet<String> = HashSet::new();
        let mut positional_index: usize = 0;

        for field_init in node.get_fields() {
            match field_init {
                StructFieldInit::Named { name, value } => {
                    value.accept(self);
                    let val = self.pop();
                    fields.insert(name.clone(), val);
                    assigned.insert(name.clone());
                }
                StructFieldInit::Positional(value) => {
                    // Check if this is a spread (same-type struct)
                    let mut is_spread = false;
                    if let Some(id) = value.as_any().downcast_ref::<Identifier>() {
                        if let Some(existing) = self.env.lookup(id.get_name()) {
                            if let RtValue::Struct(existing_type, _) = existing {
                                if existing_type == &type_name {
                                    // Spread: copy all fields from the existing struct
                                    if let RtValue::Struct(_, existing_fields) = existing {
                                        for (k, v) in existing_fields {
                                            fields.insert(k.clone(), v.clone());
                                            assigned.insert(k.clone());
                                        }
                                    }
                                    is_spread = true;
                                }
                            }
                        }
                    }

                    if !is_spread {
                        // Positional: evaluate and assign to next unset field
                        value.accept(self);
                        let val = self.pop();

                        // Find next unassigned field
                        while positional_index < field_names.len()
                            && assigned.contains(&field_names[positional_index])
                        {
                            positional_index += 1;
                        }

                        if positional_index < field_names.len() {
                            let fname = &field_names[positional_index];
                            fields.insert(fname.clone(), val);
                            assigned.insert(fname.clone());
                            positional_index += 1;
                        }
                    }
                }
            }
        }

        self.push(RtValue::Struct(type_name, fields));
    }

    fn visit_member_access(&mut self, node: &MemberAccess) {
        // Evaluate object
        if let Some(obj) = node.get_object() {
            obj.accept(self);
        }
        let obj_val = self.pop();
        let member = node.get_member();

        // Handle struct field access
        if let RtValue::Struct(_, fields) = &obj_val {
            if let Some(field_val) = fields.get(member) {
                self.push(field_val.clone());
                return;
            }
            self.error(format!("Struct has no field '{}'", member));
            self.push(RtValue::None_);
            return;
        }

        // Look up as module.member (e.g. io.print)
        if let Some(obj) = node.get_object() {
            if let Some(id) = obj.as_any().downcast_ref::<Identifier>() {
                let full_name = format!("{}.{}", id.get_name(), member);
                if self.builtins.functions.contains_key(&full_name) {
                    self.push(RtValue::Str(full_name));
                    return;
                }
            }
        }

        self.push(RtValue::None_);
    }

    fn visit_array_index(&mut self, node: &ArrayIndex) {
        // Evaluate the array
        if let Some(arr_expr) = node.get_array() {
            arr_expr.accept(self);
        }
        let arr = self.pop();

        // Evaluate the index
        if let Some(idx_expr) = node.get_index() {
            idx_expr.accept(self);
        }
        let idx = self.pop();

        match (&arr, &idx) {
            (RtValue::Array(elems), RtValue::Int(n)) => {
                let i = *n as usize;
                if i < elems.len() {
                    self.push(elems[i].clone());
                } else {
                    self.error(format!(
                        "Array index out of bounds: index={}, length={}",
                        i,
                        elems.len()
                    ));
                    self.push(RtValue::None_);
                }
            }
            (RtValue::Array(_), _) => {
                self.error("Array index must be an integer".to_string());
                self.push(RtValue::None_);
            }
            _ => {
                self.error(format!("Cannot index into value of type '{}'", arr.type_name()));
                self.push(RtValue::None_);
            }
        }
    }

    fn visit_grouped_expression(&mut self, node: &GroupedExpression) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
    }

    fn visit_range_expression(&mut self, node: &RangeExpression) {
        let mut args: Vec<RtValue> = Vec::new();
        for arg in node.get_arguments() {
            arg.accept(self);
            args.push(self.pop());
        }
        // Create a range struct (defined in lib/range.gbl): { _start, _end, _step }
        let start = args.first().cloned().unwrap_or(RtValue::Int(0));
        let end = args.get(1).cloned().unwrap_or(RtValue::Int(0));
        let step = if args.len() >= 3 {
            args[2].clone()
        } else {
            let s = start.to_number();
            let e = end.to_number();
            if s > e { RtValue::Int(-1) } else { RtValue::Int(1) }
        };
        let mut fields = HashMap::new();
        fields.insert("_start".to_string(), start);
        fields.insert("_end".to_string(), end);
        fields.insert("_step".to_string(), step);
        self.push(RtValue::Struct("range".to_string(), fields));
    }

    // Default empty visitors
    fn visit_ast_node(&mut self, _node: &dyn AstNode) {}
    fn visit_statement(&mut self, _node: &dyn Statement) {}
    fn visit_expression(&mut self, _node: &dyn Expression) {}
    fn visit_parameter(&mut self, _node: &Parameter) {}
    fn visit_basic_type(&mut self, _node: &BasicType) {}
    fn visit_type(&mut self, _node: &dyn Type) {}
    fn visit_array_type(&mut self, _node: &ArrayType) {}
}

// ==================== Helpers ====================

fn binary_arith<F>(a: &RtValue, b: &RtValue, op: F) -> RtValue
where
    F: Fn(f64, f64) -> f64,
{
    let (la, lb) = (a.to_number(), b.to_number());
    let result = op(la, lb);
    if result == (result as i64) as f64 && matches!(a, RtValue::Int(_)) && matches!(b, RtValue::Int(_))
    {
        RtValue::Int(result as i64)
    } else {
        RtValue::Float(result)
    }
}

fn values_equal(a: &RtValue, b: &RtValue) -> bool {
    match (a, b) {
        (RtValue::Int(a), RtValue::Int(b)) => a == b,
        (RtValue::Int(a), RtValue::Float(b)) => *a as f64 == *b,
        (RtValue::Float(a), RtValue::Int(b)) => *a == *b as f64,
        (RtValue::Float(a), RtValue::Float(b)) => a == b,
        (RtValue::Bool(a), RtValue::Bool(b)) => a == b,
        (RtValue::Str(a), RtValue::Str(b)) => a == b,
        (RtValue::Array(a), RtValue::Array(b)) => {
            if a.len() != b.len() {
                return false;
            }
            a.iter().zip(b.iter()).all(|(x, y)| values_equal(x, y))
        }
        (RtValue::None_, RtValue::None_) => true,
        _ => false,
    }
}

impl Executor {
    fn declare_array(&mut self, name: &str, arr_type: &ArrayType, keyword: &str) {
        let _is_mut = keyword == "var";

        let value = self.build_array_value(arr_type);
        self.env.declare(name, value);
    }

    fn build_array_value(&self, arr_type: &ArrayType) -> RtValue {
        let elem_count = match arr_type.get_size() {
            Some(size) => {
                // For compile-time constants, we can get the value from the NumberLiteral
                if let Some(num) = size.as_any().downcast_ref::<NumberLiteral>() {
                    num.get_value() as usize
                } else {
                    0
                }
            }
            None => 0,
        };

        let element_type = arr_type.get_element_type();
        let inner = if let Some(inner_arr) = element_type.as_type_any().downcast_ref::<ArrayType>() {
            // Multi-dimensional: each element is itself an array
            self.build_array_value(inner_arr)
        } else {
            RtValue::None_
        };

        RtValue::Array(vec![inner; elem_count])
    }

    fn assign_array_element(&mut self, arr_idx: &ArrayIndex, value: RtValue) {
        // Walk the index chain to get the innermost array and index
        if let Some(arr_expr) = arr_idx.get_array() {
            if let Some(idx_expr) = arr_idx.get_index() {
                // Check if nested (arr[2][2])
                if let Some(inner) = arr_expr.as_any().downcast_ref::<ArrayIndex>() {
                    // Evaluate inner array reference first
                    // We need to dig down to the base array
                    let base = self.resolve_base_array(inner);
                    let indices = self.collect_indices(arr_idx);

                    if let Some(base_name) = base {
                        if let Some(base_val) = self.env.lookup(&base_name) {
                            let mut new_val = base_val.clone();
                            if self.set_array_element(&mut new_val, &indices, &value) {
                                self.env.assign(&base_name, new_val);
                            }
                        }
                    }
                    return;
                }

                // Simple case: arr[idx]
                if let Some(id) = arr_expr.as_any().downcast_ref::<Identifier>() {
                    let name = id.get_name();
                    idx_expr.accept(self);
                    let idx_val = self.pop();
                    if let RtValue::Int(n) = idx_val {
                        if let Some(arr_val) = self.env.lookup(name) {
                            if let RtValue::Array(elems) = arr_val {
                                let mut new_elems = elems.clone();
                                let i = n as usize;
                                if i < new_elems.len() {
                                    new_elems[i] = value;
                                    self.env.assign(name, RtValue::Array(new_elems));
                                } else {
                                    self.error(format!("Array index out of bounds: {}", i));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn resolve_base_array(&self, arr: &ArrayIndex) -> Option<String> {
        if let Some(arr_expr) = arr.get_array() {
            if let Some(id) = arr_expr.as_any().downcast_ref::<Identifier>() {
                return Some(id.get_name().to_string());
            }
            if let Some(inner) = arr_expr.as_any().downcast_ref::<ArrayIndex>() {
                return self.resolve_base_array(inner);
            }
        }
        None
    }

    fn collect_indices(&self, arr: &ArrayIndex) -> Vec<i64> {
        let mut indices = Vec::new();
        self.collect_indices_recursive(arr, &mut indices);
        indices
    }

    fn collect_indices_recursive(&self, arr: &ArrayIndex, indices: &mut Vec<i64>) {
        if let Some(inner) = arr.get_array().and_then(|a| {
            a.as_any().downcast_ref::<ArrayIndex>().map(|_| ())
        }) {
            if let Some(inner_arr) = arr.get_array().unwrap().as_any().downcast_ref::<ArrayIndex>() {
                self.collect_indices_recursive(inner_arr, indices);
            }
            let _ = inner;
        }
        // Evaluate this level's index
        if let Some(idx_expr) = arr.get_index() {
            // We can't call accept on self here because we only have &self
            // Instead, we evaluate the index directly if it's a literal
            if let Some(num) = idx_expr.as_any().downcast_ref::<NumberLiteral>() {
                indices.push(num.get_value() as i64);
            }
        }
    }

    fn set_array_element(&self, arr: &mut RtValue, indices: &[i64], value: &RtValue) -> bool {
        if indices.is_empty() {
            return false;
        }
        if indices.len() == 1 {
            let i = indices[0] as usize;
            if let RtValue::Array(elems) = arr {
                if i < elems.len() {
                    elems[i] = value.clone();
                    return true;
                }
            }
            return false;
        }
        let i = indices[0] as usize;
        if let RtValue::Array(elems) = arr {
            if i < elems.len() {
                return self.set_array_element(&mut elems[i], &indices[1..], value);
            }
        }
        false
    }

    fn resolve_function_name(&self, node: &FunctionCall) -> String {
        if let Some(callee) = node.get_callee() {
            if let Some(id) = callee.as_any().downcast_ref::<Identifier>() {
                let direct = id.get_name();
                // Try io.func for builtins
                let io_name = format!("io.{}", direct);
                if self.builtins.functions.contains_key(&io_name) {
                    return io_name;
                }
                let builtin_name = format!("__builtins__.{}", direct);
                if self.builtins.functions.contains_key(&builtin_name) {
                    return builtin_name;
                }
                // Try current_module.func for user functions
                let qualified = if self.current_module.is_empty() {
                    direct.to_string()
                } else {
                    format!("{}.{}", self.current_module, direct)
                };
                if self.user_functions.contains_key(&qualified) {
                    return qualified;
                }
                return direct.to_string();
            }
            if let Some(member) = callee.as_any().downcast_ref::<MemberAccess>() {
                if let Some(obj) = member.get_object() {
                    if let Some(obj_id) = obj.as_any().downcast_ref::<Identifier>() {
                        let obj_name = obj_id.get_name();
                        let method_name = member.get_member();
                        // Check if this is a method call on a runtime variable
                        if self.env.lookup(obj_name).map_or(false, |v| !matches!(v, RtValue::None_)) {
                            // Method dispatch: look up method by name
                            let qualified = if self.current_module.is_empty() {
                                method_name.to_string()
                            } else {
                                format!("{}.{}", self.current_module, method_name)
                            };
                            if self.user_functions.contains_key(&qualified) {
                                return qualified;
                            }
                            return method_name.to_string();
                        }
                        // Module-qualified call: module.func
                        return format!("{}.{}", obj_name, method_name);
                    }
                }
            }
        }
        String::new()
    }

    fn resolve_module_path(&self, path_parts: &[String], base_dir: Option<&str>) -> Option<String> {
        let relative = path_parts.join("/") + ".gbl";

        // First: check relative to the importing module's directory (if any)
        if let Some(dir) = base_dir {
            let rel_full = format!("{}/{}", dir, relative);
            if Path::new(&rel_full).exists() {
                return Some(rel_full);
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
            let full = format!("{}/{}", lib_path, relative);
            if Path::new(&full).exists() {
                return Some(full);
            }
            // Fallback: lib/X/__setup__.gbl
            let setup_relative = format!("{}/__setup__.gbl", path_parts.join("/"));
            let setup_full = format!("{}/{}", lib_path, setup_relative);
            if Path::new(&setup_full).exists() {
                return Some(setup_full);
            }
        }
        // Third: try without lib prefix
        let direct = format!("{}.gbl", path_parts.join("/"));
        if Path::new(&direct).exists() {
            return Some(direct);
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

        let path_parts: Vec<String> = module_name.split('.').map(|s| s.to_string()).collect();
        let base_dir = self.current_module_dir.clone();
        let file_path = match self.resolve_module_path(&path_parts, base_dir.as_deref()) {
            Some(p) => p,
            None => {
                if module_name == "__builtins__" {
                    self.loaded_modules.insert(module_name.to_string());
                    return;
                }
                return;
            }
        };

        let source = match fs::read_to_string(&file_path) {
            Ok(s) => s,
            Err(e) => {
                self.error(format!("Cannot read module '{}': {}", module_name, e));
                return;
            }
        };

        let lexer = Lexer::new(source);
        let mut builder = AstBuilder::new(lexer);
        let prog = match builder.build() {
            Some(p) => p,
            None => {
                for msg in builder.get_error_message() {
                    self.error(format!("Parse error in '{}': {}", module_name, msg));
                }
                return;
            }
        };

        // Save context for nested module loading
        let prev_dir = self.current_module_dir.clone();
        let prev_module = self.current_module.clone();
        if let Some(parent) = Path::new(&file_path).parent() {
            self.current_module_dir = parent.to_str().map(|s| s.to_string());
        }

        // Derive module name from file path
        if let Some(stem) = Path::new(&file_path).file_stem().and_then(|s| s.to_str()) {
            self.current_module = stem.to_string();
        }

        self.loaded_modules.insert(module_name.to_string());
        self.load_program_declarations(&prog);
        self.loaded_programs.push(prog);

        self.current_module_dir = prev_dir;
        self.current_module = prev_module;
    }

    fn load_program_declarations(&mut self, program: &Program) {
        // First pass: register functions, structs, impls
        for stmt in program.get_statements() {
            if stmt.as_any().downcast_ref::<Function>().is_some()
                || stmt.as_any().downcast_ref::<StructDefinition>().is_some()
                || stmt.as_any().downcast_ref::<ImplBlock>().is_some()
            {
                stmt.accept(self);
            }
        }

        // Second pass: process module declarations, nested imports, and exports
        for stmt in program.get_statements() {
            if false {
                stmt.accept(self);
            } else if let Some(import_stmt) = stmt.as_any().downcast_ref::<ImportStatement>() {
                let name = import_stmt.get_module_name();
                self.load_module(&name);
                if let Some(alias) = import_stmt.get_alias() {
                    self.module_aliases.insert(alias.to_string(), name);
                }
            } else if let Some(export_stmt) = stmt.as_any().downcast_ref::<ExportStatement>() {
                for name in export_stmt.get_names() {
                    let parts: Vec<&str> = name.split('.').collect();
                    let short = parts.last().unwrap_or(&"");
                    let original_key = if parts.len() > 1 {
                        let mod_part = parts[0];
                        let resolved_mod = self.module_aliases.get(mod_part).map(|s| s.as_str()).unwrap_or(mod_part);
                        format!("{}.{}", resolved_mod, short)
                    } else {
                        name.clone()
                    };
                    let qualified = format!("{}.{}", self.current_module, short);
                    if !self.user_functions.contains_key(&qualified) {
                        if let Some(&func_ptr) = self.user_functions.get(&original_key) {
                            self.user_functions.insert(qualified, func_ptr);
                        }
                    }
                }
            }
        }
    }
}
