use super::*;
use crate::value::Value;

#[test]
fn hash_then_verify_roundtrip() {
    let mut o = String::new();
    let h = crypto_hash_password(&[Value::Str("correct horse".into())], &mut o).unwrap();
    let hash = match h {
        Value::Str(s) => s,
        other => panic!("expected a hash string, got {other:?}"),
    };
    // Standard PHC Argon2id string → interoperates with PHP password_verify.
    assert!(hash.starts_with("$argon2id$"), "got {hash}");
    // Right password verifies; wrong password does not.
    assert!(matches!(
        crypto_verify_password(
            &[Value::Str("correct horse".into()), Value::Str(hash.clone())],
            &mut o
        ),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        crypto_verify_password(&[Value::Str("wrong".into()), Value::Str(hash)], &mut o),
        Ok(Value::Bool(false))
    ));
}

#[test]
fn hashing_is_salted_nondeterministic() {
    let mut o = String::new();
    let s = |v: Value| match v {
        Value::Str(s) => s,
        other => panic!("expected a hash string, got {other:?}"),
    };
    let a = s(crypto_hash_password(&[Value::Str("same".into())], &mut o).unwrap());
    let b = s(crypto_hash_password(&[Value::Str("same".into())], &mut o).unwrap());
    assert_ne!(
        a, b,
        "a random salt must make two hashes of the same password differ"
    );
}

#[test]
fn verify_on_malformed_hash_is_false_not_a_fault() {
    let mut o = String::new();
    assert!(matches!(
        crypto_verify_password(
            &[
                Value::Str("pw".into()),
                Value::Str("not-a-phc-string".into())
            ],
            &mut o
        ),
        Ok(Value::Bool(false))
    ));
}

#[test]
fn verify_a_committed_php_argon2id_hash() {
    // A hash produced by PHP 8.5 `password_hash("secret", PASSWORD_ARGON2ID)` — proves Rust↔PHP PHC
    // interop (verify uses the params embedded in the string, not backend defaults).
    let mut o = String::new();
    let php_hash = "$argon2id$v=19$m=65536,t=4,p=1$WkZrSkZWejVrTDNocXhHVA$RbutuicM/97zsxyuasx1kZHZ5Ja45k0iDJ9YJ6LV/iY";
    assert!(matches!(
        crypto_verify_password(
            &[Value::Str("secret".into()), Value::Str(php_hash.into())],
            &mut o
        ),
        Ok(Value::Bool(true))
    ));
    assert!(matches!(
        crypto_verify_password(
            &[Value::Str("nope".into()), Value::Str(php_hash.into())],
            &mut o
        ),
        Ok(Value::Bool(false))
    ));
}

#[test]
fn crypto_natives_registered_and_emit() {
    assert!(crate::native::index_of("Core.Cryptography", "hashPassword").is_some());
    assert!(crate::native::index_of("Core.Cryptography", "verifyPassword").is_some());
    let php = |name: &str, args: &[&str]| {
        let i = crate::native::index_of("Core.Cryptography", name).unwrap();
        let a: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        (crate::native::registry()[i].php)(&a)
    };
    assert_eq!(
        php("hashPassword", &["$pw"]),
        "password_hash($pw, PASSWORD_ARGON2ID)"
    );
    assert_eq!(
        php("verifyPassword", &["$pw", "$h"]),
        "password_verify($pw, $h)"
    );
    // hashPassword is non-deterministic (quarantined); verifyPassword is deterministic (gateable).
    let reg = crate::native::registry();
    assert!(!reg[crate::native::index_of("Core.Cryptography", "hashPassword").unwrap()].pure);
    assert!(reg[crate::native::index_of("Core.Cryptography", "verifyPassword").unwrap()].pure);
}
