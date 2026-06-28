//! M-Lift L5 — the round-trip differential gate for the ↑ PHP→Phorj direction.
//!
//! `lift` carries no byte-identity guarantee on its own (it's a best-effort draft), so confidence is
//! *earned* here: for a Tier-1 PHP sample we **lift** it to Phorj, then check that the lifted Phorj
//! behaves exactly like the original PHP — running it three ways (`run` interpreter, `runvm` VM, and
//! its own transpiled-back PHP) and asserting all three match the **original PHP's** stdout. A full
//! match is evidence the lift preserved behavior; the original program is the source of truth.
//!
//! Gating mirrors the differential oracle: `PHORJ_REQUIRE_PHP=1` makes a missing `php` FAIL (CI),
//! otherwise it skips loudly. `PHORJ_PHP=<path>` overrides the binary. (The tiny php-runner helpers
//! are duplicated from `differential.rs` rather than shared — integration test files here are each
//! self-contained, the same pattern as `process.rs`/`serve.rs`.)

use phorj::cli::{cmd_run, cmd_runvm};
use phorj::{cli, lift};
use std::process::Command;

/// Resolve the php binary: `PHORJ_PHP` override, else `php` on PATH if `--version` succeeds.
fn php_bin() -> Option<String> {
    let cand = std::env::var("PHORJ_PHP").unwrap_or_else(|_| "php".to_string());
    let ok = Command::new(&cand)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then_some(cand)
}

/// The fails-not-skips gate. `Some(php)` ⇒ run; `None` ⇒ caller returns. Under `PHORJ_REQUIRE_PHP=1`
/// a missing php panics instead of skipping.
fn php_or_gate(test: &str) -> Option<String> {
    if let Some(p) = php_bin() {
        return Some(p);
    }
    assert!(
        std::env::var("PHORJ_REQUIRE_PHP").as_deref() != Ok("1"),
        "{test}: php required (PHORJ_REQUIRE_PHP=1) but not found on PATH or $PHORJ_PHP"
    );
    eprintln!("SKIP {test}: php not found — set PHORJ_REQUIRE_PHP=1 to make this a failure");
    None
}

/// The `php` flags for a hermetic run — `-n` (ignore php.ini), plus an explicit `-d extension=bcmath`
/// when bcmath isn't compiled in (it's an ini-loaded shared extension on CI's `setup-php`, which `-n`
/// would otherwise disable). Mirrors `differential.rs::php_n_args`; see its doc for the rationale.
fn php_n_args(php: &str) -> &'static [&'static str] {
    static BUILTIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let has_builtin = *BUILTIN.get_or_init(|| {
        Command::new(php)
            .args(["-n", "-r", "exit(extension_loaded('bcmath') ? 0 : 1);"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    });
    if has_builtin {
        &["-n"]
    } else {
        // `display_errors=stderr` keeps stdout clean if the `.so` can't be found; see the twin in
        // `differential.rs::php_n_args`.
        &[
            "-n",
            "-d",
            "display_errors=stderr",
            "-d",
            "extension=bcmath",
        ]
    }
}

/// Write `php_src` to a per-label temp file, run it with `php -n` (no php.ini → hermetic), return
/// stdout. Panics if php exits non-zero.
fn run_php(php: &str, php_src: &str, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = std::env::temp_dir().join(format!("phorj_lift_rt_{safe}.php"));
    std::fs::write(&path, php_src).expect("write temp php");
    let out = Command::new(php)
        .args(php_n_args(php))
        .arg(&path)
        .output()
        .expect("spawn php");
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "php exited non-zero for {label}:\n{}\n--- php ---\n{php_src}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 php stdout")
}

/// The round-trip: lift `php_src` → Phorj, then assert the lifted Phorj run three ways
/// (interpreter, VM, transpiled-back PHP) all equal the **original** PHP's stdout.
fn roundtrip(php: &str, label: &str, php_src: &str) {
    let phorj =
        lift::lifter::lift_source(php_src).unwrap_or_else(|e| panic!("{label}: lift failed: {e}"));

    let expected = run_php(php, php_src, &format!("{label}_orig"));

    let interp = cmd_run(&phorj).unwrap_or_else(|e| {
        panic!("{label}: lifted Phorj failed on the interpreter: {e}\n--- phorj ---\n{phorj}")
    });
    assert_eq!(
        interp, expected,
        "{label}: interpreter ≠ original PHP\n--- phorj ---\n{phorj}"
    );

    let vm =
        cmd_runvm(&phorj).unwrap_or_else(|e| panic!("{label}: lifted Phorj failed on the VM: {e}"));
    assert_eq!(vm, expected, "{label}: VM ≠ original PHP");

    let php_back = cli::cmd_transpile(&phorj)
        .unwrap_or_else(|e| panic!("{label}: lifted Phorj failed to transpile back: {e}"));
    assert_eq!(
        run_php(php, &php_back, &format!("{label}_back")),
        expected,
        "{label}: transpiled-back PHP ≠ original PHP\n--- php ---\n{php_back}"
    );
}

#[test]
fn lift_roundtrip_preserves_behavior() {
    let Some(php) = php_or_gate("lift_roundtrip_preserves_behavior") else {
        return;
    };

    // Each sample echoes a STRING (lift maps `echo` → `Console.print(string)`); raw int/float echo is
    // avoided on purpose — int echo would lift to a `Console.print(int)` type error and floats have a
    // known interpreter-vs-PHP formatting divergence (KNOWN_ISSUES).
    let cases: &[(&str, &str)] = &[
        (
            "concat",
            r#"<?php function greet(string $n): string { return "Hi, " . $n; } echo greet("Phorj");"#,
        ),
        (
            "if_elseif_else",
            r#"<?php
function sign(int $n): string {
    if ($n < 0) { return "neg"; } elseif ($n === 0) { return "zero"; } else { return "pos"; }
}
echo sign(-3) . sign(0) . sign(7);"#,
        ),
        (
            "for_loop_string_build",
            r#"<?php
function stars(int $n): string {
    $s = "";
    for ($i = 0; $i < $n; $i++) { $s = $s . "*"; }
    return $s;
}
echo stars(5);"#,
        ),
        (
            "class_ctor_method",
            r#"<?php
class Box {
    public function __construct(private string $v) {}
    public function get(): string { return $this->v; }
}
$b = new Box("boxed");
echo $b->get();"#,
        ),
        (
            "match_strings",
            r#"<?php
function name(int $c): string {
    return match ($c) { 0 => "red", 1 => "green", 2 => "blue", default => "?" };
}
echo name(1) . name(9);"#,
        ),
        (
            // A-6 ↔ lift: a keyless PHP `foreach ($xs as $x)` lifts to Phorj `foreach (xs as x)`
            // (element type inferred) and round-trips behavior-identically.
            "foreach",
            r#"<?php
function joined(): string {
    $xs = ["a", "b", "c"];
    $out = "";
    foreach ($xs as $x) { $out = $out . $x; }
    return $out;
}
echo joined();"#,
        ),
        (
            // C-1: PHP double-quoted interpolation — simple `$var`, simple public `$o->prop`, and
            // complex `{$o->method()}` — lifts to Phorj `"{…}"` holes and round-trips identically.
            "string_interpolation",
            r#"<?php
class Box {
    public function __construct(public string $label) {}
    public function next(): int { return 42; }
}
function describe(string $who, Box $b): string {
    return "Hi $who, label=$b->label next={$b->next()}";
}
$b = new Box("crate");
echo describe("Ada", $b);"#,
        ),
        (
            // Contextual `var`: a PHP variable/parameter literally named `$var` lifts to a Phorj
            // value named `var` (not mangled) and round-trips — the original motivating bug. PHP
            // allows `$var`; Phorj now does too (`var` is the inference keyword only at a binding
            // start). Here `$var` is a parameter (read) and also a top-level local (a `var var = …`
            // declaration), both kept verbatim.
            "var_as_identifier",
            r#"<?php
function tag(string $var): string {
    return "[" . $var . "]";
}
$var = tag("hi");
echo $var;"#,
        ),
    ];

    for (label, php_src) in cases {
        roundtrip(&php, label, php_src);
    }
}
