//! M2.5 Phase 3a — the cross-stub download-and-verify client, tested hermetically (no network, no
//! toolchain). A `file://` fixture registry + a `PHORGE_STUB_MANIFEST` fixture manifest drive
//! `bundle::cross::download_stub` directly: it must copy a matching stub into the cache, refuse (and
//! not cache) a tampered one, and produce precise errors for a missing entry / missing asset.
//!
//! All scenarios live in ONE `#[test]` so the process-global `PHORGE_STUB_REGISTRY` /
//! `PHORGE_STUB_MANIFEST` env vars are mutated by a single thread (no intra-binary race). The real
//! download→verify→embed→run path is exercised separately, toolchain-gated, in `tests/build.rs`.

use phorge::bundle::cross::download_stub;
use phorge::bundle::sha256::sha256_hex;
use std::fs;
use std::path::PathBuf;

fn unique_dir(tag: &str) -> PathBuf {
    let base =
        std::env::temp_dir().join(format!("phorge_registry_test_{tag}_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).expect("mk test dir");
    base
}

#[test]
fn download_client_verify_cache_and_reject() {
    let root = unique_dir("client");
    let registry = root.join("registry");
    let cache = root.join("cache");
    fs::create_dir_all(&registry).unwrap();
    fs::create_dir_all(&cache).unwrap();

    // A fixture "stub" — arbitrary bytes; the client never executes it, only hashes it.
    let target = "x86_64-unknown-linux-musl";
    let stub_bytes = b"#!fake phorge stub bytes for the registry client test\n".to_vec();
    let asset = registry.join(format!("phg-stub-{target}"));
    fs::write(&asset, &stub_bytes).unwrap();
    let real_hash = sha256_hex(&stub_bytes);

    // A manifest with the CORRECT hash; point the env seams at the fixtures.
    let manifest_path = root.join("manifest.txt");
    fs::write(
        &manifest_path,
        format!("# fixture\nversion 0.0.0-test\n{target} {real_hash}\n"),
    )
    .unwrap();
    std::env::set_var("PHORGE_STUB_MANIFEST", &manifest_path);
    std::env::set_var(
        "PHORGE_STUB_REGISTRY",
        format!("file://{}/", registry.display()),
    );

    // 1) Happy path: download copies the verified asset into the cache path we pass.
    let cached = cache.join(target).join("phg");
    let got = download_stub(target, &cached).expect("download should succeed");
    assert_eq!(got, cached);
    assert!(cached.is_file(), "verified stub must be cached");
    assert_eq!(fs::read(&cached).unwrap(), stub_bytes);

    // 2) Tamper: a manifest whose hash does NOT match → error, and the cache is NOT poisoned.
    let tampered_cache = cache.join("tampered").join("phg");
    let bad_manifest = root.join("manifest-bad.txt");
    let wrong_hash = sha256_hex(b"different bytes entirely");
    fs::write(&bad_manifest, format!("{target} {wrong_hash}\n")).unwrap();
    std::env::set_var("PHORGE_STUB_MANIFEST", &bad_manifest);
    let err = download_stub(target, &tampered_cache).expect_err("hash mismatch must fail");
    assert!(err.contains("integrity check failed"), "{err}");
    assert!(
        !tampered_cache.is_file(),
        "a failed verification must not poison the cache"
    );
    // The temp download artifact must also be cleaned up.
    let leftovers: Vec<_> = fs::read_dir(tampered_cache.parent().unwrap())
        .map(|rd| rd.filter_map(Result::ok).collect())
        .unwrap_or_default();
    assert!(
        leftovers.is_empty(),
        "temp download not cleaned: {leftovers:?}"
    );

    // 3) Missing manifest entry → precise "no prebuilt stub" error.
    std::env::set_var("PHORGE_STUB_MANIFEST", &manifest_path);
    let err = download_stub("aarch64-unknown-linux-gnu", &cache.join("a").join("phg"))
        .expect_err("unknown target must fail");
    assert!(err.contains("no prebuilt stub"), "{err}");

    // 4) Manifest has the entry but the asset file is absent → fetch (fs::copy) error.
    let missing_asset_manifest = root.join("manifest-missing.txt");
    let ghost = "armv7-unknown-linux-gnueabihf";
    fs::write(
        &missing_asset_manifest,
        format!("{ghost} {}\n", sha256_hex(b"whatever")),
    )
    .unwrap();
    std::env::set_var("PHORGE_STUB_MANIFEST", &missing_asset_manifest);
    let err =
        download_stub(ghost, &cache.join("g").join("phg")).expect_err("missing asset must fail");
    assert!(err.contains("cannot copy stub"), "{err}");

    // 5) Cross-implementation hash check (the tier-3 guarantee, toolchain-light): publish a REAL
    //    binary (the phg test binary itself) as a stub, hash it with the host `sha256sum`, and drive
    //    the full client with that independently-computed manifest. Success proves the hand-rolled
    //    SHA-256 agrees byte-for-byte with a reference implementation on real executable bytes.
    if let Some(reference_hash) = host_sha256sum(env!("CARGO_BIN_EXE_phg")) {
        let real_target = "x86_64-unknown-linux-musl";
        let real_bytes = fs::read(env!("CARGO_BIN_EXE_phg")).unwrap();
        // Our own hash must equal the reference (the core cross-implementation assertion).
        assert_eq!(
            sha256_hex(&real_bytes),
            reference_hash,
            "hand-rolled SHA-256 disagrees with host sha256sum on a real binary"
        );
        fs::write(
            registry.join(format!("phg-stub-{real_target}")),
            &real_bytes,
        )
        .unwrap();
        let real_manifest = root.join("manifest-real.txt");
        fs::write(&real_manifest, format!("{real_target} {reference_hash}\n")).unwrap();
        std::env::set_var("PHORGE_STUB_MANIFEST", &real_manifest);
        let real_cached = cache.join("real").join("phg");
        download_stub(real_target, &real_cached)
            .expect("download of a reference-hashed real binary should verify and cache");
        assert_eq!(fs::read(&real_cached).unwrap(), real_bytes);
    } else {
        eprintln!("skipping cross-implementation sha256sum check: sha256sum unavailable");
    }

    std::env::remove_var("PHORGE_STUB_MANIFEST");
    std::env::remove_var("PHORGE_STUB_REGISTRY");
    let _ = fs::remove_dir_all(&root);
}

/// The lowercase-hex SHA-256 of `path` per the host `sha256sum` tool (a reference implementation
/// independent of our hand-rolled one), or `None` if the tool is unavailable.
fn host_sha256sum(path: &str) -> Option<String> {
    let out = std::process::Command::new("sha256sum")
        .arg(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    text.split_whitespace().next().map(str::to_string)
}
