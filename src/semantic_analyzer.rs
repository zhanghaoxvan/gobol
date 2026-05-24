#![allow(dead_code)]

use crate::ast::*;
use crate::environment::*;
use std::collections::HashMap;

pub struct SemanticAnalyzer {
    env: Environment,
    errors: Vec<String>,
    has_error: bool,
    current_function: String,
    current_function_return_type: DataType,
    has_return_statement: bool,
    loop_depth: i32,
    current_module: String,
    type_stack: Vec<DataType>,
    struct_fields: HashMap<String, HashMap<String, DataType>>,
    current_impl_struct: Option<String>,
}

impl SemanticAnalyzer {
    pub fn new() -> Self {
        SemanticAnalyzer {
            env: Environment::new(),
            errors: Vec::new(),
            has_error: false,
            current_function: String::new(),
            current_function_return_type: DataType::None_,
            has_return_statement: false,
            loop_depth: 0,
            current_module: String::new(),
            type_stack: Vec::new(),
            struct_fields: HashMap::new(),
            current_impl_struct: None,
        }
    }

    pub fn analyze(&mut self, program: &Program) -> bool {
        // Register built-in modules
        self.env.declare_module("__builtins__");
        self.env.declare_module("io");

        // Declare builtin functions
        self.env.declare_function("range", &DataType::Int, "__builtins__");
        self.env.declare_function("print", &DataType::None_, "io");
        self.env.declare_function("read", &DataType::Str, "io");
        self.env.declare_function("panic", &DataType::None_, "__builtins__");

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

    pub fn has_errors(&self) -> bool {
        self.has_error
    }

    pub fn get_errors(&self) -> &Vec<String> {
        &self.errors
    }

    pub fn print_errors(&self) {
        if self.errors.is_empty() {
            #[cfg(debug_assertions)]
            println!("✅ Semantic analysis passed!");
        } else {
            println!("❌ Semantic analysis failed with {} errors:", self.errors.len());
            for err in &self.errors {
                println!("  ⚠️  {}", err);
            }
        }
    }

    fn error(&mut self, msg: &str) {
        self.errors.push(msg.to_string());
        self.has_error = true;
    }

    fn get_data_type_from_ast(&mut self, tp: Option<&dyn Type>) -> DataType {
        let tp = match tp {
            Some(t) => t,
            None => return DataType::None_,
        };

        // Check for ArrayType via downcast
        if let Some(arr) = tp.as_type_any().downcast_ref::<ArrayType>() {
            let elem = arr.get_element_type();
            if elem.as_type_any().downcast_ref::<ArrayType>().is_some() {
                return self.get_data_type_from_ast(Some(elem));
            }
            return self.get_data_type_from_ast(Some(elem));
        }

        match tp.get_name() {
            "int" => DataType::Int,
            "float" => DataType::Float,
            "str" => DataType::Str,
            "bool" => DataType::Bool,
            name => {
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

    fn visit_module_statement(&mut self, node: &ModuleStatement) {
        let module_name = node.get_module_name().to_string();
        #[cfg(debug_assertions)]
        println!("  Module declaration: {}", module_name);
        self.env.declare_module(&module_name);
        self.current_module = module_name;
    }

    fn visit_struct_definition(&mut self, node: &StructDefinition) {
        let struct_name = node.get_name().to_string();
        #[cfg(debug_assertions)]
        println!("  Struct definition: {}", struct_name);

        let mut fields = HashMap::new();
        for field in node.get_fields() {
            let field_type = self.get_data_type_from_ast(field.field_type.as_deref());
            fields.insert(field.name.clone(), field_type);
        }
        self.struct_fields.insert(struct_name.clone(), fields);

        self.env.declare_module(&struct_name);
    }

    fn visit_impl_block(&mut self, node: &ImplBlock) {
        #[cfg(debug_assertions)]
        println!("  Impl block for: {}", node.get_struct_name());

        let prev_impl = self.current_impl_struct.clone();
        self.current_impl_struct = Some(node.get_struct_name().to_string());

        for item in node.get_items() {
            match item {
                ImplItem::Constructor(func) | ImplItem::Method(func) | ImplItem::Convert(func) => {
                    func.accept(self);
                }
            }
        }

        self.current_impl_struct = prev_impl;
    }

    fn visit_export_statement(&mut self, _node: &ExportStatement) {
        // Export is a compile-time concept; no runtime effect
        // TODO: validate exported names are declared in current module
    }

    fn visit_import_statement(&mut self, node: &ImportStatement) {
        let module_name = node.get_module_name();
        #[cfg(debug_assertions)]
        println!("  Import module: {}", module_name);

        if module_name != "io" && module_name != "__builtins__" {
            self.error(&format!("Unknown module: '{}'", module_name));
        }
    }

    fn visit_function(&mut self, node: &Function) {
        let func_name = node.get_name().to_string();
        #[cfg(debug_assertions)]
        println!("  Function: {}", func_name);

        let return_type = self.get_data_type_from_ast(node.get_return_type());

        if !self.env.declare_function(&func_name, &return_type, &self.current_module) {
            self.error(&format!(
                "Failed to declare function '{}.{}'",
                self.current_module, func_name
            ));
            return;
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
        if let Some(body) = node.get_body() {
            body.accept(self);
        }

        if return_type != DataType::None_ && !self.has_return_statement {
            self.error(&format!(
                "Function '{}' must return a value of type {}",
                self.current_function,
                data_type_to_string(return_type)
            ));
        }

        self.env.exit_scope();

        self.current_function = prev_function;
        self.current_function_return_type = prev_return_type;
        self.has_return_statement = prev_has_return;
    }

    fn visit_parameter(&mut self, node: &Parameter) {
        let param_name = node.get_name();
        let param_type = self.get_data_type_from_ast(node.get_type());

        self.env.declare_variable(param_name, &param_type, false);
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
        self.env.declare_variable(&var_name, &declared_type, is_mut);

        if let Some(init) = node.get_initializer() {
            init.accept(self);
            let init_type = self.get_current_type();
            self.check_type_compatibility(
                declared_type,
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
        let loop_var = node.get_loop_variable().to_string();

        self.env.enter_scope();
        #[cfg(debug_assertions)]
        println!("    For loop (scope {})", self.env.get_current_scope());

        self.env.declare_variable(&loop_var, &DataType::Int, false);
        #[cfg(debug_assertions)]
        println!("      Loop variable: {} : int", loop_var);

        if let Some(iter) = node.get_iterable() {
            iter.accept(self);
            let iter_type = self.get_current_type();

            if iter_type != DataType::Int {
                self.error("For loop iterable must be range expression");
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

        // Look up in current module
        let full_name = format!("{}.{}", self.current_module, name);
        let sym = self.env.lookup_symbol(&full_name)
            .or_else(|| {
                let builtin = format!("__builtins__.{}", name);
                self.env.lookup_symbol(&builtin)
            })
            .or_else(|| self.env.lookup_symbol(name));

        match sym {
            Some(s) => self.type_stack.push(s.data_type.clone()),
            None => {
                self.error(&format!("Undeclared identifier: '{}'", name));
                self.type_stack.push(DataType::Unknown);
            }
        }
    }

    fn visit_number_literal(&mut self, node: &NumberLiteral) {
        let value = node.get_value();
        if value == (value as i64) as f64 {
            self.type_stack.push(DataType::Int);
        } else {
            self.type_stack.push(DataType::Float);
        }
    }

    fn visit_string_literal(&mut self, _node: &StringLiteral) {
        self.type_stack.push(DataType::Str);
    }

    fn visit_null_literal(&mut self, _node: &NullLiteral) {
        self.type_stack.push(DataType::None_);
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

                    if let Some(arr_id) = array.as_any().downcast_ref::<Identifier>() {
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

            if !Environment::is_numeric_type(&left_type) || !Environment::is_numeric_type(&right_type) {
                self.error(&format!("Operator '{}' requires numeric operands", op));
                self.type_stack.push(DataType::Unknown);
                return;
            }

            if left_type == DataType::Float || right_type == DataType::Float {
                self.type_stack.push(DataType::Float);
            } else {
                self.type_stack.push(DataType::Int);
            }
            return;
        }

        if op == "==" || op == "!=" || op == "<" || op == ">" || op == "<=" || op == ">=" {
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

        if op == "&&" || op == "||" {
            if left_type != DataType::Bool || right_type != DataType::Bool {
                self.error("Logical operators require boolean operands");
            }
            self.type_stack.push(DataType::Bool);
            return;
        }

        self.error(&format!("Unknown operator: {}", op));
        self.type_stack.push(DataType::Unknown);
    }

    fn visit_unary_expression(&mut self, node: &UnaryExpression) {
        if let Some(operand) = node.get_operand() {
            operand.accept(self);
        }
        let operand_type = self.get_current_type();
        let op = node.get_operator();

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
        let mut func_name = String::new();
        let mut module_name = self.current_module.clone();

        if let Some(callee) = node.get_callee() {
            if let Some(id) = callee.as_any().downcast_ref::<Identifier>() {
                func_name = id.get_name().to_string();
            } else if let Some(member) = callee.as_any().downcast_ref::<MemberAccess>() {
                if let Some(obj) = member.get_object() {
                    if let Some(obj_id) = obj.as_any().downcast_ref::<Identifier>() {
                        module_name = obj_id.get_name().to_string();
                        func_name = member.get_member().to_string();
                    }
                }
            }
        }

        // Check if it's a struct constructor call (e.g. Point(1, 2))
        if self.struct_fields.contains_key(&func_name) {
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            self.type_stack.push(DataType::Struct(func_name.clone()));
            return;
        }

        let full_name = format!("{}.{}", module_name, func_name);
        let sym_data_type = self.env.lookup_symbol(&full_name).map(|s| s.data_type.clone());

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

                // Check module-level lookup (existing behavior)
                let full_name = format!("{}.{}", obj_name, member);
                if let Some(sym) = self.env.lookup_symbol(&full_name) {
                    self.type_stack.push(sym.data_type.clone());
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
        self.type_stack.push(DataType::Int);
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
                if let Some(sym) = self.env.lookup_symbol(id.get_name()) {
                    if !sym.is_array {
                        self.error(&format!("Variable '{}' is not an array", id.get_name()));
                        self.type_stack.push(DataType::Unknown);
                        return;
                    }
                    self.type_stack.push(sym.data_type.clone());
                    return;
                }

                self.error(&format!("Array variable '{}' not declared", id.get_name()));
                self.type_stack.push(DataType::Unknown);
                return;
            }
        }

        self.type_stack.push(array_type);
    }
}
