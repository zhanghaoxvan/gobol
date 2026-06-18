#![allow(dead_code)]

use std::any::Any;

// ==================== Visitor ====================

pub trait AstVisitor {
    #[allow(dead_code)]
    fn visit_ast_node(&mut self, _node: &dyn AstNode) {}
    fn visit_program(&mut self, _node: &Program) {}
    #[allow(dead_code)]
    fn visit_statement(&mut self, _node: &dyn Statement) {}
    #[allow(dead_code)]
    fn visit_expression(&mut self, _node: &dyn Expression) {}
    fn visit_block(&mut self, _node: &Block) {}
    fn visit_function(&mut self, _node: &Function) {}
    fn visit_parameter(&mut self, _node: &Parameter) {}
    fn visit_basic_type(&mut self, _node: &BasicType) {}
    #[allow(dead_code)]
    fn visit_type(&mut self, _node: &dyn Type) {}
    fn visit_array_type(&mut self, _node: &ArrayType) {}
    fn visit_if_statement(&mut self, _node: &IfStatement) {}
    fn visit_while_statement(&mut self, _node: &WhileStatement) {}
    fn visit_for_statement(&mut self, _node: &ForStatement) {}
    fn visit_return_statement(&mut self, _node: &ReturnStatement) {}
    fn visit_break_statement(&mut self, _node: &BreakStatement) {}
    fn visit_continue_statement(&mut self, _node: &ContinueStatement) {}
    fn visit_declaration(&mut self, _node: &Declaration) {}
    fn visit_expression_statement(&mut self, _node: &ExpressionStatement) {}
    fn visit_import_statement(&mut self, _node: &ImportStatement) {}
    fn visit_export_statement(&mut self, _node: &ExportStatement) {}
    fn visit_struct_definition(&mut self, _node: &StructDefinition) {}
    fn visit_impl_block(&mut self, _node: &ImplBlock) {}
    fn visit_binary_expression(&mut self, _node: &BinaryExpression) {}
    fn visit_unary_expression(&mut self, _node: &UnaryExpression) {}
    fn visit_cast_expression(&mut self, _node: &CastExpression) {}
    fn visit_function_call(&mut self, _node: &FunctionCall) {}
    fn visit_member_access(&mut self, _node: &MemberAccess) {}
    fn visit_array_index(&mut self, _node: &ArrayIndex) {}
    fn visit_grouped_expression(&mut self, _node: &GroupedExpression) {}
    fn visit_identifier(&mut self, _node: &Identifier) {}
    fn visit_number_literal(&mut self, _node: &NumberLiteral) {}
    fn visit_string_literal(&mut self, _node: &StringLiteral) {}
    fn visit_boolean_literal(&mut self, _node: &BooleanLiteral) {}
    fn visit_null_literal(&mut self, _node: &NullLiteral) {}
    fn visit_format_string(&mut self, _node: &FormatString) {}
    fn visit_range_expression(&mut self, _node: &RangeExpression) {}
    fn visit_array_literal(&mut self, _node: &ArrayLiteral) {}
    fn visit_struct_literal(&mut self, _node: &StructLiteral) {}
    fn visit_match_expression(&mut self, _node: &MatchExpression) {}
}

// ==================== Node ====================

pub trait AstNode {
    fn accept(&self, visitor: &mut dyn AstVisitor);
    fn as_any(&self) -> &dyn Any;
}

// ==================== Statement ====================

pub trait Statement: AstNode {
    fn as_statement(&self) -> &dyn Statement;
}

// ==================== Expression ====================

pub trait Expression: AstNode {
    fn as_expression(&self) -> &dyn Expression;
}

// ==================== Type trait ====================

pub trait Type: AstNode {
    fn get_name(&self) -> &str;
    #[allow(dead_code)]
    fn as_type(&self) -> &dyn Type;
    fn as_type_any(&self) -> &dyn Any;
}

// ==================== BasicType ====================

#[derive(Debug, Clone)]
pub struct BasicType {
    pub name: String,
}

impl BasicType {
    pub fn new(name: impl Into<String>) -> Self {
        BasicType { name: name.into() }
    }
}

impl AstNode for BasicType {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_basic_type(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Type for BasicType {
    fn get_name(&self) -> &str {
        &self.name
    }
    fn as_type(&self) -> &dyn Type {
        self
    }
    fn as_type_any(&self) -> &dyn Any {
        self
    }
}

// ==================== ArrayType (完整多维数组支持) ====================

pub struct ArrayType {
    element_type: Box<dyn Type>,      // 元素类型（可以是基本类型或嵌套数组）
    size: Option<Box<dyn Expression>>,  // 当前维度的大小
}

impl ArrayType {
    /// 创建一维数组类型（基本类型 + 大小）
    pub fn new_basic(element_name: &str, size: Box<dyn Expression>) -> Self {
        ArrayType {
            element_type: Box::new(BasicType::new(element_name)),
            size: Some(size),
        }
    }

    /// 创建多维数组类型（嵌套 ArrayType + 大小）
    pub fn new_nested(element_type: Box<dyn Type>, size: Box<dyn Expression>) -> Self {
        ArrayType {
            element_type,
            size: Some(size),
        }
    }

    /// 获取元素类型（可能是基本类型或嵌套 ArrayType）
    pub fn get_element_type(&self) -> &dyn Type {
        &*self.element_type
    }

    /// 获取当前维度大小表达式
    pub fn get_size(&self) -> Option<&dyn Expression> {
        self.size.as_deref().map(|e| e.as_expression())
    }

    /// 获取基础类型（剥掉所有[]后的最内层类型）
    pub fn get_base_type(&self) -> &dyn Type {
        let mut current: &dyn Type = &*self.element_type;
        while let Some(arr) = current.as_type_any().downcast_ref::<ArrayType>() {
            current = arr.get_element_type();
        }
        current
    }

    /// 获取基础类型名称
    pub fn get_base_type_name(&self) -> &str {
        self.get_base_type().get_name()
    }

    /// 获取数组维度（[]的个数）
    pub fn get_dimension(&self) -> usize {
        let mut dim = 1;
        let mut current: &dyn Type = &*self.element_type;
        while let Some(arr) = current.as_type_any().downcast_ref::<ArrayType>() {
            dim += 1;
            current = arr.get_element_type();
        }
        dim
    }

    /// 判断是否是多维数组
    pub fn is_multi_dimensional(&self) -> bool {
        self.element_type.as_type_any().downcast_ref::<ArrayType>().is_some()
    }

    /// 获取完整类型名（如 "int[][]"）
    pub fn get_full_name(&self) -> String {
        let base_name = self.get_base_type_name();
        let dim = self.get_dimension();
        format!("{}{}", base_name, "[]".repeat(dim))
    }
}

impl AstNode for ArrayType {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_array_type(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Type for ArrayType {
    fn get_name(&self) -> &str {
        // 对于数组类型，返回完整类型名不方便，使用 get_full_name
        // 但为了 trait 兼容，返回基础类型名
        self.get_base_type_name()
    }
    fn as_type(&self) -> &dyn Type {
        self
    }
    fn as_type_any(&self) -> &dyn Any {
        self
    }
}

// ==================== NullableType ====================

pub struct NullableType {
    pub inner_type: Box<dyn Type>,
}

impl NullableType {
    pub fn new(inner_type: Box<dyn Type>) -> Self {
        NullableType { inner_type }
    }

    pub fn get_inner_type(&self) -> &dyn Type {
        &*self.inner_type
    }
}

impl AstNode for NullableType {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_basic_type(self.inner_type.as_any().downcast_ref::<BasicType>().unwrap_or_else(|| {
            panic!("NullableType inner must be BasicType");
        }));
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Type for NullableType {
    fn get_name(&self) -> &str {
        self.inner_type.get_name()
    }
    fn as_type(&self) -> &dyn Type {
        self
    }
    fn as_type_any(&self) -> &dyn Any {
        self
    }
}

// ==================== GenericType ====================

pub struct GenericType {
    pub base_name: String,
    pub type_args: Vec<Box<dyn Type>>,
}

impl GenericType {
    pub fn new(base_name: impl Into<String>, type_args: Vec<Box<dyn Type>>) -> Self {
        GenericType {
            base_name: base_name.into(),
            type_args,
        }
    }
    pub fn get_base_name(&self) -> &str { &self.base_name }
    pub fn get_type_args(&self) -> &Vec<Box<dyn Type>> { &self.type_args }
}

impl AstNode for GenericType {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_basic_type(self.type_args.first().map(|t| t.as_any().downcast_ref::<BasicType>().unwrap_or_else(|| panic!("expected BasicType"))).unwrap_or_else(|| panic!("expected type arg")));
    }
    fn as_any(&self) -> &dyn Any { self }
}

impl Type for GenericType {
    fn get_name(&self) -> &str { &self.base_name }
    fn as_type(&self) -> &dyn Type { self }
    fn as_type_any(&self) -> &dyn Any { self }
}

// ==================== Program ====================

pub struct Program {
    statements: Vec<Box<dyn Statement>>,
}

impl Program {
    pub fn new() -> Self {
        Program {
            statements: Vec::new(),
        }
    }

    pub fn add_statement(&mut self, stmt: Box<dyn Statement>) {
        self.statements.push(stmt);
    }

    pub fn get_statements(&self) -> &Vec<Box<dyn Statement>> {
        &self.statements
    }
}

impl AstNode for Program {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_program(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ==================== Block ====================

pub struct Block {
    statements: Vec<Box<dyn Statement>>,
}

impl Block {
    pub fn new() -> Self {
        Block {
            statements: Vec::new(),
        }
    }

    pub fn add_statement(&mut self, stmt: Box<dyn Statement>) {
        self.statements.push(stmt);
    }

    pub fn get_statements(&self) -> &Vec<Box<dyn Statement>> {
        &self.statements
    }
}

impl AstNode for Block {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_block(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for Block {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

impl Expression for Block {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== Parameter ====================

pub struct Parameter {
    name: String,
    r#type: Option<Box<dyn Type>>,
}

impl Parameter {
    pub fn new(name: impl Into<String>, r#type: Option<Box<dyn Type>>) -> Self {
        Parameter {
            name: name.into(),
            r#type,
        }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_type(&self) -> Option<&dyn Type> {
        self.r#type.as_deref()
    }
}

impl AstNode for Parameter {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_parameter(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ==================== Function ====================

pub struct Function {
    name: String,
    parameters: Option<Vec<Box<Parameter>>>,
    return_type: Option<Box<dyn Type>>,
    body: Option<Box<Block>>,
}

impl Function {
    pub fn new(
        name: impl Into<String>,
        parameters: Option<Vec<Box<Parameter>>>,
        return_type: Option<Box<dyn Type>>,
        body: Option<Box<Block>>,
    ) -> Self {
        Function {
            name: name.into(),
            parameters,
            return_type,
            body,
        }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_parameters(&self) -> Option<&Vec<Box<Parameter>>> {
        self.parameters.as_ref()
    }

    pub fn get_return_type(&self) -> Option<&dyn Type> {
        self.return_type.as_deref()
    }

    pub fn get_body(&self) -> Option<&Block> {
        self.body.as_deref()
    }
}

impl AstNode for Function {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_function(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for Function {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== ImportStatement ====================

pub struct ImportStatement {
    path: Vec<String>,
    alias: Option<String>,
}

impl ImportStatement {
    pub fn new(path: Vec<String>, alias: Option<String>) -> Self {
        ImportStatement { path, alias }
    }

    pub fn get_path(&self) -> &Vec<String> {
        &self.path
    }

    pub fn get_alias(&self) -> Option<&str> {
        self.alias.as_deref()
    }

    pub fn get_module_name(&self) -> String {
        self.path.join(".")
    }
}

impl AstNode for ImportStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_import_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for ImportStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== ExportStatement ====================

pub struct ExportStatement {
    names: Vec<String>,
}

impl ExportStatement {
    pub fn new(names: Vec<String>) -> Self {
        ExportStatement { names }
    }

    pub fn get_names(&self) -> &Vec<String> {
        &self.names
    }
}

impl AstNode for ExportStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_export_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for ExportStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== StructDefinition ====================

pub struct StructField {
    pub name: String,
    pub field_type: Option<Box<dyn Type>>,
}

pub struct StructDefinition {
    name: String,
    fields: Vec<StructField>,
    generic_params: Vec<String>,
}

impl StructDefinition {
    pub fn new(name: impl Into<String>, fields: Vec<StructField>, generic_params: Vec<String>) -> Self {
        StructDefinition {
            name: name.into(),
            fields,
            generic_params,
        }
    }

    pub fn get_name(&self) -> &str { &self.name }
    pub fn get_fields(&self) -> &Vec<StructField> { &self.fields }
    pub fn get_generic_params(&self) -> &Vec<String> { &self.generic_params }
}

impl AstNode for StructDefinition {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_struct_definition(self);
    }
    fn as_any(&self) -> &dyn Any { self }
}

impl Statement for StructDefinition {
    fn as_statement(&self) -> &dyn Statement { self }
}

// ==================== ImplBlock ====================

pub enum ImplItem {
    Constructor(Box<Function>),
    Method(Box<Function>),
    Convert(Box<Function>),
}

pub struct ImplBlock {
    struct_name: String,
    generic_params: Vec<String>,
    items: Vec<ImplItem>,
}

impl ImplBlock {
    pub fn new(struct_name: impl Into<String>, generic_params: Vec<String>, items: Vec<ImplItem>) -> Self {
        ImplBlock {
            struct_name: struct_name.into(),
            generic_params,
            items,
        }
    }
    pub fn get_struct_name(&self) -> &str { &self.struct_name }
    pub fn get_generic_params(&self) -> &Vec<String> { &self.generic_params }
    pub fn get_items(&self) -> &Vec<ImplItem> { &self.items }
}

impl AstNode for ImplBlock {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_impl_block(self);
    }
    fn as_any(&self) -> &dyn Any { self }
}

impl Statement for ImplBlock {
    fn as_statement(&self) -> &dyn Statement { self }
}

// ==================== IfStatement ====================

pub struct IfStatement {
    condition: Option<Box<dyn Expression>>,
    then_branch: Option<Box<dyn Statement>>,
    else_branch: Option<Box<dyn Statement>>,
}

impl IfStatement {
    pub fn new(
        condition: Option<Box<dyn Expression>>,
        then_branch: Option<Box<dyn Statement>>,
        else_branch: Option<Box<dyn Statement>>,
    ) -> Self {
        IfStatement {
            condition,
            then_branch,
            else_branch,
        }
    }

    pub fn get_condition(&self) -> Option<&dyn Expression> {
        self.condition.as_deref().map(|e| e.as_expression())
    }

    pub fn get_then_branch(&self) -> Option<&dyn Statement> {
        self.then_branch.as_deref().map(|s| s.as_statement())
    }

    pub fn get_else_branch(&self) -> Option<&dyn Statement> {
        self.else_branch.as_deref().map(|s| s.as_statement())
    }
}

impl AstNode for IfStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_if_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for IfStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

impl Expression for IfStatement {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== WhileStatement ====================

pub struct WhileStatement {
    condition: Option<Box<dyn Expression>>,
    body: Option<Box<dyn Statement>>,
}

impl WhileStatement {
    pub fn new(condition: Option<Box<dyn Expression>>, body: Option<Box<dyn Statement>>) -> Self {
        WhileStatement { condition, body }
    }

    pub fn get_condition(&self) -> Option<&dyn Expression> {
        self.condition.as_deref().map(|e| e.as_expression())
    }

    pub fn get_body(&self) -> Option<&dyn Statement> {
        self.body.as_deref().map(|s| s.as_statement())
    }
}

impl AstNode for WhileStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_while_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for WhileStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== ForStatement ====================

pub struct ForStatement {
    loop_variables: Vec<String>,
    iterable: Option<Box<dyn Expression>>,
    body: Option<Box<Block>>,
}

impl ForStatement {
    pub fn new(
        loop_variable: impl Into<String>,
        iterable: Option<Box<dyn Expression>>,
        body: Option<Box<Block>>,
    ) -> Self {
        ForStatement {
            loop_variables: vec![loop_variable.into()],
            iterable,
            body,
        }
    }

    pub fn new_multi(
        loop_variables: Vec<String>,
        iterable: Option<Box<dyn Expression>>,
        body: Option<Box<Block>>,
    ) -> Self {
        ForStatement {
            loop_variables,
            iterable,
            body,
        }
    }

    pub fn get_loop_variable(&self) -> &str {
        &self.loop_variables[0]
    }

    pub fn get_loop_variables(&self) -> &Vec<String> {
        &self.loop_variables
    }

    pub fn get_iterable(&self) -> Option<&dyn Expression> {
        self.iterable.as_deref().map(|e| e.as_expression())
    }

    pub fn get_body(&self) -> Option<&Block> {
        self.body.as_deref()
    }
}

impl AstNode for ForStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_for_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for ForStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== ReturnStatement ====================

pub struct ReturnStatement {
    value: Option<Box<dyn Expression>>,
}

impl ReturnStatement {
    pub fn new(value: Option<Box<dyn Expression>>) -> Self {
        ReturnStatement { value }
    }

    pub fn get_value(&self) -> Option<&dyn Expression> {
        self.value.as_deref().map(|e| e.as_expression())
    }
}

impl AstNode for ReturnStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_return_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for ReturnStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== BreakStatement ====================

pub struct BreakStatement;

impl BreakStatement {
    pub fn new() -> Self {
        BreakStatement
    }
}

impl AstNode for BreakStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_break_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for BreakStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== ContinueStatement ====================

pub struct ContinueStatement;

impl ContinueStatement {
    pub fn new() -> Self {
        ContinueStatement
    }
}

impl AstNode for ContinueStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_continue_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for ContinueStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== Declaration ====================

pub struct Declaration {
    keyword: String,
    name: String,
    r#type: Option<Box<dyn Type>>,
    initializer: Option<Box<dyn Expression>>,
}

impl Declaration {
    pub fn new(
        keyword: impl Into<String>,
        name: impl Into<String>,
        r#type: Option<Box<dyn Type>>,
        initializer: Option<Box<dyn Expression>>,
    ) -> Self {
        Declaration {
            keyword: keyword.into(),
            name: name.into(),
            r#type,
            initializer,
        }
    }

    pub fn get_keyword(&self) -> &str {
        &self.keyword
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_type(&self) -> Option<&dyn Type> {
        self.r#type.as_deref()
    }

    pub fn get_initializer(&self) -> Option<&dyn Expression> {
        self.initializer.as_deref().map(|e| e.as_expression())
    }
}

impl AstNode for Declaration {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_declaration(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for Declaration {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== ExpressionStatement ====================

pub struct ExpressionStatement {
    expression: Option<Box<dyn Expression>>,
}

impl ExpressionStatement {
    pub fn new(expression: Option<Box<dyn Expression>>) -> Self {
        ExpressionStatement { expression }
    }

    pub fn get_expression(&self) -> Option<&dyn Expression> {
        self.expression.as_deref().map(|e| e.as_expression())
    }
}

impl AstNode for ExpressionStatement {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_expression_statement(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for ExpressionStatement {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

// ==================== BinaryExpression ====================

pub struct BinaryExpression {
    left: Option<Box<dyn Expression>>,
    op: String,
    right: Option<Box<dyn Expression>>,
}

impl BinaryExpression {
    pub fn new(
        left: Option<Box<dyn Expression>>,
        op: impl Into<String>,
        right: Option<Box<dyn Expression>>,
    ) -> Self {
        BinaryExpression {
            left,
            op: op.into(),
            right,
        }
    }

    pub fn get_left(&self) -> Option<&dyn Expression> {
        self.left.as_deref().map(|e| e.as_expression())
    }

    pub fn get_operator(&self) -> &str {
        &self.op
    }

    pub fn get_right(&self) -> Option<&dyn Expression> {
        self.right.as_deref().map(|e| e.as_expression())
    }
}

impl AstNode for BinaryExpression {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_binary_expression(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for BinaryExpression {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== UnaryExpression ====================

pub struct UnaryExpression {
    op: String,
    operand: Option<Box<dyn Expression>>,
}

impl UnaryExpression {
    pub fn new(op: impl Into<String>, operand: Option<Box<dyn Expression>>) -> Self {
        UnaryExpression {
            op: op.into(),
            operand,
        }
    }

    pub fn get_operator(&self) -> &str {
        &self.op
    }

    pub fn get_operand(&self) -> Option<&dyn Expression> {
        self.operand.as_deref().map(|e| e.as_expression())
    }
}

impl AstNode for UnaryExpression {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_unary_expression(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for UnaryExpression {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== CastExpression ====================

pub struct CastExpression {
    expression: Option<Box<dyn Expression>>,
    target_type: Box<dyn Type>,
}

impl CastExpression {
    pub fn new(expression: Option<Box<dyn Expression>>, target_type: Box<dyn Type>) -> Self {
        CastExpression {
            expression,
            target_type,
        }
    }

    pub fn get_expression(&self) -> Option<&dyn Expression> {
        self.expression.as_deref().map(|e| e.as_expression())
    }

    pub fn get_target_type(&self) -> &dyn Type {
        &*self.target_type
    }
}

impl AstNode for CastExpression {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_cast_expression(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for CastExpression {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== FunctionCall ====================

pub struct FunctionCall {
    callee: Option<Box<dyn Expression>>,
    arguments: Option<Vec<Box<dyn Expression>>>,
}

impl FunctionCall {
    pub fn new(
        callee: Option<Box<dyn Expression>>,
        arguments: Option<Vec<Box<dyn Expression>>>,
    ) -> Self {
        FunctionCall { callee, arguments }
    }

    pub fn get_callee(&self) -> Option<&dyn Expression> {
        self.callee.as_deref().map(|e| e.as_expression())
    }

    pub fn get_arguments(&self) -> Option<&Vec<Box<dyn Expression>>> {
        self.arguments.as_ref()
    }
}

impl AstNode for FunctionCall {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_function_call(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for FunctionCall {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== MemberAccess ====================

pub struct MemberAccess {
    object: Option<Box<dyn Expression>>,
    member: String,
}

impl MemberAccess {
    pub fn new(object: Option<Box<dyn Expression>>, member: impl Into<String>) -> Self {
        MemberAccess {
            object,
            member: member.into(),
        }
    }

    pub fn get_object(&self) -> Option<&dyn Expression> {
        self.object.as_deref().map(|e| e.as_expression())
    }

    pub fn get_member(&self) -> &str {
        &self.member
    }
}

impl AstNode for MemberAccess {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_member_access(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for MemberAccess {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== ArrayIndex ====================

pub struct ArrayIndex {
    array: Option<Box<dyn Expression>>,
    index: Option<Box<dyn Expression>>,
}

impl ArrayIndex {
    pub fn new(array: Option<Box<dyn Expression>>, index: Option<Box<dyn Expression>>) -> Self {
        ArrayIndex { array, index }
    }

    pub fn get_array(&self) -> Option<&dyn Expression> {
        self.array.as_deref().map(|e| e.as_expression())
    }

    pub fn get_index(&self) -> Option<&dyn Expression> {
        self.index.as_deref().map(|e| e.as_expression())
    }
}

impl AstNode for ArrayIndex {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_array_index(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for ArrayIndex {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== GroupedExpression ====================

pub struct GroupedExpression {
    expression: Option<Box<dyn Expression>>,
}

impl GroupedExpression {
    pub fn new(expression: Option<Box<dyn Expression>>) -> Self {
        GroupedExpression { expression }
    }

    pub fn get_expression(&self) -> Option<&dyn Expression> {
        self.expression.as_deref().map(|e| e.as_expression())
    }
}

impl AstNode for GroupedExpression {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_grouped_expression(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for GroupedExpression {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== Identifier ====================

pub struct Identifier {
    name: String,
}

impl Identifier {
    pub fn new(name: impl Into<String>) -> Self {
        Identifier { name: name.into() }
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }
}

impl AstNode for Identifier {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_identifier(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for Identifier {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== NumberLiteral ====================

pub struct NumberLiteral {
    value: f64,
}

impl NumberLiteral {
    pub fn new(value: f64) -> Self {
        NumberLiteral { value }
    }

    pub fn get_value(&self) -> f64 {
        self.value
    }
}

impl AstNode for NumberLiteral {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_number_literal(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for NumberLiteral {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== StringLiteral ====================

pub struct StringLiteral {
    value: String,
}

impl StringLiteral {
    pub fn new(value: impl Into<String>) -> Self {
        let value: String = value.into();
        let mut res = String::new();
        let chars: Vec<char> = value.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                match chars[i + 1] {
                    'n' => {
                        res.push('\n');
                        i += 1;
                    }
                    't' => {
                        res.push('\t');
                        i += 1;
                    }
                    '\\' => {
                        res.push('\\');
                        i += 1;
                    }
                    '"' => {
                        res.push('"');
                        i += 1;
                    }
                    _ => res.push(chars[i]),
                }
            } else {
                res.push(chars[i]);
            }
            i += 1;
        }
        StringLiteral { value: res }
    }

    pub fn get_value(&self) -> &str {
        &self.value
    }
}

impl AstNode for StringLiteral {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_string_literal(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for StringLiteral {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== BooleanLiteral ====================

pub struct BooleanLiteral {
    value: bool,
}

impl BooleanLiteral {
    pub fn new(value: bool) -> Self {
        BooleanLiteral { value }
    }

    pub fn get_value(&self) -> bool {
        self.value
    }
}

impl AstNode for BooleanLiteral {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_boolean_literal(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for BooleanLiteral {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== NullLiteral ====================

pub struct NullLiteral;

impl NullLiteral {
    pub fn new() -> Self {
        NullLiteral
    }
}

impl AstNode for NullLiteral {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_null_literal(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for NullLiteral {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== FormatString ====================

pub struct VariablePosition {
    pub pos_in_value: i32,
    pub value: Option<Box<dyn Expression>>,
}

impl Clone for VariablePosition {
    fn clone(&self) -> Self {
        VariablePosition {
            pos_in_value: self.pos_in_value,
            value: None, // Can't clone dyn Expression
        }
    }
}

pub struct FormatString {
    value: String,
    variables: Vec<VariablePosition>,
}

impl FormatString {
    pub fn new(value: impl Into<String>) -> Self {
        let value: String = value.into();
        let mut variables = Vec::new();
        let mut var_name = String::new();
        let mut in_brace = false;
        let mut start_pos: i32 = 0;

        for (i, c) in value.chars().enumerate() {
            if c == '{' && !in_brace {
                in_brace = true;
                var_name.clear();
                start_pos = i as i32;
            } else if c == '}' && in_brace {
                in_brace = false;
                if !var_name.is_empty() {
                    let expr = FormatString::parse_value(&var_name);
                    if let Some(e) = expr {
                        variables.push(VariablePosition {
                            pos_in_value: start_pos,
                            value: Some(e),
                        });
                    }
                }
            } else if in_brace {
                var_name.push(c);
            }
        }

        let mut res = String::new();
        let chars: Vec<char> = value.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' && i + 1 < chars.len() {
                match chars[i + 1] {
                    'n' => {
                        res.push('\n');
                        i += 1;
                    }
                    't' => {
                        res.push('\t');
                        i += 1;
                    }
                    '\\' => {
                        res.push('\\');
                        i += 1;
                    }
                    '"' => {
                        res.push('"');
                        i += 1;
                    }
                    _ => res.push(chars[i]),
                }
            } else {
                res.push(chars[i]);
            }
            i += 1;
        }

        FormatString {
            value: res,
            variables,
        }
    }

    pub fn get_value(&self) -> &str {
        &self.value
    }

    pub fn get_variables(&self) -> &Vec<VariablePosition> {
        &self.variables
    }

    fn parse_value(var_name: &str) -> Option<Box<dyn Expression>> {
        if var_name.is_empty() {
            return None;
        }
        // Handle unary operators (-, !, +)
        let first = var_name.chars().next().unwrap();
        if first == '-' || first == '!' || first == '+' {
            let op: String = first.into();
            let rest = var_name[first.len_utf8()..].trim();
            if rest.is_empty() {
                return None;
            }
            if let Some(operand) = Self::parse_value(rest) {
                return Some(Box::new(UnaryExpression::new(op, Some(operand))));
            }
            return None;
        }
        if let Some(lit) = Self::try_parse_literal(var_name) {
            return Some(lit);
        }
        Self::parse_expression(var_name)
    }

    fn try_parse_literal(s: &str) -> Option<Box<dyn Expression>> {
        let mut is_number = true;
        let mut has_dot = false;
        for c in s.chars() {
            if c == '.' {
                if has_dot {
                    is_number = false;
                    break;
                }
                has_dot = true;
            } else if !c.is_ascii_digit() {
                is_number = false;
                break;
            }
        }
        if is_number && !s.is_empty() {
            if let Ok(val) = s.parse::<f64>() {
                return Some(Box::new(NumberLiteral::new(val)));
            }
        }
        if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
            let content = &s[1..s.len() - 1];
            return Some(Box::new(StringLiteral::new(content)));
        }
        if s == "true" {
            return Some(Box::new(BooleanLiteral::new(true)));
        }
        if s == "false" {
            return Some(Box::new(BooleanLiteral::new(false)));
        }
        None
    }

    fn parse_expression(expr: &str) -> Option<Box<dyn Expression>> {
        let expr = expr.trim();

        // 处理 new 表达式: new Type(args)
        if expr.starts_with("new ") {
            return Self::parse_new_in_format(&expr[4..]);
        }

        // 处理函数调用: callee(args)
        if let Some((open_idx, close_idx)) = Self::find_matching_parens(expr) {
            if close_idx == expr.len() - 1 {
                let callee_str = expr[..open_idx].trim();
                let args_str = &expr[open_idx + 1..close_idx];

                let callee = if callee_str.is_empty() {
                    return None;
                } else {
                    Self::parse_expression(callee_str)?
                };

                let args = Self::parse_arg_list(args_str);
                return Some(Box::new(FunctionCall::new(Some(callee), args)));
            }
        }

        // 处理数组索引
        if let Some(last_bracket) = expr.rfind('[') {
            if let Some(closing_bracket) = expr[last_bracket..].find(']') {
                let closing_pos = last_bracket + closing_bracket;
                if closing_pos == expr.len() - 1 {
                    let array_part = &expr[..last_bracket];
                    let index_part = &expr[last_bracket + 1..closing_pos];
                    let array = Self::parse_expression(array_part);
                    let index = Self::parse_value(index_part);
                    if let (Some(a), Some(i)) = (array, index) {
                        return Some(Box::new(ArrayIndex::new(Some(a), Some(i))));
                    }
                    return None;
                }
            }
        }
        // 处理成员访问
        if let Some(last_dot) = expr.rfind('.') {
            let object_part = &expr[..last_dot];
            let member_part = &expr[last_dot + 1..];
            let valid_member = member_part
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_');
            if valid_member {
                let object = Self::parse_expression(object_part);
                if let Some(o) = object {
                    return Some(Box::new(MemberAccess::new(Some(o), member_part)));
                }
                return None;
            }
        }
        // 处理类型转换: expr as Type
        if let Some(as_pos) = expr.rfind(" as ") {
            let lhs = &expr[..as_pos];
            let type_name = expr[as_pos + 4..].trim();
            if !type_name.is_empty()
                && type_name.chars().all(|c| c.is_alphanumeric() || c == '_')
            {
                if let Some(lhs_expr) = Self::parse_expression(lhs) {
                    let tp: Box<dyn Type> = Box::new(BasicType::new(type_name));
                    return Some(Box::new(CastExpression::new(Some(lhs_expr), tp)));
                }
            }
        }
        // 标识符
        let valid_identifier = !expr.is_empty()
            && (expr.chars().next().unwrap().is_alphabetic() || expr.starts_with('_'))
            && expr.chars().all(|c| c.is_alphanumeric() || c == '_');
        if valid_identifier {
            return Some(Box::new(Identifier::new(expr)));
        }
        None
    }

    fn find_matching_parens(s: &str) -> Option<(usize, usize)> {
        let open = s.find('(')?;
        let mut depth = 0;
        for (i, c) in s[open..].char_indices() {
            if c == '(' { depth += 1; }
            else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    return Some((open, open + i));
                }
            }
        }
        None
    }

    fn parse_arg_list(s: &str) -> Option<Vec<Box<dyn Expression>>> {
        if s.trim().is_empty() {
            return Some(Vec::new());
        }
        let mut args = Vec::new();
        let mut depth = 0;
        let mut start = 0;

        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    let arg = s[start..i].trim();
                    if !arg.is_empty() {
                        if let Some(expr) = Self::parse_value(arg) {
                            args.push(expr);
                        }
                    }
                    start = i + 1;
                }
                _ => {}
            }
        }

        let arg = s[start..].trim();
        if !arg.is_empty() {
            if let Some(expr) = Self::parse_value(arg) {
                args.push(expr);
            }
        }

        Some(args)
    }

    fn parse_new_in_format(rest: &str) -> Option<Box<dyn Expression>> {
        // "Point(1, 2)" or "Point"
        let rest = rest.trim();
        if let Some((open_idx, close_idx)) = Self::find_matching_parens(rest) {
            if close_idx == rest.len() - 1 {
                let type_name = rest[..open_idx].trim();
                if type_name.is_empty()
                    || !type_name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '_')
                {
                    return None;
                }
                let args_str = &rest[open_idx + 1..close_idx];
                let args = Self::parse_arg_list(args_str);
                return Some(Box::new(FunctionCall::new(
                    Some(Box::new(Identifier::new(type_name))),
                    args,
                )));
            }
        }
        // new Type (no args)
        let valid_identifier = !rest.is_empty()
            && (rest.chars().next().unwrap().is_alphabetic() || rest.starts_with('_'))
            && rest.chars().all(|c| c.is_alphanumeric() || c == '_');
        if valid_identifier {
            return Some(Box::new(FunctionCall::new(
                Some(Box::new(Identifier::new(rest))),
                Some(Vec::new()),
            )));
        }
        None
    }
}

impl AstNode for FormatString {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_format_string(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for FormatString {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== RangeExpression ====================

pub struct RangeExpression {
    arguments: Vec<Box<dyn Expression>>,
}

impl RangeExpression {
    pub fn new(arguments: Vec<Box<dyn Expression>>) -> Self {
        RangeExpression { arguments }
    }

    pub fn get_arguments(&self) -> &Vec<Box<dyn Expression>> {
        &self.arguments
    }
}

impl AstNode for RangeExpression {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_range_expression(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for RangeExpression {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== ArrayLiteral ====================

pub struct ArrayLiteral {
    elements: Vec<Box<dyn Expression>>,
}

impl ArrayLiteral {
    pub fn new(elements: Vec<Box<dyn Expression>>) -> Self {
        ArrayLiteral { elements }
    }

    pub fn get_elements(&self) -> &Vec<Box<dyn Expression>> {
        &self.elements
    }
}

impl AstNode for ArrayLiteral {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_array_literal(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for ArrayLiteral {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== StructFieldInit ====================

pub enum StructFieldInit {
    /// Named field: `x: 10`
    Named { name: String, value: Box<dyn Expression> },
    /// Positional field: `10` (matched to field by position)
    Positional(Box<dyn Expression>),
}

// ==================== StructLiteral ====================

pub struct StructLiteral {
    type_name: String,
    fields: Vec<StructFieldInit>,
}

impl StructLiteral {
    pub fn new(type_name: impl Into<String>, fields: Vec<StructFieldInit>) -> Self {
        StructLiteral {
            type_name: type_name.into(),
            fields,
        }
    }

    pub fn get_type_name(&self) -> &str {
        &self.type_name
    }

    pub fn get_fields(&self) -> &Vec<StructFieldInit> {
        &self.fields
    }
}

impl AstNode for StructLiteral {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_struct_literal(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Expression for StructLiteral {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}

// ==================== MatchPattern ====================

pub enum MatchPattern {
    Literal(RtValueSimple),
    Wildcard,
    Variable(String),
}

#[derive(Debug, Clone)]
pub enum RtValueSimple {
    Int(i64),
    FloatStr(String),
    Str(String),
    Bool(bool),
}

// ==================== MatchArm ====================

pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: Option<Box<dyn Statement>>,
}

// ==================== MatchExpression ====================

pub struct MatchExpression {
    scrutinee: Option<Box<dyn Expression>>,
    arms: Vec<MatchArm>,
}

impl MatchExpression {
    pub fn new(scrutinee: Option<Box<dyn Expression>>, arms: Vec<MatchArm>) -> Self {
        MatchExpression { scrutinee, arms }
    }

    pub fn get_scrutinee(&self) -> Option<&dyn Expression> {
        self.scrutinee.as_deref().map(|e| e.as_expression())
    }

    pub fn get_arms(&self) -> &Vec<MatchArm> {
        &self.arms
    }
}

impl AstNode for MatchExpression {
    fn accept(&self, visitor: &mut dyn AstVisitor) {
        visitor.visit_match_expression(self);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Statement for MatchExpression {
    fn as_statement(&self) -> &dyn Statement {
        self
    }
}

impl Expression for MatchExpression {
    fn as_expression(&self) -> &dyn Expression {
        self
    }
}
