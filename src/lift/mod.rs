//! M-Lift — the **inverse** of `transpile`: read PHP, emit a Phorge **draft** (`phg lift foo.php`).
//!
//! Deliberately named `lift`, NOT "transpile": the downward direction (Phorge→PHP) is total and
//! byte-identity-verified; the upward direction is **best-effort, review-required** — PHP is larger
//! and dynamic, Phorge is smaller and typed, so the map is partial by nature (see the plan's
//! verdict). A lift is a scaffold a human checks, annotated `// lifted (verify)`; anything dynamic
//! (`$$x`, `eval`, magic methods) is refused loudly (`// CANNOT LIFT: <reason>`), never guessed.
//!
//! Build order (demo angle first): **L1 PHP lexer** → L2 Tier-1 parser → L3 Phorge pretty-printer →
//! L4 lifter → L6 `phg lift` CLI + playground "paste PHP → see Phorge". This file is the module root;
//! L1 lives in [`lexer`].

pub mod lexer;

#[cfg(test)]
mod tests;
