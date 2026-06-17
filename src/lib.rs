// No `unsafe` anywhere in Phorge — locked in as a compile-time invariant (M2 P3.5 Wave 0, Task 0.5).
// Also forecloses the computed-goto VM dispatch the roadmap deliberately defers.
#![forbid(unsafe_code)]

pub mod ast;
pub mod bundle;
pub mod checker;
pub mod chunk;
pub mod cli;
pub mod compiler;
pub mod diagnostic;
pub mod interpreter;
pub mod lexer;
pub mod limits;
pub mod mem;
pub mod native;
pub mod parser;
pub mod token;
pub mod transpile;
pub mod types;
pub mod value;
pub mod vm;
