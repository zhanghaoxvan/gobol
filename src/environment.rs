#![allow(dead_code)]

use crate::ast::Expression;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataType {
    Int,
    Float,
    Str,
    Bool,
    None_,
    Unknown,
    Struct(String),
    Nullable(Box<DataType>),
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            DataType::Int => "int",
            DataType::Float => "float",
            DataType::Str => "str",
            DataType::Bool => "bool",
            DataType::None_ => "none",
            DataType::Unknown => "unknown",
            DataType::Struct(name) => return write!(f, "{}", name),
            DataType::Nullable(inner) => return write!(f, "{}?", inner),
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolType {
    Variable,
    Function,
    Module,
}

pub struct ArrayDimension {
    pub is_constant: bool,
    pub constant_size: i32,
    pub size_expr: Option<Box<dyn Expression>>,
}

impl ArrayDimension {
    pub fn new_constant(size: i32) -> Self {
        ArrayDimension {
            is_constant: true,
            constant_size: size,
            size_expr: None,
        }
    }

    pub fn new_expr(expr: Box<dyn Expression>) -> Self {
        ArrayDimension {
            is_constant: false,
            constant_size: 0,
            size_expr: Some(expr),
        }
    }
}

impl Clone for ArrayDimension {
    fn clone(&self) -> Self {
        ArrayDimension {
            is_constant: self.is_constant,
            constant_size: self.constant_size,
            size_expr: None, // Can't clone dyn Expression
        }
    }
}

impl std::fmt::Debug for ArrayDimension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrayDimension")
            .field("is_constant", &self.is_constant)
            .field("constant_size", &self.constant_size)
            .field("size_expr", &self.size_expr.as_ref().map(|_| "<expr>"))
            .finish()
    }
}

pub struct Symbol {
    pub name: String,
    pub symbol_type: SymbolType,
    pub data_type: DataType,
    pub scope_level: i32,
    pub module_name: String,
    pub is_mut: bool,
    pub is_array: bool,
    pub dimensions: Vec<ArrayDimension>,
}

impl Clone for Symbol {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            symbol_type: self.symbol_type,
            data_type: self.data_type.clone(),
            scope_level: self.scope_level,
            module_name: self.module_name.clone(),
            is_mut: self.is_mut,
            is_array: self.is_array,
            dimensions: self.dimensions.clone(),
        }
    }
}

impl std::fmt::Debug for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Symbol")
            .field("name", &self.name)
            .field("symbol_type", &self.symbol_type)
            .field("data_type", &self.data_type)
            .field("scope_level", &self.scope_level)
            .field("module_name", &self.module_name)
            .field("is_mut", &self.is_mut)
            .field("is_array", &self.is_array)
            .field("dimensions", &self.dimensions)
            .finish()
    }
}

impl Symbol {
    pub fn new_variable(name: &str, dt: &DataType, scope: i32) -> Self {
        Symbol {
            name: name.to_string(),
            symbol_type: SymbolType::Variable,
            data_type: dt.clone(),
            scope_level: scope,
            module_name: String::new(),
            is_mut: false,
            is_array: false,
            dimensions: Vec::new(),
        }
    }

    pub fn new_function(name: &str, module: &str, return_type: &DataType, scope: i32) -> Self {
        Symbol {
            name: name.to_string(),
            symbol_type: SymbolType::Function,
            data_type: return_type.clone(),
            scope_level: scope,
            module_name: module.to_string(),
            is_mut: false,
            is_array: false,
            dimensions: Vec::new(),
        }
    }

    pub fn new_array_constant(
        name: &str,
        dt: &DataType,
        scope: i32,
        sizes: &[i32],
        is_mut: bool,
    ) -> Self {
        let dimensions: Vec<ArrayDimension> = sizes.iter().map(|&s| ArrayDimension::new_constant(s)).collect();
        Symbol {
            name: name.to_string(),
            symbol_type: SymbolType::Variable,
            data_type: dt.clone(),
            scope_level: scope,
            module_name: String::new(),
            is_mut,
            is_array: true,
            dimensions,
        }
    }

    pub fn new_array_expr(
        name: &str,
        dt: &DataType,
        scope: i32,
        size_exprs: Vec<Box<dyn Expression>>,
        is_mut: bool,
    ) -> Self {
        let dimensions: Vec<ArrayDimension> = size_exprs.into_iter().map(ArrayDimension::new_expr).collect();
        Symbol {
            name: name.to_string(),
            symbol_type: SymbolType::Variable,
            data_type: dt.clone(),
            scope_level: scope,
            module_name: String::new(),
            is_mut,
            is_array: true,
            dimensions,
        }
    }

    pub fn get_dimension(&self) -> usize {
        self.dimensions.len()
    }

    pub fn get_constant_size(&self, dim: i32) -> i32 {
        if dim < 0 || dim as usize >= self.dimensions.len() {
            return 0;
        }
        let d = &self.dimensions[dim as usize];
        if d.is_constant {
            d.constant_size
        } else {
            0
        }
    }

    pub fn get_size_expr(&self, dim: i32) -> Option<&dyn Expression> {
        if dim < 0 || dim as usize >= self.dimensions.len() {
            return None;
        }
        self.dimensions[dim as usize].size_expr.as_deref().map(|e| e.as_expression())
    }

    pub fn is_dimension_constant(&self, dim: i32) -> bool {
        if dim < 0 || dim as usize >= self.dimensions.len() {
            return false;
        }
        self.dimensions[dim as usize].is_constant
    }
}

pub fn data_type_to_string(dt: DataType) -> String {
    dt.to_string()
}

pub struct Environment {
    scopes: Vec<HashMap<String, Symbol>>,
}

impl Environment {
    pub fn new() -> Self {
        let mut env = Environment { scopes: Vec::new() };
        env.scopes.push(HashMap::new()); // global scope
        env
    }

    pub fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    pub fn exit_scope(&mut self) {
        if !self.scopes.is_empty() {
            self.scopes.pop();
        }
    }

    pub fn get_current_scope(&self) -> i32 {
        self.scopes.len() as i32 - 1
    }

    pub fn get_scope_count(&self) -> usize {
        self.scopes.len()
    }

    pub fn declare_variable(&mut self, name: &str, dt: &DataType, is_mut: bool) -> bool {
        if self.scopes.is_empty() {
            self.scopes.push(HashMap::new());
        }

        let scope_level = self.get_current_scope();
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return false;
        }

        let mut sym = Symbol::new_variable(name, dt, scope_level);
        sym.is_mut = is_mut;
        scope.insert(name.to_string(), sym);
        true
    }

    pub fn declare_function(&mut self, name: &str, return_type: &DataType, module_name: &str) -> bool {
        if self.scopes.is_empty() {
            self.scopes.push(HashMap::new());
        }

        let full_name = format!("{}.{}", module_name, name);
        let global_scope = &mut self.scopes[0];

        if global_scope.contains_key(&full_name) {
            return false;
        }

        global_scope.insert(full_name, Symbol::new_function(name, module_name, return_type, 0));
        true
    }

    pub fn declare_module(&mut self, name: &str) -> bool {
        if self.scopes.is_empty() {
            self.scopes.push(HashMap::new());
        }

        let global_scope = &mut self.scopes[0];
        if let Some(existing) = global_scope.get(name) {
            if existing.symbol_type != SymbolType::Module {
                return false;
            }
            return true;
        }

        let mut sym = Symbol::new_variable(name, &DataType::None_, 0);
        sym.symbol_type = SymbolType::Module;
        global_scope.insert(name.to_string(), sym);
        true
    }

    pub fn declare_array_constant(
        &mut self,
        name: &str,
        element_type: &DataType,
        sizes: &[i32],
        is_mut: bool,
    ) -> bool {
        if self.scopes.is_empty() {
            self.scopes.push(HashMap::new());
        }

        let scope_level = self.get_current_scope();
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return false;
        }

        scope.insert(
            name.to_string(),
            Symbol::new_array_constant(name, element_type, scope_level, sizes, is_mut),
        );
        true
    }

    pub fn declare_array_expr(
        &mut self,
        name: &str,
        element_type: &DataType,
        size_exprs: Vec<Box<dyn Expression>>,
        is_mut: bool,
    ) -> bool {
        if self.scopes.is_empty() {
            self.scopes.push(HashMap::new());
        }

        let scope_level = self.get_current_scope();
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return false;
        }

        scope.insert(
            name.to_string(),
            Symbol::new_array_expr(name, element_type, scope_level, size_exprs, is_mut),
        );
        true
    }

    pub fn lookup_symbol(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.get(name) {
                return Some(sym);
            }
        }
        None
    }

    pub fn lookup_symbol_mut(&mut self, name: &str) -> Option<&mut Symbol> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(sym) = scope.get_mut(name) {
                return Some(sym);
            }
        }
        None
    }

    pub fn is_declared(&self, name: &str) -> bool {
        self.lookup_symbol(name).is_some()
    }

    pub fn is_declared_in_current_scope(&self, name: &str) -> bool {
        if self.scopes.is_empty() {
            return false;
        }
        self.scopes.last().unwrap().contains_key(name)
    }

    pub fn get_symbol_type(&self, name: &str) -> DataType {
        self.lookup_symbol(name).map(|s| s.data_type.clone()).unwrap_or(DataType::Unknown)
    }

    pub fn is_type_compatible(target: &DataType, source: &DataType) -> bool {
        if target == source {
            return true;
        }
        if *target == DataType::Float && *source == DataType::Int {
            return true;
        }
        false
    }

    pub fn is_numeric_type(dt: &DataType) -> bool {
        *dt == DataType::Int || *dt == DataType::Float
    }

    pub fn reset(&mut self) {
        self.scopes.clear();
        self.scopes.push(HashMap::new());
    }

    #[cfg(debug_assertions)]
    pub fn print_scope(&self) {
        if self.scopes.is_empty() {
            println!("No scopes available");
            return;
        }

        let current = self.scopes.len() - 1;
        println!("=== Current Scope (level {}) ===", current);

        let scope = self.scopes.last().unwrap();
        if scope.is_empty() {
            println!("  (empty)");
        } else {
            for (name, sym) in scope {
                print!("  {} : ", name);
                match sym.symbol_type {
                    SymbolType::Variable => print!("variable"),
                    SymbolType::Function => print!("function"),
                    SymbolType::Module => print!("module"),
                }
                print!(" ({})", sym.data_type);
                if !sym.module_name.is_empty() {
                    print!(" [module={}]", sym.module_name);
                }
                if sym.is_array {
                    print!(" array[");
                    for (j, dim) in sym.dimensions.iter().enumerate() {
                        if j > 0 {
                            print!(",");
                        }
                        if dim.is_constant {
                            print!("{}", dim.constant_size);
                        } else {
                            print!("expr");
                        }
                    }
                    print!("]");
                }
                println!(" ({})", if sym.is_mut { "var" } else { "val" });
            }
        }
    }

    #[cfg(debug_assertions)]
    pub fn print_all_scopes(&self) {
        println!("=== All Scopes ({} levels) ===", self.scopes.len());
        for (i, scope) in self.scopes.iter().enumerate() {
            println!("Scope {}:", i);
            if scope.is_empty() {
                println!("  (empty)");
            } else {
                for (name, sym) in scope {
                    print!("  {} : ", name);
                    match sym.symbol_type {
                        SymbolType::Variable => print!("variable"),
                        SymbolType::Function => print!("function"),
                        SymbolType::Module => print!("module"),
                    }
                    print!(" ({})", sym.data_type);
                    if !sym.module_name.is_empty() {
                        print!(" [module={}]", sym.module_name);
                    }
                    if sym.is_array {
                        print!(" array[");
                        for (j, dim) in sym.dimensions.iter().enumerate() {
                            if j > 0 {
                                print!(",");
                            }
                            if dim.is_constant {
                                print!("{}", dim.constant_size);
                            } else {
                                print!("expr");
                            }
                        }
                        print!("]");
                    }
                    println!(" ({})", if sym.is_mut { "var" } else { "val" });
                }
            }
        }
    }
}
