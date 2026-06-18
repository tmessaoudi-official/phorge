//! Project manifest (`phorge.toml`) — Composer's *vocabulary* in a TOML container.
//!
//! The manifest speaks the words a PHP/Composer developer reads natively —
//! `name = "vendor/package"`, `[require]` / `[require-dev]` — but it is an honest
//! `phorge.toml` that the `phorge` tool actually runs (a literal `composer.json` would
//! be a false promise: no Packagist, no autoloader Phorge uses). Each dependency
//! self-locates via `git` + a pinned `tag`/`rev` (Go-style — no central registry, no
//! Composer `repositories` side-table); version *ranges* are intentionally absent
//! (the lockfile pins exact, so no resolver is needed).
//!
//! **Scope (M5 S2a): parse + represent only.** Nothing consumes the manifest yet —
//! resolution, vendoring, and folder=path enforcement land in later M5 slices, so this
//! module changes no `.phg` execution path and the backends stay byte-identical.
//!
//! The parser handles a deliberately small, well-defined TOML subset (top-level string
//! keys, the three section headers, quoted/bare keys, inline dependency tables, and a
//! `"<git-url>@<tag>"` string shorthand). It is strict: unknown sections, unknown keys,
//! a `branch` pin, a missing pin, or an unquoted value are hard errors rather than
//! silent acceptance.

use std::path::{Path, PathBuf};

/// A pinned dependency version. Only a tag or an exact rev is allowed — never a branch
/// (M5-10: determinism requires a fixed point, which a moving branch is not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pin {
    Tag(String),
    Rev(String),
}

/// One git dependency. Self-locating: the `git` URL *is* the coordinate (there is no
/// registry to resolve a bare `vendor/package` name against).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// Composer-style `vendor/package` name (also the PSR-4 namespace root on emission).
    pub name: String,
    /// Git URL the dependency is fetched from.
    pub git: String,
    /// The pinned point (`tag` or `rev`).
    pub pin: Pin,
}

/// A parsed `phorge.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// `vendor/package` project name; doubles as the emitted PHP namespace root.
    pub name: String,
    /// Project version (empty string if the manifest omits it).
    pub version: String,
    /// Source root that anchors folder=path (default [`Manifest::DEFAULT_SOURCE`]).
    pub source: String,
    /// `[require]` dependencies.
    pub require: Vec<Dependency>,
    /// `[require-dev]` dependencies.
    pub require_dev: Vec<Dependency>,
}

impl Manifest {
    /// The manifest filename walked-up for during project detection.
    pub const MANIFEST_FILE: &'static str = "phorge.toml";
    /// Source root used when the manifest omits `source`.
    pub const DEFAULT_SOURCE: &'static str = "src";

    /// Parse a `phorge.toml` from its text. Returns a human-readable, line-numbered
    /// error on any malformed or unsupported construct.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let mut name: Option<String> = None;
        let mut version = String::new();
        let mut source: Option<String> = None;
        let mut require: Vec<Dependency> = Vec::new();
        let mut require_dev: Vec<Dependency> = Vec::new();

        enum Sec {
            Meta,
            Require,
            RequireDev,
        }
        let mut sec = Sec::Meta;

        for (i, raw) in text.lines().enumerate() {
            let lineno = i + 1;
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            if let Some(inner) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                sec = match inner.trim() {
                    "package" => Sec::Meta,
                    "require" => Sec::Require,
                    "require-dev" => Sec::RequireDev,
                    other => {
                        return Err(format!(
                            "phorge.toml:{lineno}: unknown section `[{other}]` \
                             (expected [package], [require], or [require-dev])"
                        ));
                    }
                };
                continue;
            }
            let (k, v) = line.split_once('=').ok_or_else(|| {
                format!("phorge.toml:{lineno}: expected `key = value`, found `{line}`")
            })?;
            let key = unquote_key(k);
            let val = v.trim();
            match sec {
                Sec::Meta => match key.as_str() {
                    "name" => {
                        name = Some(
                            parse_string(val).map_err(|e| format!("phorge.toml:{lineno}: {e}"))?,
                        );
                    }
                    "version" => {
                        version =
                            parse_string(val).map_err(|e| format!("phorge.toml:{lineno}: {e}"))?;
                    }
                    "source" => {
                        source = Some(
                            parse_string(val).map_err(|e| format!("phorge.toml:{lineno}: {e}"))?,
                        );
                    }
                    other => {
                        return Err(format!(
                            "phorge.toml:{lineno}: unknown key `{other}` \
                             (expected name, version, or source)"
                        ));
                    }
                },
                Sec::Require => require
                    .push(parse_dep(key, val).map_err(|e| format!("phorge.toml:{lineno}: {e}"))?),
                Sec::RequireDev => require_dev
                    .push(parse_dep(key, val).map_err(|e| format!("phorge.toml:{lineno}: {e}"))?),
            }
        }

        let name = name.ok_or_else(|| {
            "phorge.toml: missing required `name` (e.g. name = \"acme/myapp\")".to_string()
        })?;
        if name.trim().is_empty() {
            return Err("phorge.toml: `name` must not be empty".to_string());
        }
        let source = source.unwrap_or_else(|| Self::DEFAULT_SOURCE.to_string());
        Ok(Manifest {
            name,
            version,
            source,
            require,
            require_dev,
        })
    }

    /// The PSR-4 namespace root derived from `name`: `"acme/myapp"` ⇒ `"Acme\\Myapp"`
    /// (each `/`-segment PascalCased, joined with the PHP namespace separator `\`).
    pub fn namespace_root(&self) -> String {
        self.name
            .split('/')
            .map(pascal_case)
            .collect::<Vec<_>>()
            .join("\\")
    }
}

/// A detected project: its manifest, the directory the manifest lives in, and the
/// resolved source root that anchors folder=path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Directory containing `phorge.toml` (the project root).
    pub root: PathBuf,
    /// The parsed manifest.
    pub manifest: Manifest,
    /// `root` joined with the manifest `source` — where package files are anchored.
    pub source_root: PathBuf,
}

impl Project {
    /// Walk up from `start` (a file or directory) looking for a `phorge.toml`. The first
    /// one found marks the project root. Returns `Ok(None)` when none is found before the
    /// filesystem root — that is *loose-script mode* (folder=path suspended; only
    /// `package main;` is legal, enforced in a later slice). Returns `Err` only when a
    /// found manifest is unreadable or malformed.
    pub fn detect(start: &Path) -> Result<Option<Project>, String> {
        let mut cur: &Path = if start.is_dir() {
            start
        } else {
            start.parent().unwrap_or(start)
        };
        loop {
            let candidate = cur.join(Manifest::MANIFEST_FILE);
            if candidate.is_file() {
                let text = std::fs::read_to_string(&candidate)
                    .map_err(|e| format!("{}: {e}", candidate.display()))?;
                let manifest =
                    Manifest::parse(&text).map_err(|e| format!("{}: {e}", candidate.display()))?;
                let source_root = cur.join(&manifest.source);
                return Ok(Some(Project {
                    root: cur.to_path_buf(),
                    manifest,
                    source_root,
                }));
            }
            match cur.parent() {
                Some(parent) => cur = parent,
                None => return Ok(None),
            }
        }
    }
}

/// Truncate a line at the first `#` that is not inside a double-quoted string, so a `#`
/// appearing inside a value (e.g. a URL fragment) is preserved while a trailing comment
/// is dropped.
fn strip_comment(line: &str) -> &str {
    let mut in_quote = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            '#' if !in_quote => return &line[..i],
            _ => {}
        }
    }
    line
}

/// A key may be bare (`name`) or quoted (`"acme/parser"`). Strip surrounding quotes if
/// present; otherwise return the trimmed text.
fn unquote_key(raw: &str) -> String {
    let k = raw.trim();
    if k.len() >= 2 && k.starts_with('"') && k.ends_with('"') {
        k[1..k.len() - 1].to_string()
    } else {
        k.to_string()
    }
}

/// Parse a strictly double-quoted TOML basic string. An unquoted value is an error (so a
/// bare number or identifier where a string is required is rejected, not silently kept).
fn parse_string(val: &str) -> Result<String, String> {
    let v = val.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Ok(v[1..v.len() - 1].to_string())
    } else {
        Err(format!("expected a quoted string, found `{v}`"))
    }
}

/// PascalCase a single name segment: uppercase the first character, keep the rest.
fn pascal_case(seg: &str) -> String {
    let mut chars = seg.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Parse one dependency value — either an inline table `{ git = "…", tag|rev = "…" }`
/// or the `"<git-url>@<tag>"` string shorthand.
fn parse_dep(name: String, val: &str) -> Result<Dependency, String> {
    let v = val.trim();
    let (git, pin) = if v.starts_with('{') {
        parse_inline_table(&name, v)?
    } else if v.starts_with('"') {
        parse_shorthand(&name, v)?
    } else {
        return Err(format!(
            "dependency `{name}`: expected `{{ git = … }}` or a \"<git-url>@<tag>\" string, found `{v}`"
        ));
    };
    Ok(Dependency { name, git, pin })
}

/// Parse `{ git = "…", tag = "…" }`. Values are quoted strings with no embedded commas
/// (git URLs, tags, and revs never contain a comma), so splitting on `,` is safe for
/// this subset.
fn parse_inline_table(name: &str, v: &str) -> Result<(String, Pin), String> {
    let inner = v
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| format!("dependency `{name}`: malformed inline table (missing `}}`)"))?;
    let mut git: Option<String> = None;
    let mut tag: Option<String> = None;
    let mut rev: Option<String> = None;
    for pair in inner.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let (k, val) = pair
            .split_once('=')
            .ok_or_else(|| format!("dependency `{name}`: expected `key = value` in `{pair}`"))?;
        let value = parse_string(val).map_err(|e| format!("dependency `{name}`: {e}"))?;
        match k.trim() {
            "git" => git = Some(value),
            "tag" => tag = Some(value),
            "rev" => rev = Some(value),
            "branch" => {
                return Err(format!(
                    "dependency `{name}`: `branch` is not allowed — pin a `tag` or `rev` \
                     (a moving branch breaks determinism)"
                ));
            }
            other => {
                return Err(format!(
                    "dependency `{name}`: unknown key `{other}` (expected git, tag, or rev)"
                ));
            }
        }
    }
    let git = git.ok_or_else(|| format!("dependency `{name}`: missing `git`"))?;
    Ok((git, pin_from(name, tag, rev)?))
}

/// Parse the `"<git-url>@<tag>"` shorthand. The version is taken after the *last* `@`, so
/// an SSH URL like `git@host:acme/parser.phg@v1` splits correctly. The shorthand always
/// yields a tag pin (use the inline-table `rev = …` form to pin a raw commit).
fn parse_shorthand(name: &str, v: &str) -> Result<(String, Pin), String> {
    let s = parse_string(v).map_err(|e| format!("dependency `{name}`: {e}"))?;
    let (git, ver) = s
        .rsplit_once('@')
        .ok_or_else(|| format!("dependency `{name}`: shorthand must be \"<git-url>@<tag>\""))?;
    if git.is_empty() || ver.is_empty() {
        return Err(format!(
            "dependency `{name}`: shorthand must be \"<git-url>@<tag>\""
        ));
    }
    Ok((git.to_string(), Pin::Tag(ver.to_string())))
}

/// Build a [`Pin`] from the optional `tag`/`rev` of an inline table, requiring exactly one.
fn pin_from(name: &str, tag: Option<String>, rev: Option<String>) -> Result<Pin, String> {
    match (tag, rev) {
        (Some(t), None) => Ok(Pin::Tag(t)),
        (None, Some(r)) => Ok(Pin::Rev(r)),
        (Some(_), Some(_)) => Err(format!(
            "dependency `{name}`: specify exactly one of `tag` or `rev`, not both"
        )),
        (None, None) => Err(format!(
            "dependency `{name}`: missing pin — add a `tag` or `rev`"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // --- Manifest::parse ---------------------------------------------------

    #[test]
    fn parses_name_only_with_defaults() {
        let m = Manifest::parse("name = \"acme/myapp\"").unwrap();
        assert_eq!(m.name, "acme/myapp");
        assert_eq!(m.version, "");
        assert_eq!(m.source, "src"); // DEFAULT_SOURCE
        assert!(m.require.is_empty());
        assert!(m.require_dev.is_empty());
    }

    #[test]
    fn parses_full_manifest() {
        let src = r#"
            name = "acme/myapp"
            version = "0.1.0"
            source = "lib"

            [require]
            "acme/parser" = { git = "https://github.com/acme/parser.phg", tag = "v1.2.0" }
            "acme/json"   = "https://github.com/acme/json.phg@v0.3.1"

            [require-dev]
            "acme/testkit" = { git = "https://github.com/acme/testkit.phg", rev = "a1b2c3d" }
        "#;
        let m = Manifest::parse(src).unwrap();
        assert_eq!(m.name, "acme/myapp");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.source, "lib");

        assert_eq!(m.require.len(), 2);
        assert_eq!(m.require[0].name, "acme/parser");
        assert_eq!(m.require[0].git, "https://github.com/acme/parser.phg");
        assert_eq!(m.require[0].pin, Pin::Tag("v1.2.0".to_string()));
        // string shorthand desugars to a tag pin
        assert_eq!(m.require[1].name, "acme/json");
        assert_eq!(m.require[1].git, "https://github.com/acme/json.phg");
        assert_eq!(m.require[1].pin, Pin::Tag("v0.3.1".to_string()));

        assert_eq!(m.require_dev.len(), 1);
        assert_eq!(m.require_dev[0].name, "acme/testkit");
        assert_eq!(m.require_dev[0].pin, Pin::Rev("a1b2c3d".to_string()));
    }

    #[test]
    fn package_section_header_is_accepted() {
        let src = "[package]\nname = \"acme/app\"\nversion = \"2.0.0\"";
        let m = Manifest::parse(src).unwrap();
        assert_eq!(m.name, "acme/app");
        assert_eq!(m.version, "2.0.0");
    }

    #[test]
    fn shorthand_handles_ssh_url_with_at() {
        let src = "name = \"a/b\"\n[require]\n\"a/dep\" = \"git@github.com:acme/dep.phg@v9.9.9\"";
        let m = Manifest::parse(src).unwrap();
        assert_eq!(m.require[0].git, "git@github.com:acme/dep.phg");
        assert_eq!(m.require[0].pin, Pin::Tag("v9.9.9".to_string()));
    }

    #[test]
    fn comments_and_blank_lines_ignored() {
        let src = "# top comment\n\nname = \"acme/app\"  # trailing comment\n\n";
        let m = Manifest::parse(src).unwrap();
        assert_eq!(m.name, "acme/app");
    }

    #[test]
    fn hash_inside_quotes_is_not_a_comment() {
        let m = Manifest::parse("name = \"acme/a#b\"").unwrap();
        assert_eq!(m.name, "acme/a#b");
    }

    #[test]
    fn missing_name_errors() {
        let err = Manifest::parse("version = \"1.0.0\"").unwrap_err();
        assert!(err.contains("missing required `name`"), "got: {err}");
    }

    #[test]
    fn branch_pin_rejected() {
        let src = "name = \"a/b\"\n[require]\n\"a/d\" = { git = \"u\", branch = \"main\" }";
        let err = Manifest::parse(src).unwrap_err();
        assert!(err.contains("`branch` is not allowed"), "got: {err}");
    }

    #[test]
    fn tag_and_rev_together_errors() {
        let src =
            "name = \"a/b\"\n[require]\n\"a/d\" = { git = \"u\", tag = \"v1\", rev = \"abc\" }";
        let err = Manifest::parse(src).unwrap_err();
        assert!(err.contains("exactly one of `tag` or `rev`"), "got: {err}");
    }

    #[test]
    fn missing_pin_errors() {
        let src = "name = \"a/b\"\n[require]\n\"a/d\" = { git = \"u\" }";
        let err = Manifest::parse(src).unwrap_err();
        assert!(err.contains("missing pin"), "got: {err}");
    }

    #[test]
    fn missing_git_errors() {
        let src = "name = \"a/b\"\n[require]\n\"a/d\" = { tag = \"v1\" }";
        let err = Manifest::parse(src).unwrap_err();
        assert!(err.contains("missing `git`"), "got: {err}");
    }

    #[test]
    fn unknown_section_errors() {
        let err = Manifest::parse("name = \"a/b\"\n[bogus]\nx = \"y\"").unwrap_err();
        assert!(err.contains("unknown section `[bogus]`"), "got: {err}");
    }

    #[test]
    fn unknown_meta_key_errors() {
        let err = Manifest::parse("name = \"a/b\"\nauthors = \"x\"").unwrap_err();
        assert!(err.contains("unknown key `authors`"), "got: {err}");
    }

    #[test]
    fn unquoted_value_errors() {
        let err = Manifest::parse("name = acme/app").unwrap_err();
        assert!(err.contains("expected a quoted string"), "got: {err}");
    }

    #[test]
    fn namespace_root_pascalcases_segments() {
        let m = Manifest::parse("name = \"acme/myapp\"").unwrap();
        assert_eq!(m.namespace_root(), "Acme\\Myapp");
        let single = Manifest::parse("name = \"toolbox\"").unwrap();
        assert_eq!(single.namespace_root(), "Toolbox");
    }

    // --- Project::detect ---------------------------------------------------

    /// Unique temp dir for a test, removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> TempDir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let unique = N.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "phorge_manifest_test_{}_{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn detect_walks_up_to_project_root() {
        let tmp = TempDir::new();
        let root = tmp.path();
        std::fs::write(
            root.join("phorge.toml"),
            "name = \"acme/app\"\nsource = \"src\"",
        )
        .unwrap();
        let nested = root.join("src").join("acme").join("util");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("parse.phg");
        std::fs::write(&file, "package acme.util;").unwrap();

        let project = Project::detect(&file).unwrap().expect("project detected");
        assert_eq!(project.root, root);
        assert_eq!(project.manifest.name, "acme/app");
        assert_eq!(project.source_root, root.join("src"));
    }

    #[test]
    fn detect_returns_none_in_loose_mode() {
        let tmp = TempDir::new();
        let nested = tmp.path().join("nowhere");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("script.phg");
        std::fs::write(&file, "package main;").unwrap();
        // No phorge.toml anywhere under the temp dir → loose-script mode.
        assert_eq!(Project::detect(&file).unwrap(), None);
    }

    #[test]
    fn detect_propagates_malformed_manifest_error() {
        let tmp = TempDir::new();
        let root = tmp.path();
        // Missing required `name`.
        std::fs::write(root.join("phorge.toml"), "version = \"1.0\"").unwrap();
        let err = Project::detect(root).unwrap_err();
        assert!(err.contains("missing required `name`"), "got: {err}");
    }
}
