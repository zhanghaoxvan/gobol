use super::*;

impl CraneliftBackend {
    // ==================== statements ====================

    pub(super) fn translate_block(
        &mut self,
        bcx: &mut FunctionBuilder,
        block: &IRBlock,
    ) -> Result<(), String> {
        let saved_variables = self.variables.clone();
        let saved_addressable_slots = self.addressable_slots.clone();
        let saved_var_types = self.type_resolver.var_types.clone();
        let result = (|| {
            for stmt in &block.statements {
                let current_terminated = bcx
                    .current_block()
                    .and_then(|block| bcx.func.layout.last_inst(block))
                    .map_or(false, |inst| {
                        bcx.func.dfg.insts[inst].opcode().is_terminator()
                    });
                if current_terminated {
                    break;
                }
                self.translate_stmt(bcx, stmt)?;
            }
            Ok(())
        })();
        self.variables = saved_variables;
        self.addressable_slots = saved_addressable_slots;
        self.type_resolver.var_types = saved_var_types;
        result
    }

    pub(super) fn translate_stmt(
        &mut self,
        bcx: &mut FunctionBuilder,
        stmt: &IRStmt,
    ) -> Result<(), String> {
        match stmt {
            IRStmt::DebugLocation(line) if self.debugger_enabled => {
                let line = bcx.ins().iconst(types::I64, i64::from(*line));
                self.call_runtime(bcx, "gobol_debug_hook", &[line]);
            }
            IRStmt::DebugLocation(_) => {}
            IRStmt::Declaration { name, ty, init } => {
                let unresolved_generic = matches!(
                    ty,
                    DataType::Struct(struct_name)
                        if !self.type_resolver.has_struct(struct_name)
                );
                let needs_inference =
                    *ty == DataType::None_ || *ty == DataType::Unknown || unresolved_generic;
                let inferred = init
                    .as_ref()
                    .map(|e| self.type_resolver.infer_type(e))
                    .unwrap_or(DataType::Unknown);
                let inferred_non_numeric = matches!(
                    inferred,
                    DataType::Str
                        | DataType::Struct(_)
                        | DataType::Array(_)
                        | DataType::Tuple(_)
                        | DataType::Reference(_)
                        | DataType::Pointer(_)
                );
                let resolved = if (needs_inference
                    || (*ty == DataType::Int && inferred_non_numeric))
                    && !matches!(inferred, DataType::Unknown | DataType::None_)
                {
                    inferred
                } else if *ty == DataType::None_ || *ty == DataType::Unknown {
                    // Avoid types::INVALID from DataType::None_
                    DataType::Int
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
                let v =
                    self.coerce(bcx, v, &self.type_resolver.infer_type(e), &self.return_type)?;
                let ret_ty = self
                    .data_type_to_clif(&self.return_type)
                    .unwrap_or(types::I64);
                let v = self.bitcast_to(bcx, v, ret_ty);
                bcx.ins().return_(&[v]);
                self.diverged = true;
            }
            IRStmt::If {
                cond,
                then_block,
                else_block,
            } => {
                self.translate_if(bcx, cond, then_block, else_block.as_ref())?;
            }
            IRStmt::While { cond, body } => {
                self.translate_while(bcx, cond, body)?;
            }
            IRStmt::For {
                vars,
                iterable,
                body,
            } => {
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
            IRStmt::MethodCall {
                object,
                method,
                args,
                ..
            } => {
                self.translate_method_call(bcx, object, method, args)?;
            }
        }
        Ok(())
    }

    pub(super) fn translate_if(
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
        let then_diverged = self.diverged;
        if !then_diverged {
            bcx.ins().jump(merge_b, &[]);
        }
        bcx.switch_to_block(else_b);
        self.diverged = false;
        if let Some(eb) = else_block {
            self.translate_block(bcx, eb)?;
        }
        let else_diverged = self.diverged;
        if !else_diverged {
            bcx.ins().jump(merge_b, &[]);
        }
        // If both branches diverged (e.g. both return), the merge block is
        // unreachable — propagate divergence so the caller doesn't emit
        // dead code after the if. Otherwise switch to merge_b.
        if then_diverged && else_diverged {
            // Neither branch references the merge block, and it has not been
            // inserted into the layout because we never switched to it.
            self.diverged = true;
        } else {
            bcx.seal_block(merge_b);
            bcx.switch_to_block(merge_b);
            self.diverged = false;
        }
        Ok(())
    }

    pub(super) fn translate_while(
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

        bcx.switch_to_block(body_b);
        self.loop_stack.push((end_b, cond_b));
        self.diverged = false;
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(cond_b, &[]);
        }
        bcx.seal_block(cond_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(end_b);
        self.diverged = false;
        Ok(())
    }

    pub(super) fn translate_for(
        &mut self,
        bcx: &mut FunctionBuilder,
        vars: &[String],
        iterable: &IRExpr,
        body: &IRBlock,
    ) -> Result<(), String> {
        let saved_variables = self.variables.clone();
        let saved_addressable_slots = self.addressable_slots.clone();
        let saved_var_types = self.type_resolver.var_types.clone();
        let result = (|| {
            // Range: range::new(start, end[, step])  or  Range::new(...)  or  start..end
            if let IRExpr::Call { func, args, .. } = iterable {
                if func == "range::new" || func == "Range::new" {
                    return self.translate_for_range(bcx, vars, args, body);
                }
            }
            // String literal iteration: for ch in "abc"
            if let IRExpr::Literal(LitValue::Str(s)) = iterable {
                return self.translate_for_string(bcx, vars, s, body);
            }
            // Array iteration
            self.translate_for_array(bcx, vars, iterable, body)
        })();
        self.variables = saved_variables;
        self.addressable_slots = saved_addressable_slots;
        self.type_resolver.var_types = saved_var_types;
        result
    }

    pub(super) fn translate_for_range(
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

        // Determine comparison direction from compile-time step literal.
        // If step is a negative literal, loop while cur > end; else cur < end.
        let cmp_op = if args.len() >= 3 {
            match &args[2] {
                IRExpr::Literal(LitValue::Int(n)) if *n < 0 => IntCC::SignedGreaterThan,
                _ => IntCC::SignedLessThan,
            }
        } else {
            IntCC::SignedLessThan
        };

        let loop_var_name = if vars.len() >= 2 { &vars[1] } else { &vars[0] };
        let idx_var_name = if vars.len() >= 2 {
            Some(vars[0].as_str())
        } else {
            None
        };

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
        // incr_b is sealed after the body's fall-through jump, so Cranelift
        // knows the body block as a predecessor before sealing.
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let cur = bcx.use_var(iv);
        let cmp = bcx.ins().icmp(cmp_op, cur, end);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);

        bcx.switch_to_block(body_b);
        self.loop_stack.push((end_b, incr_b));
        self.diverged = false;
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(incr_b, &[]);
        }
        bcx.seal_block(incr_b);

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
        bcx.seal_block(end_b);

        bcx.switch_to_block(end_b);
        self.diverged = false;
        Ok(())
    }

    pub(super) fn translate_for_array(
        &mut self,
        bcx: &mut FunctionBuilder,
        vars: &[String],
        iterable: &IRExpr,
        body: &IRBlock,
    ) -> Result<(), String> {
        let arr_ptr = self.translate_expr(bcx, iterable)?;
        let element_type = match self.type_resolver.infer_type(iterable) {
            DataType::Array(element) => *element,
            _ => DataType::Int,
        };
        let val_name = if vars.len() >= 2 { &vars[1] } else { &vars[0] };
        let idx_name = if vars.len() >= 2 {
            Some(vars[0].as_str())
        } else {
            None
        };

        let element_clif_type = self.data_type_to_clif(&element_type).unwrap_or(types::I64);
        let val_var = self.declare_variable(bcx, val_name, element_clif_type, &element_type);
        let idx_var = self.declare_variable(
            bcx,
            &format!("__idx_{}", val_name),
            types::I64,
            &DataType::Int,
        );
        let user_idx_var = match idx_name {
            Some(n) => Some(self.declare_variable(bcx, n, types::I64, &DataType::Int)),
            None => None,
        };

        let initial_value = match element_type {
            DataType::Float => bcx.ins().f64const(0.0),
            DataType::Bool | DataType::Byte => bcx.ins().iconst(types::I8, 0),
            _ => bcx.ins().iconst(types::I64, 0),
        };
        let zero = bcx.ins().iconst(types::I64, 0);
        bcx.def_var(val_var, initial_value);
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
        // incr_b sealed after the body's fall-through jump (see translate_while).
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let len = self.call_runtime(bcx, "gobol_array_len", &[arr_ptr]);
        let cmp = bcx.ins().icmp(IntCC::SignedLessThan, i, len);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);

        bcx.switch_to_block(body_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let raw_val = self.call_runtime(bcx, "gobol_array_get", &[arr_ptr, i]);
        let val = self.coerce_array_element(bcx, raw_val, &element_type);
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
        bcx.seal_block(incr_b);

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
        bcx.seal_block(end_b);

        bcx.switch_to_block(end_b);
        self.diverged = false;
        Ok(())
    }

    pub(super) fn translate_for_string(
        &mut self,
        bcx: &mut FunctionBuilder,
        vars: &[String],
        s: &str,
        body: &IRBlock,
    ) -> Result<(), String> {
        let str_ptr = self.intern_string(bcx, s);
        let ch_name = &vars[0];
        let ch_var = self.declare_variable(bcx, ch_name, types::I64, &DataType::Str);
        let idx_var = self.declare_variable(
            bcx,
            &format!("__idx_{}", ch_name),
            types::I64,
            &DataType::Int,
        );
        let zero = bcx.ins().iconst(types::I64, 0);
        let empty_str = self.intern_string(bcx, "");
        bcx.def_var(ch_var, empty_str);
        bcx.def_var(idx_var, zero);

        let cond_b = bcx.create_block();
        let body_b = bcx.create_block();
        let incr_b = bcx.create_block();
        let end_b = bcx.create_block();

        bcx.ins().jump(cond_b, &[]);
        // cond_b sealed after the incr back-edge (see translate_while).
        // incr_b sealed after the body's fall-through jump (see translate_while).
        bcx.switch_to_block(cond_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let len = self.call_runtime(bcx, "gobol_str_len", &[str_ptr]);
        let cmp = bcx.ins().icmp(IntCC::SignedLessThan, i, len);
        bcx.ins().brif(cmp, body_b, &[], end_b, &[]);
        bcx.seal_block(body_b);

        bcx.switch_to_block(body_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let ch_code = self.call_runtime(bcx, "gobol_str_get", &[str_ptr, i]);
        let ch = self.call_runtime(bcx, "gobol_str_char", &[ch_code]);
        bcx.def_var(ch_var, ch);
        self.loop_stack.push((end_b, incr_b));
        self.translate_block(bcx, body)?;
        self.loop_stack.pop();
        if !self.diverged {
            bcx.ins().jump(incr_b, &[]);
        }
        bcx.seal_block(incr_b);

        bcx.switch_to_block(incr_b);
        self.diverged = false;
        let i = bcx.use_var(idx_var);
        let one = bcx.ins().iconst(types::I64, 1);
        let next = bcx.ins().iadd(i, one);
        bcx.def_var(idx_var, next);
        bcx.ins().jump(cond_b, &[]);
        bcx.seal_block(cond_b);
        bcx.seal_block(end_b);

        bcx.switch_to_block(end_b);
        self.diverged = false;
        Ok(())
    }

    pub(super) fn translate_assignment(
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
                let raw_val = self.translate_expr(bcx, value)?;
                let val = self.to_storage_word(bcx, raw_val, value);
                if let Some(off) = self.type_resolver.field_offset(sname, member) {
                    let addr = self.field_addr(bcx, obj_val, off);
                    self.call_runtime(bcx, "gobol_mem_store", &[addr, val]);
                    return Ok(());
                }
            }
        }
        // Array index assignment: arr[i] = value
        // Degraded to the Ref<T> path: arr.index_mut(i).write(value).
        // For raw arrays: gobol_array_elem_addr(arr, i) → gobol_mem_store(addr, val).
        // For structs with index_mut: call the method, then call write on the Ref.
        if let IRExpr::ArrayIndex { array, index } = target {
            let arr_ty = self.type_resolver.infer_type(array);

            // Check if this is a nested array assignment (e.g., arr[2][2] = value)
            if let IRExpr::ArrayIndex {
                array: inner_array,
                index: inner_idx,
            } = array.as_ref()
            {
                // 2D array assignment: get inner array, then store element
                let base = self.translate_expr(bcx, inner_array)?;
                let i1 = self.translate_expr(bcx, inner_idx)?;
                let inner_arr = self.call_runtime(bcx, "gobol_array_get", &[base, i1]);
                let i2 = self.translate_expr(bcx, index)?;
                let raw_val = self.translate_expr(bcx, value)?;
                let val = self.to_storage_word(bcx, raw_val, value);
                let addr = self.call_runtime(bcx, "gobol_array_elem_addr", &[inner_arr, i2]);
                self.call_runtime(bcx, "gobol_mem_store", &[addr, val]);
                return Ok(());
            }

            // Struct type with an index_mut method (e.g. vec<T>): use method dispatch.
            if let DataType::Struct(ref sname) = arr_ty {
                let full = format!("{}::index_mut", sname);
                // index_mut(self, index) → arity = 2
                if self.func_symbols.contains_key(&(full.clone(), 2)) {
                    // arr.index_mut(i) → returns a Ref<T>
                    let arr_val = self.translate_expr(bcx, array)?;
                    let idx_val = self.translate_expr(bcx, index)?;
                    let raw_val = self.translate_expr(bcx, value)?;
                    let val = self.to_storage_word(bcx, raw_val, value);
                    let ref_val = self.translate_call_with_args(bcx, &full, &[arr_val, idx_val])?;
                    // ref.write(value)
                    let write_full = format!("{}::write", "Ref");
                    // Ref::write(self, value) → arity = 2
                    if self.func_symbols.contains_key(&(write_full.clone(), 2)) {
                        self.translate_call_with_args(bcx, &write_full, &[ref_val, val])?;
                    } else {
                        // Fallback: direct memory store via runtime
                        self.call_runtime(bcx, "gobol_mem_store", &[ref_val, val]);
                    }
                    return Ok(());
                }
            }

            // Raw array (DataType::Unknown or DataType::Array): use the runtime functions.
            let arr = self.translate_expr(bcx, array)?;
            let idx = self.translate_expr(bcx, index)?;
            let raw_val = self.translate_expr(bcx, value)?;
            let val = self.to_storage_word(bcx, raw_val, value);
            let addr = self.call_runtime(bcx, "gobol_array_elem_addr", &[arr, idx]);
            self.call_runtime(bcx, "gobol_mem_store", &[addr, val]);
            return Ok(());
        }
        if let IRExpr::Unary { op, operand } = target {
            if op == "*" {
                let DataType::Pointer(inner) = self.type_resolver.infer_type(operand) else {
                    return Err("cannot assign through a non-pointer value".to_string());
                };
                let ptr = self.translate_expr(bcx, operand)?;
                let val = self.translate_expr(bcx, value)?;
                let from = self.type_resolver.infer_type(value);
                let val = self.coerce(bcx, val, &from, &inner)?;
                let val = self.bitcast_to(bcx, val, self.data_type_to_clif(&inner)?);
                bcx.ins().store(MemFlagsData::new(), val, ptr, 0);
                return Ok(());
            }
        }
        // Simple variable assignment
        if let IRExpr::Variable(name) = target {
            // Module-level mutable `var` global: store into shared slot.
            if self.global_vars.contains_key(name) {
                let from = self.type_resolver.infer_type(value);
                let gval = self.translate_expr(bcx, value)?;
                self.translate_global_write(bcx, name, gval, &from)?;
                return Ok(());
            }
            let val = self.translate_expr(bcx, value)?;
            if let Some(var) = self.variables.get(name) {
                let var_ty = self.type_resolver.var_type(name);
                if let DataType::Reference(inner) = &var_ty {
                    let ptr = bcx.use_var(*var);
                    let from = self.type_resolver.infer_type(value);
                    let val = self.coerce(bcx, val, &from, inner)?;
                    let val = self.bitcast_to(bcx, val, self.data_type_to_clif(inner)?);
                    bcx.ins().store(MemFlagsData::new(), val, ptr, 0);
                    return Ok(());
                }
                let v = self.coerce(bcx, val, &self.type_resolver.infer_type(value), &var_ty)?;
                let target_ty = self.data_type_to_clif(&var_ty).unwrap_or(types::I64);
                let v = self.bitcast_to(bcx, v, target_ty);
                bcx.def_var(*var, v);
                if let Some(slot) = self.addressable_slots.get(name).copied() {
                    bcx.ins().stack_store(types::I64, v, slot, 0);
                }
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
}
