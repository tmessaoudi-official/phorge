//! `Core.Crypto` — password hashing (Argon2id). The FIRST module backed by an external crate
//! (RustCrypto `argon2`), admitted under `docs/specs/2026-06-27-dependency-policy.md` (the
//! audited-crypto-only exception to `std`-only). Rationale: secure password hashing demands a *vetted*
//! implementation ("never roll your own crypto"), `std` ships none, and the capability must be NATIVE
//! to Phorge's Rust backends — never delegated to the PHP transpile target. The `php` closures emit
//! `password_hash`/`password_verify` as a **peer** target; because both sides speak the standard PHC
//! string (`$argon2id$…`), a hash made by either backend verifies in the other.
//!
//! These natives are **`pure: false`** where non-deterministic: `hashPassword` uses a random salt, so
//! it is quarantined from the byte-identity oracle (tested in `tests/crypto.rs`). `verifyPassword` is
//! deterministic for a fixed `(password, hash)` pair, so it CAN appear in a byte-identity-gated
//! example (against a committed PHC hash).

use super::*;
use crate::types::Ty;
use crate::value::Value;
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

/// `Crypto.hashPassword(string) -> string` — Argon2id over a fresh random salt; returns the standard
/// PHC string. Non-deterministic (`pure: false`). PHP: `password_hash($pw, PASSWORD_ARGON2ID)`.
fn crypto_hash_password(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(pw)] => {
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default()
                .hash_password(pw.as_bytes(), &salt)
                .map_err(|e| format!("password hashing failed: {e}"))?
                .to_string();
            Ok(Value::Str(hash))
        }
        _ => Err("Crypto.hashPassword expects (string)".into()),
    }
}

/// `Crypto.verifyPassword(string password, string hash) -> bool` — constant-time verify against a PHC
/// hash. A malformed hash string is `false` (mirrors PHP `password_verify`), never a fault.
/// Deterministic. PHP: `password_verify($pw, $hash)`.
fn crypto_verify_password(args: &[Value], _: &mut String) -> Result<Value, String> {
    match args {
        [Value::Str(pw), Value::Str(hash)] => {
            let parsed = match PasswordHash::new(hash) {
                Ok(p) => p,
                Err(_) => return Ok(Value::Bool(false)),
            };
            Ok(Value::Bool(
                Argon2::default()
                    .verify_password(pw.as_bytes(), &parsed)
                    .is_ok(),
            ))
        }
        _ => Err("Crypto.verifyPassword expects (string, string)".into()),
    }
}

pub(crate) fn crypto_natives() -> Vec<NativeFn> {
    vec![
        NativeFn {
            module: "Core.Crypto",
            name: "hashPassword",
            params: vec![Ty::String],
            ret: Ty::String,
            pure: false, // random salt → quarantined from the oracle
            eval: NativeEval::Pure(crypto_hash_password),
            php: |a| format!("password_hash({}, PASSWORD_ARGON2ID)", parg(a, 0)),
        },
        NativeFn {
            module: "Core.Crypto",
            name: "verifyPassword",
            params: vec![Ty::String, Ty::String],
            ret: Ty::Bool,
            pure: true, // deterministic for a fixed (password, hash) → gateable
            eval: NativeEval::Pure(crypto_verify_password),
            php: |a| format!("password_verify({}, {})", parg(a, 0), parg(a, 1)),
        },
    ]
}

#[cfg(test)]
#[path = "crypto_tests.rs"]
mod tests;
