// No `unsafe` anywhere in Phorj — locked in as a compile-time invariant (M2 P3.5 Wave 0, Task 0.5).
// Also forecloses the computed-goto VM dispatch the roadmap deliberately defers.
#![forbid(unsafe_code)]

pub mod ast;
pub mod bundle;
pub mod checker;
pub mod chunk;
pub mod cli;
pub mod compiler;
pub mod diagnostic;
pub mod dispatch;
pub mod fmt;
pub mod interpreter;
pub mod lexer;
pub mod lift;
pub mod limits;
pub mod loader;
pub mod lock;
pub mod lsp;
pub mod manifest;
pub mod mem;
pub mod native;
pub mod parser;
pub mod serve;
pub mod token;
pub mod transpile;
pub mod types;
pub mod value;
pub mod vendor;
pub mod vm;
