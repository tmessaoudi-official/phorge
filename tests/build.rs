//! M2.5: `phg build` produces a self-executing binary whose output is byte-identical to
//! `phg runvm` on the same program (the parity spine extended to the distribution layer).
//! Phase 1 = host; Phase 2 adds cross-target parity (toolchain-gated, graceful skip).
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_phg");

/// Skip-aware: true iff cargo-zigbuild and the given rustup target are both available.
fn cross_toolchain_ready(target: &str) -> bool {
    let zb = Command::new("cargo-zigbuild").arg("--version").output();
    if !matches!(zb, Ok(o) if o.status.success()) {
        eprintln!("skipping: cargo-zigbuild unavailable");
        return false;
    }
    let tl = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output();
    let ok = matches!(&tl, Ok(o) if String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == target));
    if !ok {
        eprintln!("skipping: rustup target {target} not installed");
    }
    ok
}

/// Skip-aware: true iff `llvm-objcopy` (or `$PHORGE_OBJCOPY`) can run. Host `phg build` shells out
/// to it to embed the `.phorge` section, so a host-build test must **skip — not fail** where it is
/// absent (a lean CI runner, a contributor without LLVM tools). The `cross-build` CI job installs
/// it, so these tests still run for real there; only the lean `gate` job skips them. Mirrors
/// `cross_toolchain_ready`'s philosophy for the zig / cargo-zigbuild toolchain.
fn objcopy_available() -> bool {
    let obj = std::env::var("PHORGE_OBJCOPY").unwrap_or_else(|_| "llvm-objcopy".into());
    let ok = Command::new(&obj)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("skipping: {obj} unavailable (host `phg build` needs llvm-objcopy)");
    }
    ok
}

#[test]
fn cross_musl_binary_matches_runvm() {
    // Tier 3 — native execution: x86_64-musl runs on this x86_64-linux box.
    let target = "x86_64-unknown-linux-musl";
    if !cross_toolchain_ready(target) {
        return;
    }
    let src = "examples/guide/operators.phg";
    let out = std::env::temp_dir().join("phorge-musl-parity");
    let built = Command::new(BIN)
        .args(["build", src, "--target", target, "-o"])
        .arg(&out)
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let ran = Command::new(&out).output().expect("run musl binary");
    let runvm = Command::new(BIN)
        .args(["runvm", src])
        .output()
        .expect("runvm");
    let _ = std::fs::remove_file(&out);
    assert_eq!(ran.stdout, runvm.stdout, "musl binary output != runvm");
}

#[test]
fn cross_windows_section_round_trips() {
    // Tier 2 — dump round-trip: the windows .exe can't execute here; verify its embedded section.
    let target = "x86_64-pc-windows-gnu";
    if !cross_toolchain_ready(target) {
        return;
    }
    let src = "examples/guide/operators.phg";
    let out = std::env::temp_dir().join("phorge-win-parity.exe");
    let built = Command::new(BIN)
        .args(["build", src, "--target", target, "-o"])
        .arg(&out)
        .output()
        .expect("build");
    assert!(
        built.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    // Dump the .phorge section back out and confirm it decodes to the original source.
    let dumped = std::env::temp_dir().join("phorge-win-section.bin");
    let objcopy = std::env::var("PHORGE_OBJCOPY").unwrap_or_else(|_| "llvm-objcopy".into());
    let st = Command::new(objcopy)
        .args(["--dump-section"])
        .arg(format!(".phorge={}", dumped.display()))
        .arg(&out)
        .status()
        .expect("objcopy dump");
    assert!(st.success());
    let section = std::fs::read(&dumped).expect("read dumped section");
    let expected = std::fs::read_to_string(src).expect("read src");
    assert_eq!(
        phorge::bundle::container::decode_container(&section).as_deref(),
        Some(expected.as_bytes())
    );
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&dumped);
}

#[test]
fn built_binary_matches_runvm() {
    if !objcopy_available() {
        return;
    }
    let prog = "examples/realworld/ledger.phg";
    let out_bin = std::env::temp_dir().join(format!("phorge_built_{}", std::process::id()));
    let _ = std::fs::remove_file(&out_bin);

    let build = Command::new(BIN)
        .args(["build", prog, "-o", out_bin.to_str().unwrap()])
        .output()
        .expect("spawn build");
    assert!(
        build.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let produced = Command::new(&out_bin).output().expect("run built binary");
    let expected = Command::new(BIN)
        .args(["runvm", prog])
        .output()
        .expect("spawn runvm");
    let _ = std::fs::remove_file(&out_bin);

    assert!(produced.status.success(), "built binary exited non-zero");
    assert_eq!(
        produced.stdout, expected.stdout,
        "built binary output diverged from runvm"
    );
}

#[test]
fn built_binary_ignores_argv_runs_embedded() {
    if !objcopy_available() {
        return;
    }
    // v1 limitation: the embedded program ignores argv. Passing args must not change behavior.
    let prog = "examples/hello.phg";
    let out_bin = std::env::temp_dir().join(format!("phorge_built_argv_{}", std::process::id()));
    let _ = std::fs::remove_file(&out_bin);
    let build = Command::new(BIN)
        .args(["build", prog, "-o", out_bin.to_str().unwrap()])
        .output()
        .expect("spawn build");
    assert!(build.status.success());
    let with_args = Command::new(&out_bin)
        .args(["run", "ignored", "--whatever"])
        .output()
        .expect("run built");
    let _ = std::fs::remove_file(&out_bin);
    assert_eq!(
        String::from_utf8_lossy(&with_args.stdout),
        "Hello, Phorge!\n"
    );
}

#[test]
fn build_rejects_ill_typed_program() {
    let bad = std::env::temp_dir().join(format!("phorge_bad_{}.phg", std::process::id()));
    std::fs::write(&bad, "function main() { int x = \"no\"; }").unwrap();
    let out_bin = std::env::temp_dir().join(format!("phorge_bad_out_{}", std::process::id()));
    let _ = std::fs::remove_file(&out_bin);
    let build = Command::new(BIN)
        .args([
            "build",
            bad.to_str().unwrap(),
            "-o",
            out_bin.to_str().unwrap(),
        ])
        .output()
        .expect("spawn build");
    let _ = std::fs::remove_file(&bad);
    // Assert BEFORE cleanup: a meaningful "no binary emitted" check must observe the real state.
    assert_eq!(build.status.code(), Some(1), "ill-typed build must fail");
    assert!(String::from_utf8_lossy(&build.stderr).contains("type error"));
    assert!(
        !out_bin.exists(),
        "no binary should be emitted on validation failure"
    );
    let _ = std::fs::remove_file(&out_bin);
}

#[test]
fn build_rejects_dangling_o_flag() {
    // `build f.phg -o` with no value must be a usage error (exit 2), not a silent default-named
    // build. Run in a temp cwd with an absolute source so a buggy default build can't pollute the repo.
    let cwd = std::env::temp_dir().join(format!("phorge_argtest_o_{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let src = std::fs::canonicalize("examples/hello.phg").unwrap();
    let out = Command::new(BIN)
        .current_dir(&cwd)
        .args(["build", src.to_str().unwrap(), "-o"])
        .output()
        .expect("spawn build");
    let _ = std::fs::remove_dir_all(&cwd);
    assert_eq!(
        out.status.code(),
        Some(2),
        "dangling -o must be a usage error"
    );
}

#[test]
fn build_rejects_target_and_all_together() {
    let out = Command::new(BIN)
        .args([
            "build",
            "examples/guide/operators.phg",
            "--target",
            "x86_64-unknown-linux-musl",
            "--all",
        ])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn build_rejects_sign_flag_as_phase3() {
    let out = Command::new(BIN)
        .args(["build", "examples/guide/operators.phg", "--sign", "x"])
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Phase 3"));
}

#[test]
fn build_rejects_macos_target_as_deferred() {
    // F7: an apple/darwin --target must error clearly (deferred), never silently emit a Mach-O with a
    // mismatched `.phorge` section. The guard fires before rustup-target resolution, so this holds
    // even without the apple target installed. build_target -> Err -> main exits 1.
    let out = Command::new(BIN)
        .args([
            "build",
            "examples/guide/operators.phg",
            "--target",
            "x86_64-apple-darwin",
        ])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(1),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("deferred"));
}

#[test]
fn build_rejects_unknown_trailing_arg() {
    // An unrecognized trailing argument must error, not be silently ignored (which would write a
    // default-named binary). Same temp-cwd + absolute-source isolation.
    let cwd = std::env::temp_dir().join(format!("phorge_argtest_x_{}", std::process::id()));
    std::fs::create_dir_all(&cwd).unwrap();
    let src = std::fs::canonicalize("examples/hello.phg").unwrap();
    let out = Command::new(BIN)
        .current_dir(&cwd)
        .args(["build", src.to_str().unwrap(), "--bogus"])
        .output()
        .expect("spawn build");
    let _ = std::fs::remove_dir_all(&cwd);
    assert_eq!(
        out.status.code(),
        Some(2),
        "unknown trailing arg must be a usage error"
    );
}
