//! Gobol intermediate representation and AST-to-IR lowering.

pub use gobol_common::{ast, environment};

pub mod ir;

#[cfg(test)]
pub use gobol_frontend::{ast_builder, lexer};
