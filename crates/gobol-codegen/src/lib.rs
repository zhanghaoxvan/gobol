//! Cranelift code generation and native toolchain discovery.

pub use gobol_common::environment;
pub use gobol_ir::ir;

pub mod cranelift;
pub mod toolchain;

#[cfg(test)]
pub use gobol_frontend::{ast_builder, lexer};
