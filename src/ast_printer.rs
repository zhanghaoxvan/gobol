use crate::ast::*;

pub struct AstPrinter {
    indent_level: i32,
}
#[allow(dead_code)]
impl AstPrinter {
    pub fn new() -> Self {
        AstPrinter { indent_level: 0 }
    }

    fn print_indent(&self) {
        print!("{}", " ".repeat((self.indent_level * 2) as usize));
    }

    pub fn visit(&mut self, node: &dyn AstNode) {
        node.accept(self);
    }
}

impl AstVisitor for AstPrinter {
    fn visit_ast_node(&mut self, _node: &dyn AstNode) {
        self.print_indent();
        println!("ASTNode");
    }

    fn visit_program(&mut self, node: &Program) {
        self.print_indent();
        println!("Program");
        self.indent_level += 1;
        for stmt in node.get_statements() {
            stmt.accept(self);
        }
        self.indent_level -= 1;
    }

    fn visit_statement(&mut self, _node: &dyn Statement) {
        self.print_indent();
        println!("Statement");
    }

    fn visit_expression(&mut self, _node: &dyn Expression) {
        self.print_indent();
        println!("Expression");
    }

    fn visit_block(&mut self, node: &Block) {
        self.print_indent();
        println!("Block");
        self.indent_level += 1;
        for stmt in node.get_statements() {
            stmt.accept(self);
        }
        self.indent_level -= 1;
    }

    fn visit_function(&mut self, node: &Function) {
        self.print_indent();
        println!("Function");
        self.indent_level += 1;

        self.print_indent();
        println!("name: {}", node.get_name());

        self.print_indent();
        println!("parameters:");
        self.indent_level += 1;
        if let Some(params) = node.get_parameters() {
            for param in params {
                param.accept(self);
            }
        }
        self.indent_level -= 1;

        self.print_indent();
        print!("return-type: ");
        if let Some(rt) = node.get_return_type() {
            rt.accept(self);
        }
        println!();

        self.print_indent();
        println!("body:");
        self.indent_level += 1;
        if let Some(body) = node.get_body() {
            body.accept(self);
        }
        self.indent_level -= 1;

        self.indent_level -= 1;
    }

    fn visit_parameter(&mut self, node: &Parameter) {
        self.print_indent();
        print!("{}: ", node.get_name());
        if let Some(t) = node.get_type() {
            t.accept(self);
        }
        println!();
    }

    fn visit_basic_type(&mut self, node: &BasicType) {
        print!("{}", node.get_name());
    }

    fn visit_type(&mut self, node: &dyn Type) {
        if let Some(arr) = node.as_type_any().downcast_ref::<ArrayType>() {
            arr.accept(self);
        } else {
            print!("{}", node.get_name());
        }
    }

    fn visit_array_type(&mut self, node: &ArrayType) {
        if node.is_multi_dimensional() {
            let element = node.get_element_type();
            if let Some(arr) = element.as_type_any().downcast_ref::<ArrayType>() {
                arr.accept(self);
            } else {
                print!("{}", element.get_name());
            }
        } else {
            print!("{}", node.get_base_type_name());
        }

        print!("[");
        if let Some(size) = node.get_size() {
            size.accept(self);
        } else {
            print!("?");
        }
        print!("]");
    }

    fn visit_if_statement(&mut self, node: &IfStatement) {
        self.print_indent();
        println!("IfStatement");
        self.indent_level += 1;

        self.print_indent();
        print!("condition:");
        if let Some(cond) = node.get_condition() {
            cond.accept(self);
        }
        println!();

        self.print_indent();
        println!("then:");
        self.indent_level += 1;
        if let Some(then_branch) = node.get_then_branch() {
            then_branch.accept(self);
        }
        self.indent_level -= 1;

        if let Some(else_branch) = node.get_else_branch() {
            self.print_indent();
            println!("else:");
            self.indent_level += 1;
            else_branch.accept(self);
            self.indent_level -= 1;
        }

        self.indent_level -= 1;
    }

    fn visit_while_statement(&mut self, node: &WhileStatement) {
        self.print_indent();
        println!("WhileStatement");
        self.indent_level += 1;

        self.print_indent();
        println!("condition:");
        self.indent_level += 1;
        if let Some(cond) = node.get_condition() {
            cond.accept(self);
        }
        self.indent_level -= 1;

        self.print_indent();
        println!("body:");
        if let Some(body) = node.get_body() {
            body.accept(self);
        }

        self.indent_level -= 1;
    }

    fn visit_for_statement(&mut self, node: &ForStatement) {
        self.print_indent();
        println!("ForStatement");
        self.indent_level += 1;

        self.print_indent();
        println!("variable: {}", node.get_loop_variable());

        self.print_indent();
        print!("iterable: ");
        self.indent_level += 1;
        if let Some(iter) = node.get_iterable() {
            iter.accept(self);
        }
        println!();
        self.indent_level -= 1;

        self.print_indent();
        println!("body:");
        self.indent_level += 1;
        if let Some(body) = node.get_body() {
            body.accept(self);
        }

        self.indent_level -= 1;
        self.indent_level -= 1;
    }

    fn visit_return_statement(&mut self, node: &ReturnStatement) {
        self.print_indent();
        print!("ReturnStatement");
        if let Some(val) = node.get_value() {
            print!(" ");
            val.accept(self);
        }
        println!();
    }

    fn visit_break_statement(&mut self, _node: &BreakStatement) {
        self.print_indent();
        println!("BreakStatement");
    }

    fn visit_continue_statement(&mut self, _node: &ContinueStatement) {
        self.print_indent();
        println!("ContinueStatement");
    }

    fn visit_declaration(&mut self, node: &Declaration) {
        self.print_indent();
        print!("{} {}", node.get_keyword(), node.get_name());
        if let Some(t) = node.get_type() {
            print!(": ");
            t.accept(self);
        }
        if let Some(init) = node.get_initializer() {
            print!(" = ");
            init.accept(self);
        }
        println!();
    }

    fn visit_expression_statement(&mut self, node: &ExpressionStatement) {
        self.print_indent();
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
        println!(";");
    }

    fn visit_import_statement(&mut self, node: &ImportStatement) {
        self.print_indent();
        print!("Import(path = {})", node.get_module_name());
        if let Some(alias) = node.get_alias() {
            print!(" as {}", alias);
        }
        println!();
    }

    fn visit_module_statement(&mut self, node: &ModuleStatement) {
        self.print_indent();
        println!("Module(moduleName = {})", node.get_module_name());
    }

    fn visit_struct_definition(&mut self, node: &StructDefinition) {
        self.print_indent();
        print!("Struct {}(", node.get_name());
        for (i, field) in node.get_fields().iter().enumerate() {
            if i > 0 { print!(", "); }
            print!("{}: ", field.name);
            if let Some(t) = &field.field_type {
                t.accept(self);
            }
        }
        println!(")");
    }

    fn visit_impl_block(&mut self, node: &ImplBlock) {
        self.print_indent();
        println!("Impl {} {{", node.get_struct_name());
        self.indent_level += 1;
        for item in node.get_items() {
            match item {
                ImplItem::Constructor(func) => {
                    self.print_indent();
                    println!("Constructor: {}", func.get_name());
                }
                ImplItem::Method(func) => {
                    func.accept(self);
                }
                ImplItem::Convert(func) => {
                    self.print_indent();
                    println!("Convert: {}", func.get_name());
                }
            }
        }
        self.indent_level -= 1;
        self.print_indent();
        println!("}}");
    }

    fn visit_export_statement(&mut self, node: &ExportStatement) {
        self.print_indent();
        print!("Export(");
        for (i, name) in node.get_names().iter().enumerate() {
            if i > 0 {
                print!(", ");
            }
            print!("{}", name);
        }
        println!(")");
    }

    fn visit_binary_expression(&mut self, node: &BinaryExpression) {
        print!("(");
        if let Some(left) = node.get_left() {
            left.accept(self);
        }
        print!(" {} ", node.get_operator());
        if let Some(right) = node.get_right() {
            right.accept(self);
        }
        print!(")");
    }

    fn visit_cast_expression(&mut self, node: &CastExpression) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
        print!(" as ");
        node.get_target_type().accept(self);
    }

    fn visit_unary_expression(&mut self, node: &UnaryExpression) {
        print!("{}", node.get_operator());
        if let Some(operand) = node.get_operand() {
            operand.accept(self);
        }
    }

    fn visit_function_call(&mut self, node: &FunctionCall) {
        if let Some(callee) = node.get_callee() {
            callee.accept(self);
        }
        print!("(");
        if let Some(args) = node.get_arguments() {
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                arg.accept(self);
            }
        }
        print!(")");
    }

    fn visit_member_access(&mut self, node: &MemberAccess) {
        if let Some(obj) = node.get_object() {
            obj.accept(self);
        }
        print!(".{}", node.get_member());
    }

    fn visit_array_index(&mut self, node: &ArrayIndex) {
        if let Some(arr) = node.get_array() {
            arr.accept(self);
        }
        print!("[");
        if let Some(idx) = node.get_index() {
            idx.accept(self);
        }
        print!("]");
    }

    fn visit_grouped_expression(&mut self, node: &GroupedExpression) {
        print!("(");
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
        print!(")");
    }

    fn visit_identifier(&mut self, node: &Identifier) {
        print!("{}", node.get_name());
    }

    fn visit_number_literal(&mut self, node: &NumberLiteral) {
        print!("{}", node.get_value());
    }

    fn visit_string_literal(&mut self, node: &StringLiteral) {
        print!("\"");
        for c in node.get_value().chars() {
            match c {
                '\t' => print!("\\t"),
                '\n' => print!("\\n"),
                '\\' => print!("\\\\"),
                '"' => print!("\\\""),
                _ => print!("{}", c),
            }
        }
        print!("\"");
    }

    fn visit_null_literal(&mut self, _node: &NullLiteral) {
        print!("null");
    }

    fn visit_boolean_literal(&mut self, node: &BooleanLiteral) {
        print!("{}", if node.get_value() { "true" } else { "false" });
    }

    fn visit_format_string(&mut self, node: &FormatString) {
        print!("@\"");
        for c in node.get_value().chars() {
            match c {
                '\n' => print!("\\n"),
                '\t' => print!("\\t"),
                '\\' => print!("\\\\"),
                '"' => print!("\\\""),
                _ => print!("{}", c),
            }
        }
        print!("\"");

        let vars = node.get_variables();
        if !vars.is_empty() {
            print!(" [");
            for (i, var) in vars.iter().enumerate() {
                if i > 0 {
                    print!(", ");
                }
                if let Some(ref val) = var.value {
                    val.accept(self);
                    print!(":{}", var.pos_in_value);
                } else {
                    print!("?@{}", var.pos_in_value);
                }
            }
            print!("]");
        }
    }

    fn visit_range_expression(&mut self, node: &RangeExpression) {
        print!("range(");
        for (i, arg) in node.get_arguments().iter().enumerate() {
            if i > 0 {
                print!(", ");
            }
            arg.accept(self);
        }
        print!(")");
    }

    fn visit_array_literal(&mut self, node: &ArrayLiteral) {
        print!("[");
        for (i, elem) in node.get_elements().iter().enumerate() {
            if i > 0 {
                print!(", ");
            }
            elem.accept(self);
        }
        print!("]");
    }

    fn visit_struct_literal(&mut self, node: &StructLiteral) {
        print!("{}", node.get_type_name());
        print!("{{");
        for (i, field) in node.get_fields().iter().enumerate() {
            if i > 0 {
                print!(", ");
            }
            match field {
                StructFieldInit::Named { name, value } => {
                    print!("{}: ", name);
                    value.accept(self);
                }
                StructFieldInit::Positional(value) => {
                    value.accept(self);
                }
            }
        }
        print!("}}");
    }
}
