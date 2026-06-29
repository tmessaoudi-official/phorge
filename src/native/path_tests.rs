use super::*;
use crate::value::Value;

fn s1(f: fn(&[Value], &mut String) -> Result<Value, String>, input: &str) -> String {
    match f(&[Value::Str(input.into())], &mut String::new()).unwrap() {
        Value::Str(t) => t,
        other => panic!("expected string, got {other:?}"),
    }
}
fn s2(f: fn(&[Value], &mut String) -> Result<Value, String>, a: &str, b: &str) -> String {
    match f(
        &[Value::Str(a.into()), Value::Str(b.into())],
        &mut String::new(),
    )
    .unwrap()
    {
        Value::Str(t) => t,
        other => panic!("expected string, got {other:?}"),
    }
}

// All reference values captured from real `php -n` 8.5 (basename / dirname / pathinfo).

#[test]
fn basename_matches_php() {
    assert_eq!(s1(path_basename, "/a/b.txt"), "b.txt");
    assert_eq!(s1(path_basename, "/a/b/"), "b");
    assert_eq!(s1(path_basename, "a/b"), "b");
    assert_eq!(s1(path_basename, "a"), "a");
    assert_eq!(s1(path_basename, "/"), "");
    assert_eq!(s1(path_basename, ""), "");
    assert_eq!(s1(path_basename, "a.b.c"), "a.b.c");
    assert_eq!(s1(path_basename, "/a//b//"), "b");
    assert_eq!(s1(path_basename, ".hidden"), ".hidden");
    assert_eq!(s1(path_basename, "dir/"), "dir");
}

#[test]
fn dirname_matches_php() {
    assert_eq!(s1(path_dirname, "/a/b.txt"), "/a");
    assert_eq!(s1(path_dirname, "/a/b/"), "/a");
    assert_eq!(s1(path_dirname, "a/b"), "a");
    assert_eq!(s1(path_dirname, "a"), ".");
    assert_eq!(s1(path_dirname, "/"), "/");
    assert_eq!(s1(path_dirname, ""), "");
    assert_eq!(s1(path_dirname, "/a"), "/");
    assert_eq!(s1(path_dirname, "./a"), ".");
    assert_eq!(s1(path_dirname, "../a"), "..");
    assert_eq!(s1(path_dirname, "/a//b//"), "/a");
    assert_eq!(s1(path_dirname, "dir/"), ".");
}

#[test]
fn extension_matches_php() {
    assert_eq!(s1(path_extension, "/a/b.txt"), "txt");
    assert_eq!(s1(path_extension, "/a/b/"), "");
    assert_eq!(s1(path_extension, "a"), "");
    assert_eq!(s1(path_extension, "a.b.c"), "c");
    assert_eq!(s1(path_extension, "/a/b.TXT"), "TXT");
    assert_eq!(s1(path_extension, ".hidden"), "hidden");
    assert_eq!(s1(path_extension, "a."), "");
    assert_eq!(s1(path_extension, "a..b"), "b");
}

#[test]
fn stem_matches_php() {
    assert_eq!(s1(path_stem, "/a/b.txt"), "b");
    assert_eq!(s1(path_stem, "a.b.c"), "a.b");
    assert_eq!(s1(path_stem, "noext"), "noext");
    assert_eq!(s1(path_stem, "/a/b/"), "b");
    assert_eq!(s1(path_stem, ".hidden"), "");
    assert_eq!(s1(path_stem, "a."), "a");
    assert_eq!(s1(path_stem, "a..b"), "a.");
}

#[test]
fn join_collapses_one_separator() {
    assert_eq!(s2(path_join, "a", "b"), "a/b");
    assert_eq!(s2(path_join, "a/", "/b"), "a/b");
    assert_eq!(s2(path_join, "/usr", "bin"), "/usr/bin");
    assert_eq!(s2(path_join, "a", ""), "a/");
    assert_eq!(s2(path_join, "", "b"), "/b");
    assert_eq!(s2(path_join, "a/", "b/"), "a/b/");
}
