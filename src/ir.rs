// ir.rs
use crate::ast::*;
use crate::environment::DataType;
use std::collections::{HashMap, HashSet};

// ==================== IR 数据结构 ====================

#[derive(Debug, Clone)]
pub struct GobolIR {
    pub functions: Vec<IRFunction>,
    pub structs: Vec<IRStruct>,
    pub impls: Vec<IRImpl>,
    pub main_function: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IRFunction {
    pub name: String,
    pub generic_params: Vec<String>,
    pub params: Vec<IRParam>,
    pub return_type: DataType,
    pub body: Option<IRBlock>,
    pub is_main: bool,
    pub is_method: bool,
    pub struct_name: Option<String>,
    pub attributes: Vec<String>,
    /// extern "C" variadic functions (declared with `...`) set this to true.
    pub is_variadic: bool,
}

#[derive(Debug, Clone)]
pub struct IRParam {
    pub name: String,
    pub ty: DataType,
}

#[derive(Debug, Clone)]
pub struct IRStruct {
    pub name: String,
    pub generic_params: Vec<String>,
    pub fields: Vec<IRField>,
    /// Simple string attributes (e.g. "no_gc", "internal").
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IRField {
    pub name: String,
    pub ty: DataType,
}

#[derive(Debug, Clone)]
pub struct IRImpl {
    pub struct_name: String,
    pub generic_params: Vec<String>,
    pub methods: Vec<IRFunction>,
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IRBlock {
    pub statements: Vec<IRStmt>,
}

#[derive(Debug, Clone)]
pub enum IRStmt {
    Declaration { name: String, ty: DataType, init: Option<IRExpr> },
    Expression(IRExpr),
    Return(Option<IRExpr>),
    If { cond: IRExpr, then_block: IRBlock, else_block: Option<IRBlock> },
    While { cond: IRExpr, body: IRBlock },
    Break,
    Continue,
    Assignment { target: IRExpr, value: IRExpr },
    Call { func: String, args: Vec<IRExpr>, generic_args: Vec<DataType> },
    MethodCall { object: Box<IRExpr>, method: String, args: Vec<IRExpr>, generic_args: Vec<DataType> },
    For { vars: Vec<String>, iterable: IRExpr, body: IRBlock },
}

#[derive(Debug, Clone)]
pub enum IRExpr {
    Literal(LitValue),
    Variable(String),
    Binary { op: String, left: Box<IRExpr>, right: Box<IRExpr> },
    Unary { op: String, operand: Box<IRExpr> },
    Call { func: String, args: Vec<IRExpr>, generic_args: Vec<DataType> },
    MethodCall { object: Box<IRExpr>, method: String, args: Vec<IRExpr>, generic_args: Vec<DataType> },
    MemberAccess { object: Box<IRExpr>, member: String },
    ArrayIndex { array: Box<IRExpr>, index: Box<IRExpr> },
    ArrayLiteral(Vec<IRExpr>),
    StructLiteral { name: String, fields: Vec<(String, IRExpr)> },
    Cast { expr: Box<IRExpr>, target: DataType },
    Assignment { target: Box<IRExpr>, value: Box<IRExpr> },
    /// Address of a named function (used to pass functions as arguments).
    FuncRef(String),
    /// Call a function through a pointer value (e.g. a `func(...)` parameter).
    IndirectCall { callee: Box<IRExpr>, args: Vec<IRExpr> },
    None,
}

#[derive(Debug, Clone)]
pub enum LitValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    None,
}

// ==================== IR 构建器 ====================

pub struct IRBuilder {
    // 输出
    ir: GobolIR,
    
    // 当前状态
    current_function: Option<String>,
    current_struct: Option<String>,
    current_function_return: DataType,
    current_ir_function: Option<IRFunction>,
    current_block: Vec<IRStmt>,
    expr_stack: Vec<IRExpr>,
    in_function: bool,
    in_impl: bool,
    block_depth: usize,
    
    // 泛型上下文
    generic_stack: Vec<HashMap<String, DataType>>,
    
    // 类型环境
    structs: HashMap<String, IRStruct>,
    methods: HashMap<String, Vec<IRFunction>>,
    
    // 错误收集
    errors: Vec<String>,

    // #[expand] functions: name → (param names, body IR)
    expand_functions: HashMap<String, (Vec<String>, IRBlock)>,
    // Current file path for file() macro in #[expand] context
    current_file: String,
    // Counter for generating unique temp variable names (e.g. __try_tmp_N)
    tmp_counter: usize,
    // Counter for generating unique anonymous lambda function names (__lambda_N)
    lambda_counter: usize,
    // Tracks local variable types in the current function scope so that
    // captured variables (free vars in lambdas) can be typed correctly.
    var_types: HashMap<String, DataType>,
}

impl IRBuilder {
    pub fn new() -> Self {
        IRBuilder {
            ir: GobolIR {
                functions: Vec::new(),
                structs: Vec::new(),
                impls: Vec::new(),
                main_function: None,
            },
            current_function: None,
            current_struct: None,
            current_function_return: DataType::None_,
            current_ir_function: None,
            current_block: Vec::new(),
            expr_stack: Vec::new(),
            in_function: false,
            in_impl: false,
            block_depth: 0,
            generic_stack: Vec::new(),
            structs: HashMap::new(),
            methods: HashMap::new(),
            errors: Vec::new(),
            expand_functions: HashMap::new(),
            current_file: String::new(),
            tmp_counter: 0,
            lambda_counter: 0,
            var_types: HashMap::new(),
        }
    }

    /// Set the current source file path for `file()` macro in #[expand] context.
    pub fn set_current_file(&mut self, file: impl Into<String>) {
        self.current_file = file.into();
    }

    pub fn build(mut self, program: &Program) -> Result<GobolIR, Vec<String>> {
        // 第一遍：收集结构体定义
        for stmt in program.get_statements() {
            if stmt.as_any().downcast_ref::<StructDefinition>().is_some() {
                stmt.accept(&mut self);
            }
        }

        // 第二遍：收集 impl 块
        for stmt in program.get_statements() {
            if stmt.as_any().downcast_ref::<ImplBlock>().is_some() {
                stmt.accept(&mut self);
            }
        }

        // 第三遍：收集函数
        for stmt in program.get_statements() {
            if stmt.as_any().downcast_ref::<Function>().is_some() {
                stmt.accept(&mut self);
            }
        }

        // 第四遍：收集 extern 块
        for stmt in program.get_statements() {
            if let Some(extern_block) = stmt.as_any().downcast_ref::<ExternBlock>() {
                for func in extern_block.get_functions() {
                    let name = func.get_name().to_string();

                    let params: Vec<IRParam> = func.get_params()
                        .iter()
                        .map(|p| {
                            let pname = p.get_name().to_string();
                            let ty = self.ast_type_to_data_type(p.get_type());
                            IRParam { name: pname, ty }
                        })
                        .collect();

                    let return_type = self.ast_type_to_data_type(func.get_return_type());

                    self.ir.functions.push(IRFunction {
                        name: name.clone(),
                        generic_params: Vec::new(),
                        params,
                        return_type,
                        body: None,
                        is_main: false,
                        is_method: false,
                        struct_name: None,
                        attributes: vec!["extern".to_string()],
                        is_variadic: func.is_variadic(),
                    });
                }
            }
        }

        if !self.errors.is_empty() {
            return Err(self.errors);
        }

        Ok(self.ir)
    }

    // ==================== 辅助方法 ====================

    fn push_expr(&mut self, expr: IRExpr) {
        self.expr_stack.push(expr);
    }

    fn pop_expr(&mut self) -> IRExpr {
        self.expr_stack.pop().unwrap_or(IRExpr::None)
    }

    /// Maps an operator string to its trait method name.
    /// Returns None for operators that should stay as Binary (logical, bitwise, etc.)
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
            _ => None, // &&, ||, &, |, ^ stay as Binary
        }
    }

    fn push_generic_scope(&mut self, params: &[String]) {
        let mut bindings = HashMap::new();
        for p in params {
            bindings.insert(p.clone(), DataType::Struct(p.clone()));
        }
        self.generic_stack.push(bindings);
    }

    fn pop_generic_scope(&mut self) {
        self.generic_stack.pop();
    }

    fn lookup_generic(&self, name: &str) -> Option<DataType> {
        for scope in self.generic_stack.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    fn ast_type_to_data_type(&mut self, ty: Option<&dyn Type>) -> DataType {
        let ty = match ty {
            Some(t) => t,
            None => return DataType::None_,
        };

        // 检查是否是泛型参数
        let name = ty.get_name();
        if let Some(binding) = self.lookup_generic(name) {
            return binding;
        }

        // 检查数组类型 → return Array with element type
        if let Some(arr) = ty.as_type_any().downcast_ref::<ArrayType>() {
            let elem = self.ast_type_to_data_type(Some(arr.get_element_type()));
            return DataType::Array(Box::new(elem));
        }

        // Function type `func(T1, T2): R` → function pointer (i64).
        if ty.as_type_any().downcast_ref::<FunctionType>().is_some() {
            return DataType::Unknown;
        }

        // 检查泛型类型（如 vec<int>）
        if let Some(gt) = ty.as_type_any().downcast_ref::<GenericType>() {
            let base_name = gt.get_base_name();
            // 如果是 vec，当作数组
            if base_name == "vec" && !gt.get_type_args().is_empty() {
                let elem = self.ast_type_to_data_type(Some(&*gt.get_type_args()[0]));
                return elem;
            }
            // 其他泛型类型当作结构体
            return DataType::Struct(base_name.to_string());
        }

        // 检查可空类型
        if let Some(nullable) = ty.as_type_any().downcast_ref::<NullableType>() {
            let inner = self.ast_type_to_data_type(Some(nullable.get_inner_type()));
            return DataType::Nullable(Box::new(inner));
        }

        // 基本类型
        match name {
            "int" => DataType::Int,
            "float" => DataType::Float,
            "bool" => DataType::Bool,
            "str" => DataType::Str,
            "none" => DataType::None_,
            name => DataType::Struct(name.to_string()),
        }
    }

    fn extract_generic_params(&self, func: &Function) -> Vec<String> {
        let mut params = Vec::new();
        
        if let Some(param_list) = func.get_parameters() {
            for p in param_list {
                self.collect_generic_names(p.get_type(), &mut params);
            }
        }
        
        if let Some(ret) = func.get_return_type() {
            self.collect_generic_names(Some(ret), &mut params);
        }
        
        params.dedup();
        params
    }

    fn collect_generic_names(&self, ty: Option<&dyn Type>, params: &mut Vec<String>) {
        let ty = match ty {
            Some(t) => t,
            None => return,
        };

        let name = ty.get_name();
        // Only treat as generic param if lowercase AND not a built-in type
        let is_builtin = matches!(name, "int" | "float" | "bool" | "str" | "none");
        if !is_builtin && name.chars().next().map_or(false, |c| c.is_lowercase()) {
            params.push(name.to_string());
        }

        // 检查嵌套类型
        if let Some(gt) = ty.as_type_any().downcast_ref::<GenericType>() {
            for arg in gt.get_type_args() {
                self.collect_generic_names(Some(arg.as_type()), params);
            }
        }

        if let Some(arr) = ty.as_type_any().downcast_ref::<ArrayType>() {
            self.collect_generic_names(Some(arr.get_element_type()), params);
        }

        if let Some(nullable) = ty.as_type_any().downcast_ref::<NullableType>() {
            self.collect_generic_names(Some(nullable.get_inner_type()), params);
        }
    }

    fn finish_function(&mut self) {
        if let Some(mut func) = self.current_ir_function.take() {
            // Bodyless declarations (e.g. `#[intrinsic(...)] func foo(...);`)
            // keep body = None so the backend knows to dispatch them to the
            // C runtime instead of compiling a (non-existent) body.
            if Self::has_attr(&func.attributes, "intrinsic") && self.current_block.is_empty() {
                func.body = None;
                std::mem::take(&mut self.current_block);
            } else {
                func.body = Some(IRBlock {
                    statements: std::mem::take(&mut self.current_block),
                });
            }
            if func.is_main {
                self.ir.main_function = Some(func.name.clone());
            }
            self.ir.functions.push(func);
        }
        self.in_function = false;
        self.current_function = None;
        self.current_function_return = DataType::None_;
    }

    fn build_match_condition(&mut self, scrutinee: &IRExpr, pattern: &MatchPattern) -> IRExpr {
        match pattern {
            MatchPattern::Wildcard => {
                IRExpr::Literal(LitValue::Bool(true))
            }
            MatchPattern::Literal(lit) => {
                let lit_expr = match lit {
                    RtValueSimple::Int(n) => IRExpr::Literal(LitValue::Int(*n)),
                    RtValueSimple::FloatStr(s) => {
                        if let Ok(f) = s.parse::<f64>() {
                            IRExpr::Literal(LitValue::Float(f))
                        } else {
                            IRExpr::Literal(LitValue::Str(s.clone()))
                        }
                    }
                    RtValueSimple::Str(s) => IRExpr::Literal(LitValue::Str(s.clone())),
                    RtValueSimple::Bool(b) => IRExpr::Literal(LitValue::Bool(*b)),
                };
                
                IRExpr::Binary {
                    op: "==".to_string(),
                    left: Box::new(scrutinee.clone()),
                    right: Box::new(lit_expr),
                }
            }
            MatchPattern::Variable(_name) => {
                // 变量模式总是匹配，在 body 中处理绑定
                IRExpr::Literal(LitValue::Bool(true))
            }
        }
    }

    #[allow(dead_code)]
    fn build_arm_body(&mut self, arm: &MatchArm) -> IRBlock {
        let mut block = IRBlock { statements: Vec::new() };

        // 如果是变量模式，在 body 中声明变量
        if let MatchPattern::Variable(name) = &arm.pattern {
            // 这里需要从 scrutinee 推导类型，暂时用 Unknown
            block.statements.push(IRStmt::Declaration {
                name: name.clone(),
                ty: DataType::Unknown,
                init: Some(self.pop_expr()), // 这里需要保留 scrutinee
            });
            // 实际上我们应该把 scrutinee 推回栈，因为上面已经 pop 了
            // 更好的方式是在构建条件时不消耗 scrutinee
        }

        // 处理 body
        if let Some(body) = &arm.body {
            // 使用子构建器处理 body
            let mut sub_builder = IRBuilder::new();
            sub_builder.generic_stack = self.generic_stack.clone();
            // Set block_depth > 1 so tail expressions don't generate Returns,
            // but instead leave values on the expression stack for the parent context
            sub_builder.block_depth = 2;

            if let Some(block_node) = body.as_any().downcast_ref::<Block>() {
                for stmt in block_node.get_statements() {
                    stmt.accept(&mut sub_builder);
                }
            } else {
                body.accept(&mut sub_builder);
            }

            // Extract the last expression value if a tail expression was processed
            let last_val = sub_builder.pop_expr();
            block.statements.extend(sub_builder.current_block);
            // Push the value as an expression statement so it gets captured
            if !matches!(last_val, IRExpr::None) {
                block.statements.push(IRStmt::Expression(last_val));
            }
        }

        block
    }

    #[allow(dead_code)]
    fn error(&mut self, msg: &str) {
        self.errors.push(msg.to_string());
    }

    /// Try to evaluate an `#[expand]` function call at compile time.
    /// Returns `Some(LitValue)` if all arguments are literals and the
    /// function body can be evaluated; returns `None` to fall back to
    /// a runtime call.
    fn try_expand_call(&self, func_name: &str, args: &[IRExpr]) -> Option<LitValue> {
        let (param_names, body) = self.expand_functions.get(func_name)?;

        // All arguments must be literals for compile-time evaluation.
        let mut bindings: HashMap<String, LitValue> = HashMap::new();
        for (i, param_name) in param_names.iter().enumerate() {
            if i >= args.len() {
                return None;
            }
            match &args[i] {
                IRExpr::Literal(lit) => {
                    bindings.insert(param_name.clone(), lit.clone());
                }
                _ => return None, // Non-literal argument: fall back to runtime
            }
        }

        self.eval_ir_block(body, &mut bindings)
    }

    /// AST-level `#[expand]` macro expansion: substitute the call arguments
    /// (which may be arbitrary expressions, not just literals) directly into
    /// the macro body's result expression, returning the substituted
    /// `IRExpr`. This is the "macro_rules!"-style expansion — the call site
    /// is replaced by the macro body with parameters bound to the argument
    /// expressions.
    ///
    /// Example: `#[expand] func add(a, b) { a + b }` called as `add(x, y+1)`
    /// expands to `(x) + (y + 1)` (as an IR tree — precedence is preserved
    /// structurally because each operand is its own sub-tree).
    ///
    /// Falls back to the existing literal constant-folding path (which
    /// produces a single `IRExpr::Literal`) when all arguments are literals,
    /// so `square(7)` still folds to `49` at compile time.
    fn try_expand_call_ast(&self, name: &str, args: &[IRExpr]) -> Option<IRExpr> {
        const MAX_DEPTH: usize = 32;
        self.try_expand_call_ast_depth(name, args, 0, MAX_DEPTH)
    }

    fn try_expand_call_ast_depth(
        &self,
        name: &str,
        args: &[IRExpr],
        depth: usize,
        max_depth: usize,
    ) -> Option<IRExpr> {
        if depth >= max_depth {
            return None; // recursion guard
        }
        let (params, body) = self.expand_functions.get(name)?;
        if params.len() != args.len() {
            return None;
        }
        // The result expression is the body's `return expr;`, or — when there
        // is no explicit return — the trailing expression statement (the
        // implicit return). `return expr;` is converted to `expr` per the
        // design decision in the prompt.
        let result_expr = self.expand_result_expr(body)?;
        // Bind parameter names to the call argument expressions, then walk
        // the result tree replacing each `Variable(param)` with its argument.
        let env: HashMap<&str, &IRExpr> = params
            .iter()
            .zip(args.iter())
            .map(|(p, a)| (p.as_str(), a))
            .collect();
        let mut expanded = self.subst_expr(&result_expr, &env);
        // Recursively expand any `#[expand]` macro calls that appeared inside
        // the body (递归展开).
        expanded = self.expand_nested_macros(&mut expanded, depth + 1, max_depth);
        Some(expanded)
    }

    /// Extract the result expression of a macro body: the operand of the
    /// first `return expr;`, or the trailing `Expression` statement when the
    /// body uses an implicit return.
    fn expand_result_expr(&self, block: &IRBlock) -> Option<IRExpr> {
        // Explicit `return expr;` wins.
        for stmt in &block.statements {
            if let IRStmt::Return(Some(e)) = stmt {
                return Some(e.clone());
            }
        }
        // Otherwise the last bare expression statement is the implicit return.
        let mut last: Option<IRExpr> = None;
        for stmt in &block.statements {
            if let IRStmt::Expression(e) = stmt {
                last = Some(e.clone());
            }
        }
        last
    }

    /// Substitute `Variable(name)` nodes inside `expr` with the bound
    /// argument expressions from `env`. Other expression forms are
    /// reconstructed recursively, preserving their tree shape so operator
    /// precedence is encoded structurally (no re-parenthesization needed).
    fn subst_expr(&self, expr: &IRExpr, env: &HashMap<&str, &IRExpr>) -> IRExpr {
        match expr {
            IRExpr::Variable(name) => env
                .get(name.as_str())
                .map(|e| (**e).clone())
                .unwrap_or_else(|| expr.clone()),
            IRExpr::Binary { op, left, right } => IRExpr::Binary {
                op: op.clone(),
                left: Box::new(self.subst_expr(left, env)),
                right: Box::new(self.subst_expr(right, env)),
            },
            IRExpr::Unary { op, operand } => IRExpr::Unary {
                op: op.clone(),
                operand: Box::new(self.subst_expr(operand, env)),
            },
            IRExpr::Call { func, args, generic_args } => IRExpr::Call {
                func: func.clone(),
                args: args.iter().map(|a| self.subst_expr(a, env)).collect(),
                generic_args: generic_args.clone(),
            },
            IRExpr::MethodCall { object, method, args, generic_args } => IRExpr::MethodCall {
                object: Box::new(self.subst_expr(object, env)),
                method: method.clone(),
                args: args.iter().map(|a| self.subst_expr(a, env)).collect(),
                generic_args: generic_args.clone(),
            },
            IRExpr::MemberAccess { object, member } => IRExpr::MemberAccess {
                object: Box::new(self.subst_expr(object, env)),
                member: member.clone(),
            },
            IRExpr::ArrayIndex { array, index } => IRExpr::ArrayIndex {
                array: Box::new(self.subst_expr(array, env)),
                index: Box::new(self.subst_expr(index, env)),
            },
            IRExpr::ArrayLiteral(elems) => {
                IRExpr::ArrayLiteral(elems.iter().map(|a| self.subst_expr(a, env)).collect())
            }
            IRExpr::StructLiteral { name, fields } => IRExpr::StructLiteral {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(n, e)| (n.clone(), self.subst_expr(e, env)))
                    .collect(),
            },
            IRExpr::Cast { expr, target } => IRExpr::Cast {
                expr: Box::new(self.subst_expr(expr, env)),
                target: target.clone(),
            },
            IRExpr::Assignment { target, value } => IRExpr::Assignment {
                target: Box::new(self.subst_expr(target, env)),
                value: Box::new(self.subst_expr(value, env)),
            },
            IRExpr::IndirectCall { callee, args } => IRExpr::IndirectCall {
                callee: Box::new(self.subst_expr(callee, env)),
                args: args.iter().map(|a| self.subst_expr(a, env)).collect(),
            },
            // Leaves: literals, function refs, None — copied as-is.
            _ => expr.clone(),
        }
    }

    /// Walk `expr` and expand any call to a known `#[expand]` macro, so a
    /// macro body that calls another macro is fully inlined (递归展开). This
    /// runs after parameter substitution so nested calls have their argument
    /// expressions already in place.
    fn expand_nested_macros(&self, expr: &mut IRExpr, depth: usize, max_depth: usize) -> IRExpr {
        if depth >= max_depth {
            return expr.clone();
        }
        match expr {
            IRExpr::Call { func, args, generic_args } => {
                // First recurse into the arguments.
                let new_args: Vec<IRExpr> = args
                    .iter()
                    .map(|a| {
                        let mut a = a.clone();
                        self.expand_nested_macros(&mut a, depth, max_depth)
                    })
                    .collect();
                // Then, if this call targets an #[expand] macro, expand it.
                if let Some(expanded) =
                    self.try_expand_call_ast_depth(func, &new_args, depth, max_depth)
                {
                    let mut e = expanded;
                    return self.expand_nested_macros(&mut e, depth, max_depth);
                }
                IRExpr::Call {
                    func: func.clone(),
                    args: new_args,
                    generic_args: generic_args.clone(),
                }
            }
            IRExpr::Binary { op, left, right } => {
                let l = self.expand_nested_macros(left, depth, max_depth);
                let r = self.expand_nested_macros(right, depth, max_depth);
                IRExpr::Binary { op: op.clone(), left: Box::new(l), right: Box::new(r) }
            }
            IRExpr::Unary { op, operand } => IRExpr::Unary {
                op: op.clone(),
                operand: Box::new(self.expand_nested_macros(operand, depth, max_depth)),
            },
            IRExpr::MethodCall { object, method, args, generic_args } => {
                let o = self.expand_nested_macros(object, depth, max_depth);
                let new_args: Vec<IRExpr> = args
                    .iter()
                    .map(|a| {
                        let mut a = a.clone();
                        self.expand_nested_macros(&mut a, depth, max_depth)
                    })
                    .collect();
                IRExpr::MethodCall { object: Box::new(o), method: method.clone(), args: new_args, generic_args: generic_args.clone() }
            }
            IRExpr::MemberAccess { object, member } => IRExpr::MemberAccess {
                object: Box::new(self.expand_nested_macros(object, depth, max_depth)),
                member: member.clone(),
            },
            IRExpr::ArrayIndex { array, index } => IRExpr::ArrayIndex {
                array: Box::new(self.expand_nested_macros(array, depth, max_depth)),
                index: Box::new(self.expand_nested_macros(index, depth, max_depth)),
            },
            IRExpr::ArrayLiteral(elems) => IRExpr::ArrayLiteral(
                elems
                    .iter()
                    .map(|a| {
                        let mut a = a.clone();
                        self.expand_nested_macros(&mut a, depth, max_depth)
                    })
                    .collect(),
            ),
            IRExpr::StructLiteral { name, fields } => IRExpr::StructLiteral {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(n, e)| {
                        let mut e = e.clone();
                        (n.clone(), self.expand_nested_macros(&mut e, depth, max_depth))
                    })
                    .collect(),
            },
            IRExpr::Cast { expr, target } => IRExpr::Cast {
                expr: Box::new(self.expand_nested_macros(expr, depth, max_depth)),
                target: target.clone(),
            },
            IRExpr::Assignment { target, value } => IRExpr::Assignment {
                target: Box::new(self.expand_nested_macros(target, depth, max_depth)),
                value: Box::new(self.expand_nested_macros(value, depth, max_depth)),
            },
            IRExpr::IndirectCall { callee, args } => {
                let c = self.expand_nested_macros(callee, depth, max_depth);
                let new_args: Vec<IRExpr> = args
                    .iter()
                    .map(|a| {
                        let mut a = a.clone();
                        self.expand_nested_macros(&mut a, depth, max_depth)
                    })
                    .collect();
                IRExpr::IndirectCall { callee: Box::new(c), args: new_args }
            }
            // Leaves — no nested calls to expand.
            _ => expr.clone(),
        }
    }

    /// Evaluate an IR block at compile time. Returns the value of the
    /// first `Return` statement, or the tail expression (last Expression
    /// statement) if there is no explicit return.
    fn eval_ir_block(
        &self,
        block: &IRBlock,
        bindings: &mut HashMap<String, LitValue>,
    ) -> Option<LitValue> {
        let mut last_expr_val: Option<LitValue> = None;
        for stmt in &block.statements {
            match stmt {
                IRStmt::Return(Some(expr)) => {
                    return self.eval_ir_expr(expr, bindings);
                }
                IRStmt::Return(None) => {
                    return Some(LitValue::None);
                }
                IRStmt::Expression(expr) => {
                    // Track the value; the last Expression in a block is
                    // the implicit return (tail expression).
                    last_expr_val = self.eval_ir_expr(expr, bindings);
                }
                IRStmt::Declaration { name, init, .. } => {
                    if let Some(init_expr) = init {
                        if let Some(val) = self.eval_ir_expr(init_expr, bindings) {
                            bindings.insert(name.clone(), val);
                        }
                    }
                }
                IRStmt::If { cond, then_block, else_block } => {
                    let cond_val = self.eval_ir_expr(cond, bindings)?;
                    let is_true = match cond_val {
                        LitValue::Bool(b) => b,
                        LitValue::Int(n) => n != 0,
                        _ => return None,
                    };
                    if is_true {
                        return self.eval_ir_block(then_block, bindings);
                    } else if let Some(else_b) = else_block {
                        return self.eval_ir_block(else_b, bindings);
                    }
                }
                _ => {}
            }
        }
        last_expr_val
    }

    /// Evaluate an IR expression at compile time.
    fn eval_ir_expr(
        &self,
        expr: &IRExpr,
        bindings: &HashMap<String, LitValue>,
    ) -> Option<LitValue> {
        match expr {
            IRExpr::Literal(lit) => Some(lit.clone()),
            IRExpr::Variable(name) => bindings.get(name).cloned(),
            IRExpr::Binary { op, left, right } => {
                let l = self.eval_ir_expr(left, bindings)?;
                let r = self.eval_ir_expr(right, bindings)?;
                self.eval_binary_op(op, l, r)
            }
            IRExpr::Unary { op, operand } => {
                let v = self.eval_ir_expr(operand, bindings)?;
                self.eval_unary_op(op, v)
            }
            // Operator-trait method calls (e.g. `a.add(b)`) are produced by
            // the IR builder for every arithmetic/comparison operator. Fold
            // them back to the matching binary operation at compile time.
            IRExpr::MethodCall { object, method, args, .. } => {
                let op = match method.as_str() {
                    "add" => "+", "sub" => "-", "mul" => "*", "div" => "/",
                    "rem" => "%", "eq" => "==", "ne" => "!=",
                    "lt" => "<", "gt" => ">", "le" => "<=", "ge" => ">=",
                    _ => return None,
                };
                let l = self.eval_ir_expr(object, bindings)?;
                let r = self.eval_ir_expr(args.first()?, bindings)?;
                self.eval_binary_op(op, l, r)
            }
            // file() macro: returns current source file path
            IRExpr::Call { func, .. } if func == "file" => {
                Some(LitValue::Str(self.current_file.clone()))
            }
            // line() macro: returns current line number (best-effort: 0)
            IRExpr::Call { func, .. } if func == "line" => {
                Some(LitValue::Int(0))
            }
            _ => None,
        }
    }

    fn eval_binary_op(&self, op: &str, l: LitValue, r: LitValue) -> Option<LitValue> {
        match (l, r) {
            (LitValue::Int(a), LitValue::Int(b)) => match op {
                "+" => Some(LitValue::Int(a + b)),
                "-" => Some(LitValue::Int(a - b)),
                "*" => Some(LitValue::Int(a * b)),
                "/" => if b != 0 { Some(LitValue::Int(a / b)) } else { None },
                "%" => if b != 0 { Some(LitValue::Int(a % b)) } else { None },
                "==" => Some(LitValue::Bool(a == b)),
                "!=" => Some(LitValue::Bool(a != b)),
                "<" => Some(LitValue::Bool(a < b)),
                ">" => Some(LitValue::Bool(a > b)),
                "<=" => Some(LitValue::Bool(a <= b)),
                ">=" => Some(LitValue::Bool(a >= b)),
                "&&" => Some(LitValue::Bool(a != 0 && b != 0)),
                "||" => Some(LitValue::Bool(a != 0 || b != 0)),
                _ => None,
            },
            (LitValue::Float(a), LitValue::Float(b)) => match op {
                "+" => Some(LitValue::Float(a + b)),
                "-" => Some(LitValue::Float(a - b)),
                "*" => Some(LitValue::Float(a * b)),
                "/" => if b != 0.0 { Some(LitValue::Float(a / b)) } else { None },
                "==" => Some(LitValue::Bool(a == b)),
                "!=" => Some(LitValue::Bool(a != b)),
                "<" => Some(LitValue::Bool(a < b)),
                ">" => Some(LitValue::Bool(a > b)),
                "<=" => Some(LitValue::Bool(a <= b)),
                ">=" => Some(LitValue::Bool(a >= b)),
                _ => None,
            },
            (LitValue::Str(a), LitValue::Str(b)) => match op {
                "+" => Some(LitValue::Str(a + &b)),
                "==" => Some(LitValue::Bool(a == b)),
                "!=" => Some(LitValue::Bool(a != b)),
                _ => None,
            },
            (LitValue::Bool(a), LitValue::Bool(b)) => match op {
                "==" => Some(LitValue::Bool(a == b)),
                "!=" => Some(LitValue::Bool(a != b)),
                "&&" => Some(LitValue::Bool(a && b)),
                "||" => Some(LitValue::Bool(a || b)),
                _ => None,
            },
            _ => None,
        }
    }

    fn eval_unary_op(&self, op: &str, v: LitValue) -> Option<LitValue> {
        match v {
            LitValue::Int(n) => match op {
                "-" => Some(LitValue::Int(-n)),
                "!" => Some(LitValue::Bool(n == 0)),
                _ => None,
            },
            LitValue::Float(f) => match op {
                "-" => Some(LitValue::Float(-f)),
                _ => None,
            },
            LitValue::Bool(b) => match op {
                "!" => Some(LitValue::Bool(!b)),
                _ => None,
            },
            _ => None,
        }
    }

    fn has_attr(attrs: &[String], name: &str) -> bool {
        attrs.iter().any(|a| a == name)
    }

    fn collect_attr_names(attrs: &[Attribute]) -> Vec<String> {
        attrs.iter().map(|a| a.name.clone()).collect()
    }

    /// Best-effort type inference for an IR expression. Used to infer the
    /// type of `var x = expr;` declarations that omit an explicit type, so
    /// that captured variables in lambdas get a concrete type instead of
    /// `None_` (which Cranelift cannot lower to a parameter).
    fn infer_expr_type(&self, e: &IRExpr) -> DataType {
        match e {
            IRExpr::Literal(LitValue::Int(_)) => DataType::Int,
            IRExpr::Literal(LitValue::Float(_)) => DataType::Float,
            IRExpr::Literal(LitValue::Bool(_)) => DataType::Bool,
            IRExpr::Literal(LitValue::Str(_)) => DataType::Str,
            IRExpr::Literal(LitValue::None) => DataType::None_,
            IRExpr::Variable(name) => self.var_types.get(name).cloned().unwrap_or(DataType::Int),
            IRExpr::StructLiteral { name, .. } => DataType::Struct(name.clone()),
            IRExpr::Call { func, .. } => {
                // Look up the function's return type among already-collected functions.
                for f in &self.ir.functions {
                    if f.name == *func {
                        return f.return_type.clone();
                    }
                }
                DataType::Int
            }
            IRExpr::MethodCall { object, method, .. } => {
                // Struct constructor: Type::new(...)
                if method == "new" {
                    if let IRExpr::Variable(name) = object.as_ref() {
                        if self.structs.contains_key(name) {
                            return DataType::Struct(name.clone());
                        }
                    }
                }
                let obj_ty = self.infer_expr_type(object);
                if let DataType::Struct(sname) = &obj_ty {
                    let full = format!("{}::{}", sname, method);
                    for f in &self.ir.functions {
                        if f.name == full {
                            return f.return_type.clone();
                        }
                    }
                }
                DataType::Int
            }
            IRExpr::Binary { op, left, right } => {
                if matches!(op.as_str(), "==" | "!=" | "<" | ">" | "<=" | ">=" | "&&" | "||") {
                    return DataType::Bool;
                }
                let lt = self.infer_expr_type(left);
                let rt = self.infer_expr_type(right);
                if matches!(lt, DataType::Float) || matches!(rt, DataType::Float) {
                    return DataType::Float;
                }
                // String concatenation: if either side is str, result is str.
                if op == "+" && (matches!(lt, DataType::Str) || matches!(rt, DataType::Str)) {
                    return DataType::Str;
                }
                lt
            }
            IRExpr::Unary { op, operand } => {
                if op == "!" {
                    return DataType::Bool;
                }
                self.infer_expr_type(operand)
            }
            IRExpr::Cast { target, .. } => target.clone(),
            IRExpr::ArrayIndex { .. } => DataType::Int,
            IRExpr::Assignment { target, .. } => self.infer_expr_type(target),
            IRExpr::ArrayLiteral(_) => DataType::Array(Box::new(DataType::Unknown)),
            IRExpr::MemberAccess { .. } => DataType::Int,
            IRExpr::FuncRef(name) => {
                // Look up the referenced function's return type.
                for f in &self.ir.functions {
                    if f.name == *name {
                        return f.return_type.clone();
                    }
                }
                DataType::Int
            }
            IRExpr::IndirectCall { .. } => DataType::Int,
            IRExpr::None => DataType::None_,
        }
    }

    // ==================== Lambda compilation ====================

    /// Compile a lambda expression into an anonymous IR function.
    ///
    /// Captures `var` variables from the enclosing scope by:
    /// 1. Scanning the lambda body for free identifiers (not bound by the
    ///    lambda's own parameters or inner declarations).
    /// 2. Filtering those against the enclosing function's variable table
    ///    (`var_types`) to determine which are actually captured and their
    ///    types.
    /// 3. Adding captured variables as leading parameters to the generated
    ///    anonymous function (`__lambda_N`).
    ///
    /// Returns `(function_name, captured_vars)` where `captured_vars` is a
    /// list of `(name, type)` pairs. Call sites must prepend the current
    /// values of captured variables as leading arguments.
    fn compile_lambda_function(&mut self, lambda: &Lambda) -> (String, Vec<(String, DataType)>) {
        let name = format!("__lambda_{}", self.lambda_counter);
        self.lambda_counter += 1;

        // Collect lambda parameter names (these are bound, not captured).
        let param_names: Vec<String> = lambda.get_parameters()
            .map(|ps| ps.iter().map(|p| p.get_name().to_string()).collect())
            .unwrap_or_default();

        // Collect free variable names referenced in the lambda body.
        let mut bound: HashSet<String> = param_names.iter().cloned().collect();
        let mut free_names: Vec<String> = Vec::new();
        collect_free_vars_from_block(lambda.get_body(), &mut bound, &mut free_names);

        // Filter free vars to those that exist in the enclosing scope.
        let mut captured: Vec<(String, DataType)> = Vec::new();
        for n in &free_names {
            if let Some(ty) = self.var_types.get(n) {
                if !captured.iter().any(|(cn, _)| cn == n) {
                    captured.push((n.clone(), ty.clone()));
                }
            }
        }

        // Build the full parameter list: captured vars first, then lambda params.
        let mut params: Vec<IRParam> = Vec::new();
        for (cn, ct) in &captured {
            params.push(IRParam { name: cn.clone(), ty: ct.clone() });
        }
        if let Some(ps) = lambda.get_parameters() {
            for p in ps {
                let pname = p.get_name().to_string();
                let pty = self.ast_type_to_data_type(p.get_type());
                params.push(IRParam { name: pname, ty: pty });
            }
        }

        let return_type = lambda.get_return_type()
            .map(|t| self.ast_type_to_data_type(Some(t)))
            .unwrap_or(DataType::None_);

        // Save the enclosing function's compilation context.
        let saved_function = self.current_function.take();
        let saved_return = self.current_function_return.clone();
        let saved_ir_function = self.current_ir_function.take();
        let saved_block = std::mem::take(&mut self.current_block);
        let saved_in_function = self.in_function;
        let saved_var_types = std::mem::take(&mut self.var_types);
        let saved_generic_stack = self.generic_stack.clone();

        // Register captured vars + lambda params in the lambda's var_types.
        for p in &params {
            self.var_types.insert(p.name.clone(), p.ty.clone());
        }

        // Push generic params from the lambda (combined with enclosing ones).
        let mut combined_generics: Vec<String> = Vec::new();
        for g in lambda.get_generic_params() {
            if !combined_generics.contains(g) {
                combined_generics.push(g.clone());
            }
        }
        self.push_generic_scope(&combined_generics);

        self.current_function = Some(name.clone());
        self.in_function = true;
        self.current_function_return = return_type.clone();
        self.current_ir_function = Some(IRFunction {
            name: name.clone(),
            // Lambdas are emitted as concrete functions (not monomorphized):
            // the generic scope above still lets type names like `T` resolve
            // within the body, but the function itself is not marked generic
            // so the monomorphizer keeps it as-is and call sites resolve.
            generic_params: Vec::new(),
            params: params.clone(),
            return_type: return_type.clone(),
            body: None,
            is_main: false,
            is_method: false,
            struct_name: None,
            attributes: Vec::new(),
            is_variadic: false,
        });

        // Compile the lambda body in this fresh context.
        lambda.get_body().accept(self);

        // Register the anonymous function.
        self.finish_function();

        // Restore the enclosing function's context.
        self.pop_generic_scope();
        self.current_function = saved_function;
        self.current_function_return = saved_return;
        self.current_ir_function = saved_ir_function;
        self.current_block = saved_block;
        self.in_function = saved_in_function;
        self.var_types = saved_var_types;
        self.generic_stack = saved_generic_stack;

        (name, captured)
    }
}

// ==================== AstVisitor 实现 ====================

impl AstVisitor for IRBuilder {
    fn visit_program(&mut self, _node: &Program) {
        // 在 build() 中处理
    }

    fn visit_struct_definition(&mut self, node: &StructDefinition) {
        let name = node.get_name().to_string();
        let generic_params = node.get_generic_params().clone();

        self.push_generic_scope(&generic_params);

        let fields: Vec<IRField> = node.get_fields()
            .iter()
            .map(|f| {
                IRField {
                    name: f.name.clone(),
                    ty: self.ast_type_to_data_type(f.field_type.as_deref()),
                }
            })
            .collect();

        self.pop_generic_scope();

        let struct_attrs = Self::collect_attr_names(node.get_attributes());

        let ir_struct = IRStruct {
            name: name.clone(),
            generic_params: generic_params.clone(),
            fields,
            attributes: struct_attrs,
        };

        self.structs.insert(name.clone(), ir_struct.clone());
        self.ir.structs.push(ir_struct);
    }

    fn visit_enum_definition(&mut self, node: &EnumDefinition) {
        let name = node.get_name().to_string();
        let generic_params = node.get_generic_params().clone();

        self.push_generic_scope(&generic_params);

        // Lower enum to a tagged struct: _tag (int) + _N payloads
        let mut fields = vec![IRField {
            name: "_tag".to_string(),
            ty: DataType::Int,
        }];

        let mut variant_idx = 0i32;
        for variant in node.get_variants() {
            if let Some(ref payload) = variant.payload_type {
                fields.push(IRField {
                    name: format!("_{}", variant_idx),
                    ty: self.ast_type_to_data_type(Some(payload.as_ref())),
                });
            }
            variant_idx += 1;
        }

        let enum_attrs = Self::collect_attr_names(node.get_attributes());

        let ir_struct = IRStruct {
            name: name.clone(),
            generic_params: generic_params.clone(),
            fields,
            attributes: enum_attrs,
        };
        self.structs.insert(name.clone(), ir_struct.clone());
        self.ir.structs.push(ir_struct);

        self.pop_generic_scope();

        // Generate variant constructor methods for each variant as an impl block.
        let mut methods = Vec::new();
        variant_idx = 0;
        for variant in node.get_variants() {
            let ctor_name = variant.name.clone();
            let has_payload = variant.payload_type.is_some();

            let mut body_stmts = Vec::new();

            // self._tag = variant_idx
            body_stmts.push(IRStmt::Assignment {
                target: IRExpr::MemberAccess {
                    object: Box::new(IRExpr::Variable("self".to_string())),
                    member: "_tag".to_string(),
                },
                value: IRExpr::Literal(LitValue::Int(variant_idx as i64)),
            });

            // If payload: self._N = value
            if has_payload {
                body_stmts.push(IRStmt::Assignment {
                    target: IRExpr::MemberAccess {
                        object: Box::new(IRExpr::Variable("self".to_string())),
                        member: format!("_{}", variant_idx),
                    },
                    value: IRExpr::Variable("_val".to_string()),
                });
            }

            // return self
            body_stmts.push(IRStmt::Return(Some(IRExpr::Variable("self".to_string()))));

            let mut params = vec![IRParam {
                name: "self".to_string(),
                ty: DataType::Struct(name.clone()),
            }];

            if has_payload {
                params.push(IRParam {
                    name: "_val".to_string(),
                    ty: DataType::Int, // generic payload → i64
                });
            }

            let ir_func = IRFunction {
                name: ctor_name.clone(),
                generic_params: generic_params.clone(),
                params,
                return_type: DataType::Struct(name.clone()),
                body: Some(IRBlock { statements: body_stmts }),
                is_main: false,
                is_method: true,
                struct_name: Some(name.clone()),
                attributes: vec![],
                is_variadic: false,
            };

            self.methods
                .entry(name.clone())
                .or_insert_with(Vec::new)
                .push(ir_func.clone());
            methods.push(ir_func);

            variant_idx += 1;
        }

        let ir_impl = IRImpl {
            struct_name: name.clone(),
            generic_params,
            methods,
            attributes: Vec::new(),
        };
        self.ir.impls.push(ir_impl);
    }

    fn visit_impl_block(&mut self, node: &ImplBlock) {
        let struct_name = node.get_struct_name().to_string();
        let generic_params = node.get_generic_params().clone();

        // Build the full struct name with generic parameters for consistent
        // lookup.  The parser may already include generic parameters in
        // struct_name (e.g. "Vec<T>"), so we must not double-append them.
        //
        // Case A — parser already included generics:
        //   struct_name = "Vec<T>", generic_params = ["T"]
        //     → full_struct_name = "Vec<T>"  (already correct)
        //
        // Case B — parser did not include generics:
        //   struct_name = "Vec",   generic_params = ["T"]
        //     → full_struct_name = "Vec<T>"  (build it here)
        //
        // Case C — no generics:
        //   struct_name = "Point", generic_params = []
        //     → full_struct_name = "Point"
        let full_struct_name = if struct_name.contains('<') {
            // Already has generics embedded, use as-is
            struct_name.clone()
        } else if generic_params.is_empty() {
            struct_name.clone()
        } else {
            format!("{}<{}>", struct_name, generic_params.join(", "))
        };

        self.current_struct = Some(full_struct_name.clone());
        self.in_impl = true;
        self.push_generic_scope(&generic_params);

        let mut methods = Vec::new();
        
        for item in node.get_items() {
            match item {
                ImplItem::Method(func) | ImplItem::Convert(func) => {
                    // 保存当前状态
                    let prev_function = self.current_function.clone();
                    let prev_return = self.current_function_return.clone();
                    let prev_block = std::mem::take(&mut self.current_block);
                    let prev_ir_func = self.current_ir_function.take();
                    let prev_expr_stack = std::mem::take(&mut self.expr_stack);
                    let prev_in_function = self.in_function;
                    
                    // 处理方法
                    func.accept(self);
                    
                    // 提取方法
                    if let Some(mut ir_func) = self.current_ir_function.take() {
                        ir_func.body = Some(IRBlock {
                            statements: std::mem::take(&mut self.current_block),
                        });
                        ir_func.struct_name = Some(full_struct_name.clone());
                        ir_func.is_method = true;
                        
                        // 保存方法名供后续查找
                        let _method_name = ir_func.name.clone();
                        methods.push(ir_func.clone());
                        self.methods
                            .entry(full_struct_name.clone())
                            .or_insert_with(Vec::new)
                            .push(ir_func);
                    }
                    
                    // 恢复状态
                    self.current_function = prev_function;
                    self.current_function_return = prev_return;
                    self.current_block = prev_block;
                    self.current_ir_function = prev_ir_func;
                    self.expr_stack = prev_expr_stack;
                    self.in_function = prev_in_function;
                }
            }
        }

        self.pop_generic_scope();
        self.current_struct = None;
        self.in_impl = false;

        let ir_impl = IRImpl {
            struct_name,
            generic_params,
            methods,
            attributes: Vec::new(),
        };
        self.ir.impls.push(ir_impl);
    }

    fn visit_function(&mut self, node: &Function) {
        let name = node.get_name().to_string();
        let is_main = name == "main";
        let is_method = self.current_struct.is_some();

        // 提取泛型参数
        let generic_params = self.extract_generic_params(node);

        self.current_function = Some(name.clone());
        self.in_function = true;
        self.push_generic_scope(&generic_params);
        // Fresh variable-type table for this function's scope.
        self.var_types.clear();

        // 解析参数
        let mut params: Vec<IRParam> = node.get_parameters()
            .map(|ps| {
                ps.iter()
                    .map(|p| {
                        let pname = p.get_name().to_string();
                        let ty = if pname == "self" {
                            if let Some(s) = &self.current_struct {
                                DataType::Struct(s.clone())
                            } else {
                                self.ast_type_to_data_type(p.get_type())
                            }
                        } else {
                            self.ast_type_to_data_type(p.get_type())
                        };
                        self.var_types.insert(pname.clone(), ty.clone());
                        IRParam { name: pname, ty }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // For methods without explicit self parameter
        // prepend self as the first parameter
        if is_method && !params.iter().any(|p| p.name == "self") {
            if let Some(ref sname) = self.current_struct {
                params.insert(0, IRParam {
                    name: "self".to_string(),
                    ty: DataType::Struct(sname.clone()),
                });
            }
        }

        // 解析返回类型
        let return_type = if is_main {
            DataType::Int
        } else if let Some(ret) = node.get_return_type() {
            self.ast_type_to_data_type(Some(ret))
        } else {
            DataType::None_
        };
        self.current_function_return = return_type.clone();

        // 创建 IR 函数
        let full_name = if is_method {
            if let Some(s) = &self.current_struct {
                format!("{}::{}", s, name)
            } else {
                name.clone()
            }
        } else {
            name.clone()
        };

        self.current_ir_function = Some(IRFunction {
            name: full_name.clone(),
            generic_params: generic_params.clone(),
            params: params.clone(),
            return_type: return_type.clone(),
            body: None,
            is_main,
            is_method,
            struct_name: self.current_struct.clone(),
            attributes: Self::collect_attr_names(node.get_attributes()),
            is_variadic: false,
        });

        // 处理函数体
        if let Some(body) = node.get_body() {
            // 先注册 self 参数（如果是方法）
            if is_method && self.current_struct.is_some() {
                // self 作为隐式参数
            }
            body.accept(self);
        }

        // Capture #[expand] function body for compile-time evaluation.
        // The body IR is cloned before finish_function moves current_block.
        if Attribute::has_attr(node.get_attributes(), "expand") {
            let param_names: Vec<String> = params.iter()
                .filter(|p| p.name != "self")
                .map(|p| p.name.clone())
                .collect();
            let body_block = IRBlock {
                statements: self.current_block.clone(),
            };
            self.expand_functions.insert(name.clone(), (param_names, body_block));
        }

        self.pop_generic_scope();
        
        // 如果是普通函数，立即结束；方法由 impl 块处理
        if !is_method {
            self.finish_function();
        }
    }

    fn visit_block(&mut self, node: &Block) {
        for stmt in node.get_statements() {
            stmt.accept(self);
        }
    }

    fn visit_declaration(&mut self, node: &Declaration) {
        let name = node.get_name().to_string();
        let orig_type = node.get_type();
        let ty = self.ast_type_to_data_type(orig_type);

        // Extract fixed-size array size if applicable
        // Extract fixed-size array sizes for multi-dimensional arrays
        let (arr_size, inner_size): (Option<i64>, Option<i64>) = if let Some(tp) = orig_type {
            if let Some(arr) = tp.as_type_any().downcast_ref::<ArrayType>() {
                let outer_size = if let Some(size_expr) = arr.get_size() {
                    if let Some(num) = size_expr.as_any().downcast_ref::<NumberLiteral>() {
                        Some(num.get_value() as i64)
                    } else {
                        None
                    }
                } else {
                    None
                };
                // Check if element type is also an array (for 2D)
                let inner_arr = arr.get_element_type();
                let inner_size = if let Some(inner) = inner_arr.as_type_any().downcast_ref::<ArrayType>() {
                    if let Some(size_expr) = inner.get_size() {
                        if let Some(num) = size_expr.as_any().downcast_ref::<NumberLiteral>() {
                            Some(num.get_value() as i64)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                (outer_size, inner_size)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let init = if let Some(init_expr) = node.get_initializer() {
            // The initializer may be a control-flow expression (if/match/block)
            // which the AST layer represents by implementing Statement +
            // Expression on the same struct.  All of these go through the
            // sub-builder path so that branch-level Return(Some(v)) is
            // converted to a plain Expression(v) and the overall produced
            // value is returned as the init expr.
            //
            // Pure expressions (literals, variables, calls, binary ops, ...)
            // that push a single value onto expr_stack skip the sub-builder.
            let is_stmt_like = init_expr.as_any().downcast_ref::<Block>().is_some()
                || init_expr.as_any().downcast_ref::<IfStatement>().is_some()
                || init_expr.as_any().downcast_ref::<MatchExpression>().is_some();
            if is_stmt_like {
                let mut sub = IRBuilder::new();
                sub.generic_stack = self.generic_stack.clone();
                sub.structs = self.structs.clone();
                sub.in_function = self.in_function;
                // If it's a Block, iterate its statements. Otherwise run the
                // `accept` on the node directly (it already implements
                // Statement visitor dispatch).
                if let Some(block) = init_expr.as_any().downcast_ref::<Block>() {
                    for stmt in block.get_statements() {
                        stmt.accept(&mut sub);
                    }
                } else {
                    init_expr.accept(&mut sub);
                }
                // Extract the value from the last Return statement (control
                // flow blocks lower branches as IRStmt::Return of the tail
                // value).  Convert Return → Expression so subsequent Cranelift
                // translation doesn't prematurely exit the enclosing function.
                let mut expr = IRExpr::None;
                for stmt in sub.current_block.iter_mut().rev() {
                    if let IRStmt::Return(Some(val)) = stmt {
                        expr = val.clone();
                        *stmt = IRStmt::Expression(val.clone());
                        break;
                    } else if matches!(stmt, IRStmt::Return(None)) {
                        *stmt = IRStmt::Expression(IRExpr::None);
                        break;
                    }
                }
                // Copy sub-builder's statements to the parent block
                self.current_block.extend(sub.current_block);
                // Copy sub-builder's variable types to parent so type resolution works
                for (name, ty) in sub.var_types {
                    self.var_types.insert(name, ty);
                }
                Some(expr)
            } else {
                init_expr.accept(self);
                let expr = self.pop_expr();
                Some(expr)
            }
        } else {
            // For fixed-size arrays without an initializer, generate a call
            // to gobol_array_new_with_size(size) or gobol_array_new_2d(rows, cols)
            if let Some(size) = arr_size {
                if let Some(inner) = inner_size {
                    // 2D array: gobol_array_new_2d(rows, cols)
                    Some(IRExpr::Call {
                        func: "gobol_array_new_2d".to_string(),
                        args: vec![
                            IRExpr::Literal(LitValue::Int(size)),
                            IRExpr::Literal(LitValue::Int(inner)),
                        ],
                        generic_args: vec![],
                    })
                } else {
                    // 1D array: gobol_array_new_with_size(size)
                    Some(IRExpr::Call {
                        func: "gobol_array_new_with_size".to_string(),
                        args: vec![IRExpr::Literal(LitValue::Int(size))],
                        generic_args: vec![],
                    })
                }
            } else {
                None
            }
        };

        // If the declaration omitted an explicit type, infer it from the
        // initializer so that lambda captures get a concrete type (Cranelift
        // cannot lower None_/Unknown to a function parameter).
        let ty = if matches!(ty, DataType::None_ | DataType::Unknown) {
            if let Some(e) = &init {
                let inferred = self.infer_expr_type(e);
                if matches!(inferred, DataType::None_) {
                    DataType::Int
                } else {
                    inferred
                }
            } else {
                DataType::Int
            }
        } else if matches!(ty, DataType::Array(_)) {
            // Preserve array type (fixed-size arrays lose size info here,
            // but the Cranelift backend will handle initialization separately)
            ty
        } else {
            ty
        };

        self.current_block.push(IRStmt::Declaration { name: name.clone(), ty: ty.clone(), init });
        self.var_types.insert(name, ty);
    }

    fn visit_expression_statement(&mut self, node: &ExpressionStatement) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
            let ir_expr = self.pop_expr();

            if node.tail {
                self.current_block.push(IRStmt::Return(Some(ir_expr)));
            } else {
                self.current_block.push(IRStmt::Expression(ir_expr));
            }
        }
    }

    fn visit_return_statement(&mut self, node: &ReturnStatement) {
        let value = if let Some(expr) = node.get_value() {
            expr.accept(self);
            Some(self.pop_expr())
        } else {
            None
        };
        self.current_block.push(IRStmt::Return(value));
    }

    fn visit_if_statement(&mut self, node: &IfStatement) {
        // 条件
        let cond = if let Some(c) = node.get_condition() {
            c.accept(self);
            self.pop_expr()
        } else {
            IRExpr::Literal(LitValue::Bool(false))
        };

        // then 分支 — visit with self (preserving current_struct & structs
        // context) by swapping out current_block.
        let then_block = if let Some(then_branch) = node.get_then_branch() {
            let saved = std::mem::take(&mut self.current_block);
            then_branch.accept(self);
            let block = IRBlock {
                statements: std::mem::replace(&mut self.current_block, saved),
            };
            block
        } else {
            IRBlock { statements: vec![] }
        };

        // else 分支
        let else_block = if let Some(else_branch) = node.get_else_branch() {
            let saved = std::mem::take(&mut self.current_block);
            else_branch.accept(self);
            Some(IRBlock {
                statements: std::mem::replace(&mut self.current_block, saved),
            })
        } else {
            None
        };

        self.current_block.push(IRStmt::If { cond, then_block, else_block });
    }

    fn visit_while_statement(&mut self, node: &WhileStatement) {
        let cond = if let Some(c) = node.get_condition() {
            c.accept(self);
            self.pop_expr()
        } else {
            IRExpr::Literal(LitValue::Bool(false))
        };

        let body = if let Some(b) = node.get_body() {
            let saved = std::mem::take(&mut self.current_block);
            b.accept(self);
            IRBlock {
                statements: std::mem::replace(&mut self.current_block, saved),
            }
        } else {
            IRBlock { statements: vec![] }
        };

        self.current_block.push(IRStmt::While { cond, body });
    }

    fn visit_break_statement(&mut self, _node: &BreakStatement) {
        self.current_block.push(IRStmt::Break);
    }

    fn visit_continue_statement(&mut self, _node: &ContinueStatement) {
        self.current_block.push(IRStmt::Continue);
    }

    // ==================== Expressions ====================

    fn visit_identifier(&mut self, node: &Identifier) {
        let name = node.get_name().to_string();

        // 检查是否是泛型参数
        if let Some(_ty) = self.lookup_generic(&name) {
            self.push_expr(IRExpr::Variable(name));
            return;
        }

        // 检查是否是 self
        if name == "self" {
            self.push_expr(IRExpr::Variable("self".to_string()));
            return;
        }

        // Inside a method body, bare field names (e.g. `_start`) are implicit
        // `self._start` accesses. Resolve them to MemberAccess so the backend
        // loads from the struct pointer instead of treating them as undefined
        // local variables (which would silently return 0).
        // BUT: don't override if there's already a local variable (parameter) with this name.
        if let Some(ref sname) = self.current_struct {
            if self.var_types.contains_key(&name) {
                // There's a local variable/parameter with this name — use it
                self.push_expr(IRExpr::Variable(name));
                return;
            }
            // Try looking up the struct by its name. The current_struct may
            // include generic parameters (e.g. "Vec<T>"), but the structs map
            // stores them without generics (e.g. "Vec"). Try both forms.
            let lookup_name = if let Some(angle_pos) = sname.find('<') {
                &sname[..angle_pos]
            } else {
                sname.as_str()
            };
            if let Some(ir_struct) = self.structs.get(lookup_name) {
                if ir_struct.fields.iter().any(|f| f.name == name) {
                    self.push_expr(IRExpr::MemberAccess {
                        object: Box::new(IRExpr::Variable("self".to_string())),
                        member: name,
                    });
                    return;
                }
            }
        }

        self.push_expr(IRExpr::Variable(name));
    }

    fn visit_number_literal(&mut self, node: &NumberLiteral) {
        let v = node.get_value();
        // Use the source-level form to decide int vs float: a literal like
        // `3.0` stays float even though its value is integral, so it matches
        // float-typed parameters/returns. Plain `3` (no decimal point) is int.
        if node.is_float_literal() {
            self.push_expr(IRExpr::Literal(LitValue::Float(v)));
        } else {
            self.push_expr(IRExpr::Literal(LitValue::Int(v as i64)));
        }
    }

    fn visit_string_literal(&mut self, node: &StringLiteral) {
        self.push_expr(IRExpr::Literal(LitValue::Str(node.get_value().to_string())));
    }

    fn visit_boolean_literal(&mut self, node: &BooleanLiteral) {
        self.push_expr(IRExpr::Literal(LitValue::Bool(node.get_value())));
    }

    fn visit_null_literal(&mut self, _node: &NullLiteral) {
        self.push_expr(IRExpr::Literal(LitValue::None));
    }

    fn visit_binary_expression(&mut self, node: &BinaryExpression) {
        let op = node.get_operator().to_string();
        
        // 处理赋值
        if op == "=" || op == "+=" || op == "-=" || op == "*=" || op == "/=" {
            let right = node.get_right().unwrap();
            right.accept(self);
            let right_val = self.pop_expr();

            let left = node.get_left().unwrap();
            // Visit left once for the assignment target
            left.accept(self);
            let target = self.pop_expr();

            // For compound ops: x += y → x = x + y
            let value = if op == "=" {
                right_val
            } else {
                let real_op = &op[..1]; // "+=" → "+", "-=" → "-", etc.
                left.accept(self);  // push left again as the value operand
                let left_val = self.pop_expr();
                // Convert to method call via trait
                let method = Self::operator_to_method(real_op);
                if let Some(method_name) = method {
                    IRExpr::MethodCall {
                        object: Box::new(left_val),
                        method: method_name.to_string(),
                        args: vec![right_val],
                        generic_args: Vec::new(),
                    }
                } else {
                    IRExpr::Binary {
                        op: real_op.to_string(),
                        left: Box::new(left_val),
                        right: Box::new(right_val),
                    }
                }
            };

            self.push_expr(IRExpr::Assignment { target: Box::new(target), value: Box::new(value) });
            return;
        }
        
        // 处理 && 和 || (短路求值)
        if op == "&&" || op == "||" {
            let left = node.get_left().unwrap();
            left.accept(self);
            let left_expr = self.pop_expr();
            
            let right = node.get_right().unwrap();
            right.accept(self);
            let right_expr = self.pop_expr();
            
            self.push_expr(IRExpr::Binary {
                op,
                left: Box::new(left_expr),
                right: Box::new(right_expr),
            });
            return;
        }
        
        // Operand expressions
        let left = node.get_left().unwrap();
        left.accept(self);
        let left_expr = self.pop_expr();

        let right = node.get_right().unwrap();
        right.accept(self);
        let right_expr = self.pop_expr();

        // Convert arithmetic/comparison operators to trait method calls
        let method = Self::operator_to_method(&op);
        if let Some(method_name) = method {
            // Binary op → MethodCall via trait (e.g. a + b → a.add(b))
            self.push_expr(IRExpr::MethodCall {
                object: Box::new(left_expr),
                method: method_name.to_string(),
                args: vec![right_expr],
                generic_args: Vec::new(),
            });
        } else {
            // Logical/bitwise operators: keep as Binary
            self.push_expr(IRExpr::Binary {
                op,
                left: Box::new(left_expr),
                right: Box::new(right_expr),
            });
        }
    }

    fn visit_unary_expression(&mut self, node: &UnaryExpression) {
        let op = node.get_operator().to_string();
        let operand = node.get_operand().unwrap();

        // `&name` — address-of operator: take a function's address as a FuncRef.
        if op == "&" {
            if let Some(id) = operand.as_any().downcast_ref::<Identifier>() {
                self.push_expr(IRExpr::FuncRef(id.get_name().to_string()));
                return;
            }
        }

        operand.accept(self);
        let operand_expr = self.pop_expr();

        self.push_expr(IRExpr::Unary {
            op,
            operand: Box::new(operand_expr),
        });
    }

    fn visit_cast_expression(&mut self, node: &CastExpression) {
        let expr = node.get_expression().unwrap();
        expr.accept(self);
        let expr_expr = self.pop_expr();
        
        let target = self.ast_type_to_data_type(Some(node.get_target_type()));
        
        self.push_expr(IRExpr::Cast {
            expr: Box::new(expr_expr),
            target,
        });
    }

    fn visit_function_call(&mut self, node: &FunctionCall) {
        let callee = node.get_callee();
        let mut args = Vec::new();
        let generic_args = Vec::new();

        if let Some(arg_list) = node.get_arguments() {
            for arg in arg_list {
                arg.accept(self);
                args.push(self.pop_expr());
            }
        }

        if let Some(callee_expr) = callee {
            // Lambda immediate invocation: lambda(params): ret { body }(args)
            // Compile the lambda into an anonymous function and emit a call
            // to it, prepending captured variables as leading arguments.
            if let Some(lambda) = callee_expr.as_any().downcast_ref::<Lambda>() {
                let (func_name, captured) = self.compile_lambda_function(lambda);
                let mut full_args: Vec<IRExpr> = captured.iter()
                    .map(|(n, _)| IRExpr::Variable(n.clone()))
                    .collect();
                full_args.extend(args);
                self.push_expr(IRExpr::Call {
                    func: func_name,
                    args: full_args,
                    generic_args,
                });
                return;
            }

            // 检查是否是方法调用 (obj.method)
            if let Some(member) = callee_expr.as_any().downcast_ref::<MemberAccess>() {
                let obj = member.get_object().unwrap();
                obj.accept(self);
                let object = self.pop_expr();
                let method = member.get_member().to_string();

                self.push_expr(IRExpr::MethodCall {
                    object: Box::new(object),
                    method,
                    args,
                    generic_args,
                });
                return;
            }

            // 命名空间路径调用 (::): std::io::println(args)
            if let Some(path_access) = callee_expr.as_any().downcast_ref::<PathAccess>() {
                let func_name = path_access.get_full_name();

                // Try #[expand] compile-time evaluation for qualified calls.
                // Check both the full name (e.g. "mymodule::my_func") and
                // the short member name (e.g. "my_func").
                let member_name = path_access.get_member();
                if let Some(val) = self.try_expand_call(&func_name, &args)
                    .or_else(|| self.try_expand_call(member_name, &args))
                {
                    // Literal constant folding (all-args-literal path).
                    self.push_expr(IRExpr::Literal(val));
                    return;
                }
                // AST-level macro expansion: substitute argument expressions
                // into the macro body (works for non-literal arguments too).
                if let Some(expanded) = self.try_expand_call_ast(&func_name, &args)
                    .or_else(|| self.try_expand_call_ast(member_name, &args))
                {
                    self.push_expr(expanded);
                    return;
                }

                self.push_expr(IRExpr::Call {
                    func: func_name,
                    args,
                    generic_args,
                });
                return;
            }

            // 普通函数调用
            if let Some(id) = callee_expr.as_any().downcast_ref::<Identifier>() {
                let func_name = id.get_name().to_string();

                // Try #[expand] compile-time evaluation (literal fold).
                if let Some(val) = self.try_expand_call(&func_name, &args) {
                    self.push_expr(IRExpr::Literal(val));
                    return;
                }
                // AST-level macro expansion — substitutes the argument
                // expressions into the macro body. `add(x, y)` → `x + y`.
                if let Some(expanded) = self.try_expand_call_ast(&func_name, &args) {
                    self.push_expr(expanded);
                    return;
                }

                // 检查是否是结构体构造函数
                if self.structs.contains_key(&func_name) {
                    // 结构体字面量
                    let mut fields = Vec::new();
                    if let Some(struct_def) = self.structs.get(&func_name) {
                        for (i, field) in struct_def.fields.iter().enumerate() {
                            if i < args.len() {
                                fields.push((field.name.clone(), args[i].clone()));
                            }
                        }
                    }
                    self.push_expr(IRExpr::StructLiteral {
                        name: func_name,
                        fields,
                    });
                    return;
                }

                // If the name is a local variable / parameter (not a known
                // function), treat it as an indirect call through a function
                // pointer. This supports higher-order functions where a
                // `func(...)` parameter is called inside the body.
                if self.var_types.contains_key(&func_name) {
                    self.push_expr(IRExpr::IndirectCall {
                        callee: Box::new(IRExpr::Variable(func_name)),
                        args,
                    });
                    return;
                }

                self.push_expr(IRExpr::Call {
                    func: func_name,
                    args,
                    generic_args,
                });
                return;
            }
        }

        self.push_expr(IRExpr::Call {
            func: "unknown".to_string(),
            args,
            generic_args,
        });
    }

    fn visit_member_access(&mut self, node: &MemberAccess) {
        let obj = node.get_object().unwrap();
        obj.accept(self);
        let object = self.pop_expr();
        let member = node.get_member().to_string();
        
        self.push_expr(IRExpr::MemberAccess {
            object: Box::new(object),
            member,
        });
    }

    fn visit_array_index(&mut self, node: &ArrayIndex) {
        let array = node.get_array().unwrap();
        array.accept(self);
        let array_expr = self.pop_expr();
        
        let index = node.get_index().unwrap();
        index.accept(self);
        let index_expr = self.pop_expr();
        
        self.push_expr(IRExpr::ArrayIndex {
            array: Box::new(array_expr),
            index: Box::new(index_expr),
        });
    }

    fn visit_array_literal(&mut self, node: &ArrayLiteral) {
        let mut elements = Vec::new();
        for elem in node.get_elements() {
            elem.accept(self);
            elements.push(self.pop_expr());
        }
        self.push_expr(IRExpr::ArrayLiteral(elements));
    }

    fn visit_struct_literal(&mut self, node: &StructLiteral) {
        let name = node.get_type_name().to_string();
        let mut fields = Vec::new();
        
        for field in node.get_fields() {
            match field {
                StructFieldInit::Named { name: fname, value } => {
                    value.accept(self);
                    fields.push((fname.clone(), self.pop_expr()));
                }
                StructFieldInit::Positional(value) => {
                    value.accept(self);
                    fields.push((format!("_{}", fields.len()), self.pop_expr()));
                }
            }
        }
        
        self.push_expr(IRExpr::StructLiteral { name, fields });
    }

    fn visit_match_expression(&mut self, node: &MatchExpression) {
        // 1. Evaluate scrutinee
        let scrutinee = if let Some(scrut) = node.get_scrutinee() {
            scrut.accept(self);
            self.pop_expr()
        } else {
            IRExpr::None
        };

        let arms = node.get_arms();
        if arms.is_empty() {
            self.push_expr(IRExpr::None);
            return;
        }

        // 2. Create temp variable for match result
        let tmp_name = format!("__match_result_{}", self.tmp_counter);
        self.tmp_counter += 1;

        // Determine result type from the arms
        let result_type = DataType::Str; // Default for string results

        // Initialize temp variable with default value
        self.current_block.push(IRStmt::Declaration {
            name: tmp_name.clone(),
            ty: result_type.clone(),
            init: Some(IRExpr::Literal(LitValue::Str("".to_string()))),
        });
        self.var_types.insert(tmp_name.clone(), result_type.clone());

        // 3. Build if-else chain (iterating arms from back to front)
        let mut else_block = None;
        
        for arm in arms.iter().rev() {
            // Build condition
            let cond = self.build_match_condition(&scrutinee, &arm.pattern);
            
            // Build arm body with assignment to temp variable
            let mut then_block = IRBlock { statements: Vec::new() };

            if let Some(body) = &arm.body {
                let mut sub_builder = IRBuilder::new();
                sub_builder.generic_stack = self.generic_stack.clone();
                sub_builder.block_depth = 2;

                if let Some(block_node) = body.as_any().downcast_ref::<Block>() {
                    for stmt in block_node.get_statements() {
                        stmt.accept(&mut sub_builder);
                    }
                } else {
                    body.accept(&mut sub_builder);
                }

                // Extract the result value from the last statement in the
                // sub-builder's current_block.  ExpressionStatements store
                // values in current_block (not on expr_stack), so we need to
                // look at the last statement to get the arm's result.
                if let Some(last_stmt) = sub_builder.current_block.last_mut() {
                    match last_stmt {
                        IRStmt::Expression(expr) => {
                            // Replace the Expression with an Assignment to the
                            // temp variable so the result is captured.
                            *last_stmt = IRStmt::Assignment {
                                target: IRExpr::Variable(tmp_name.clone()),
                                value: expr.clone(),
                            };
                        }
                        IRStmt::Return(Some(expr)) => {
                            *last_stmt = IRStmt::Assignment {
                                target: IRExpr::Variable(tmp_name.clone()),
                                value: expr.clone(),
                            };
                        }
                        _ => {}
                    }
                }

                then_block.statements.extend(sub_builder.current_block);
            }

            // Create if statement
            let if_stmt = IRStmt::If {
                cond,
                then_block,
                else_block: else_block.take(),
            };
            
            // Wrap in a Block
            let mut block = IRBlock { statements: Vec::new() };
            block.statements.push(if_stmt);
            else_block = Some(block);
        }

        // 4. Insert the if-else chain
        if let Some(final_block) = else_block {
            self.current_block.extend(final_block.statements);
        }

        // 5. Generate a Return statement so the sub-builder logic can extract the result
        self.current_block.push(IRStmt::Return(Some(IRExpr::Variable(tmp_name))));
    }

    fn visit_try_operator(&mut self, node: &TryOperator) {
        // Desugar `expr?` into:
        //   let __tmpN = expr;
        //   if __tmpN.is_err() { return __tmpN; }
        //   __tmpN.take()
        //
        // The IR builder generates statements in current_block and pushes
        // the result expression onto the stack, matching the pattern used
        // by visit_match_expression.

        if let Some(inner) = node.get_inner() {
            inner.accept(self);
            let inner_expr = self.pop_expr();

            // Generate a unique temp variable name
            let tmp_name = format!("__try_tmp_{}", self.tmp_counter);
            self.tmp_counter += 1;

            // Declare the temp variable
            self.current_block.push(IRStmt::Declaration {
                name: tmp_name.clone(),
                ty: DataType::Unknown,
                init: Some(inner_expr),
            });

            // Build condition: __tmp.is_err()
            // Access the _tag field (offset 0) and compare with 1.
            let cond = IRExpr::Binary {
                op: "==".to_string(),
                left: Box::new(IRExpr::MemberAccess {
                    object: Box::new(IRExpr::Variable(tmp_name.clone())),
                    member: "_tag".to_string(),
                }),
                right: Box::new(IRExpr::Literal(LitValue::Int(1))),
            };

            // then-block: return __tmp (the whole Result, as Err)
            let mut then_block = IRBlock { statements: Vec::new() };
            then_block.statements.push(IRStmt::Return(Some(
                IRExpr::Variable(tmp_name.clone()),
            )));

            // Push the if statement
            self.current_block.push(IRStmt::If {
                cond,
                then_block,
                else_block: None,
            });

            // Result: __tmp.take() → method call on temp variable
            self.push_expr(IRExpr::MethodCall {
                object: Box::new(IRExpr::Variable(tmp_name)),
                method: "take".to_string(),
                args: Vec::new(),
                generic_args: Vec::new(),
            });
        } else {
            self.push_expr(IRExpr::None);
        }
    }

    fn visit_range_expression(&mut self, node: &RangeExpression) {
        let mut args = Vec::new();
        for arg in node.get_arguments() {
            arg.accept(self);
            args.push(self.pop_expr());
        }
        // Range 被转换为 range::new 调用
        self.push_expr(IRExpr::Call {
            func: "range::new".to_string(),
            args,
            generic_args: Vec::new(),
        });
    }

    fn visit_grouped_expression(&mut self, node: &GroupedExpression) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
            // 分组表达式直接传递内部表达式
        } else {
            self.push_expr(IRExpr::None);
        }
    }

    fn visit_format_string(&mut self, node: &FormatString) {
        // 格式字符串转换为字符串拼接
        let template = node.get_value();
        let vars = node.get_variables();
        
        if vars.is_empty() {
            self.push_expr(IRExpr::Literal(LitValue::Str(template.to_string())));
            return;
        }
        
        // 构建字符串拼接表达式
        let mut expr = IRExpr::Literal(LitValue::Str(String::new()));
        let mut last_pos = 0;
        
        for var in vars {
            let pos = var.pos_in_value as usize;
            // 添加字面量部分
            if pos > last_pos {
                let lit = &template[last_pos..pos];
                expr = IRExpr::Binary {
                    op: "+".to_string(),
                    left: Box::new(expr),
                    right: Box::new(IRExpr::Literal(LitValue::Str(lit.to_string()))),
                };
            }
            // 添加变量部分
            if let Some(ref value) = var.value {
                // Visit the embedded expression with the *current* builder so
                // that `#[expand]` macros (stored in `self.expand_functions`)
                // are inlined here too. A fresh `IRBuilder::new()` would have
                // an empty macro table, so `@"{add(x, y)}"` inside a format
                // string would emit a runtime call instead of the inlined body.
                value.accept(self);
                let var_expr = self.pop_expr();
                expr = IRExpr::Binary {
                    op: "+".to_string(),
                    left: Box::new(expr),
                    right: Box::new(var_expr),
                };
            }
            last_pos = pos + 1; // 跳过 {
            // 找到 }
            let mut depth = 0;
            let chars: Vec<char> = template.chars().collect();
            let mut i = pos;
            while i < chars.len() {
                if chars[i] == '{' { depth += 1; }
                else if chars[i] == '}' { 
                    depth -= 1;
                    if depth == 0 {
                        last_pos = i + 1;
                        break;
                    }
                }
                i += 1;
            }
        }
        
        // 添加剩余字面量
        if last_pos < template.len() {
            let lit = &template[last_pos..];
            expr = IRExpr::Binary {
                op: "+".to_string(),
                left: Box::new(expr),
                right: Box::new(IRExpr::Literal(LitValue::Str(lit.to_string()))),
            };
        }
        
        self.push_expr(expr);
    }

    // ==================== Lambda ====================

    fn visit_lambda(&mut self, node: &Lambda) {
        // A standalone lambda (used as an argument, stored in a var, etc.)
        // is compiled into an anonymous function and its address is yielded
        // as a FuncRef so it can be passed to higher-order functions.
        let (name, _captured) = self.compile_lambda_function(node);
        self.push_expr(IRExpr::FuncRef(name));
    }

    // ==================== Stub visitors ====================

    fn visit_ast_node(&mut self, _node: &dyn AstNode) {}
    fn visit_statement(&mut self, _node: &dyn Statement) {}
    fn visit_expression(&mut self, _node: &dyn Expression) {}
    fn visit_parameter(&mut self, _node: &Parameter) {}
    fn visit_basic_type(&mut self, _node: &BasicType) {}
    fn visit_type(&mut self, _node: &dyn Type) {}
    fn visit_array_type(&mut self, _node: &ArrayType) {}
    fn visit_for_statement(&mut self, node: &ForStatement) {
        let vars = node.get_loop_variables().clone();
        if !vars.is_empty() {
            if let Some(iterable) = node.get_iterable() {
                iterable.accept(self);
                let iter_expr = self.pop_expr();
                let body = if let Some(b) = node.get_body() {
                    let saved = std::mem::take(&mut self.current_block);
                    b.accept(self);
                    IRBlock {
                        statements: std::mem::replace(&mut self.current_block, saved),
                    }
                } else {
                    IRBlock { statements: vec![] }
                };
                self.current_block.push(IRStmt::For {
                    vars,
                    iterable: iter_expr,
                    body,
                });
            }
        }
    }
    fn visit_import_statement(&mut self, _node: &ImportStatement) {}
    fn visit_from_import_statement(&mut self, _node: &FromImportStatement) {}
    fn visit_export_statement(&mut self, _node: &ExportStatement) {}
}

// ==================== 自由变量收集器（Lambda 捕获分析） ====================

/// Walk a block and collect free variable names (identifiers not bound by
/// `bound` or by inner declarations). Results are appended to `free` in
/// first-use order without duplicates.
fn collect_free_vars_from_block(block: &Block, bound: &mut HashSet<String>, free: &mut Vec<String>) {
    // Inner block scope: local declarations only shadow within this block,
    // so clone the bound set so siblings don't see each other's locals.
    let mut local_bound = bound.clone();
    for stmt in block.get_statements() {
        collect_free_vars_from_stmt(stmt.as_ref(), &mut local_bound, free);
    }
}

fn collect_free_vars_from_stmt(stmt: &dyn Statement, bound: &mut HashSet<String>, free: &mut Vec<String>) {
    let any = stmt.as_any();
    if let Some(decl) = any.downcast_ref::<Declaration>() {
        if let Some(init) = decl.get_initializer() {
            collect_free_vars_from_expr(init, bound, free);
        }
        bound.insert(decl.get_name().to_string());
        return;
    }
    if let Some(block) = any.downcast_ref::<Block>() {
        collect_free_vars_from_block(block, bound, free);
        return;
    }
    if let Some(expr_stmt) = any.downcast_ref::<ExpressionStatement>() {
        if let Some(e) = expr_stmt.get_expression() {
            collect_free_vars_from_expr(e, bound, free);
        }
        return;
    }
    if let Some(ret) = any.downcast_ref::<ReturnStatement>() {
        if let Some(v) = ret.get_value() {
            collect_free_vars_from_expr(v, bound, free);
        }
        return;
    }
    if let Some(if_stmt) = any.downcast_ref::<IfStatement>() {
        if let Some(c) = if_stmt.get_condition() {
            collect_free_vars_from_expr(c, bound, free);
        }
        if let Some(t) = if_stmt.get_then_branch() {
            collect_free_vars_from_stmt(t, bound, free);
        }
        if let Some(e) = if_stmt.get_else_branch() {
            collect_free_vars_from_stmt(e, bound, free);
        }
        return;
    }
    if let Some(while_stmt) = any.downcast_ref::<WhileStatement>() {
        if let Some(c) = while_stmt.get_condition() {
            collect_free_vars_from_expr(c, bound, free);
        }
        if let Some(b) = while_stmt.get_body() {
            collect_free_vars_from_stmt(b, bound, free);
        }
        return;
    }
    if let Some(for_stmt) = any.downcast_ref::<ForStatement>() {
        if let Some(it) = for_stmt.get_iterable() {
            collect_free_vars_from_expr(it, bound, free);
        }
        for v in for_stmt.get_loop_variables() {
            bound.insert(v.clone());
        }
        if let Some(b) = for_stmt.get_body() {
            let mut local = bound.clone();
            collect_free_vars_from_block(b, &mut local, free);
        }
        return;
    }
    // MatchExpression is both Statement and Expression.
    if let Some(m) = any.downcast_ref::<MatchExpression>() {
        collect_free_vars_from_expr(m.as_expression(), bound, free);
        return;
    }
}

fn collect_free_vars_from_expr(expr: &dyn Expression, bound: &HashSet<String>, free: &mut Vec<String>) {
    let any = expr.as_any();
    if let Some(id) = any.downcast_ref::<Identifier>() {
        let name = id.get_name();
        if !bound.contains(name) && !free.iter().any(|n| n == name) {
            free.push(name.to_string());
        }
        return;
    }
    if let Some(bin) = any.downcast_ref::<BinaryExpression>() {
        if let Some(l) = bin.get_left() { collect_free_vars_from_expr(l, bound, free); }
        if let Some(r) = bin.get_right() { collect_free_vars_from_expr(r, bound, free); }
        return;
    }
    if let Some(un) = any.downcast_ref::<UnaryExpression>() {
        if let Some(o) = un.get_operand() { collect_free_vars_from_expr(o, bound, free); }
        return;
    }
    if let Some(cast) = any.downcast_ref::<CastExpression>() {
        if let Some(e) = cast.get_expression() { collect_free_vars_from_expr(e, bound, free); }
        return;
    }
    if let Some(call) = any.downcast_ref::<FunctionCall>() {
        if let Some(c) = call.get_callee() {
            // A bare Identifier callee is a function name, not a variable
            // reference — skip it so we don't falsely "capture" function names.
            if c.as_any().downcast_ref::<Identifier>().is_none() {
                collect_free_vars_from_expr(c, bound, free);
            }
        }
        if let Some(args) = call.get_arguments() {
            for a in args {
                collect_free_vars_from_expr(a.as_ref(), bound, free);
            }
        }
        return;
    }
    if let Some(mem) = any.downcast_ref::<MemberAccess>() {
        if let Some(o) = mem.get_object() { collect_free_vars_from_expr(o, bound, free); }
        return;
    }
    if let Some(arr) = any.downcast_ref::<ArrayIndex>() {
        if let Some(a) = arr.get_array() { collect_free_vars_from_expr(a, bound, free); }
        if let Some(i) = arr.get_index() { collect_free_vars_from_expr(i, bound, free); }
        return;
    }
    if let Some(grp) = any.downcast_ref::<GroupedExpression>() {
        if let Some(e) = grp.get_expression() { collect_free_vars_from_expr(e, bound, free); }
        return;
    }
    if let Some(arr_lit) = any.downcast_ref::<ArrayLiteral>() {
        for e in arr_lit.get_elements() {
            collect_free_vars_from_expr(e.as_ref(), bound, free);
        }
        return;
    }
    if let Some(struct_lit) = any.downcast_ref::<StructLiteral>() {
        for f in struct_lit.get_fields() {
            match f {
                StructFieldInit::Named { value, .. } => {
                    collect_free_vars_from_expr(value.as_ref(), bound, free);
                }
                StructFieldInit::Positional(e) => {
                    collect_free_vars_from_expr(e.as_ref(), bound, free);
                }
            }
        }
        return;
    }
    if let Some(rng) = any.downcast_ref::<RangeExpression>() {
        for a in rng.get_arguments() {
            collect_free_vars_from_expr(a.as_ref(), bound, free);
        }
        return;
    }
    if let Some(try_op) = any.downcast_ref::<TryOperator>() {
        if let Some(e) = try_op.get_inner() { collect_free_vars_from_expr(e, bound, free); }
        return;
    }
    if let Some(fmt) = any.downcast_ref::<FormatString>() {
        for v in fmt.get_variables() {
            // VariablePosition.value holds the expression referencing the interpolated value.
            if let Some(e) = v.value.as_ref() {
                collect_free_vars_from_expr(e.as_ref(), bound, free);
            }
        }
        return;
    }
    if let Some(m) = any.downcast_ref::<MatchExpression>() {
        if let Some(s) = m.get_scrutinee() { collect_free_vars_from_expr(s, bound, free); }
        for arm in m.get_arms() {
            // Variable patterns bind a name, so add it to a local bound set.
            let mut arm_bound = bound.clone();
            if let MatchPattern::Variable(name) = &arm.pattern {
                arm_bound.insert(name.clone());
            }
            if let Some(body) = &arm.body {
                collect_free_vars_from_stmt(body.as_ref(), &mut arm_bound, free);
            }
        }
        return;
    }
    // Lambda inside a lambda: nested captures are not tracked here (the inner
    // lambda would be compiled separately and capture from this lambda's
    // scope). Skip its internals to avoid false captures.
    if any.downcast_ref::<Lambda>().is_some() {
        return;
    }
    // Literals (Number, String, Boolean, Null), PathAccess: no variable refs.
}

// ==================== 单态化器（Monomorphizer） ====================

#[allow(dead_code)]
pub struct Monomorphizer {
    instance_counter: usize,
    instances: HashMap<String, String>,
}

impl Monomorphizer {
    pub fn new() -> Self {
        Monomorphizer {
            instance_counter: 0,
            instances: HashMap::new(),
        }
    }

    /// 对 IR 进行单态化，展开所有泛型
    pub fn monomorphize(&mut self, ir: &GobolIR) -> GobolIR {
        let mut result = ir.clone();
        
        // 收集需要实例化的泛型函数
        let mut generic_functions: Vec<IRFunction> = Vec::new();
        let mut concrete_functions: Vec<IRFunction> = Vec::new();
        
        for func in &ir.functions {
            if !func.generic_params.is_empty() {
                // 泛型函数：需要实例化
                // 从调用点收集实际类型参数
                let instances = self.collect_instances(ir, func);
                for (_type_args, instance) in instances {
                    generic_functions.push(instance);
                }
            } else {
                // 非泛型函数：直接保留
                concrete_functions.push(func.clone());
            }
        }
        
        // 更新结果
        result.functions = concrete_functions;
        result.functions.extend(generic_functions);
        
        result
    }

    fn collect_instances(&mut self, _ir: &GobolIR, func: &IRFunction) -> Vec<(Vec<DataType>, IRFunction)> {
        let mut instances = Vec::new();
        
        // 从函数体中收集类型参数
        if let Some(body) = &func.body {
            self.scan_for_type_args(body, &func.generic_params, &mut instances, func);
        }
        
        // 如果没有找到任何实例，使用默认类型
        if instances.is_empty() {
            // 默认使用 int, float, str
            for ty in [DataType::Int, DataType::Float, DataType::Str] {
                let type_args = vec![ty.clone()];
                let instance = self.instantiate_function(func, &type_args);
                instances.push((type_args, instance));
            }
        }
        
        instances
    }

    fn scan_for_type_args(
        &mut self,
        block: &IRBlock,
        _generic_params: &[String],
        instances: &mut Vec<(Vec<DataType>, IRFunction)>,
        func: &IRFunction,
    ) {
        for stmt in &block.statements {
            match stmt {
                IRStmt::Call { func: call_name, args: _args, generic_args } => {
                    if call_name == &func.name && !generic_args.is_empty() {
                        // 找到了一个泛型调用
                        let type_args = generic_args.clone();
                        let instance = self.instantiate_function(func, &type_args);
                        instances.push((type_args, instance));
                    }
                }
                IRStmt::MethodCall { args: _args, generic_args, .. } => {
                    if !generic_args.is_empty() {
                        let type_args = generic_args.clone();
                        let instance = self.instantiate_function(func, &type_args);
                        instances.push((type_args, instance));
                    }
                }
                _ => {}
            }
        }
    }

    fn instantiate_function(&mut self, func: &IRFunction, type_args: &[DataType]) -> IRFunction {
        // 生成实例化名称: func_T1_T2
        let type_suffix: String = type_args.iter()
            .map(|t| format!("_{}", t))
            .collect();
        let instance_name = format!("{}{}", func.name, type_suffix);
        
        // 创建类型映射
        let mut type_map = HashMap::new();
        for (i, param) in func.generic_params.iter().enumerate() {
            if i < type_args.len() {
                type_map.insert(param.clone(), type_args[i].clone());
            }
        }
        
        // 替换函数体中的泛型类型
        let mut instance = func.clone();
        instance.name = instance_name;
        instance.generic_params = Vec::new(); // 已经实例化，不再是泛型
        
        // 替换参数类型
        for param in &mut instance.params {
            param.ty = self.substitute_type(&param.ty, &type_map);
        }
        
        // 替换返回类型
        instance.return_type = self.substitute_type(&instance.return_type, &type_map);
        
        // 替换 body 中的类型
        if let Some(body) = &mut instance.body {
            self.substitute_in_block(body, &type_map);
        }
        
        instance
    }

    fn substitute_type(&self, dt: &DataType, type_map: &HashMap<String, DataType>) -> DataType {
        match dt {
            DataType::Struct(name) => {
                // 检查是否是泛型参数
                if let Some(actual) = type_map.get(name) {
                    actual.clone()
                } else {
                    dt.clone()
                }
            }
            DataType::Nullable(inner) => {
                DataType::Nullable(Box::new(self.substitute_type(inner, type_map)))
            }
            DataType::Array(inner) => {
                DataType::Array(Box::new(self.substitute_type(inner, type_map)))
            }
            _ => dt.clone(),
        }
    }

    fn substitute_in_block(&self, block: &mut IRBlock, type_map: &HashMap<String, DataType>) {
        for stmt in &mut block.statements {
            match stmt {
                IRStmt::Declaration { ty, .. } => {
                    *ty = self.substitute_type(ty, type_map);
                }
                IRStmt::Call { generic_args: _generic_args, .. } => {
                    // 替换泛型参数
                }
                _ => {}
            }
        }
    }
}
