use super::*;

impl AstVisitor for SemanticAnalyzer {
    fn visit_ast_node(&mut self, _node: &dyn AstNode) {}

    fn visit_statement(&mut self, _node: &dyn Statement) {}

    fn visit_expression(&mut self, _node: &dyn Expression) {}

    fn visit_program(&mut self, node: &Program) {
        for stmt in node.get_statements() {
            stmt.accept(self);
        }
    }

    fn visit_struct_definition(&mut self, node: &StructDefinition) {
        let struct_name = node.get_name().to_string();
        #[cfg(debug_assertions)]
        println!("  Struct definition: {}", struct_name);

        // Push generic scope so that type params like T are resolved in field types
        let prev_generic = self.current_generic_params.clone();
        self.current_generic_params = node.get_generic_params().clone();
        self.generic_struct_params
            .insert(struct_name.clone(), node.get_generic_params().clone());
        let mut fields = HashMap::new();
        for field in node.get_fields() {
            let field_type = self.get_data_type_from_ast(field.field_type.as_deref());
            fields.insert(field.name.clone(), field_type);
        }
        self.struct_fields.insert(struct_name.clone(), fields);

        self.env.declare_module(&struct_name);
        self.current_generic_params = prev_generic;
    }

    fn visit_enum_definition(&mut self, node: &EnumDefinition) {
        let enum_name = node.get_name().to_string();
        let generic_params = node.get_generic_params().clone();
        #[cfg(debug_assertions)]
        println!(
            "  Enum definition: {} ({} variants)",
            enum_name,
            node.get_variants().len()
        );

        let prev_generic = self.current_generic_params.clone();
        self.current_generic_params = generic_params.clone();
        self.generic_struct_params
            .insert(enum_name.clone(), generic_params.clone());

        let mut fields = HashMap::new();
        fields.insert("_tag".to_string(), DataType::Int);

        let mut variant_names = Vec::new();
        let mut variant_payloads = Vec::new();
        let mut max_payload_count = 0;
        for variant in node.get_variants() {
            variant_names.push(variant.name.clone());
            let payload_types = variant
                .payload_types
                .iter()
                .map(|payload| self.get_data_type_from_ast(Some(payload.as_ref())))
                .collect::<Vec<_>>();
            max_payload_count = max_payload_count.max(payload_types.len());
            variant_payloads.push(payload_types);
        }
        for payload_index in 0..max_payload_count {
            fields.insert(format!("_{}", payload_index), DataType::Int);
        }

        self.enum_info
            .insert(enum_name.clone(), (variant_names, variant_payloads));
        self.enum_generic_params
            .insert(enum_name.clone(), generic_params.clone());

        self.struct_fields.insert(enum_name.clone(), fields);
        self.env.declare_module(&enum_name);

        for variant in node.get_variants() {
            let ctor_name = variant.name.clone();
            let return_type = DataType::Struct(enum_name.clone());

            self.env
                .declare_function(&ctor_name, &return_type, &enum_name);
            self.env.declare_function(
                &format!("{}::{}", enum_name, ctor_name),
                &return_type,
                &self.current_module,
            );
        }

        self.current_generic_params = prev_generic;
    }

    fn visit_impl_block(&mut self, node: &ImplBlock) {
        #[cfg(debug_assertions)]
        println!("  Impl block for: {}", node.get_struct_name());

        // If this is `impl Trait for Type`, only validate if the trait has
        // already been loaded. Otherwise defer to validate_trait_impls()
        // which runs after all modules are loaded (the first pass already
        // queued this block in pending_trait_impls).
        if let Some(trait_name) = node.get_trait_name() {
            if let Some(trait_info) = self.lookup_trait(trait_name) {
                let impl_methods: HashMap<String, Vec<usize>> = node
                    .get_items()
                    .iter()
                    .filter_map(|item| match item {
                        ImplItem::Method(func) | ImplItem::Convert(func) => {
                            if self.build_mode == BuildMode::Release
                                && Attribute::has_attr(func.get_attributes(), "debug")
                            {
                                None
                            } else {
                                let name = func.get_name().to_string();
                                let param_count = func.get_parameters().iter().len();
                                Some((name, param_count))
                            }
                        }
                    })
                    .fold(HashMap::new(), |mut map, (name, count)| {
                        map.entry(name).or_insert_with(Vec::new).push(count);
                        map
                    });

                for required in &trait_info.methods {
                    if !impl_methods.contains_key(&required.name) {
                        self.error(&format!(
                            "Trait '{}' requires method '{}', but it is missing in impl for '{}'",
                            trait_name,
                            required.name,
                            node.get_struct_name()
                        ));
                        // #[dynamic] methods can have any signature — skip param check
                        if required.dynamic {
                            continue;
                        }
                    }
                }
            }
            // Trait not loaded yet — validate_trait_impls() will handle it.
        }

        let prev_impl = self.current_impl_struct.clone();
        let prev_generic = self.current_generic_params.clone();
        self.current_impl_struct = Some(
            node.get_struct_name()
                .split('<')
                .next()
                .unwrap_or(node.get_struct_name())
                .to_string(),
        );
        self.current_generic_params = node.get_generic_params().clone();

        // === Pass 1: Register all method names (without analyzing bodies) ===
        // This ensures that methods defined in later impl blocks are available
        // for lookup when analyzing earlier impl blocks (e.g. Vec::push is needed
        // by New::new which is in a different impl block).
        for item in node.get_items() {
            match item {
                ImplItem::Method(func) | ImplItem::Convert(func) => {
                    if self.build_mode == BuildMode::Release
                        && Attribute::has_attr(func.get_attributes(), "debug")
                    {
                        continue;
                    }
                    let struct_name = node.get_struct_name().to_string();
                    let method_name = func.get_name().to_string();
                    let arity = func.get_parameters().map(|p| p.len()).unwrap_or(0);
                    if !self
                        .impl_methods
                        .insert((struct_name.clone(), method_name.clone(), arity))
                    {
                        continue;
                    }
                    let prev_fn_generic = self.current_generic_params.clone();
                    for g in func.get_generic_params() {
                        if !self.current_generic_params.contains(g) {
                            self.current_generic_params.push(g.clone());
                        }
                    }
                    let return_type = self.get_data_type_from_ast(func.get_return_type());
                    self.env
                        .declare_function(&method_name, &return_type, &self.current_module);
                    if let Some(ref struct_name) = self.current_impl_struct {
                        self.env
                            .declare_function(&method_name, &return_type, struct_name);
                    }
                    self.current_generic_params = prev_fn_generic;
                }
            }
        }

        // === Pass 2: Analyze all method bodies ===
        for item in node.get_items() {
            match item {
                ImplItem::Method(func) | ImplItem::Convert(func) => {
                    if self.build_mode == BuildMode::Release
                        && Attribute::has_attr(func.get_attributes(), "debug")
                    {
                        continue;
                    }
                    let struct_name = node.get_struct_name().to_string();
                    let method_name = func.get_name().to_string();
                    let arity = func.get_parameters().map(|p| p.len()).unwrap_or(0);
                    // Skip if not registered in pass 1 (dedup)
                    if !self
                        .impl_methods
                        .contains(&(struct_name, method_name, arity))
                    {
                        continue;
                    }
                    let prev_fn_generic = self.current_generic_params.clone();
                    for g in func.get_generic_params() {
                        if !self.current_generic_params.contains(g) {
                            self.current_generic_params.push(g.clone());
                        }
                    }
                    func.accept(self);
                    self.current_generic_params = prev_fn_generic;
                }
            }
        }

        self.current_impl_struct = prev_impl;
        self.current_generic_params = prev_generic;
    }

    fn visit_trait_definition(&mut self, node: &TraitDefinition) {
        #[cfg(debug_assertions)]
        println!("  Trait definition: {}", node.get_name());

        let methods: Vec<TraitMethodSig> = node
            .get_methods()
            .iter()
            .map(|m| TraitMethodSig {
                name: m.name.clone(),
                param_count: m.parameters.len(),
                dynamic: Attribute::has_attr(&m.attributes, "dynamic"),
            })
            .collect();
        self.trait_defs.insert(
            node.get_name().to_string(),
            TraitDefInfo {
                name: node.get_name().to_string(),
                methods,
                generic_params: node.get_generic_params().clone(),
            },
        );
    }

    fn visit_export_statement(&mut self, _node: &ExportStatement) {
        // Export is a compile-time concept; no runtime effect
        // TODO: validate exported names are declared in current module
    }

    fn visit_extern_block(&mut self, node: &ExternBlock) {
        #[cfg(debug_assertions)]
        println!("  Extern block: lib={:?}", node.get_library());

        if !self.platform_matches(node.get_attributes()) {
            return;
        }

        // Every extern "C" block MUST specify a #[header("path")] attribute
        // pointing to the C header that declares these functions. The header
        // is used for signature validation only — symbols are NOT auto-imported;
        // the user must explicitly list each function in the block.
        let header_path = match Attribute::get_attr_value(node.get_attributes(), "header") {
            Some(path) => path.to_string(),
            None => {
                self.error(
                    "extern \"C\" block requires a #[header(\"path/to/header.h\")] attribute \
                     specifying the C header that provides these functions",
                );
                return;
            }
        };

        // Validate that the header file exists and contains declarations
        // for every function in the block.
        let header_content = match self.read_header_file(&header_path) {
            Ok(content) => content,
            Err(e) => {
                self.error(&format!(
                    "Cannot read C header '{}' specified by #[header]: {}",
                    header_path, e
                ));
                return;
            }
        };

        if let Some(lib) = node.get_library() {
            // "C" is an ABI specifier, not a library name — don't pass -lC.
            // Only collect actual library names (e.g. "m" → -lm).
            if lib != "C" && !self.extern_libs.iter().any(|l| l == lib) {
                self.extern_libs.push(lib.to_string());
            }

            for attr in node.get_attributes().iter().filter(|a| a.name == "link") {
                let Some(name) = attr
                    .named
                    .iter()
                    .find(|(key, _)| key == "name")
                    .map(|(_, value)| value.clone())
                else {
                    self.error("#[link] requires a name=\"...\" argument");
                    continue;
                };
                let kind = attr
                    .named
                    .iter()
                    .find(|(key, _)| key == "kind")
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| "dylib".to_string());
                if !matches!(kind.as_str(), "dylib" | "static" | "framework") {
                    self.error(&format!("unsupported #[link] kind '{}'", kind));
                    continue;
                }
                if kind == "framework"
                    && !self.target.to_ascii_lowercase().contains("darwin")
                    && !self.target.to_ascii_lowercase().contains("macos")
                {
                    self.error("#[link(kind = \"framework\")] is only supported on macOS");
                    continue;
                }
                let rename = attr
                    .named
                    .iter()
                    .find(|(key, _)| key == "rename")
                    .map(|(_, value)| value.clone());
                let version = attr
                    .named
                    .iter()
                    .find(|(key, _)| key == "version")
                    .map(|(_, value)| value.clone());
                let optional = attr
                    .named
                    .iter()
                    .find(|(key, _)| key == "optional")
                    .map(|(_, value)| value.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                let link = ExternLink {
                    name,
                    kind,
                    rename,
                    version,
                    optional,
                };
                if !self.extern_links.contains(&link) {
                    self.extern_links.push(link);
                }
            }
        }

        for func in node.get_functions() {
            let func_name = func.get_name().to_string();

            // Validate that the function is declared in the specified header.
            // We do a lightweight text search for the function name appearing
            // as a function declaration (preceded by a non-identifier char to
            // avoid matching substrings of other names).
            if !Self::header_declares_function(&header_content, &func_name) {
                self.error(&format!(
                    "Function '{}' is not declared in header '{}'",
                    func_name, header_path
                ));
            }

            let return_type = self.get_data_type_from_ast(func.get_return_type());
            // Register extern "C" functions in the current module so that
            // call sites (e.g. `printf(...)`) resolve to `<module>::printf`.
            self.env
                .declare_function(&func_name, &return_type, &self.current_module);
            func.accept(self);
        }
    }

    fn visit_import_statement(&mut self, node: &ImportStatement) {
        let module_name = node.get_module_name();
        #[cfg(debug_assertions)]
        println!(
            "  Import module: {} (alias: {:?})",
            module_name,
            node.get_alias()
        );

        self.load_module(&module_name);
        self.note_import(&module_name);
        if let Some(alias) = node.get_alias() {
            self.module_aliases
                .insert(alias.to_string(), module_name.clone());
            // Also make the alias usable as a bare qualifier.
            self.ensure_visible(&self.current_module.clone());
            self.visible_modules
                .get_mut(&self.current_module)
                .map(|s| s.insert(alias.to_string()));
        }
    }

    fn visit_from_import_statement(&mut self, node: &FromImportStatement) {
        let module_name = node.get_module();
        #[cfg(debug_assertions)]
        println!(
            "  From-import: module={}, wildcard={}, members={:?}",
            module_name,
            node.is_wildcard(),
            node.get_members()
        );

        // First, ensure the module is loaded (this registers all its
        // public symbols under `module::member` keys).
        self.load_module(module_name);
        self.note_import(module_name);

        // Handle wildcard import: `from module import *`
        if node.is_wildcard() {
            let module_symbols = self.env.get_module_symbols(module_name);
            for (name, sym) in module_symbols {
                let return_type = sym.data_type.clone();
                self.env
                    .declare_function(&name, &return_type, &self.current_module);
            }
            return;
        }

        // Then, re-declare each requested member as a bare name in the
        // current module so it can be called without the `module::` qualifier.
        for (member_name, alias) in node.get_members() {
            let qualified = format!("{}::{}", module_name, member_name);
            if let Some(sym) = self.env.lookup_symbol(&qualified) {
                let return_type = sym.data_type.clone();
                // Use alias if provided, otherwise use original name
                let effective_name = alias.as_ref().unwrap_or(member_name);
                self.env
                    .declare_function(effective_name, &return_type, &self.current_module);
            } else {
                self.error(&format!(
                    "Cannot import '{}' from module '{}': member not found",
                    member_name, module_name
                ));
            }
        }
    }

    fn visit_function(&mut self, node: &Function) {
        if self.build_mode == BuildMode::Release
            && Attribute::has_attr(node.get_attributes(), "debug")
        {
            return;
        }
        let func_name = node.get_name().to_string();
        #[cfg(debug_assertions)]
        println!("  Function: {}", func_name);

        // Save impl context for self parameter handling
        let prev_impl = self.current_impl_struct.clone();
        // If this is a top-level function with `self: StructType` parameter,
        // set current_impl_struct so that field access (_data, _cap, _len) works.
        if self.current_impl_struct.is_none() {
            if let Some(params) = node.get_parameters() {
                if let Some(first_param) = params.first() {
                    if first_param.get_name() == "self" {
                        if let Some(tp) = first_param.get_type() {
                            let type_name = tp.get_name();
                            if self.struct_fields.contains_key(type_name) {
                                self.current_impl_struct = Some(type_name.to_string());
                            }
                        }
                    }
                }
            }
        }
        // Set generic params BEFORE type resolution
        // Merge function-level generics with existing (e.g., impl-block-level) generics
        let prev_generic_params = self.current_generic_params.clone();
        let mut combined = prev_generic_params.clone();
        for g in node.get_generic_params() {
            if !combined.contains(g) {
                combined.push(g.clone());
            }
        }
        self.current_generic_params = combined;

        let return_type = self.get_data_type_from_ast(node.get_return_type());

        if !self
            .env
            .declare_function(&func_name, &return_type, &self.current_module)
        {
            self.error(&format!(
                "Failed to declare function '{}.{}'",
                self.current_module, func_name
            ));
            self.current_impl_struct = prev_impl;
            self.current_generic_params = prev_generic_params;
            return;
        }

        // Also register with struct name prefix for Type.method() calls
        if let Some(ref struct_name) = self.current_impl_struct {
            self.env
                .declare_function(&func_name, &return_type, struct_name);
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
        let mut has_tail_expr = false;
        if let Some(body) = node.get_body() {
            body.accept(self);
            // Check if last statement is a tail expression (implicit return)
            let stmts = body.get_statements();
            if let Some(last) = stmts.last() {
                if let Some(es) = last.as_any().downcast_ref::<ExpressionStatement>() {
                    if es.tail
                        || es.get_expression().map_or(false, |expr| {
                            expr.as_any().downcast_ref::<MatchExpression>().is_some()
                        })
                    {
                        has_tail_expr = true;
                    }
                }
                // Also treat if/match/block expressions as implicit returns
                if last.as_any().downcast_ref::<IfStatement>().is_some()
                    || last.as_any().downcast_ref::<MatchExpression>().is_some()
                {
                    has_tail_expr = true;
                }
            }
        }

        if !Attribute::has_attr(node.get_attributes(), "intrinsic") {
            if return_type != DataType::None_ && !self.has_return_statement && !has_tail_expr {
                self.error(&format!(
                    "Function '{}' must return a value of type {}",
                    self.current_function,
                    data_type_to_string(return_type)
                ));
            }
        }

        self.env.exit_scope();

        self.current_function = prev_function;
        self.current_function_return_type = prev_return_type;
        self.has_return_statement = prev_has_return;
        self.current_generic_params = prev_generic_params;
    }

    fn visit_lambda(&mut self, node: &Lambda) {
        // Lambda is analyzed in the context of a function call (inline).
        // Set up generic params, a new scope for parameters, and analyze the body.
        let prev_generic_params = self.current_generic_params.clone();
        let mut combined = prev_generic_params.clone();
        for g in node.get_generic_params() {
            if !combined.contains(g) {
                combined.push(g.clone());
            }
        }
        self.current_generic_params = combined;

        let return_type = self.get_data_type_from_ast(node.get_return_type());

        let prev_function = self.current_function.clone();
        let prev_return_type = self.current_function_return_type.clone();
        let prev_has_return = self.has_return_statement;

        self.current_function = "__lambda".to_string();
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
        node.get_body().accept(self);

        self.env.exit_scope();

        self.current_function = prev_function;
        self.current_function_return_type = prev_return_type;
        self.has_return_statement = prev_has_return;
        self.current_generic_params = prev_generic_params;

        // The lambda expression's type is its return type (used when the
        // lambda is immediately invoked).
        self.type_stack.push(return_type);
    }

    fn visit_parameter(&mut self, node: &Parameter) {
        let param_name = node.get_name();
        let param_type = self.get_data_type_from_ast(node.get_type());

        // Array parameters are always mutable (reference type)
        let is_array = node.get_type().map_or(false, |t| {
            t.as_type_any().downcast_ref::<ArrayType>().is_some()
        });
        let is_reference = matches!(param_type, DataType::Reference(_));
        // Keep the complete array type in the symbol table. Indexing unwraps
        // it at the use site; storing only the element type loses information
        // when the array is passed into a struct field or another function.
        let stored_type = param_type.clone();
        self.env
            .declare_variable(param_name, &stored_type, is_array || is_reference);
        // Mark as array if the parameter type is an array
        if is_array {
            if let Some(sym) = self.env.lookup_symbol_mut(param_name) {
                sym.is_array = true;
            }
        }
        #[cfg(debug_assertions)]
        println!(
            "    Parameter: {} : {}",
            param_name,
            data_type_to_string(param_type)
        );
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
        self.current_error_line = node.source_line();
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
                            if let Some(num) = (size as &dyn AstNode)
                                .as_any()
                                .downcast_ref::<NumberLiteral>()
                            {
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
                    self.env
                        .declare_array_constant(&var_name, &elem_type, &constant_sizes, is_mut);
                } else {
                    // For non-constant arrays
                    self.env.declare_variable(&var_name, &elem_type, is_mut);
                }

                return;
            }
        }

        // Normal variable
        let declared_type = self.get_data_type_from_ast(node.get_type());
        let actual_type = if declared_type == DataType::None_ {
            // Infer type from initializer
            if let Some(init) = node.get_initializer() {
                init.accept(self);
                let init_type = self.get_current_type();
                self.env.declare_variable(&var_name, &init_type, is_mut);
                return;
            } else {
                self.env.declare_variable(&var_name, &declared_type, is_mut);
                return;
            }
        } else {
            self.env.declare_variable(&var_name, &declared_type, is_mut);
            declared_type
        };

        if let Some(init) = node.get_initializer() {
            init.accept(self);
            let init_type = self.get_current_type();
            self.check_type_compatibility(
                actual_type,
                init_type,
                &format!("variable '{}' initialization", var_name),
            );
        }
    }

    fn visit_if_statement(&mut self, node: &IfStatement) {
        self.current_error_line = node.source_line();
        #[cfg(debug_assertions)]
        println!("    IfStatement");

        if let Some(cond) = node.get_condition() {
            cond.accept(self);
            let cond_type = self.get_current_type();
            self.type_stack.pop(); // Pop condition type after use

            if cond_type != DataType::Bool
                && cond_type != DataType::Unknown
                && !Environment::is_numeric_type(&cond_type)
            {
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
        self.current_error_line = node.source_line();
        #[cfg(debug_assertions)]
        println!("    WhileStatement");

        if let Some(cond) = node.get_condition() {
            cond.accept(self);
            let cond_type = self.get_current_type();
            self.type_stack.pop(); // Pop condition type after use

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
        self.current_error_line = node.source_line();
        let loop_vars = node.get_loop_variables().clone();

        self.env.enter_scope();

        // Declare index variable (first) as Int
        self.env
            .declare_variable(&loop_vars[0], &DataType::Int, false);

        if let Some(iter) = node.get_iterable() {
            iter.accept(self);
            let iter_type = self.get_current_type();
            self.type_stack.pop(); // Pop iterable type after use
            // Accept Range, Str, array types, generic type params (like T in T[]),
            // Unknown, and variables that are arrays
            let iter_is_valid = matches!(iter_type, DataType::Int)
                || matches!(&iter_type, DataType::Struct(s) if s == "Range")
                || matches!(iter_type, DataType::Str)
                || matches!(iter_type, DataType::Unknown)
                || matches!(&iter_type, DataType::Struct(s) if self.current_generic_params.contains(&s.to_string()))
                || matches!(iter_type, DataType::Array(_))
                || matches!(&iter_type, DataType::Struct(s) if s == "Vec");
            if !iter_is_valid {
                // Check if the iterable is a variable that is an array
                let is_array_var = if let Some(iter_expr) = node.get_iterable() {
                    if let Some(id) = iter_expr.as_any().downcast_ref::<crate::ast::Identifier>() {
                        self.env
                            .lookup_symbol(id.get_name())
                            .map_or(false, |s| s.is_array)
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !is_array_var {
                    self.error("For loop iterable must be range, string, or array");
                }
            }

            // Declare value variable (second) with appropriate type
            if loop_vars.len() >= 2 {
                let val_type = match &iter_type {
                    DataType::Struct(s) if s == "Range" => DataType::Int,
                    DataType::Str => DataType::Str,
                    DataType::Array(elem) => (**elem).clone(),
                    _ => DataType::Int, // default for arrays
                };
                self.env.declare_variable(&loop_vars[1], &val_type, false);
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
        self.current_error_line = node.source_line();
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
        self.current_error_line = node.source_line();
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

        // Look up: bare name first (local vars/params shadow module-level),
        // then module-prefixed name (for module-level functions).
        // This ensures local parameters like `index: int` are found before
        // module-level methods like `vec::index` (Vec::index).
        let sym = self.env.lookup_symbol(name).or_else(|| {
            let full_name = format!("{}::{}", self.current_module, name);
            self.env.lookup_symbol(&full_name)
        });

        match sym {
            Some(s) => {
                let data_type = match &s.data_type {
                    DataType::Reference(inner) => (**inner).clone(),
                    other => other.clone(),
                };
                self.type_stack.push(data_type);
            }
            None => {
                self.error(&format!("Undeclared identifier: '{}'", name));
                self.type_stack.push(DataType::Unknown);
            }
        }
    }

    fn visit_path_access(&mut self, node: &PathAccess) {
        let full_name = node.get_full_name();
        let member = node.get_member().to_string();
        let path_prefix = node.get_path().join("::");
        let mut candidates = vec![full_name.clone()];

        if !path_prefix.is_empty() {
            let stripped_prefix = path_prefix
                .split("::")
                .map(|part| part.split('<').next().unwrap_or(part))
                .collect::<Vec<_>>()
                .join("::");
            candidates.push(format!("{}::{}", stripped_prefix, member));
        }

        if !node.get_path().is_empty() {
            let enum_name = node
                .get_path()
                .last()
                .map(|s| s.split('<').next().unwrap_or(s).to_string())
                .unwrap_or_default();
            if !enum_name.is_empty() {
                candidates.push(format!("{}::{}", enum_name, member));
            }
        }

        for candidate in candidates {
            if let Some(symbol) = self.env.lookup_symbol(&candidate) {
                self.type_stack.push(symbol.data_type.clone());
                return;
            }
            if let Some((variant_names, _)) = self.enum_info.get(&candidate) {
                if variant_names.iter().any(|name| name == &member) {
                    self.type_stack.push(DataType::Struct(candidate.clone()));
                    return;
                }
            }
        }

        // A bare enum variant like `Option::None` or `Color::Red` is a value,
        // not a function call, so resolve it via the enum metadata even when
        // the zero-arg form is written without parentheses.
        if let Some((variant_names, _)) = node
            .get_path()
            .last()
            .and_then(|path| self.enum_info.get(path.split('<').next().unwrap_or(path)))
        {
            if variant_names.iter().any(|name| name == &member) {
                let enum_name = node
                    .get_path()
                    .last()
                    .map(|s| s.split('<').next().unwrap_or(s).to_string())
                    .unwrap_or_default();
                if !enum_name.is_empty() {
                    self.type_stack.push(DataType::Struct(enum_name));
                    return;
                }
            }
        }

        if let Some(sym) = self.env.lookup_symbol(&member) {
            self.type_stack.push(sym.data_type.clone());
            return;
        }

        self.error(&format!("Undeclared path: '{}'", full_name));
        self.type_stack.push(DataType::Unknown);
    }

    fn visit_number_literal(&mut self, node: &NumberLiteral) {
        // Source-level form decides int vs float (e.g. `3.0` is float, `3` is int).
        if node.is_float_literal() {
            self.type_stack.push(DataType::Float);
        } else {
            self.type_stack.push(DataType::Int);
        }
    }

    fn visit_string_literal(&mut self, _node: &StringLiteral) {
        self.type_stack.push(DataType::Str);
    }

    fn visit_null_literal(&mut self, _node: &NullLiteral) {
        self.type_stack.push(DataType::None_);
    }

    fn visit_array_literal(&mut self, node: &ArrayLiteral) {
        let mut first = true;
        let mut elem_type = DataType::Unknown;
        let contains_nested_array = node
            .get_elements()
            .iter()
            .any(|elem| elem.as_any().downcast_ref::<ArrayLiteral>().is_some());
        for elem in node.get_elements() {
            elem.accept(self);
            let et = self.get_current_type();
            self.type_stack.pop();
            if first {
                elem_type = et;
                first = false;
            } else if !contains_nested_array
                && !(matches!(
                    (&elem_type, &et),
                    (DataType::Str, DataType::Bool) | (DataType::Bool, DataType::Str)
                ))
                && !Environment::is_type_compatible(&elem_type, &et)
            {
                self.error(&format!(
                    "Mixed types in array literal: {} and {}",
                    data_type_to_string(elem_type.clone()),
                    data_type_to_string(et)
                ));
            }
        }
        if contains_nested_array {
            elem_type = DataType::Unknown;
        }
        self.type_stack.push(DataType::Array(Box::new(elem_type)));
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
                            if sym.is_mut || matches!(sym.data_type, DataType::Reference(_)) {
                                is_assignable = true;
                            } else {
                                self.error(&format!(
                                    "Cannot assign to constant variable '{}'",
                                    name
                                ));
                            }
                        }
                    }
                } else if let Some(unary) = left.as_any().downcast_ref::<UnaryExpression>() {
                    if unary.get_operator() == "*" {
                        is_assignable = matches!(left_type, DataType::Unknown)
                            || unary.get_operand().map_or(false, |operand| {
                                operand.accept(self);
                                matches!(self.type_stack.pop(), Some(DataType::Pointer(_)))
                            });
                        if !is_assignable {
                            self.error("Cannot assign through a non-pointer value");
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

                    // Check if the base is a struct field (e.g., _data in impl block)
                    let is_struct_field_array =
                        if let Some(arr_id) = array.as_any().downcast_ref::<Identifier>() {
                            self.current_impl_struct.as_ref().map_or(false, |s| {
                                self.struct_fields
                                    .get(s)
                                    .map_or(false, |f| f.contains_key(arr_id.get_name()))
                            })
                        } else {
                            false
                        };

                    if is_struct_field_array {
                        is_assignable = true;
                    } else if let Some(arr_id) = array.as_any().downcast_ref::<Identifier>() {
                        if let Some(sym) = self.env.lookup_symbol(arr_id.get_name()) {
                            if !sym.is_mut {
                                self.error(&format!(
                                    "Cannot assign to constant array '{}'",
                                    arr_id.get_name()
                                ));
                            } else if !sym.is_array {
                                self.error(&format!(
                                    "Cannot index non-array variable '{}'",
                                    arr_id.get_name()
                                ));
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

            // `__new_array` is a compiler intrinsic produced by the parser for
            // `new T[size]?` / `new T[size]` array allocations. The element
            // type is determined at codegen from the allocation site, so the
            // intrinsic's static type is `Unknown`. Accept the assignment
            // against any array / nullable-array LHS without flagging a type
            // mismatch (the actual element type is enforced by the surrounding
            // `var x: T[] = new T[n]` annotation, which the ArrayType path
            // already type-checks).
            let rhs_is_new_array = node.get_right().map_or(false, |r| {
                r.as_any()
                    .downcast_ref::<Identifier>()
                    .map_or(false, |id| id.get_name() == "__new_array")
                    || r.as_any()
                        .downcast_ref::<FunctionCall>()
                        .and_then(|call| call.get_callee())
                        .and_then(|callee| callee.as_any().downcast_ref::<Identifier>())
                        .map_or(false, |id| id.get_name() == "__new_array")
            });
            if rhs_is_new_array {
                self.type_stack.push(left_type);
                return;
            }

            // `null` assigned to a generic element slot (e.g. `_data[i] = null`
            // inside `Vec<T>::pop`). The slot type is the generic `T`, which
            // may or may not be nullable — at this point we cannot know. Treat
            // `null` as compatible with a generic slot to allow optional
            // post-pop cleanup without forcing every `T` to be nullable.
            let rhs_is_null = node.get_right().map_or(false, |r| {
                r.as_any().downcast_ref::<NullLiteral>().is_some()
            });
            if rhs_is_null && right_type == DataType::None_ {
                let slot_is_generic = match &left_type {
                    DataType::Struct(name) => self.current_generic_params.contains(name),
                    _ => false,
                };
                if slot_is_generic {
                    self.type_stack.push(left_type);
                    return;
                }
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

            // Numeric operands: hardcoded path
            if Environment::is_numeric_type(&left_type) && Environment::is_numeric_type(&right_type)
            {
                if left_type == DataType::Float || right_type == DataType::Float {
                    self.type_stack.push(DataType::Float);
                } else {
                    self.type_stack.push(DataType::Int);
                }
                return;
            }

            // User struct types: desugar operator to trait method call
            if let DataType::Struct(ref struct_name) = left_type {
                if let Some(method_name) = Self::operator_to_method(op) {
                    let full_method = format!("{}::{}", struct_name, method_name);
                    if let Some(sym) = self.env.lookup_symbol(&full_method) {
                        // Process arguments (right operand has already been type-checked)
                        self.type_stack.push(sym.data_type.clone());
                        return;
                    }
                    self.error(&format!(
                        "Type '{}' does not implement operator '{}' (method '{}::{}' not found)",
                        struct_name, op, struct_name, method_name
                    ));
                    self.type_stack.push(DataType::Unknown);
                    return;
                }
            }

            self.error(&format!("Operator '{}' requires numeric operands", op));
            self.type_stack.push(DataType::Unknown);
            return;
        }

        // 处理复合赋值运算符 +=, -=, *=, /=
        if op == "+=" || op == "-=" || op == "*=" || op == "/=" {
            let mut is_assignable = false;
            let mut var_name = String::new();

            if let Some(left) = node.get_left() {
                if let Some(id) = left.as_any().downcast_ref::<Identifier>() {
                    var_name = id.get_name().to_string();
                    // Check if it's a bare struct field first
                    let is_field = self.current_impl_struct.as_ref().map_or(false, |s| {
                        self.struct_fields
                            .get(s)
                            .map_or(false, |f| f.contains_key(&var_name))
                    });
                    if is_field {
                        is_assignable = true;
                    } else if let Some(sym) = self.env.lookup_symbol(&var_name) {
                        if sym.is_mut {
                            is_assignable = true;
                        } else {
                            self.error(&format!(
                                "Cannot assign to constant variable '{}'",
                                var_name
                            ));
                        }
                    } else {
                        self.error(&format!("Undeclared variable '{}'", var_name));
                    }
                } else {
                    self.error("Left side of compound assignment must be a variable");
                    self.type_stack.push(DataType::Unknown);
                    return;
                }
            }

            if !is_assignable {
                if var_name.is_empty() {
                    self.error("Left side of compound assignment must be a mutable variable");
                }
                self.type_stack.push(DataType::Unknown);
                return;
            }

            // 检查类型兼容性
            let base_op = &op[0..1];

            // 字符串拼接
            if base_op == "+" && (left_type == DataType::Str || right_type == DataType::Str) {
                self.type_stack.push(DataType::Str);
                return;
            }

            // 数值运算
            if !Environment::is_numeric_type(&left_type)
                || !Environment::is_numeric_type(&right_type)
            {
                self.error(&format!("Operator '{}' requires numeric operands", op));
                self.type_stack.push(DataType::Unknown);
                return;
            }

            self.type_stack.push(left_type);
            return;
        }

        if op == "==" || op == "!=" || op == "<" || op == ">" || op == "<=" || op == ">=" {
            // Nullable and None_ comparisons: e.g., opt == null, opt != null
            let is_nullable = matches!(left_type, DataType::Nullable(_))
                || matches!(right_type, DataType::Nullable(_));
            let has_none = left_type == DataType::None_ || right_type == DataType::None_;
            if is_nullable || has_none {
                if op == "==" || op == "!=" {
                    self.type_stack.push(DataType::Bool);
                    return;
                }
                self.error(&format!(
                    "Cannot compare {} and {} with '{}'",
                    data_type_to_string(left_type),
                    data_type_to_string(right_type),
                    op
                ));
                self.type_stack.push(DataType::Unknown);
                return;
            }

            // Numeric/bool comparison: hardcoded path
            if Environment::is_numeric_type(&left_type) || left_type == DataType::Bool {
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

            // User struct types: desugar comparison to trait method call
            if let DataType::Struct(ref struct_name) = left_type {
                if let Some(method_name) = Self::operator_to_method(op) {
                    let full_method = format!("{}::{}", struct_name, method_name);
                    if let Some(sym) = self.env.lookup_symbol(&full_method) {
                        self.type_stack.push(sym.data_type.clone());
                        return;
                    }
                    self.error(&format!(
                        "Type '{}' does not implement operator '{}' (method '{}::{}' not found)",
                        struct_name, op, struct_name, method_name
                    ));
                    self.type_stack.push(DataType::Unknown);
                    return;
                }
            }

            self.error(&format!(
                "Cannot compare {} and {}",
                data_type_to_string(left_type),
                data_type_to_string(right_type)
            ));
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

        if op == ".." {
            // Range literal: a..b → Range struct. Ensure Range is declared.
            if !self.struct_fields.contains_key("Range") {
                self.struct_fields
                    .insert("Range".to_string(), HashMap::new());
                self.env.declare_module("Range");
            }
            self.type_stack.push(DataType::Struct("Range".to_string()));
            return;
        }

        self.error(&format!("Unknown operator: {}", op));
        self.type_stack.push(DataType::Unknown);
    }

    fn visit_cast_expression(&mut self, node: &CastExpression) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
        let target_type = self.get_data_type_from_ast(Some(node.get_target_type()));
        self.type_stack.push(target_type);
    }

    fn visit_unary_expression(&mut self, node: &UnaryExpression) {
        let op = node.get_operator();

        // Preserve address-of function references; variable address-taking is
        // represented as a typed reference.
        if op == "&" {
            if let Some(operand) = node.get_operand() {
                if let Some(id) = operand.as_any().downcast_ref::<Identifier>() {
                    if self
                        .env
                        .lookup_symbol(id.get_name())
                        .is_some_and(|symbol| symbol.symbol_type != SymbolType::Variable)
                    {
                        let signature = self
                            .function_signatures
                            .get(id.get_name())
                            .cloned()
                            .or_else(|| {
                                self.function_signatures
                                    .get(&format!("{}::{}", self.current_module, id.get_name()))
                                    .cloned()
                            })
                            .unwrap_or(DataType::Unknown);
                        self.type_stack.push(signature);
                        return;
                    }
                }
                operand.accept(self);
            }
            let operand_type = self.type_stack.pop().unwrap_or(DataType::Unknown);
            self.type_stack
                .push(DataType::Reference(Box::new(operand_type)));
            return;
        }

        if let Some(operand) = node.get_operand() {
            operand.accept(self);
        }
        let operand_type = self.get_current_type();

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
        } else if op == "*" {
            match operand_type {
                DataType::Pointer(inner) => self.type_stack.push(*inner),
                other => {
                    self.error(&format!("Cannot dereference non-pointer type '{}'", other));
                    self.type_stack.push(DataType::Unknown);
                }
            }
        } else {
            self.error(&format!("Unknown unary operator: {}", op));
            self.type_stack.push(DataType::Unknown);
        }
    }

    fn visit_function_call(&mut self, node: &FunctionCall) {
        // Lambda immediate invocation: lambda(params): ret { body }(args)
        if let Some(callee) = node.get_callee() {
            if let Some(lambda) = callee.as_any().downcast_ref::<Lambda>() {
                // Analyze the lambda (pushes its return type onto type_stack)
                lambda.accept(self);
                let return_type = self.type_stack.pop().unwrap_or(DataType::Unknown);

                // Analyze and consume argument types
                if let Some(args) = node.get_arguments() {
                    for arg in args {
                        arg.accept(self);
                        self.type_stack.pop();
                    }
                }

                self.type_stack.push(return_type);
                return;
            }
        }

        let mut func_name = String::new();
        let mut module_name = self.current_module.clone();
        let mut receiver_type = None;

        if let Some(callee) = node.get_callee() {
            if let Some(id) = callee.as_any().downcast_ref::<Identifier>() {
                func_name = id.get_name().to_string();
            } else if let Some(path_access) = callee.as_any().downcast_ref::<PathAccess>() {
                // :: namespace path: std::io::println
                // path = ["std", "io"], member = "println" → module = "std::io", func = "println"
                module_name = path_access.get_path().join("::");
                func_name = path_access.get_member().to_string();
            } else if let Some(member) = callee.as_any().downcast_ref::<MemberAccess>() {
                if let Some(obj) = member.get_object() {
                    if let Some(obj_id) = obj.as_any().downcast_ref::<Identifier>() {
                        let obj_name = obj_id.get_name().to_string();
                        let member_name = member.get_member().to_string();

                        let is_module = self
                            .env
                            .lookup_symbol(&obj_name)
                            .map_or(false, |s| s.symbol_type == SymbolType::Module);
                        if self.struct_fields.contains_key(&obj_name) {
                            // A type name may use `Type.method()` as a
                            // convenience for the canonical `Type::method()`.
                            module_name = obj_name;
                            func_name = member_name;
                        } else if is_module && obj_name != "self" {
                            // Reject dot notation for module access (e.g.,
                            // io.println → io::println).
                            self.error(&format!(
                                "Use '::' for module access: '{0}::{1}' instead of '{0}.{1}'",
                                obj_name, member_name
                            ));
                            func_name = member_name;
                        } else {
                            // Resolve instance methods from the variable's
                            // declared struct type, rather than using the
                            // variable name as a module name.
                            receiver_type = self
                                .env
                                .lookup_symbol(&obj_name)
                                .map(|symbol| symbol.data_type.clone());
                            if obj_name == "self" {
                                if let Some(ref struct_name) = self.current_impl_struct {
                                    module_name = struct_name.clone();
                                } else {
                                    module_name = obj_name;
                                }
                            } else {
                                module_name = self
                                    .env
                                    .lookup_symbol(&obj_name)
                                    .and_then(|s| match &s.data_type {
                                        DataType::Struct(name) => {
                                            Some(name.split('<').next().unwrap_or(name).to_string())
                                        }
                                        _ => None,
                                    })
                                    .unwrap_or(obj_name);
                            }
                            func_name = member_name;
                        }
                    } else {
                        // Chained method call: obj is not a simple Identifier
                        // (e.g. `arr.index_mut(i).write(v)` — the outer call's
                        // obj is a FunctionCall). Visit the obj expression to
                        // discover its DataType, then dispatch via its struct
                        // name so that trait/struct methods like `Ref::write`
                        // are resolved correctly.
                        obj.accept(self);
                        let obj_dt = self.get_current_type();
                        self.type_stack.pop();
                        receiver_type = Some(obj_dt.clone());
                        let member_name = member.get_member().to_string();
                        if let DataType::Struct(struct_name) = obj_dt {
                            module_name = struct_name
                                .split('<')
                                .next()
                                .unwrap_or(&struct_name)
                                .to_string();
                        }
                        func_name = member_name;
                    }
                }
            }
        }

        if func_name == "__new_array" {
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    let size_type = self.get_current_type();
                    self.type_stack.pop();
                    if !matches!(size_type, DataType::Int | DataType::Unknown) {
                        self.error("Array allocation size must be an integer");
                    }
                }
            }
            self.type_stack.push(DataType::Unknown);
            return;
        }

        // Check if it's a struct constructor call (e.g. Point(1, 2) or vec::VecIterator(...))
        // Also check bare identifier that matches a struct name (e.g., VecIterator(...))
        // Strip generic parameters from func_name (e.g., "VecIterator<T>" → "VecIterator")
        let func_name_bare = func_name
            .split('<')
            .next()
            .unwrap_or(&func_name)
            .to_string();
        let constructor_name = func_name_bare
            .split("::")
            .last()
            .unwrap_or(&func_name_bare)
            .to_string();
        let is_struct_ctor = self.struct_fields.contains_key(&func_name_bare)
            || self.struct_fields.contains_key(&constructor_name);
        if is_struct_ctor {
            let resolved_ctor = if self.struct_fields.contains_key(&func_name_bare) {
                func_name_bare.clone()
            } else {
                constructor_name
            };
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            self.type_stack.push(DataType::Struct(resolved_ctor));
            return;
        }

        // Static-method constructor form: `TypeName::new(args...)` — returns
        // `TypeName` as the expression type. This enables automatic type
        // inference for declarations such as `var x = Range::new(0, 10);`
        // without requiring a type annotation.
        // Also handle `new T[n]` where T is a generic type parameter.
        let module_name_bare = module_name
            .split('<')
            .next()
            .unwrap_or(&module_name)
            .to_string();
        let enum_name = module_name_bare
            .rsplit("::")
            .next()
            .unwrap_or(&module_name_bare);
        if let Some((variant_names, payload_types)) = self.enum_info.get(enum_name).cloned() {
            if let Some(variant_index) = variant_names
                .iter()
                .position(|variant| variant == &func_name_bare)
            {
                let mut argument_types = Vec::new();
                if let Some(args) = node.get_arguments() {
                    for arg in args {
                        arg.accept(self);
                        argument_types.push(self.get_current_type());
                        self.type_stack.pop();
                    }
                }

                let expected_types = &payload_types[variant_index];
                if expected_types.len() != argument_types.len() {
                    self.error(&format!(
                        "Enum variant '{}::{}' expects {} payload(s), got {}",
                        enum_name,
                        func_name_bare,
                        expected_types.len(),
                        argument_types.len()
                    ));
                } else {
                    let generic_params = self
                        .enum_generic_params
                        .get(enum_name)
                        .cloned()
                        .unwrap_or_default();
                    let enum_type = DataType::Struct(module_name.clone());
                    for (expected, actual) in expected_types.iter().zip(argument_types) {
                        let expected =
                            resolve_enum_payload_type(&enum_type, expected, &generic_params);
                        if !module_name.contains('<')
                            && matches!(&expected, DataType::Struct(name) if generic_params.contains(name))
                        {
                            continue;
                        }
                        self.check_type_compatibility(expected, actual, "enum variant payload");
                    }
                }
                self.type_stack
                    .push(DataType::Struct(if module_name.contains('<') {
                        module_name.clone()
                    } else {
                        module_name_bare.clone()
                    }));
                return;
            }
        }

        if func_name == "new"
            && (self.struct_fields.contains_key(&module_name_bare)
                || self.current_generic_params.contains(&module_name_bare))
        {
            let mut argument_types = Vec::new();
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    argument_types.push(self.get_current_type());
                    self.type_stack.pop();
                }
            }
            let constructed_type = if module_name.contains('<') && module_name.ends_with('>') {
                module_name.clone()
            } else if module_name_bare == "Vec" {
                argument_types
                    .first()
                    .and_then(|arg| match arg {
                        DataType::Array(element) => {
                            Some(format!("Vec<{}>", data_type_to_string((**element).clone())))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| module_name_bare.clone())
            } else {
                module_name_bare.clone()
            };
            self.type_stack.push(DataType::Struct(constructed_type));
            return;
        }

        // Range constructor: `range::new(a, b)` / `Range::new(a, b)` produces a Range struct.
        // The `..` operator is desugared to `range::new(start, end)` by the parser, and
        // `new Range(a, b)` desugars to `Range::new(a, b)`. Both must be recognised even
        // when std/range.gbl is not loaded.
        if func_name == "new" && (module_name == "range" || module_name == "Range") {
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            if !self.struct_fields.contains_key("Range") {
                self.struct_fields
                    .insert("Range".to_string(), HashMap::new());
                self.env.declare_module("range");
            }
            self.type_stack.push(DataType::Struct("Range".to_string()));
            return;
        }

        // Compile-time macros `file()` and `line()` are valid inside #[expand]
        // functions. They are folded to literals by the IR builder; here we
        // only need to give them a type so semantic analysis passes.
        if (func_name == "file" || func_name == "line") && module_name == self.current_module {
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            self.type_stack.push(if func_name == "file" {
                DataType::Str
            } else {
                DataType::Int
            });
            return;
        }

        // Resolve module aliases (e.g., "import lib.math as m" → "m" maps to "lib.math")
        let resolved_module = self
            .module_aliases
            .get(&module_name)
            .cloned()
            .unwrap_or_else(|| module_name.clone());

        // Strip generic parameter suffixes from the module half of the lookup
        // name so that `Result<U, E>::Ok` resolves the same way `Result::Ok`
        // does. Variant constructors and methods are registered under the
        // bare type name, never the monomorphised form.
        let resolved_module_bare = resolved_module
            .split('<')
            .next()
            .unwrap_or(&resolved_module)
            .to_string();

        // Build lookup name. For method calls (obj.method), resolve via struct type
        let full_name = if resolved_module_bare != self.current_module {
            // Check if resolved_module is a variable (not a module) → method dispatch
            let is_var = self
                .env
                .lookup_symbol(&resolved_module_bare)
                .map_or(false, |s| s.symbol_type != SymbolType::Module);
            if is_var {
                // For struct types, look up method in current module
                // For arrays, the method is handled by the executor
                format!("{}::{}", self.current_module, func_name)
            } else {
                format!("{}::{}", resolved_module_bare, func_name)
            }
        } else {
            format!("{}::{}", resolved_module_bare, func_name)
        };

        // ---- Module-scope isolation ----
        // A bare `mod::fn` qualifier is only valid when `mod` is actually
        // imported/visible at this call site. `import std;` grants `std::`
        // but NOT bare `io::` — to use `io::println` you must `import io;` or
        // `import std::io;` (which registers the `io` short name).
        let is_module_qual = (resolved_module_bare.contains("::")
            || self
                .env
                .lookup_symbol(&resolved_module_bare)
                .map(|s| s.symbol_type == SymbolType::Module)
                .unwrap_or(false))
            // A locally-declared struct/enum name is not an "external module"
            // qualifier at all (e.g. `Color::Red()`), so don't gate it.
            && !self.struct_fields.contains_key(&resolved_module_bare);
        if is_module_qual
            && resolved_module_bare != self.current_module
            && !self.module_visible(&resolved_module_bare)
        {
            self.error(&format!("Undeclared function: '{}'", full_name));
            // Still visit arguments so nested expressions keep their types.
            if let Some(args) = node.get_arguments() {
                for arg in args {
                    arg.accept(self);
                    self.type_stack.pop();
                }
            }
            self.type_stack.push(DataType::Unknown);
            return;
        }

        // Try qualified lookup, then short-name lookup
        let sym_data_type = self
            .env
            .lookup_symbol(&full_name)
            .or_else(|| self.env.lookup_symbol(&func_name))
            .map(|s| s.data_type.clone());

        // Qualified paths can include module prefixes that are not part of
        // the symbol's registered name (for example
        // `net::TcpStream::connect` is registered as `TcpStream::connect`).
        let sym_data_type = sym_data_type.or_else(|| {
            resolved_module_bare
                .split("::")
                .enumerate()
                .skip(1)
                .map(|(index, _)| {
                    let suffix = resolved_module_bare
                        .split("::")
                        .skip(index)
                        .collect::<Vec<_>>()
                        .join("::");
                    format!("{}::{}", suffix, func_name)
                })
                .into_iter()
                .map(|c| self.env.lookup_symbol(&c).map(|s| s.data_type.clone()))
                .find(|o| o.is_some())
                .flatten()
        });

        if sym_data_type.is_none() {
            if let Some(receiver_type) = &receiver_type {
                match receiver_type {
                    DataType::Array(_) if func_name == "len" => {
                        if let Some(args) = node.get_arguments() {
                            for arg in args {
                                arg.accept(self);
                                self.type_stack.pop();
                            }
                        }
                        self.type_stack.push(DataType::Int);
                        return;
                    }
                    DataType::Array(_) if func_name == "add" => {
                        if let Some(args) = node.get_arguments() {
                            for arg in args {
                                arg.accept(self);
                                self.type_stack.pop();
                            }
                        }
                        self.type_stack.push(DataType::None_);
                        return;
                    }
                    DataType::Str => {
                        let result_type = match func_name.as_str() {
                            "len" | "index_of" | "count" => Some(DataType::Int),
                            "contains" | "starts_with" | "ends_with" => Some(DataType::Bool),
                            "trim" | "replace" | "to_upper" | "to_lower" => Some(DataType::Str),
                            _ => None,
                        };
                        if let Some(result_type) = result_type {
                            if let Some(args) = node.get_arguments() {
                                for arg in args {
                                    arg.accept(self);
                                    self.type_stack.pop();
                                }
                            }
                            self.type_stack.push(result_type);
                            return;
                        }
                    }
                    _ => {}
                }
            }
        }

        // If not found but it's a method call on a variable (e.g. arr.len, arr.add),
        // allow it with sensible return types
        if sym_data_type.is_none() && module_name != self.current_module {
            if let Some(var_sym) = self.env.lookup_symbol(&module_name) {
                if var_sym.is_array {
                    // Array methods: len() -> Int, add() -> None_
                    if func_name == "len" {
                        // Process explicit arguments (none expected)
                        if let Some(args) = node.get_arguments() {
                            for arg in args {
                                arg.accept(self);
                                self.type_stack.pop();
                            }
                        }
                        self.type_stack.push(DataType::Int);
                        return;
                    }
                    if func_name == "add" {
                        if let Some(args) = node.get_arguments() {
                            for arg in args {
                                arg.accept(self);
                                self.type_stack.pop();
                            }
                        }
                        self.type_stack.push(DataType::None_);
                        return;
                    }
                }
                // String methods: primitive operations are lowered directly
                // by Cranelift and therefore do not require a user symbol.
                if matches!(var_sym.data_type, DataType::Str) {
                    let ret = match func_name.as_str() {
                        "len" => Some(DataType::Int),
                        "contains" => Some(DataType::Bool),
                        "trim" => Some(DataType::Str),
                        "replace" => Some(DataType::Str),
                        "starts_with" => Some(DataType::Bool),
                        "ends_with" => Some(DataType::Bool),
                        "index_of" => Some(DataType::Int),
                        "count" => Some(DataType::Int),
                        "to_upper" | "to_lower" => Some(DataType::Str),
                        _ => None,
                    };
                    if let Some(rt) = ret {
                        if let Some(args) = node.get_arguments() {
                            for arg in args {
                                arg.accept(self);
                                self.type_stack.pop();
                            }
                        }
                        self.type_stack.push(rt);
                        return;
                    }
                }
            }
        }

        // Struct method dispatch: if the callee is obj.method() and obj is
        // a variable of type DataType::Struct(S), look up S::method in the
        // symbol table. This enables method calls on imported struct types
        // (e.g. Result::take, Channel::send) without requiring the method
        // to be in the current module namespace.
        if sym_data_type.is_none() {
            // Special case: if module_name is "self", use current_impl_struct directly
            let dispatch_struct = receiver_type
                .as_ref()
                .and_then(|ty| match ty {
                    DataType::Struct(name) => {
                        Some(name.split('<').next().unwrap_or(name).to_string())
                    }
                    _ => None,
                })
                .or_else(|| {
                    if module_name == "self" {
                        self.current_impl_struct.clone()
                    } else {
                        self.env.lookup_symbol(&module_name).and_then(|s| {
                            if let DataType::Struct(ref sn) = s.data_type {
                                Some(sn.split('<').next().unwrap_or(sn).to_string())
                            } else {
                                None
                            }
                        })
                    }
                });
            if let Some(struct_name) = dispatch_struct {
                let struct_method = format!("{}::{}", struct_name, func_name);
                let method_type = self
                    .env
                    .lookup_symbol(&struct_method)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = method_type {
                    if let Some(args) = node.get_arguments() {
                        for arg in args {
                            arg.accept(self);
                            self.type_stack.pop();
                        }
                    }
                    let resolved = receiver_type
                        .as_ref()
                        .map(|receiver| self.resolve_generic_method_type(receiver, &dt))
                        .unwrap_or(dt);
                    self.type_stack.push(resolved);
                    return;
                }
                // Also try without struct prefix (bare method name) for generic impls
                let bare_method_type = self
                    .env
                    .lookup_symbol(&func_name)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = bare_method_type {
                    if let Some(args) = node.get_arguments() {
                        for arg in args {
                            arg.accept(self);
                            self.type_stack.pop();
                        }
                    }
                    let resolved = receiver_type
                        .as_ref()
                        .map(|receiver| self.resolve_generic_method_type(receiver, &dt))
                        .unwrap_or(dt);
                    self.type_stack.push(resolved);
                    return;
                }
                // Try looking up in current module with struct name prefix (e.g., vec::push)
                let current_module_method = format!("{}::{}", self.current_module, func_name);
                let current_method_type = self
                    .env
                    .lookup_symbol(&current_module_method)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = current_method_type {
                    if let Some(args) = node.get_arguments() {
                        for arg in args {
                            arg.accept(self);
                            self.type_stack.pop();
                        }
                    }
                    let resolved = receiver_type
                        .as_ref()
                        .map(|receiver| self.resolve_generic_method_type(receiver, &dt))
                        .unwrap_or(dt);
                    self.type_stack.push(resolved);
                    return;
                }
                // Also try with bare struct name (e.g., "Vec" instead of "Vec<T>")
                let bare_struct_method = format!(
                    "{}::{}",
                    struct_name.split('<').next().unwrap_or(&struct_name),
                    func_name
                );
                let bare_struct_method_type = self
                    .env
                    .lookup_symbol(&bare_struct_method)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = bare_struct_method_type {
                    if let Some(args) = node.get_arguments() {
                        for arg in args {
                            arg.accept(self);
                            self.type_stack.pop();
                        }
                    }
                    let resolved = receiver_type
                        .as_ref()
                        .map(|receiver| self.resolve_generic_method_type(receiver, &dt))
                        .unwrap_or(dt);
                    self.type_stack.push(resolved);
                    return;
                }
            }
        }

        match sym_data_type {
            Some(dt) => {
                // Process arguments (collect their types for generic resolution).
                let mut arg_types: Vec<DataType> = Vec::new();
                if let Some(args) = node.get_arguments() {
                    for arg in args {
                        arg.accept(self);
                        arg_types.push(self.get_current_type());
                        self.type_stack.pop();
                    }
                }
                if let DataType::Function {
                    parameters,
                    return_type,
                } = &dt
                {
                    if parameters.len() != arg_types.len() {
                        self.error(&format!(
                            "Function value expects {} argument(s), got {}",
                            parameters.len(),
                            arg_types.len()
                        ));
                    }
                    for (expected, actual) in parameters.iter().zip(&arg_types) {
                        self.check_type_compatibility(
                            expected.clone(),
                            actual.clone(),
                            "function argument",
                        );
                    }
                    self.type_stack.push(return_type.as_ref().clone());
                    return;
                }
                // Resolve the concrete return type for top-level generic calls
                // (e.g. `identity(3.5): float`) by unifying argument types with
                // the generic signature's parameter types.
                let resolved = self
                    .resolve_generic_call_type(&full_name, &func_name, &arg_types)
                    .unwrap_or(dt.clone());
                let resolved = receiver_type
                    .as_ref()
                    .map(|receiver| self.resolve_generic_method_type(receiver, &resolved))
                    .unwrap_or(resolved);
                self.type_stack.push(resolved);
            }
            None => {
                // Intrinsic network methods are declared in the standard
                // module but may be reached through an instance or static
                // type path. Preserve their source-level result types even
                // when the imported method symbol is not present.
                let intrinsic_type = match func_name.as_str() {
                    "connect" | "bind" | "accept" | "set_keepalive" | "set_read_timeout"
                    | "set_write_timeout" | "close" | "local_addr" | "remote_addr" => {
                        Some(DataType::Struct("Result".to_string()))
                    }
                    "is_alive" => Some(DataType::Bool),
                    _ => None,
                };
                if let Some(dt) = intrinsic_type {
                    self.type_stack.push(dt);
                    return;
                }
                self.error(&format!("Undeclared function: '{}'", full_name));
                self.type_stack.push(DataType::Unknown);
            }
        }
    }

    fn visit_match_expression(&mut self, node: &MatchExpression) {
        if let Some(scrut) = node.get_scrutinee() {
            scrut.accept(self);
            let scrut_type = self.get_current_type();
            self.type_stack.pop();

            let mut result_type: Option<DataType> = None;
            // let mut arm_index = 0;

            for arm in node.get_arms() {
                self.env.enter_scope();

                // 处理枚举变体模式
                if let MatchPattern::EnumVariant {
                    enum_name,
                    variant_name,
                    payloads,
                    ..
                } = &arm.pattern
                {
                    let resolved_enum_name = if enum_name.is_empty() {
                        match &scrut_type {
                            DataType::Struct(name) => {
                                name.split('<').next().unwrap_or(name).to_string()
                            }
                            _ => {
                                self.error("Pattern must match a enum type");
                                String::new()
                            }
                        }
                    } else {
                        enum_name.split('<').next().unwrap_or(enum_name).to_string()
                    };

                    if let DataType::Struct(scrutinee_name) = &scrut_type {
                        let scrutinee_enum =
                            scrutinee_name.split('<').next().unwrap_or(scrutinee_name);
                        if !resolved_enum_name.is_empty() && resolved_enum_name != scrutinee_enum {
                            self.error(&format!(
                                "Enum pattern '{}' cannot match '{}'",
                                resolved_enum_name, scrutinee_enum
                            ));
                        }
                    }

                    if let Some((variant_names, payload_types)) =
                        self.enum_info.get(&resolved_enum_name).cloned()
                    {
                        let mut idx = None;
                        for (i, vname) in variant_names.iter().enumerate() {
                            if vname == variant_name {
                                idx = Some(i);
                                break;
                            }
                        }

                        let idx = match idx {
                            Some(i) => i,
                            None => {
                                self.error(&format!(
                                    "Variant '{}' not found in enum '{}'",
                                    variant_name, resolved_enum_name
                                ));
                                continue;
                            }
                        };

                        let expected_types = &payload_types[idx];
                        if payloads.len() != expected_types.len() {
                            self.error(&format!(
                                "Variant '{}::{}' has {} payload(s), but the pattern provides {}",
                                resolved_enum_name,
                                variant_name,
                                expected_types.len(),
                                payloads.len()
                            ));
                        } else {
                            let generic_params = self
                                .enum_generic_params
                                .get(&resolved_enum_name)
                                .cloned()
                                .unwrap_or_default();
                            let mut bindings = HashMap::new();
                            for (pattern, expected_type) in payloads.iter().zip(expected_types) {
                                match pattern {
                                    MatchPattern::Variable(var_name) => {
                                        let binding_type = resolve_enum_payload_type(
                                            &scrut_type,
                                            expected_type,
                                            &generic_params,
                                        );
                                        self.env.declare_variable(
                                            var_name,
                                            &binding_type,
                                            false,
                                        );
                                        bindings.insert(var_name.clone(), binding_type);
                                    }
                                    MatchPattern::Wildcard => {}
                                    _ => self.error(
                                        "Only variable and wildcard enum payload patterns are supported",
                                    ),
                                }
                            }
                            self.match_bindings.push(bindings);
                        }
                    } else if !resolved_enum_name.is_empty() {
                        self.error(&format!(
                            "Unknown enum type '{}' in match pattern",
                            resolved_enum_name
                        ));
                    }
                }

                // 处理变量模式
                if let MatchPattern::Variable(var_name) = &arm.pattern {
                    self.env.declare_variable(var_name, &scrut_type, false);
                }

                // 检查 body
                if let Some(ref body) = arm.body {
                    let arm_returns =
                        body.as_any()
                            .downcast_ref::<Block>()
                            .map_or(false, |block| {
                                block.get_statements().iter().any(|stmt| {
                                    stmt.as_any().downcast_ref::<ReturnStatement>().is_some()
                                })
                            });
                    body.accept(self);
                    // A `return expr` arm has no expression type on the
                    // stack, but its type is the enclosing function's return
                    // type and it still contributes to match control flow.
                    let arm_type = if self.type_stack.is_empty() {
                        self.current_function_return_type.clone()
                    } else {
                        let ty = self.get_current_type();
                        self.type_stack.pop();
                        ty
                    };

                    if arm_returns {
                        self.env.exit_scope();
                        continue;
                    }
                    match &result_type {
                        None => result_type = Some(arm_type.clone()),
                        Some(existing) => {
                            if !Environment::is_type_compatible(existing, &arm_type)
                                && !Environment::is_type_compatible(&arm_type, existing)
                                && arm_type != DataType::Unknown
                                && *existing != DataType::Unknown
                            {
                                self.error("Match arms have incompatible types");
                            }
                        }
                    }
                }

                self.env.exit_scope();
                // arm_index += 1;
            }

            if let Some(rt) = result_type {
                self.type_stack.push(rt);
            } else {
                self.type_stack.push(DataType::None_);
            }
        }
    }

    fn visit_try_operator(&mut self, node: &TryOperator) {
        // Visit the inner expression to get its type
        if let Some(inner) = node.get_inner() {
            inner.accept(self);
            let inner_type = self.get_current_type();
            self.type_stack.pop();

            // The ? operator works on Result<T, E>.
            // For now, since Result is a struct, we check if the type is
            // DataType::Struct("Result"). The result type of expr? is T
            // (the inner value type). Since all values are i64 at the
            // Cranelift level, we can safely return Int as the type.
            if let DataType::Struct(ref sname) = inner_type {
                if sname == "Result" {
                    // Result<T, E> ? → T (represented as Int at runtime)
                    self.type_stack.push(DataType::Int);
                    return;
                }
            }

            self.error(&format!(
                "The '?' operator requires Result<T, E>, got {}",
                data_type_to_string(inner_type.clone())
            ));
            self.type_stack.push(DataType::Unknown);
        } else {
            self.type_stack.push(DataType::Unknown);
        }
    }

    fn visit_struct_literal(&mut self, node: &StructLiteral) {
        let type_name = node.get_type_name().to_string();

        // Look up struct definition
        let struct_fields = match self.struct_fields.get(&type_name) {
            Some(fields) => fields.clone(),
            None => {
                self.error(&format!("Unknown struct type: '{}'", type_name));
                self.type_stack.push(DataType::Unknown);
                return;
            }
        };

        let field_names: Vec<String> = struct_fields.keys().cloned().collect();
        let mut named_assigned: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut covered: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut positional_index = 0;

        for field_init in node.get_fields() {
            match field_init {
                StructFieldInit::Named { name, value } => {
                    // Verify field exists
                    if !struct_fields.contains_key(name) {
                        self.error(&format!("Struct '{}' has no field '{}'", type_name, name));
                        self.type_stack.push(DataType::Unknown);
                        return;
                    }
                    // Only error on duplicate NAMED assignments (overriding spread/positional is OK)
                    if named_assigned.contains(name) {
                        self.error(&format!(
                            "Field '{}' assigned multiple times in struct literal",
                            name
                        ));
                        self.type_stack.push(DataType::Unknown);
                        return;
                    }
                    named_assigned.insert(name.clone());
                    covered.insert(name.clone());

                    // Type-check the value
                    value.accept(self);
                    let value_type = self.get_current_type();
                    self.type_stack.pop();

                    let expected_type = struct_fields
                        .get(name)
                        .cloned()
                        .unwrap_or(DataType::Unknown);
                    if expected_type != DataType::Unknown && value_type != DataType::Unknown {
                        self.check_type_compatibility(
                            expected_type,
                            value_type,
                            &format!("struct literal field '{}'", name),
                        );
                    }
                }
                StructFieldInit::Positional(value) => {
                    // Check if it's a spread (identifier of same struct type)
                    let mut is_spread = false;
                    if let Some(id) = value.as_any().downcast_ref::<Identifier>() {
                        let id_name = id.get_name();
                        let full_name = format!("{}::{}", self.current_module, id_name);
                        if let Some(sym) = self
                            .env
                            .lookup_symbol(&full_name)
                            .or_else(|| self.env.lookup_symbol(id_name))
                        {
                            if let DataType::Struct(ref s) = sym.data_type {
                                if s == &type_name {
                                    is_spread = true;
                                }
                            }
                        }
                    }

                    if is_spread {
                        // Spread: all unassigned fields are filled from this struct
                        value.accept(self);
                        let _spread_type = self.get_current_type();
                        self.type_stack.pop();
                        // Mark all fields as covered (but NOT named_assigned, so named inits can override)
                        for fname in &field_names {
                            covered.insert(fname.clone());
                        }
                    } else {
                        // Positional: match to next uncovered field
                        value.accept(self);
                        let value_type = self.get_current_type();
                        self.type_stack.pop();

                        // Skip fields already covered by spread or previous positional
                        while positional_index < field_names.len()
                            && covered.contains(&field_names[positional_index])
                        {
                            positional_index += 1;
                        }

                        if positional_index >= field_names.len() {
                            self.error(&format!(
                                "Too many positional fields in struct literal for '{}'",
                                type_name
                            ));
                            self.type_stack.push(DataType::Unknown);
                            return;
                        }

                        let field_name = &field_names[positional_index];
                        covered.insert(field_name.clone());
                        positional_index += 1;

                        let expected_type = struct_fields
                            .get(field_name)
                            .cloned()
                            .unwrap_or(DataType::Unknown);
                        if expected_type != DataType::Unknown && value_type != DataType::Unknown {
                            self.check_type_compatibility(
                                expected_type,
                                value_type,
                                &format!("struct literal field '{}'", field_name),
                            );
                        }
                    }
                }
            }
        }

        self.type_stack.push(DataType::Struct(type_name));
    }

    fn visit_member_access(&mut self, node: &MemberAccess) {
        if let Some(obj) = node.get_object() {
            obj.accept(self);
        }
        let obj_type = self.get_current_type();
        self.type_stack.pop();

        if node.get_member().parse::<usize>().is_ok() {
            if let DataType::Array(element_type) = obj_type {
                self.type_stack.push(*element_type);
            } else {
                self.type_stack.push(DataType::Unknown);
            }
            return;
        }

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
                            self.error(&format!(
                                "Struct '{}' has no field '{}'",
                                struct_name, member
                            ));
                            self.type_stack.push(DataType::Unknown);
                            return;
                        }
                    }
                    self.error("'self' used outside of impl block");
                    self.type_stack.push(DataType::Unknown);
                    return;
                }

                // Check module-level lookup via dot (deprecated — use ::)
                let full_name = format!("{}::{}", obj_name, member);
                let module_sym_type = self
                    .env
                    .lookup_symbol(&full_name)
                    .map(|s| s.data_type.clone());
                if let Some(dt) = module_sym_type {
                    self.error(&format!(
                        "Use '::' for module access: '{0}::{1}' instead of '{0}.{1}'",
                        obj_name, member
                    ));
                    self.type_stack.push(dt);
                    return;
                }

                // Check struct field access on a variable
                if let DataType::Struct(ref struct_name) = obj_type {
                    if let Some(fields) = self.struct_fields.get(struct_name) {
                        if let Some(field_type) = fields.get(member) {
                            // _-prefixed fields are private to the struct's impl blocks
                            if member.starts_with('_') {
                                let in_own_impl = self.current_impl_struct.as_deref()
                                    == Some(struct_name.as_str());
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
                        self.error(&format!(
                            "Struct '{}' has no field '{}'",
                            struct_name, member
                        ));
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
        // Range literal a..b produces a `Range` struct value, not an Int.
        if !self.struct_fields.contains_key("Range") {
            self.struct_fields
                .insert("Range".to_string(), HashMap::new());
            self.env.declare_module("Range");
        }
        self.type_stack.push(DataType::Struct("Range".to_string()));
    }

    fn visit_grouped_expression(&mut self, node: &GroupedExpression) {
        if let Some(expr) = node.get_expression() {
            expr.accept(self);
        }
    }

    fn visit_basic_type(&mut self, _node: &BasicType) {}
    fn visit_type(&mut self, _node: &dyn Type) {}

    fn visit_pointer_type(&mut self, node: &PointerType) {
        let pointee = node.get_pointee();
        pointee.accept(self);

        let inner_type = self.get_current_type();
        self.type_stack.pop();

        self.type_stack
            .push(DataType::Pointer(Box::new(inner_type)));
    }

    fn visit_reference_type(&mut self, node: &ReferenceType) {
        node.get_referent().accept(self);
        let inner_type = self.type_stack.pop().unwrap_or(DataType::Unknown);
        self.type_stack
            .push(DataType::Reference(Box::new(inner_type)));
    }

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

        if matches!(array_type, DataType::Str) {
            self.type_stack.push(DataType::Str);
            return;
        }

        if let Some(arr) = node.get_array() {
            if let Some(id) = arr.as_any().downcast_ref::<Identifier>() {
                let id_name = id.get_name();
                // Check if it's a struct field (e.g., _data in impl block) — type already on stack
                let is_struct_field = self.current_impl_struct.as_ref().map_or(false, |s| {
                    self.struct_fields
                        .get(s)
                        .map_or(false, |f| f.contains_key(id_name))
                });
                if is_struct_field {
                    // Indexing a struct array field yields its element type,
                    // just like indexing a local array. Keep the array
                    // wrapper only for the field declaration itself.
                    let elem_type = match &array_type {
                        DataType::Array(inner) => (**inner).clone(),
                        DataType::Nullable(inner) => match inner.as_ref() {
                            DataType::Array(element) => (**element).clone(),
                            _ => (**inner).clone(),
                        },
                        _ => array_type.clone(),
                    };
                    self.type_stack.push(elem_type);
                    return;
                }
                // Otherwise look up in environment (local variables)
                if let Some(sym) = self.env.lookup_symbol(id_name) {
                    if !sym.is_array {
                        self.error(&format!("Variable '{}' is not an array", id_name));
                        self.type_stack.push(DataType::Unknown);
                        return;
                    }
                    let element_type = match &sym.data_type {
                        DataType::Array(element) => (**element).clone(),
                        other => other.clone(),
                    };
                    self.type_stack.push(element_type);
                    return;
                }
                self.error(&format!("Array variable '{}' not declared", id_name));
                self.type_stack.push(DataType::Unknown);
                return;
            }
        }

        let element_type = match array_type {
            DataType::Array(element) => *element,
            DataType::Nullable(inner) => match *inner {
                DataType::Array(element) => *element,
                other => other,
            },
            other => other,
        };
        self.type_stack.push(element_type);
    }
}
