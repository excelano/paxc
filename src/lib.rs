// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

//! paxc — a compiler for the pax DSL that emits Power Automate cloud flow definitions.
//!
//! This crate is a command-line tool. The modules are public so the `paxc` and
//! `paxr` binaries and the integration tests can reach them, and they are hidden
//! from the documentation because they are not a supported Rust API: the crate
//! version follows the pax language and the CLI, and a change to these modules
//! does not move it.

#[doc(hidden)]
pub mod ast;
#[doc(hidden)]
pub mod check;
#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod diagnostic;
#[doc(hidden)]
pub mod interpreter;
#[doc(hidden)]
pub mod lexer;
#[doc(hidden)]
pub mod pa;
#[doc(hidden)]
pub mod parser;
#[doc(hidden)]
pub mod resolver;
#[doc(hidden)]
pub mod skill;
