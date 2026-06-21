// ir.rs
use crate::ast::*;
use crate::environment::DataType;
use std::collections::HashMap;

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
    Assignment { target: Box<IRExpr>, value: Box<IRExpr> },  // ← 添加这个
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
    
    // 泛型上下文
    generic_stack: Vec<HashMap<String, DataType>>,
    
    // 类型环境
    structs: HashMap<String, IRStruct>,
    methods: HashMap<String, Vec<IRFunction>>,
    
    // 错误收集
    errors: Vec<String>,
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
            generic_stack: Vec::new(),
            structs: HashMap::new(),
            methods: HashMap::new(),
            errors: Vec::new(),
        }
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

        // 检查数组类型 → return Unknown to mark as array
        if ty.as_type_any().downcast_ref::<ArrayType>().is_some() {
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
            func.body = Some(IRBlock {
                statements: std::mem::take(&mut self.current_block),
            });
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
            
            if let Some(block_node) = body.as_any().downcast_ref::<Block>() {
                for stmt in block_node.get_statements() {
                    stmt.accept(&mut sub_builder);
                }
            } else {
                body.accept(&mut sub_builder);
            }
            
            block.statements.extend(sub_builder.current_block);
        }
        
        block
    }

    #[allow(dead_code)]
    fn error(&mut self, msg: &str) {
        self.errors.push(msg.to_string());
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

        let ir_struct = IRStruct {
            name: name.clone(),
            generic_params: generic_params.clone(),
            fields,
        };

        self.structs.insert(name.clone(), ir_struct.clone());
        self.ir.structs.push(ir_struct);
    }

    fn visit_impl_block(&mut self, node: &ImplBlock) {
        let struct_name = node.get_struct_name().to_string();
        let generic_params = node.get_generic_params().clone();

        self.current_struct = Some(struct_name.clone());
        self.in_impl = true;
        self.push_generic_scope(&generic_params);

        let mut methods = Vec::new();
        
        for item in node.get_items() {
            match item {
                ImplItem::Method(func) | ImplItem::Constructor(func) | ImplItem::Convert(func) => {
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
                        ir_func.struct_name = Some(struct_name.clone());
                        ir_func.is_method = true;
                        
                        // 保存方法名供后续查找
                        let _method_name = ir_func.name.clone();
                        methods.push(ir_func.clone());
                        self.methods
                            .entry(struct_name.clone())
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

        // 解析参数
        let params: Vec<IRParam> = node.get_parameters()
            .map(|ps| {
                ps.iter()
                    .map(|p| {
                        let name = p.get_name().to_string();
                        let ty = if name == "self" {
                            if let Some(s) = &self.current_struct {
                                DataType::Struct(s.clone())
                            } else {
                                self.ast_type_to_data_type(p.get_type())
                            }
                        } else {
                            self.ast_type_to_data_type(p.get_type())
                        };
                        IRParam { name, ty }
                    })
                    .collect()
            })
            .unwrap_or_default();

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
                format!("{}.{}", s, name)
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
        });

        // 处理函数体
        if let Some(body) = node.get_body() {
            // 先注册 self 参数（如果是方法）
            if is_method && self.current_struct.is_some() {
                // self 作为隐式参数
            }
            body.accept(self);
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
        let ty = self.ast_type_to_data_type(node.get_type());
        
        let init = if let Some(init_expr) = node.get_initializer() {
            init_expr.accept(self);
            let expr = self.pop_expr();
            Some(expr)
        } else {
            None
        };

        self.current_block.push(IRStmt::Declaration { name, ty, init });
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

        // then 分支
        let then_block = if let Some(then_branch) = node.get_then_branch() {
            let mut builder = IRBuilder::new();
            then_branch.accept(&mut builder);
            IRBlock {
                statements: builder.current_block,
            }
        } else {
            IRBlock { statements: vec![] }
        };

        // else 分支
        let else_block = if let Some(else_branch) = node.get_else_branch() {
            let mut builder = IRBuilder::new();
            else_branch.accept(&mut builder);
            Some(IRBlock {
                statements: builder.current_block,
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
            let mut builder = IRBuilder::new();
            b.accept(&mut builder);
            IRBlock {
                statements: builder.current_block,
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
            // 泛型参数作为类型使用，不是变量
            // 在这种情况下，它应该在类型上下文中处理
            self.push_expr(IRExpr::Variable(name));
            return;
        }
        
        // 检查是否是 self
        if name == "self" {
            self.push_expr(IRExpr::Variable("self".to_string()));
            return;
        }

        self.push_expr(IRExpr::Variable(name));
    }

    fn visit_number_literal(&mut self, node: &NumberLiteral) {
        let v = node.get_value();
        if v == (v as i64) as f64 {
            self.push_expr(IRExpr::Literal(LitValue::Int(v as i64)));
        } else {
            self.push_expr(IRExpr::Literal(LitValue::Float(v)));
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
                IRExpr::Binary {
                    op: real_op.to_string(),
                    left: Box::new(left_val),
                    right: Box::new(right_val),
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
        
        // 普通二元运算
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
    }

    fn visit_unary_expression(&mut self, node: &UnaryExpression) {
        let op = node.get_operator().to_string();
        let operand = node.get_operand().unwrap();
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

            // 普通函数调用
            if let Some(id) = callee_expr.as_any().downcast_ref::<Identifier>() {
                let func_name = id.get_name().to_string();
                
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
        // 1. 求值 scrutinee
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

        // 2. 构建 match 的 if-else 链
        // 使用递归构建：从最后一个 arm 开始反向构建
        let mut else_block = None;
        
        // 从后往前遍历 arms
        for arm in arms.iter().rev() {
            // 构建条件表达式
            let cond = self.build_match_condition(&scrutinee, &arm.pattern);
            
            // 构建 arm body
            let then_block = self.build_arm_body(arm);
            
            // 创建 if 语句
            let if_stmt = IRStmt::If {
                cond,
                then_block,
                else_block: else_block.take(),
            };
            
            // 将 if 语句包装成 Block
            let mut block = IRBlock { statements: Vec::new() };
            block.statements.push(if_stmt);
            else_block = Some(block);
        }

        // 3. 将生成的 if-else 链插入当前块
        if let Some(final_block) = else_block {
            self.current_block.extend(final_block.statements);
        }

        // 4. match 表达式的结果
        // 在 IR 层面，match 表达式的值就是最后一个匹配的 arm 的值
        // 但我们无法确定哪个 arm 会匹配，所以用 None 占位
        // 后续代码生成器需要特殊处理 match 表达式
        self.push_expr(IRExpr::None);
    }

    fn visit_range_expression(&mut self, node: &RangeExpression) {
        let mut args = Vec::new();
        for arg in node.get_arguments() {
            arg.accept(self);
            args.push(self.pop_expr());
        }
        // range 被转换为函数调用
        self.push_expr(IRExpr::Call {
            func: "range".to_string(),
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
                // 需要克隆或重新构建表达式
                // 由于表达式是 trait 对象，我们只能重新访问
                // 这里简化处理
                let mut temp_builder = IRBuilder::new();
                value.accept(&mut temp_builder);
                let var_expr = temp_builder.pop_expr();
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
                let mut temp = IRBuilder::new();
                iterable.accept(&mut temp);
                let iter_expr = temp.pop_expr();
                let mut body_builder = IRBuilder::new();
                if let Some(b) = node.get_body() {
                    b.accept(&mut body_builder);
                }
                self.current_block.push(IRStmt::For {
                    vars,
                    iterable: iter_expr,
                    body: IRBlock { statements: body_builder.current_block },
                });
            }
        }
    }
    fn visit_import_statement(&mut self, _node: &ImportStatement) {}
    fn visit_export_statement(&mut self, _node: &ExportStatement) {}
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
