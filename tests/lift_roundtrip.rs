//! M-Lift L5 — the round-trip differential gate for the ↑ PHP→Phorge direction.
//!
//! `lift` carries no byte-identity guarantee on its own (it's a best-effort draft), so confidence is
//! *earned* here: for a Tier-1 PHP sample we **lift** it to Phorge, then check that the lifted Phorge
//! behaves exactly like the original PHP — running it three ways (`run` interpreter, `runvm` VM, and
//! its own transpiled-back PHP) and asserting all three match the **original PHP's** stdout. A full
//! match is evidence the lift preserved behavior; the original program is the source of truth.
//!
//! Gating mirrors the differential oracle: `PHORGE_REQUIRE_PHP=1` makes a missing `php` FAIL (CI),
//! otherwise it skips loudly. `PHORGE_PHP=<path>` overrides the binary. (The tiny php-runner helpers
//! are duplicated from `differential.rs` rather than shared — integration test files here are each
//! self-contained, the same pattern as `process.rs`/`serve.rs`.)

use phorge::cli::{cmd_run, cmd_runvm};
use phorge::{cli, lift};
use std::process::Command;

/// Resolve the php binary: `PHORGE_PHP` override, else `php` on PATH if `--version` succeeds.
fn php_bin() -> Option<String> {
    let cand = std::env::var("PHORGE_PHP").unwrap_or_else(|_| "php".to_string());
    let ok = Command::new(&cand)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then_some(cand)
}

/// The fails-not-skips gate. `Some(php)` ⇒ run; `None` ⇒ caller returns. Under `PHORGE_REQUIRE_PHP=1`
/// a missing php panics instead of skipping.
fn php_or_gate(test: &str) -> Option<String> {
    if let Some(p) = php_bin() {
        return Some(p);
    }
    assert!(
        std::env::var("PHORGE_REQUIRE_PHP").as_deref() != Ok("1"),
        "{test}: php required (PHORGE_REQUIRE_PHP=1) but not found on PATH or $PHORGE_PHP"
    );
    eprintln!("SKIP {test}: php not found — set PHORGE_REQUIRE_PHP=1 to make this a failure");
    None
}

/// Write `php_src` to a per-label temp file, run it with `php -n` (no php.ini → hermetic), return
/// stdout. Panics if php exits non-zero.
fn run_php(php: &str, php_src: &str, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = std::env::temp_dir().join(format!("phorge_lift_rt_{safe}.php"));
    std::fs::write(&path, php_src).expect("write temp php");
    let out = Command::new(php)
        .arg("-n")
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

/// The round-trip: lift `php_src` → Phorge, then assert the lifted Phorge run three ways
/// (interpreter, VM, transpiled-back PHP) all equal the **original** PHP's stdout.
fn roundtrip(php: &str, label: &str, php_src: &str) {
    let phorge =
        lift::lifter::lift_source(php_src).unwrap_or_else(|e| panic!("{label}: lift failed: {e}"));

    let expected = run_php(php, php_src, &format!("{label}_orig"));

    let interp = cmd_run(&phorge).unwrap_or_else(|e| {
        panic!("{label}: lifted Phorge failed on the interpreter: {e}\n--- phorge ---\n{phorge}")
    });
    assert_eq!(
        interp, expected,
        "{label}: interpreter ≠ original PHP\n--- phorge ---\n{phorge}"
    );

    let vm = cmd_runvm(&phorge)
        .unwrap_or_else(|e| panic!("{label}: lifted Phorge failed on the VM: {e}"));
    assert_eq!(vm, expected, "{label}: VM ≠ original PHP");

    let php_back = cli::cmd_transpile(&phorge)
        .unwrap_or_else(|e| panic!("{label}: lifted Phorge failed to transpile back: {e}"));
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
            r#"<?php function greet(string $n): string { return "Hi, " . $n; } echo greet("Phorge");"#,
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
    ];

    for (label, php_src) in cases {
        roundtrip(&php, label, php_src);
    }
}
