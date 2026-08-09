//! paxc — a compiler for the pax DSL that emits Power Automate cloud flow definitions.
//!
//! This is the library crate. The `paxc` and `paxr` binaries live in
//! `src/bin/`.

pub mod ast;
pub mod check;
pub mod cli;
pub mod diagnostic;
pub mod interpreter;
pub mod lexer;
pub mod pa;
pub mod parser;
pub mod resolver;
pub mod skill;
