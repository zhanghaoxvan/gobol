use super::*;

impl CraneliftBackend {
    // ==================== expressions ====================

    pub(super) fn translate_expr(
        &mut self,
        bcx: &mut FunctionBuilder,
        expr: &IRExpr,
    ) -> Result<ir::Value, String> {
        match expr {
            IRExpr::Literal(lit) => Ok(self.translate_literal(bcx, lit)),
            IRExpr::Variable(name) => {
                // Module-level mutable `var` global: read from shared slot.
                if self.global_vars.contains_key(name) {
                    return Ok(self.translate_global_read(bcx, name));
                }
                if let Some(var) = self.variables.get(name) {
                    if let Some(slot) = self.addressable_slots.get(name).copied() {
                        let ty = self.data_type_to_clif(&self.type_resolver.var_type(name))?;
                        return Ok(bcx.ins().stack_load(types::I64, ty, slot, 0));
                    }
                    let v = bcx.use_var(*var);
                    match self.type_resolver.var_type(name) {
                        DataType::Reference(inner) => {
                            let ty = self.data_type_to_clif(&inner)?;
                            Ok(bcx.ins().load(ty, MemFlagsData::new(), v, 0))
                        }
                        _ => Ok(v),
                    }
                } else {
                    match self.translate_func_ref(bcx, name) {
                        Ok(v) => Ok(v),
                        Err(_) => Ok(bcx.ins().iconst(types::I64, 0)),
                    }
                }
            }
            IRExpr::Binary { op, left, right } => self.translate_binary(bcx, op, left, right),
            IRExpr::Unary { op, operand } => self.translate_unary(bcx, op, operand),
            IRExpr::Call { func, args, .. } => self.translate_call(bcx, func, args),
            IRExpr::MethodCall {
                object,
                method,
                args,
                ..
            } => self.translate_method_call(bcx, object, method, args),
            IRExpr::MemberAccess { object, member } => {
                self.translate_member_access(bcx, object, member)
            }
            IRExpr::ArrayIndex { array, index } => {
                if matches!(self.type_resolver.infer_type(array), DataType::Str) {
                    let string = self.translate_expr(bcx, array)?;
                    let index = self.translate_expr(bcx, index)?;
                    let codepoint = self.call_runtime(bcx, "gobol_str_get", &[string, index]);
                    return Ok(self.call_runtime(bcx, "gobol_str_char", &[codepoint]));
                }
                let element_type = match self.type_resolver.infer_type(array) {
                    DataType::Array(element) => *element,
                    _ => DataType::Unknown,
                };

                // Check if this is a nested array access (e.g., arr[2][2])
                if let IRExpr::ArrayIndex {
                    array: inner_array,
                    index: inner_idx,
                } = array.as_ref()
                {
                    // 2D array access: first get the inner array, then get the element
                    let base = self.translate_expr(bcx, inner_array)?;
                    let i1 = self.translate_expr(bcx, inner_idx)?;
                    let inner_arr = self.call_runtime(bcx, "gobol_array_get", &[base, i1]);
                    let i2 = self.translate_expr(bcx, index)?;
                    let value = self.call_runtime(bcx, "gobol_array_get", &[inner_arr, i2]);
                    Ok(self.coerce_array_element(bcx, value, &element_type))
                } else {
                    let arr = self.translate_expr(bcx, array)?;
                    let idx = self.translate_expr(bcx, index)?;
                    let value = self.call_runtime(bcx, "gobol_array_get", &[arr, idx]);
                    Ok(self.coerce_array_element(bcx, value, &element_type))
                }
            }
            IRExpr::ArrayLiteral(elems) => {
                let arr = self.call_runtime(bcx, "gobol_array_new", &[]);
                for e in elems {
                    let value = self.translate_expr(bcx, e)?;
                    let v = self.to_storage_word(bcx, value, e);
                    self.call_runtime(bcx, "gobol_array_add", &[arr, v]);
                }
                Ok(arr)
            }
            IRExpr::StructLiteral { name, fields } => {
                self.translate_struct_literal(bcx, name, fields)
            }
            IRExpr::Cast { expr, target } => self.translate_cast(bcx, expr, target),
            IRExpr::Bitcast { expr, target } => {
                let value = self.translate_expr(bcx, expr)?;
                let target_type = self.data_type_to_clif(target)?;
                let source_type = bcx.func.dfg.value_type(value);
                if source_type == target_type {
                    Ok(value)
                } else {
                    Ok(bcx.ins().bitcast(target_type, MemFlagsData::new(), value))
                }
            }
            IRExpr::Assignment { target, value } => {
                self.translate_assignment(bcx, target, value)?;
                Ok(self.translate_expr(bcx, target)?)
            }
            IRExpr::FuncRef(name) => self.translate_func_ref(bcx, name),
            IRExpr::IndirectCall { callee, args } => {
                self.translate_indirect_call(bcx, callee, args)
            }
            IRExpr::None => Ok(bcx.ins().iconst(types::I64, 0)),
        }
    }

    pub(super) fn translate_literal(
        &mut self,
        bcx: &mut FunctionBuilder,
        lit: &LitValue,
    ) -> ir::Value {
        match lit {
            LitValue::Int(n) => bcx.ins().iconst(types::I64, *n),
            LitValue::Float(f) => bcx.ins().f64const(*f),
            LitValue::Bool(b) => bcx.ins().iconst(types::I8, *b as i64),
            LitValue::Byte(n) => bcx.ins().iconst(types::I8, *n as i64),
            LitValue::Str(s) => self.intern_string(bcx, s),
            LitValue::None => bcx.ins().iconst(types::I64, 0),
        }
    }

    pub(super) fn translate_binary(
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
        let lty = self.type_resolver.infer_type(left);

        // Short-circuit evaluation for && and ||: the right operand is only
        // evaluated when needed. `&&` skips the right side when the left is
        // false; `||` skips it when the left is true.
        if op == "&&" || op == "||" {
            return self.translate_short_circuit(bcx, op, l, &lty, right);
        }

        let r = self.translate_expr(bcx, right)?;

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
            "&" => bcx.ins().band(l, r),
            "|" => bcx.ins().bor(l, r),
            "^" => bcx.ins().bxor(l, r),
            _ => {
                let _ = format!("unsupported operator: {}", op);
                bcx.ins().iconst(types::I64, 0)
            }
        })
    }

    /// Lower `&&` / `||` with short-circuit semantics using Cranelift control
    /// flow: the right operand is only evaluated when needed.
    ///
    /// Both outcomes (short-circuit and full-eval) converge on `end_b`,
    /// which carries an i8 block-param holding the final boolean result.
    pub(super) fn translate_short_circuit(
        &mut self,
        bcx: &mut FunctionBuilder,
        op: &str,
        l_val: ir::Value,
        lty: &DataType,
        right: &IRExpr,
    ) -> Result<ir::Value, String> {
        let l_b = self.to_bool(bcx, l_val, lty);
        let zero = BlockArg::Value(bcx.ins().iconst(types::I8, 0));
        let one = BlockArg::Value(bcx.ins().iconst(types::I8, 1));

        let rhs_b = bcx.create_block();
        let end_b = bcx.create_block();
        let end_param = bcx.append_block_param(end_b, types::I8);

        // Branch on the left operand:
        //   && : left true  -> evaluate right ; left false -> short-circuit to 0
        //   || : left true  -> short-circuit to 1 ; left false -> evaluate right
        if op == "&&" {
            bcx.ins().brif(l_b, rhs_b, &[], end_b, &[zero]);
        } else {
            bcx.ins().brif(l_b, end_b, &[one], rhs_b, &[]);
        }
        bcx.seal_block(rhs_b);

        // Right-hand side: evaluate and jump to the merge block with its result.
        bcx.switch_to_block(rhs_b);
        let r_val = self.translate_expr(bcx, right)?;
        let r_arg =
            BlockArg::Value(self.to_bool(bcx, r_val, &self.type_resolver.infer_type(right)));
        bcx.ins().jump(end_b, &[r_arg]);

        bcx.seal_block(end_b);
        bcx.switch_to_block(end_b);
        Ok(end_param)
    }

    pub(super) fn binary_float(
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

    pub(super) fn translate_unary(
        &mut self,
        bcx: &mut FunctionBuilder,
        op: &str,
        operand: &IRExpr,
    ) -> Result<ir::Value, String> {
        if op == "&" {
            if let IRExpr::Variable(name) = operand {
                let var =
                    self.variables.get(name).copied().ok_or_else(|| {
                        format!("cannot take address of unknown variable '{}'", name)
                    })?;
                if matches!(self.type_resolver.var_type(name), DataType::Reference(_)) {
                    return Ok(bcx.use_var(var));
                }
                let slot = if let Some(slot) = self.addressable_slots.get(name).copied() {
                    slot
                } else {
                    let slot = bcx.create_sized_stack_slot(StackSlotData::new(
                        StackSlotKind::ExplicitSlot,
                        8,
                        3,
                    ));
                    let initial = bcx.use_var(var);
                    bcx.ins().stack_store(types::I64, initial, slot, 0);
                    self.addressable_slots.insert(name.clone(), slot);
                    slot
                };
                return Ok(bcx.ins().stack_addr(types::I64, slot, 0));
            }
            return Err("address-of currently requires a local variable".to_string());
        }

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
            "*" => {
                let DataType::Pointer(inner) = ty else {
                    return Err(format!("cannot dereference non-pointer type '{}'", ty));
                };
                let load_ty = self.data_type_to_clif(&inner)?;
                return Ok(bcx.ins().load(load_ty, MemFlagsData::new(), v, 0));
            }
            _ => v,
        })
    }

    pub(super) fn translate_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        func: &str,
        args: &[IRExpr],
    ) -> Result<ir::Value, String> {
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

        // Struct static method via :: notation: StructName::method(args)
        // The IR builder prepends `self` for methods (constructors, enum
        // variants, etc.), so we must allocate and prepend self.
        if let Some((owner, method)) = func.rsplit_once("::") {
            let struct_name = owner
                .rsplit("::")
                .next()
                .unwrap_or(owner)
                .split('<')
                .next()
                .unwrap_or(owner);
            if self.type_resolver.has_struct(struct_name) {
                // Check that the method expects self (arity = args + 1)
                let arity = args.len() + 1;
                if let Some(sym) = self
                    .resolve_struct_method_symbol(struct_name, method, arity)
                    .cloned()
                {
                    let size = self.type_resolver.struct_size(struct_name);
                    let size_val = bcx.ins().iconst(types::I64, size);
                    let alloc_fn = if self.alloc_uses_no_gc(struct_name) {
                        "gobol_alloc"
                    } else {
                        "gobol_gc_alloc"
                    };
                    let self_ptr = self.call_runtime(bcx, alloc_fn, &[size_val]);
                    let mut all_args = vec![self_ptr];
                    let mut payload_args = self.translate_args(bcx, args)?;
                    if self.type_resolver.is_enum(struct_name) {
                        for (arg, value) in args.iter().zip(payload_args.iter_mut()) {
                            match self.type_resolver.infer_type(arg) {
                                DataType::Float
                                    if bcx.func.dfg.value_type(*value) == types::F64 =>
                                {
                                    *value =
                                        bcx.ins().bitcast(types::I64, MemFlagsData::new(), *value);
                                }
                                DataType::Bool | DataType::Byte
                                    if bcx.func.dfg.value_type(*value) == types::I8 =>
                                {
                                    *value = bcx.ins().uextend(types::I64, *value);
                                }
                                _ => {}
                            }
                        }
                    }
                    all_args.append(&mut payload_args);
                    let fid = *self
                        .func_ids
                        .get(&sym)
                        .ok_or_else(|| format!("missing function id for '{}'", sym))?;
                    let fref = self.module.declare_func_in_func(fid, &mut bcx.func);
                    let call = bcx.ins().call(fref, &all_args);
                    return Ok(self.call_result(
                        bcx,
                        call,
                        self.type_resolver.func_return_type(func),
                    ));
                }
            }
        }

        let arg_vals = self.translate_args(bcx, args)?;

        // Variadic extern "C" functions: route through a per-arity stub.
        // We check after translate_args so that arg evaluation side effects
        // still happen even if we bail, but the actual call uses the stub.
        if self.is_variadic_func(func) {
            return self.translate_variadic_call(bcx, func, args, &arg_vals);
        }

        self.translate_call_with_args(bcx, func, &arg_vals)
    }

    /// Resolve a function name to its linker symbol, looking up both the
    /// full name and the short name (for cross-module calls).
    pub(super) fn resolve_func_symbol(&self, func: &str, arity: usize) -> Option<String> {
        if let Some(sym) = self.func_symbols.get(&(func.to_string(), arity)) {
            return Some(sym.clone());
        }
        let short = func.rsplit("::").next().unwrap_or(func);
        if let Some(sym) = self.func_symbols.get(&(short.to_string(), arity)) {
            return Some(sym.clone());
        }
        None
    }

    pub(super) fn resolve_struct_method_symbol(
        &self,
        struct_name: &str,
        method: &str,
        arity: usize,
    ) -> Option<&String> {
        let full = format!("{}::{}", struct_name, method);
        self.func_symbols.get(&(full, arity)).or_else(|| {
            let base_name = struct_name.split('<').next().unwrap_or(struct_name);
            self.func_symbols
                .iter()
                .find_map(|((name, candidate_arity), symbol)| {
                    if *candidate_arity != arity {
                        return None;
                    }
                    let (owner, candidate_method) = name.rsplit_once("::")?;
                    (candidate_method == method
                        && owner.split('<').next().unwrap_or(owner) == base_name)
                        .then_some(symbol)
                })
        })
    }

    /// Translate a function reference (`FuncRef(name)`) into the function's
    /// address as an i64 value. Used to pass functions as arguments to
    /// higher-order functions.
    pub(super) fn translate_func_ref(
        &mut self,
        bcx: &mut FunctionBuilder,
        name: &str,
    ) -> Result<ir::Value, String> {
        // Try arity 0..=16 to find a declared symbol for this function name.
        // Function references don't carry arity info, so probe common arities.
        for arity in 0..=32usize {
            if let Some(sym) = self.resolve_func_symbol(name, arity) {
                if let Some(fid) = self.func_ids.get(&sym) {
                    let fref = self.module.declare_func_in_func(*fid, &mut bcx.func);
                    let addr = bcx.ins().func_addr(types::I64, fref);
                    return Ok(addr);
                }
            }
        }
        Err(format!(
            "cannot take address of unknown function '{}'",
            name
        ))
    }

    pub(super) fn coerce_array_element(
        &self,
        bcx: &mut FunctionBuilder,
        value: ir::Value,
        element_type: &DataType,
    ) -> ir::Value {
        match element_type {
            DataType::Float => bcx.ins().bitcast(types::F64, MemFlagsData::new(), value),
            DataType::Bool | DataType::Byte => bcx.ins().ireduce(types::I8, value),
            _ => value,
        }
    }

    /// Translate an indirect call through a function pointer value.
    /// The callee's IR function type supplies the ABI signature.
    pub(super) fn translate_indirect_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        callee: &IRExpr,
        args: &[IRExpr],
    ) -> Result<ir::Value, String> {
        let arg_vals = self.translate_args(bcx, args)?;
        let arity = arg_vals.len();

        // 如果 callee 是 FuncRef，根据实际参数个数解析重载
        if let IRExpr::FuncRef(name) = callee {
            if let Some(sym) = self.resolve_func_symbol(name, arity) {
                if let Some(fid) = self.func_ids.get(&sym) {
                    let fref = self.module.declare_func_in_func(*fid, &mut bcx.func);
                    let call = bcx.ins().call(fref, &arg_vals);
                    let results = bcx.inst_results(call);
                    if results.is_empty() {
                        return Ok(bcx.ins().iconst(types::I64, 0));
                    } else {
                        return Ok(results[0]);
                    }
                }
            }
        }

        let callee_val = self.translate_expr(bcx, callee)?;
        let callee_type = self.type_resolver.infer_type(callee);
        let DataType::Function {
            parameters,
            return_type,
        } = callee_type.clone()
        else {
            return Err(format!(
                "indirect call requires a known function signature, got '{}'",
                callee_type
            ));
        };
        if parameters.len() != arg_vals.len() {
            return Err(format!(
                "indirect call expects {} argument(s), got {}",
                parameters.len(),
                arg_vals.len()
            ));
        }

        let mut sig = self.module.make_signature();
        for parameter in &parameters {
            let ty = self.data_type_to_clif(parameter)?;
            if ty == types::INVALID {
                return Err(format!(
                    "unsupported indirect-call parameter type '{}'",
                    parameter
                ));
            }
            sig.params.push(AbiParam::new(ty));
        }
        let return_type = *return_type;
        if return_type != DataType::None_ {
            let ty = self.data_type_to_clif(&return_type)?;
            if ty == types::INVALID {
                return Err(format!(
                    "unsupported indirect-call return type '{}'",
                    return_type
                ));
            }
            sig.returns.push(AbiParam::new(ty));
        }
        let sig_ref = bcx.import_signature(sig);

        let call = bcx.ins().call_indirect(sig_ref, callee_val, &arg_vals);
        let results = bcx.inst_results(call);
        if results.is_empty() {
            Ok(bcx.ins().iconst(types::I64, 0))
        } else {
            Ok(results[0])
        }
    }

    /// Check whether `func` (or its short name) is a variadic extern "C" function.
    pub(super) fn is_variadic_func(&self, func: &str) -> bool {
        if self.variadic_funcs.contains(func) {
            return true;
        }
        let short = func.rsplit("::").next().unwrap_or(func);
        self.variadic_funcs.contains(short)
    }

    /// Call a variadic extern "C" function through a per-arity C stub.
    ///
    /// Each distinct (function, arity) pair gets a non-variadic import symbol
    /// `__gobol_va_<name>_<arity>` declared in the object file and a matching
    /// C wrapper generated at link time.
    pub(super) fn translate_variadic_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        func: &str,
        args: &[IRExpr],
        arg_vals: &[ir::Value],
    ) -> Result<ir::Value, String> {
        // Resolve the canonical variadic function name (full or short).
        let canon_name = if self.variadic_funcs.contains(func) {
            func.to_string()
        } else {
            func.rsplit("::").next().unwrap_or(func).to_string()
        };

        let arity = arg_vals.len();
        let param_types: Vec<DataType> = args
            .iter()
            .map(|a| self.type_resolver.infer_type(a))
            .collect();
        let return_type = self.type_resolver.func_return_type(&canon_name);

        // Deduplicate: reuse an existing stub for the same (name, arity).
        let stub = VariadicStub {
            func_name: canon_name.clone(),
            arity,
            param_types: param_types.clone(),
            return_type: return_type.clone(),
        };
        let stub_sym = stub.symbol_name();
        let already = self
            .variadic_stubs
            .iter()
            .any(|s| s.func_name == stub.func_name && s.arity == stub.arity);
        if !already {
            self.variadic_stubs.push(stub);
        }

        // Declare the stub import (idempotent — declare_import checks func_ids).
        let mut sig = self.module.make_signature();
        for dt in &param_types {
            sig.params.push(AbiParam::new(self.data_type_to_clif(dt)?));
        }
        if !matches!(return_type, DataType::None_) {
            sig.returns
                .push(AbiParam::new(self.data_type_to_clif(&return_type)?));
        }
        self.declare_import(&stub_sym, sig);

        let fid = self.func_ids[&stub_sym];
        let fref = self.module.declare_func_in_func(fid, &mut bcx.func);
        let call = bcx.ins().call(fref, arg_vals);
        Ok(self.call_result(bcx, call, return_type))
    }

    /// Call a user function with pre-translated Cranelift value arguments.
    pub(super) fn translate_call_with_args(
        &mut self,
        bcx: &mut FunctionBuilder,
        func: &str,
        arg_vals: &[ir::Value],
    ) -> Result<ir::Value, String> {
        // Built-in IO functions — call runtime directly with pre-translated args.
        if let Some(rt) = builtin_runtime(func) {
            // Void-ish runtime IO helpers: call runtime but return dummy value.
            // `read` actually returns a string (i64), so forward that value.
            // For the others, just discard the call result.
            if rt == "gobol_read" {
                return Ok(self.call_runtime(bcx, rt, arg_vals));
            }
            self.call_runtime(bcx, rt, arg_vals);
            return Ok(bcx.ins().iconst(types::I64, 0));
        }
        // panic(msg)
        if func == "panic" {
            return Ok(bcx.ins().iconst(types::I64, 0));
        }

        // Struct intrinsic static methods (e.g., File::open, TcpStream::connect).
        // These have no body — intercept before user function lookup.
        if let Some((owner, method)) = func.rsplit_once("::") {
            let struct_name = owner.rsplit("::").next().unwrap_or(owner);
            if let Some(rt) = self.struct_intrinsic_runtime(struct_name, method) {
                let fret = self.type_resolver.func_return_type(func);
                if struct_name == "TcpStream" || struct_name == "TcpListener" {
                    if let Some(kind) = self.net_intrinsic_kind(method, &fret) {
                        return self.translate_net_intrinsic(bcx, rt, kind, None, arg_vals);
                    }
                }
                if matches!(fret, DataType::None_) {
                    self.call_runtime(bcx, rt, arg_vals);
                    return Ok(bcx.ins().iconst(types::I64, 0));
                }
                return Ok(self.call_runtime(bcx, rt, arg_vals));
            }
        }

        // User function lookup. Try (full name, arity) first, then
        // (short name, arity) for cross-module calls where the function
        // is registered under just its short name.
        let arity = arg_vals.len();
        let lookup_name = {
            if self.func_symbols.contains_key(&(func.to_string(), arity)) {
                func.to_string()
            } else {
                let short = func.rsplit("::").next().unwrap_or(func);
                if self.func_symbols.contains_key(&(short.to_string(), arity)) {
                    short.to_string()
                } else {
                    func.to_string()
                }
            }
        };
        if let Some(sym) = self.func_symbols.get(&(lookup_name.clone(), arity)) {
            if let Some(fid) = self.func_ids.get(sym) {
                let fret = self.type_resolver.func_return_type(&lookup_name);
                let fref = self.module.declare_func_in_func(*fid, &mut bcx.func);
                let call = bcx.ins().call(fref, arg_vals);
                return Ok(self.call_result(bcx, call, fret));
            }
        }
        // Unknown function: evaluate args for side effects, return 0.
        Ok(bcx.ins().iconst(types::I64, 0))
    }

    pub(super) fn translate_method_call(
        &mut self,
        bcx: &mut FunctionBuilder,
        object: &IRExpr,
        method: &str,
        args: &[IRExpr],
    ) -> Result<ir::Value, String> {
        let obj_ty = self.type_resolver.infer_type(object);

        if method == "take" && self.type_resolver.net_result_payload_type(object).is_some() {
            let result = self.translate_expr(bcx, object)?;
            let payload = self.field_addr(bcx, result, 8);
            return Ok(self.call_runtime(bcx, "gobol_mem_load", &[payload]));
        }

        // Struct constructor / static methods: Type::new(...) or Type::method(...)
        // Must be BEFORE the qualified function check so we can allocate self
        if let IRExpr::Variable(name) = object {
            if self.type_resolver.has_struct(name) {
                let full = format!("{}::{}", name, method);
                // For constructors (new), allocate self and pass it as the first arg
                if method == "new" {
                    return self.translate_call(bcx, &full, args);
                }
                return self.translate_call(bcx, &full, args);
            }
        }

        // Qualified function call via . notation (backward compat): module.func(args)
        // e.g., m.add(5, 3) where m is an alias for an imported module
        if let IRExpr::Variable(module_var) = object {
            let full = format!("{}::{}", module_var, method);
            // Module function: arity = args.len() (no implicit self)
            if self.func_symbols.contains_key(&(full.clone(), args.len())) {
                return self.translate_call(bcx, &full, args);
            }
        }

        // Match expressions currently erase the concrete type of an enum
        // payload. Dispatch networking methods before the primitive/string
        // fallbacks so a TcpStream extracted from Result<T, E> still reaches
        // the C runtime.
        if matches!(
            method,
            "local_addr"
                | "remote_addr"
                | "send_str"
                | "send_bytes"
                | "recv_bytes"
                | "recv_exact"
                | "set_read_timeout"
                | "set_write_timeout"
                | "set_keepalive"
        ) {
            let obj_val = self.translate_expr(bcx, object)?;
            let arg_vals = self.translate_args(bcx, args)?;
            let rt = self
                .struct_intrinsic_runtime("TcpStream", method)
                .ok_or_else(|| format!("unknown TcpStream intrinsic '{}'", method))?;
            let payload = match method {
                "local_addr" | "remote_addr" => OkPayload::Str,
                "send_str" | "send_bytes" => OkPayload::Int,
                "recv_bytes" | "recv_exact" => OkPayload::Bytes,
                _ => OkPayload::Unit,
            };
            return self.translate_net_intrinsic(
                bcx,
                rt,
                NetIntrinsicKind::Result_(payload),
                Some(obj_val),
                &arg_vals,
            );
        }

        if method == "add"
            && (self.contains_str(object) || args.iter().any(|arg| self.contains_str(arg)))
        {
            let mut values = Vec::with_capacity(args.len() + 1);
            values.push(self.to_string_value(bcx, object)?);
            for arg in args {
                values.push(self.to_string_value(bcx, arg)?);
            }
            return Ok(self.call_runtime(bcx, "gobol_str_cat", &values));
        }

        // Intrinsic arithmetic methods on primitive types (int, float)
        if let Some(result) = self.try_intrinsic_method(bcx, object, method, args, &obj_ty)? {
            return Ok(result);
        }

        if method == "is_alive" {
            let obj_val = self.translate_expr(bcx, object)?;
            let arg_vals = self.translate_args(bcx, args)?;
            return self.translate_net_intrinsic(
                bcx,
                "gobol_tcp_is_alive",
                NetIntrinsicKind::Plain,
                Some(obj_val),
                &arg_vals,
            );
        }

        // Match-derived handles may not retain their concrete struct type in
        // the IR type resolver.  Still dispatch the unambiguous TcpStream
        // methods here rather than treating them as unknown method calls.
        if matches!(
            method,
            "local_addr"
                | "remote_addr"
                | "send_str"
                | "send_bytes"
                | "recv_bytes"
                | "recv_exact"
                | "set_read_timeout"
                | "set_write_timeout"
                | "set_keepalive"
        ) {
            let obj_val = self.translate_expr(bcx, object)?;
            let arg_vals = self.translate_args(bcx, args)?;
            let rt = self
                .struct_intrinsic_runtime("TcpStream", method)
                .ok_or_else(|| format!("unknown TcpStream intrinsic '{}'", method))?;
            let payload = match method {
                "recv_bytes" | "recv_exact" => OkPayload::Bytes,
                "local_addr" | "remote_addr" => OkPayload::Str,
                "send_str" | "send_bytes" => OkPayload::Int,
                _ => OkPayload::Unit,
            };
            let kind = NetIntrinsicKind::Result_(payload);
            return self.translate_net_intrinsic(bcx, rt, kind, Some(obj_val), &arg_vals);
        }

        // Array methods: arr.add(x), arr.len(), arr.get(i)
        if matches!(obj_ty, DataType::Unknown | DataType::Array(_)) || self.is_array_var(object) {
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
                "index" => {
                    // Index trait: arr.index(i) → gobol_array_get
                    let arr = self.translate_expr(bcx, object)?;
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![arr];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_array_get", &all));
                }
                "index_mut" => {
                    // IndexMut trait: arr.index_mut(i) → returns address as a Ref-like value
                    let arr = self.translate_expr(bcx, object)?;
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![arr];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_array_elem_addr", &all));
                }
                _ => {}
            }
        }

        // String methods: s.len(), s.contains(sub), s.trim(), s.replace(from, to)
        if matches!(obj_ty, DataType::Str) {
            let s = self.translate_expr(bcx, object)?;
            match method {
                "len" => {
                    return Ok(self.call_runtime(bcx, "gobol_str_len", &[s]));
                }
                "contains" => {
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![s];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_str_contains", &all));
                }
                "trim" => {
                    return Ok(self.call_runtime(bcx, "gobol_str_trim", &[s]));
                }
                "replace" => {
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![s];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_str_replace", &all));
                }
                "starts_with" => {
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![s];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_str_starts_with", &all));
                }
                "ends_with" => {
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![s];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_str_ends_with", &all));
                }
                "index_of" => {
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![s];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_str_index_of", &all));
                }
                "count" => {
                    let mut vals = self.translate_args(bcx, args)?;
                    let mut all = vec![s];
                    all.append(&mut vals);
                    return Ok(self.call_runtime(bcx, "gobol_str_count", &all));
                }
                "to_upper" => {
                    return Ok(self.call_runtime(bcx, "gobol_str_to_upper", &[s]));
                }
                "to_lower" => {
                    return Ok(self.call_runtime(bcx, "gobol_str_to_lower", &[s]));
                }
                _ => {}
            }
        }

        // Struct intrinsic methods: File.open/read_all/write/close,
        // TcpStream.connect/send/recv/close, TcpListener.bind/accept.
        // These are dispatched to C runtime before falling through to
        // the regular user-function lookup (which would find the bodyless
        // intrinsic declaration and return 0).
        if let DataType::Struct(sname) = &obj_ty {
            if let Some(rt) = self.struct_intrinsic_runtime(sname, method) {
                let obj_val = self.translate_expr(bcx, object)?;
                let arg_vals = self.translate_args(bcx, args)?;
                let ret_ty = self
                    .type_resolver
                    .func_return_type(&format!("{}::{}", sname, method));
                if sname == "TcpStream" || sname == "TcpListener" {
                    if let Some(kind) = self.net_intrinsic_kind(method, &ret_ty) {
                        return self.translate_net_intrinsic(
                            bcx,
                            rt,
                            kind,
                            Some(obj_val),
                            &arg_vals,
                        );
                    }
                }
                let mut vals = vec![obj_val];
                vals.extend(arg_vals);
                if matches!(ret_ty, DataType::None_) {
                    self.call_runtime(bcx, rt, &vals);
                    return Ok(bcx.ins().iconst(types::I64, 0));
                }
                return Ok(self.call_runtime(bcx, rt, &vals));
            }
        }

        // Instance method call: obj.method(args) -> StructName_method(obj, args...)
        if let DataType::Struct(sname) = &obj_ty {
            let full = format!("{}::{}", sname, method);
            let obj_val = self.translate_expr(bcx, object)?;
            let mut vals = vec![obj_val];
            vals.append(&mut self.translate_args(bcx, args)?);
            // Instance method: arity = vals.len() (includes implicit self)
            if let Some(sym) = self.resolve_struct_method_symbol(sname, method, vals.len()) {
                if let Some(fid) = self.func_ids.get(sym) {
                    let fref = self.module.declare_func_in_func(*fid, &mut bcx.func);
                    let call = bcx.ins().call(fref, &vals);
                    return Ok(self.call_result(
                        bcx,
                        call,
                        self.type_resolver.func_return_type(&full),
                    ));
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

    /// Map a (struct_name, method_name) pair to its C runtime function.
    /// Returns None for non-intrinsic methods (which use regular dispatch).
    pub(super) fn struct_intrinsic_runtime(
        &self,
        struct_name: &str,
        method: &str,
    ) -> Option<&'static str> {
        match (struct_name, method) {
            ("File", "open") => Some("gobol_fs_open"),
            ("File", "read_all") => Some("gobol_fs_read_all"),
            ("File", "write") => Some("gobol_fs_write"),
            ("File", "close") => Some("gobol_fs_close"),
            ("TcpStream", "connect") => Some("gobol_tcp_connect"),
            ("TcpStream", "send_str") => Some("gobol_tcp_send_str"),
            ("TcpStream", "send_bytes") => Some("gobol_tcp_send_bytes"),
            ("TcpStream", "recv_bytes") => Some("gobol_tcp_recv_bytes"),
            ("TcpStream", "recv_exact") => Some("gobol_tcp_recv_exact"),
            ("TcpStream", "close") => Some("gobol_tcp_close"),
            ("TcpStream", "set_read_timeout") => Some("gobol_tcp_set_read_timeout"),
            ("TcpStream", "set_write_timeout") => Some("gobol_tcp_set_write_timeout"),
            ("TcpStream", "set_keepalive") => Some("gobol_tcp_set_keepalive"),
            ("TcpStream", "local_addr") => Some("gobol_tcp_local_addr"),
            ("TcpStream", "remote_addr") => Some("gobol_tcp_remote_addr"),
            ("TcpStream", "is_alive") => Some("gobol_tcp_is_alive"),
            ("TcpListener", "bind") => Some("gobol_tcp_bind"),
            ("TcpListener", "accept") => Some("gobol_tcp_accept"),
            ("TcpListener", "close") => Some("gobol_tcp_listener_close"),
            _ => None,
        }
    }

    /// Which net struct-intrinsic methods return a `Result<_, str>` that needs
    /// to be constructed in code, vs. a plain value.
    pub(super) fn net_intrinsic_kind(
        &self,
        method: &str,
        ret_ty: &DataType,
    ) -> Option<NetIntrinsicKind> {
        use NetIntrinsicKind::*;
        let payload = match method {
            "connect" | "bind" | "accept" => Some(OkPayload::Struct),
            "local_addr" | "remote_addr" => Some(OkPayload::Str),
            "recv_bytes" | "recv_exact" => Some(OkPayload::Bytes),
            "send_str" | "send_bytes" => Some(OkPayload::Int),
            "close" | "set_read_timeout" | "set_write_timeout" | "set_keepalive" => {
                Some(OkPayload::Unit)
            }
            _ => None,
        };
        if let Some(payload) = payload {
            return Some(Result_(payload));
        }

        match ret_ty {
            DataType::Struct(s) if s.split('<').next() == Some("Result") => {
                Some(Result_(OkPayload::Unit))
            }
            DataType::Struct(s) if s == "TcpStream" || s == "TcpListener" => {
                Some(ValuePayload(OkPayload::Struct))
            }
            DataType::Str | DataType::Int | DataType::Bool => Some(Plain),
            _ => None,
        }
    }

    /// Lower a net struct-intrinsic call.
    ///
    /// `self_val` is the receiver (for instance methods) or `None` for static
    /// methods (connect/bind).  `arg_vals` are the already-translated explicit
    /// arguments.  Every net C function takes a trailing `char **err_msg`
    /// out-param; we allocate a scratch slot, call the C function, and then
    /// wrap the raw result into a `Result<_, str>` value when the gobol
    /// signature asks for one.
    pub(super) fn translate_net_intrinsic(
        &mut self,
        bcx: &mut FunctionBuilder,
        rt: &str,
        kind: NetIntrinsicKind,
        self_val: Option<ir::Value>,
        arg_vals: &[ir::Value],
    ) -> Result<ir::Value, String> {
        use NetIntrinsicKind::*;
        match kind {
            Plain | ValuePayload(..) => {
                let mut vals: Vec<ir::Value> = Vec::new();
                if let Some(s) = self_val {
                    // Instance methods receive a pointer to the wrapper
                    // struct, while the C networking API expects its `_fd`
                    // field value.
                    let fd = self.call_runtime(bcx, "gobol_mem_load", &[s]);
                    vals.push(fd);
                }
                vals.extend_from_slice(arg_vals);
                return Ok(self.call_runtime(bcx, rt, &vals));
            }
            Result_(_payload) => {
                // Allocate a scratch slot for the C `char **err_msg` out-param.
                let eight = bcx.ins().iconst(types::I64, 8);
                let slot = self.call_runtime(bcx, "gobol_gc_alloc", &[eight]);
                let zero0 = bcx.ins().iconst(types::I64, 0);
                self.call_runtime(bcx, "gobol_mem_store", &[slot, zero0]);
                let out_len_slot = if matches!(rt, "gobol_tcp_recv_bytes" | "gobol_tcp_recv_exact")
                {
                    let length_slot = self.call_runtime(bcx, "gobol_gc_alloc", &[eight]);
                    self.call_runtime(bcx, "gobol_mem_store", &[length_slot, zero0]);
                    Some(length_slot)
                } else {
                    None
                };

                let mut vals: Vec<ir::Value> = Vec::new();
                if let Some(s) = self_val {
                    // Instance methods receive the wrapper pointer in Gobol,
                    // but the runtime APIs operate on the stored file
                    // descriptor.
                    let fd = self.call_runtime(bcx, "gobol_mem_load", &[s]);
                    vals.push(fd);
                }
                vals.extend_from_slice(arg_vals);
                if let Some(length_slot) = out_len_slot {
                    vals.push(length_slot);
                }
                vals.push(slot);
                let raw = self.call_runtime(bcx, rt, &vals);

                // Read the error message (a GC'd char*, or 0 / null).
                let err_val = self.call_runtime(bcx, "gobol_mem_load", &[slot]);

                // Allocate the Result<T, str> struct and write its tag plus
                // the shared payload slot used by every variant.
                let rsize = self.type_resolver.struct_size("Result").max(16);
                let rsize_val = bcx.ins().iconst(types::I64, rsize);
                let res = self.call_runtime(bcx, "gobol_gc_alloc", &[rsize_val]);

                // Tag is at offset 0 and the variant payload starts at offset 8.
                let zero = bcx.ins().iconst(types::I64, 0);
                let one = bcx.ins().iconst(types::I64, 1);
                let success = bcx.ins().icmp(IntCC::SignedGreaterThanOrEqual, raw, zero);

                let join = bcx.create_block();
                let join_param = bcx.append_block_param(join, types::I64);
                let okb = bcx.create_block();
                let errb = bcx.create_block();
                bcx.ins().brif(success, okb, &[], errb, &[]);
                bcx.seal_block(okb);
                bcx.seal_block(errb);

                // Ok branch: _tag = 0, _0 = payload.
                bcx.switch_to_block(okb);
                self.store_result_field(bcx, res, 0, zero);
                let ok_payload = match _payload {
                    OkPayload::Struct => {
                        let size = bcx.ins().iconst(types::I64, 8);
                        let wrapper = self.call_runtime(bcx, "gobol_gc_alloc", &[size]);
                        self.store_result_field(bcx, wrapper, 0, raw);
                        wrapper
                    }
                    OkPayload::Bytes => {
                        let length_slot = out_len_slot
                            .ok_or_else(|| "missing byte count from TCP receive".to_string())?;
                        let length = self.call_runtime(bcx, "gobol_mem_load", &[length_slot]);
                        let array = self.byte_buffer_to_array(bcx, raw, length);
                        self.call_runtime(bcx, "gobol_free", &[raw]);
                        array
                    }
                    _ => raw,
                };
                self.store_result_field(bcx, res, 8, ok_payload);
                bcx.ins().jump(join, &[BlockArg::Value(res)]);

                // Err branch: _tag = 1, _0 = error message.
                bcx.switch_to_block(errb);
                self.store_result_field(bcx, res, 0, one);
                self.store_result_field(bcx, res, 8, err_val);
                bcx.ins().jump(join, &[BlockArg::Value(res)]);

                bcx.seal_block(join);
                bcx.switch_to_block(join);
                Ok(join_param)
            }
        }
    }

    pub(super) fn byte_buffer_to_array(
        &mut self,
        bcx: &mut FunctionBuilder,
        buffer: ir::Value,
        length: ir::Value,
    ) -> ir::Value {
        let array = self.call_runtime(bcx, "gobol_array_new", &[]);
        let cond_b = bcx.create_block();
        let body_b = bcx.create_block();
        let end_b = bcx.create_block();
        let index = bcx.append_block_param(cond_b, types::I64);
        let zero = bcx.ins().iconst(types::I64, 0);
        bcx.ins().jump(cond_b, &[BlockArg::Value(zero)]);

        bcx.switch_to_block(cond_b);
        let index_value = bcx.block_params(cond_b)[0];
        let has_next = bcx.ins().icmp(IntCC::SignedLessThan, index_value, length);
        bcx.ins().brif(has_next, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(body_b);
        let address = bcx.ins().iadd(buffer, index);
        let byte = bcx
            .ins()
            .load(types::I8, MemFlagsData::trusted(), address, 0);
        let byte = bcx.ins().uextend(types::I64, byte);
        self.call_runtime(bcx, "gobol_array_add", &[array, byte]);
        let next = bcx.ins().iadd_imm_s(index, 1);
        bcx.ins().jump(cond_b, &[BlockArg::Value(next)]);
        bcx.seal_block(cond_b);

        bcx.switch_to_block(end_b);
        array
    }

    pub(super) fn byte_array_to_string(
        &mut self,
        bcx: &mut FunctionBuilder,
        array: ir::Value,
    ) -> ir::Value {
        let length = self.call_runtime(bcx, "gobol_array_len", &[array]);
        let terminator_space = bcx.ins().iadd_imm_s(length, 1);
        let string = self.call_runtime(bcx, "gobol_gc_alloc", &[terminator_space]);
        let cond_b = bcx.create_block();
        let body_b = bcx.create_block();
        let end_b = bcx.create_block();
        let index = bcx.append_block_param(cond_b, types::I64);
        let zero = bcx.ins().iconst(types::I64, 0);
        bcx.ins().jump(cond_b, &[BlockArg::Value(zero)]);

        bcx.switch_to_block(cond_b);
        let index_value = bcx.block_params(cond_b)[0];
        let has_next = bcx.ins().icmp(IntCC::SignedLessThan, index_value, length);
        bcx.ins().brif(has_next, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(body_b);
        let byte = self.call_runtime(bcx, "gobol_array_get", &[array, index]);
        let byte = bcx.ins().ireduce(types::I8, byte);
        let address = bcx.ins().iadd(string, index);
        bcx.ins().store(MemFlagsData::trusted(), byte, address, 0);
        let next = bcx.ins().iadd_imm_s(index, 1);
        bcx.ins().jump(cond_b, &[BlockArg::Value(next)]);
        bcx.seal_block(cond_b);

        bcx.switch_to_block(end_b);
        let terminator = bcx.ins().iconst(types::I8, 0);
        let terminator_address = bcx.ins().iadd(string, length);
        bcx.ins()
            .store(MemFlagsData::trusted(), terminator, terminator_address, 0);
        string
    }

    /// Store a value into a GC struct field at the given byte offset.
    pub(super) fn store_result_field(
        &mut self,
        bcx: &mut FunctionBuilder,
        base: ir::Value,
        offset: i64,
        val: ir::Value,
    ) {
        let addr = self.field_addr(bcx, base, offset);
        let val = if bcx.func.dfg.value_type(val) == types::I8 {
            bcx.ins().uextend(types::I64, val)
        } else {
            val
        };
        self.call_runtime(bcx, "gobol_mem_store", &[addr, val]);
    }

    /// Try to handle an intrinsic arithmetic method call (add, sub, mul, etc.)
    /// on a primitive type (int, float). Returns None if not an intrinsic.
    pub(super) fn try_intrinsic_method(
        &mut self,
        bcx: &mut FunctionBuilder,
        object: &IRExpr,
        method: &str,
        args: &[IRExpr],
        obj_ty: &DataType,
    ) -> Result<Option<ir::Value>, String> {
        let value_ty = match obj_ty {
            DataType::Reference(inner) => inner.as_ref(),
            _ => obj_ty,
        };
        let is_int = matches!(value_ty, DataType::Int | DataType::Byte);
        let is_float = matches!(value_ty, DataType::Float);
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
            let result = match method {
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
            };
            if matches!(value_ty, DataType::Byte) {
                if matches!(method, "eq" | "ne" | "lt" | "gt" | "le" | "ge") {
                    Ok(Some(result))
                } else {
                    // 算术运算截断到 8 位
                    Ok(Some(bcx.ins().ireduce(types::I8, result)))
                }
            } else {
                Ok(Some(result))
            }
        }
    }
}
