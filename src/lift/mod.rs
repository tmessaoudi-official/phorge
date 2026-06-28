//! M-Lift — the **inverse** of `transpile`: read PHP, emit a Phorj **draft** (`phg lift foo.php`).
//!
//! Deliberately named `lift`, NOT "transpile": the downward direction (Phorj→PHP) is total and
//! byte-identity-verified; the upward direction is **best-effort, review-required** — PHP is larger
//! and dynamic, Phorj is smaller and typed, so the map is partial by nature (see the plan's
//! verdict). A lift is a scaffold a human checks, annotated `// lifted (verify)`; anything dynamic
//! (`$$x`, `eval`, magic methods) is refused loudly (`// CANNOT LIFT: <reason>`), never guessed.
//!
//! Build order (demo angle first): **L1 PHP lexer** → L2 Tier-1 parser → L3 Phorj pretty-printer →
//! L4 lifter → L6 `phg lift` CLI + playground "paste PHP → see Phorj". This file is the module root;
//! L1 lives in [`lexer`].

pub mod ast;
pub mod lexer;
pub mod lifter;
pub mod parser;
pub mod printer;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod parser_tests;

#[cfg(test)]
mod printer_tests;

#[cfg(test)]
mod lifter_tests;
