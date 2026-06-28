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

/// Tier 3 (Phase 3a) — the full *distributed* path: a sourceless phg DOWNLOADS a prebuilt musl stub
/// from a fixture registry, sha256-verifies it (hand-rolled SHA-256 vs the host `sha256sum` that wrote
/// the manifest — a cross-implementation check), embeds the program, and the produced musl binary runs
/// byte-identically to `runvm`. Forces the download branch by running `phg build` from a dir with no
/// `Cargo.toml` and a fresh `XDG_CACHE_HOME`. Reuses the musl stub the cache already holds (built by
/// `phg build --target` here) so this adds no second full cross-compile. Toolchain-gated graceful skip.
#[test]
fn distributed_download_embed_run_matches_runvm() {
    let target = "x86_64-unknown-linux-musl";
    if !cross_toolchain_ready(target) || !objcopy_available() {
        return;
    }
    if !matches!(Command::new("sha256sum").arg("--version").output(), Ok(o) if o.status.success()) {
        eprintln!("skipping: sha256sum unavailable");
        return;
    }
    let src = "examples/guide/operators.phg";

    // 1) Populate the standard cache with a real musl stub via the local build branch (we have
    //    Cargo.toml at the repo root), then locate that cached stub via the public `cache_dir`.
    let warm = std::env::temp_dir().join("phorge-dist-warm");
    let built = Command::new(BIN)
        .args(["build", src, "--target", target, "-o"])
        .arg(&warm)
        .output()
        .expect("warm build");
    assert!(
        built.status.success(),
        "warm build failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let _ = std::fs::remove_file(&warm);
    let bin_bytes = std::fs::read(BIN).expect("read phg bin");
    let cached_stub = phorge::bundle::cross::cache_dir(&bin_bytes)
        .expect("cache dir")
        .join(target)
        .join("phg");
    assert!(
        cached_stub.is_file(),
        "expected a cached musl stub to reuse"
    );

    // 2) Build a fixture registry from the cached stub + a manifest with its host-sha256sum hash.
    let root = std::env::temp_dir().join(format!("phorge-dist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let reg = root.join("reg");
    let fresh_cache = root.join("cache");
    std::fs::create_dir_all(&reg).unwrap();
    std::fs::create_dir_all(&fresh_cache).unwrap();
    std::fs::copy(&cached_stub, reg.join(format!("phg-stub-{target}"))).unwrap();
    let sum = Command::new("sha256sum")
        .arg(reg.join(format!("phg-stub-{target}")))
        .output()
        .expect("sha256sum");
    let hash = String::from_utf8_lossy(&sum.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    std::fs::write(root.join("manifest.txt"), format!("{target} {hash}\n")).unwrap();

    // 3) A standalone program in a working dir WITHOUT Cargo.toml → forces the download branch; a
    //    fresh XDG_CACHE_HOME guarantees a cache miss so the download actually runs.
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::copy(src, work.join("prog.phg")).unwrap();
    let out = root.join("prog-musl");
    let dist = Command::new(BIN)
        .args(["build", "prog.phg", "--target", target, "-o"])
        .arg(&out)
        .current_dir(&work)
        .env("XDG_CACHE_HOME", &fresh_cache)
        .env("PHORGE_STUB_REGISTRY", format!("file://{}/", reg.display()))
        .env("PHORGE_STUB_MANIFEST", root.join("manifest.txt"))
        .output()
        .expect("distributed build");
    assert!(
        dist.status.success(),
        "distributed build failed: {}",
        String::from_utf8_lossy(&dist.stderr)
    );
    // The downloaded stub must have been verified and cached under the fresh cache.
    assert!(
        fresh_cache.join("phorge").join("stubs").is_dir(),
        "download did not populate the fresh cache"
    );

    // 4) The produced musl binary runs byte-identically to runvm.
    let ran = Command::new(&out)
        .output()
        .expect("run downloaded-stub binary");
    let runvm = Command::new(BIN)
        .args(["runvm", src])
        .output()
        .expect("runvm");
    assert_eq!(
        ran.stdout, runvm.stdout,
        "downloaded-stub binary output != runvm"
    );
    let _ = std::fs::remove_dir_all(&root);
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
    std::fs::write(&bad, "function main() -> void { int x = \"no\"; }").unwrap();
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
