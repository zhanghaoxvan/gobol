use crate::ast::*;
use crate::environment::DataType;

/// C code generator — walks the AST and emits C source code.
pub struct CodeGenC {
    output: String,
    indent: usize,
    current_function_ret: DataType,
    in_function: bool,
}

impl CodeGenC {
    pub fn new() -> Self {
        CodeGenC {
            output: String::new(),
            indent: 0,
            current_function_ret: DataType::None_,
            in_function: false,
        }
    }

    pub fn generate(&mut self, program: &Program) -> String {
        // Built-in C implementations
        self.emit_line("#include <stdio.h>");
        self.emit_line("#include <stdint.h>");
        self.emit_line("#include <stdbool.h>");
        self.emit_line("#include <string.h>");
        self.emit_line("#include <stdlib.h>");
        self.emit_line("");
        // ── built-in: _print ──
        self.emit_line("void gobol_print(const char* s) {");
        self.emit_line("    printf(\"%s\", s);");
        self.emit_line("}");
        self.emit_line("");
        // ── built-in: _read ──
        self.emit_line("char* gobol_read(void) {");
        self.emit_line("    static char buf[4096];");
        self.emit_line("    if (fgets(buf, sizeof(buf), stdin)) {");
        self.emit_line("        size_t len = strlen(buf);");
        self.emit_line("        if (len > 0 && buf[len-1] == '\\n') buf[len-1] = '\\0';");
        self.emit_line("        return buf;");
        self.emit_line("    }");
        self.emit_line("    return \"\";");
        self.emit_line("}");
        self.emit_line("");

        program.accept(self);
        std::mem::take(&mut self.output)
    }

    pub fn compile(source: &str, out_name: &str) -> Result<i32, String> {
        use std::process::Command;
        let c_file = format!("{}.c", out_name);
        std::fs::write(&c_file, source).map_err(|e| format!("write: {}", e))?;
        let status = Command::new("cc")
            .args(&[&c_file, "-o", out_name, "-std=c11", "-O2"])
            .status()
            .map_err(|e| format!("cc: {}", e))?;
        if !status.success() {
            return Err("C compilation failed".to_string());
        }
        let exe = if out_name.starts_with('/') || out_name.starts_with("./") {
            out_name.to_string()
        } else {
            format!("./{}", out_name)
        };
        let status = Command::new(&exe)
            .status()
            .map_err(|e| format!("run {}: {}", exe, e))?;
        Ok(status.code().unwrap_or(1))
    }

    // ── helpers ──

    fn emit(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn emit_line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        self.output.push_str(s);
        self.output.push('\n');
    }

    fn c_type_name(dt: &DataType) -> &str {
        match dt {
            DataType::Int => "int64_t",
            DataType::Float => "double",
            DataType::Bool => "bool",
            DataType::Str => "const char*",
            DataType::None_ => "void",
            DataType::Unknown => "int64_t",
            DataType::Struct(_) => "void*",
            DataType::Nullable(inner) => Self::c_type_name(inner),
        }
    }
}

impl AstVisitor for CodeGenC {
    fn visit_program(&mut self, node: &Program) {
        for stmt in node.get_statements() {
            if stmt.as_any().downcast_ref::<Function>().is_some()
                || stmt.as_any().downcast_ref::<StructDefinition>().is_some() {
                stmt.accept(self);
            }
        }
    }

    fn visit_function(&mut self, node: &Function) {
        let name = node.get_name();
        let ret_type = if name == "main" {
            DataType::Int // main always returns int in C
        } else if node.get_return_type().is_some() {
            DataType::Int
        } else {
            DataType::None_
        };
        self.current_function_ret = ret_type.clone();
        self.in_function = true;

        let ret = Self::c_type_name(&ret_type);

        // Parameters
        let mut param_strs = Vec::new();
        if let Some(params) = node.get_parameters() {
            for p in params {
                let pname = p.get_name();
                param_strs.push(format!("int64_t {}", pname));
            }
        }

        self.emit(&format!("{} {}({}) ", ret, name, param_strs.join(", ")));
        self.emit_line("{");
        self.indent += 1;

        if let Some(body) = node.get_body() {
            body.accept(self);
        }

        self.indent -= 1;
        self.emit_line("}");
        self.emit_line("");
        self.in_function = false;
    }

    fn visit_block(&mut self, node: &Block) {
        let stmts = node.get_statements();
        let len = stmts.len();
        for (i, stmt) in stmts.iter().enumerate() {
            let is_last = i == len - 1;
            // Last statement: if tail expression in function, wrap with return
            if is_last && self.in_function {
                if let Some(es) = stmt.as_any().downcast_ref::<ExpressionStatement>() {
                    if es.tail {
                        self.emit("return ");
                        es.get_expression().unwrap().accept(self);
                        self.emit_line(";");
                        continue;
                    }
                }
            }
            stmt.accept(self);
        }
    }

    fn visit_declaration(&mut self, node: &Declaration) {
        let ctype = if let Some(tp) = node.get_type() {
            match tp.get_name() {
                "int" => "int64_t",
                "float" => "double",
                "bool" => "bool",
                "str" => "const char*",
                _ => "int64_t",
            }
        } else {
            "int64_t"
        };

        self.emit(&format!("{} {} = ", ctype, node.get_name()));
        if let Some(init) = node.get_initializer() {
            init.accept(self);
        } else {
            self.emit("0");
        }
        self.emit_line(";");
    }

    fn visit_return_statement(&mut self, node: &ReturnStatement) {
        self.emit("return ");
        if let Some(val) = node.get_value() {
            val.accept(self);
        }
        self.emit_line(";");
    }

    fn visit_expression_statement(&mut self, node: &ExpressionStatement) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
            if !node.tail {
                self.emit_line(";");
            }
        }
    }

    fn visit_binary_expression(&mut self, node: &BinaryExpression) {
        let op = node.get_operator();
        if op == "=" {
            if let Some(left) = node.get_left() {
                left.accept(self);
            }
            self.emit(" = ");
            if let Some(right) = node.get_right() {
                right.accept(self);
            }
            return;
        }
        if let Some(left) = node.get_left() {
            left.accept(self);
        }
        let cop = match op {
            "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||"
            | "+" | "-" | "*" | "/" | "%"
            | "+=" | "-=" | "*=" | "/=" => op,
            _ => op,
        };
        self.emit(&format!(" {} ", cop));
        if let Some(right) = node.get_right() {
            right.accept(self);
        }
    }

    fn visit_identifier(&mut self, node: &Identifier) {
        self.emit(node.get_name());
    }

    fn visit_number_literal(&mut self, node: &NumberLiteral) {
        let v = node.get_value();
        if v == (v as i64) as f64 && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
            self.emit(&format!("{}", v as i64));
        } else {
            self.emit(&format!("{}", v));
        }
    }

    fn visit_string_literal(&mut self, node: &StringLiteral) {
        let s = node.get_value()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t");
        self.emit(&format!("\"{}\"", s));
    }

    fn visit_boolean_literal(&mut self, node: &BooleanLiteral) {
        self.emit(if node.get_value() { "true" } else { "false" });
    }

    fn visit_function_call(&mut self, node: &FunctionCall) {
        if let Some(callee) = node.get_callee() {
            // Handle io.print / io.read (stdlib wrappers) AND __builtins__._print / __builtins__._read
            if let Some(member) = callee.as_any().downcast_ref::<MemberAccess>() {
                if let Some(obj) = member.get_object() {
                    if let Some(obj_id) = obj.as_any().downcast_ref::<Identifier>() {
                        let mod_name = obj_id.get_name();
                        let func = member.get_member();
                        // io.println → puts
                        if mod_name == "io" && func == "println" {
                            self.emit("puts(");
                            if let Some(args) = node.get_arguments() {
                                if let Some(a) = args.first() { a.accept(self); }
                            }
                            self.emit(")");
                            return;
                        }
                        // io.print → printf
                        if mod_name == "io" && func == "print" {
                            if let Some(args) = node.get_arguments() {
                                if let Some(a) = args.first() {
                                    if a.as_any().downcast_ref::<StringLiteral>().is_some() {
                                        self.emit("printf(");
                                        a.accept(self);
                                        self.emit(")");
                                    } else {
                                        self.emit("printf(\"%ld\", ");
                                        a.accept(self);
                                        self.emit(")");
                                    }
                                    return;
                                }
                            }
                            self.emit("printf(\"\")");
                            return;
                        }
                        // io.read → gobol_read
                        if mod_name == "io" && func == "read" {
                            self.emit("gobol_read()");
                            return;
                        }
                        // __builtins__._print → gobol_print
                        if mod_name == "__builtins__" && func == "_print" {
                            self.emit("gobol_print(");
                            if let Some(args) = node.get_arguments() {
                                if let Some(a) = args.first() { a.accept(self); }
                            }
                            self.emit(")");
                            return;
                        }
                        // __builtins__._read → gobol_read
                        if mod_name == "__builtins__" && func == "_read" {
                            self.emit("gobol_read()");
                            return;
                        }
                        // __builtins__.exit / exit → exit()
                        if mod_name == "__builtins__" && func == "exit" {
                            self.emit("exit(");
                            if let Some(args) = node.get_arguments() {
                                if let Some(a) = args.first() { a.accept(self); }
                            }
                            self.emit(")");
                            return;
                        }
                    }
                }
            }
            // Normal function call
            callee.accept(self);
            self.emit("(");
            if let Some(args) = node.get_arguments() {
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 { self.emit(", "); }
                    arg.accept(self);
                }
            }
            self.emit(")");
        } else {
            self.emit("(");
            self.emit(")");
        }
    }

    fn visit_member_access(&mut self, node: &MemberAccess) {
        if let Some(obj) = node.get_object() {
            obj.accept(self);
        }
        self.emit(&format!(".{}", node.get_member()));
    }

    fn visit_if_statement(&mut self, node: &IfStatement) {
        self.emit("if (");
        if let Some(cond) = node.get_condition() {
            cond.accept(self);
        }
        self.emit(") ");
        self.emit_line("{");
        self.indent += 1;
        if let Some(then_branch) = node.get_then_branch() {
            then_branch.accept(self);
        }
        self.indent -= 1;
        if let Some(else_branch) = node.get_else_branch() {
            self.emit_line("} else {");
            self.indent += 1;
            else_branch.accept(self);
            self.indent -= 1;
        }
        self.emit_line("}");
    }

    fn visit_while_statement(&mut self, node: &WhileStatement) {
        self.emit("while (");
        if let Some(cond) = node.get_condition() {
            cond.accept(self);
        }
        self.emit(") ");
        self.emit_line("{");
        self.indent += 1;
        if let Some(body) = node.get_body() {
            body.accept(self);
        }
        self.indent -= 1;
        self.emit_line("}");
    }

    // ── stub visitors ──

    fn visit_ast_node(&mut self, _node: &dyn AstNode) {}
    fn visit_statement(&mut self, _node: &dyn Statement) {}
    fn visit_expression(&mut self, _node: &dyn Expression) {}
    fn visit_parameter(&mut self, _node: &Parameter) {}
    fn visit_basic_type(&mut self, _node: &BasicType) {}
    fn visit_type(&mut self, _node: &dyn Type) {}
    fn visit_array_type(&mut self, _node: &ArrayType) {}
    fn visit_for_statement(&mut self, _node: &ForStatement) {}
    fn visit_break_statement(&mut self, _node: &BreakStatement) {}
    fn visit_continue_statement(&mut self, _node: &ContinueStatement) {}
    fn visit_import_statement(&mut self, _node: &ImportStatement) {}
    fn visit_export_statement(&mut self, _node: &ExportStatement) {}
    fn visit_struct_definition(&mut self, _node: &StructDefinition) {}
    fn visit_impl_block(&mut self, _node: &ImplBlock) {}
    fn visit_cast_expression(&mut self, _node: &CastExpression) {}
    fn visit_unary_expression(&mut self, _node: &UnaryExpression) {}
    fn visit_grouped_expression(&mut self, _node: &GroupedExpression) {}
    fn visit_null_literal(&mut self, _node: &NullLiteral) {}
    fn visit_format_string(&mut self, _node: &FormatString) {}
    fn visit_range_expression(&mut self, _node: &RangeExpression) {}
    fn visit_array_literal(&mut self, _node: &ArrayLiteral) {}
    fn visit_array_index(&mut self, _node: &ArrayIndex) {}
    fn visit_struct_literal(&mut self, _node: &StructLiteral) {}
    fn visit_match_expression(&mut self, _node: &MatchExpression) {}
}
