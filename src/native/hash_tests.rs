use super::*;
use crate::value::Value;
use std::rc::Rc;

fn h(f: fn(&[Value], &mut String) -> Result<Value, String>, s: &str) -> String {
    match f(
        &[Value::Bytes(Rc::new(s.as_bytes().to_vec()))],
        &mut String::new(),
    )
    .unwrap()
    {
        Value::Str(t) => t,
        other => panic!("expected string, got {other:?}"),
    }
}

// All reference values captured from real `php -n` (hash("crc32b"/"sha256"), md5, sha1).

#[test]
fn crc32_matches_php() {
    assert_eq!(h(crc32_native, ""), "00000000");
    assert_eq!(h(crc32_native, "hi"), "d8932aac");
    assert_eq!(h(crc32_native, "Hello, Phorge!"), "7e132e35");
    assert_eq!(h(crc32_native, "The quick brown fox"), "b74574de");
}

#[test]
fn md5_matches_php() {
    assert_eq!(h(md5_native, ""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(h(md5_native, "hi"), "49f68a5c8493ec2c0bf489821c21fc3b");
    assert_eq!(
        h(md5_native, "Hello, Phorge!"),
        "294415d42bae8f233fd7b481be8f6da9"
    );
    assert_eq!(
        h(md5_native, "The quick brown fox"),
        "a2004f37730b9445670a738fa0fc9ee5"
    );
}

#[test]
fn sha1_matches_php() {
    assert_eq!(
        h(sha1_native, ""),
        "da39a3ee5e6b4b0d3255bfef95601890afd80709"
    );
    assert_eq!(
        h(sha1_native, "hi"),
        "c22b5f9178342609428d6f51b2c5af4c0bde6a42"
    );
    assert_eq!(
        h(sha1_native, "The quick brown fox"),
        "c519c1a06cdbeb2bc499e22137fb48683858b345"
    );
}

#[test]
fn sha256_matches_php() {
    assert_eq!(
        h(sha256_native, ""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        h(sha256_native, "hi"),
        "8f434346648f6b96df89dda901c5176b10a6d83961dd3c1ac88b59b2dc327aa4"
    );
    assert_eq!(
        h(sha256_native, "Hello, Phorge!"),
        "b75303d703cc4571de11e167d06fb62e1facbb10c596ae192b407ff73407f00c"
    );
}

#[test]
fn digests_handle_multiblock_input() {
    // > 64 bytes exercises the multi-chunk padding path; pinned to php -n.
    let long = "a".repeat(100);
    // php -n: md5(str_repeat('a',100)), sha256(...) — computed below at test authoring time.
    assert_eq!(h(md5_native, &long), "36a92cc94a9e0fa21f625f8bfb007adf");
    assert_eq!(
        h(sha256_native, &long),
        "2816597888e4a0d3a36b82b83316ab32680eb8f00f8cd3b904d681246d285a0e"
    );
}
