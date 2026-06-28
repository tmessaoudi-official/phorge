# Track P — Build / deploy / distribution — gap audit

## Track summary

Phorj's distribution story is unusually strong for a pre-1.0 language: `phg build` already
produces standalone native executables (host `x86_64-linux-gnu` plus cross-compiled
`x86_64-musl`, `aarch64-{gnu,musl}`, and `x86_64-pc-windows-gnu` via cargo-zigbuild + a zig
linker), with hand-rolled std-only ELF/PE/Mach-O section readers, a CRC-guarded versioned
container, an FNV-keyed per-target stub cache, and `tests/build.rs` gating cross-parity. CI
(`.github/workflows/ci.yml`) enforces the three-way oracle and exercises the cross targets. The
M6 web deploy path is further along than the milestone docs imply: `phg serve` (single-threaded
HTTP/1.1 over `src/serve.rs` behind a `Transport` trait) ships, and a hand-written PHP
front-controller (`examples/web/server.php`) bridges the transpiled `handle(Request) -> Response`
to `php -S`. The remaining gaps cluster in four bands: (1) the **parked M2.5 Phase 3** — the
prebuilt stub registry (designed in detail, *not built*: no `build.rs`, no `sha256.rs`/`manifest.rs`,
no `release.yml`) and code signing (Authenticode / macOS codesign+notarize); (2) **`phg build`
does not merge `vendor/`**, so any multi-package or dependency-using program cannot be compiled to
a single binary — the single biggest *usability* gap; (3) **PHP-native deploy ergonomics** a PHP
dev expects — PHAR output, a generated (not hand-written) front-controller, and modern PHP runtime
targets (FrankenPHP / RoadRunner worker mode); (4) **release/reproducibility hygiene** — `phg
release` automation (M12), reproducible builds, SBOM/provenance, install script, container images.
Most of band 3/4 is `adopt` because it maps directly to idiomatic PHP deployment and a PHP dev
would immediately recognize it; the theory-maximalist items (a package registry, FaaS-specific
adapters) are `defer` or `reject` on the philosophy lens.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| P-stub-registry | M2.5 Phase 3a: prebuilt cross-stub registry (download+verify) | port | strong | adopt | M2.5 P3a | L |
| P-codesign | M2.5 Phase 3b: code signing (Authenticode + macOS notarize) | port | ok | defer | M2.5 P3b | L |
| P-macos-stub | macOS stub production (signed Mach-O) | port | ok | defer | M2.5 P3b | M |
| P-build-vendor | `phg build` merges `vendor/` + multi-package projects | port | strong | adopt | M2.5 P3 / M5 | M |
| P-build-argv | Built binaries pass through argv / exit codes | port | strong | adopt | M2.5 P3 | S |
| P-phar | PHAR output target (`phg transpile --phar` / `phg package`) | port | strong | adopt | M6/M12 | M |
| P-frontcontroller-gen | Generated PHP front-controller (`phg serve --emit-php`) | port | strong | adopt | M6 W4 | M |
| P-release-cmd | `phg release` automation + version stamping | port | strong | adopt | M12 | M |
| P-repro-builds | Reproducible builds + SBOM + build provenance | new | strong | adopt | M12 | M |
| P-install-script | One-line installer + checksummed release assets | port | strong | adopt | M12 | S |
| P-container-img | Container image / OCI build target (`phg build --oci`) | new | ok | defer | M12+ | M |
| P-frankenphp | Modern PHP runtime targets (FrankenPHP / RoadRunner worker) | map | ok | defer | M6+ | L |
| P-faas | Serverless/FaaS deploy adapters (Lambda/Cloud Run) | map | weak | reject | — | L |
| P-pkg-registry | Hosted package registry (Packagist analogue) | new | weak | reject | v2+ | L |
| P-build-bytecode | Bytecode (not source) payload in built binaries | defer | ok | defer | v2 | M |
| P-strip-meta | Build flags: `--strip`, `--release`/`--debug`, size report | port | strong | adopt | M2.5 P3 | S |
| P-win-runtime | Windows host-build + native-exec verification in CI | port | ok | defer | M2.5 P3 | M |

## ADOPT rationale

**P-stub-registry** — This is the one explicitly *designed* but unbuilt deliverable in the whole
track (`docs/specs/2026-06-17-m2.5-phase3a-stub-registry-design.md`, decisions P3-1..P3-8). Today a
*distributed* (sourceless) phorj can only host-build; `--target`/`--all` error with "needs a source
checkout." The design is complete and 100% verifiable without any secrets: a hand-rolled std SHA-256
(`bundle/sha256.rs`), a baked-manifest seam (`build.rs` + `PHORJ_BAKE_STUB_MANIFEST` breaking the
stub↔manifest fixpoint), a `download_stub` third branch in `cross.rs`, and a 2-pass `release.yml`.
It closes the distribution loop and is squarely on-philosophy (zero new runtime deps, verify-before-
cache protects the parity spine). High effort but fully spec'd — the highest-leverage item here.

**P-build-vendor** — `phg build` embeds *one source file*; KNOWN_ISSUES is explicit that a program
importing any cross-package or vendored dependency "cannot yet be compiled to a standalone
executable." Since M5 made packages *mandatory* and the project loader already does the flat
name-mangling merge before any backend, this is the single biggest gap between what `phg run` can
execute and what `phg build` can ship. The fix is to run the loader's merge and embed the merged
program (the container's `payload_kind` already anticipates non-trivial payloads). Without it, the
standalone-binary story only works for toy single-file programs — a real adoption blocker. Strong
philosophy fit: a PHP dev expects `box`/PHAR to bundle the whole app, not one file.

**P-build-argv** — Built binaries currently ignore `argv` and always exit 0 (KNOWN_ISSUES). A
standalone CLI tool is the *primary* use case for `phg build`; a tool that can't read its arguments
or signal failure via exit code is not deployable. Small, mechanical fix (thread `std::env::args`
and the program's exit value through the `main()` self-detect hook). Strong fit — every PHP CLI
script reads `$argv` and `exit()`s.

**P-phar** — PHAR is *the* canonical PHP single-file distribution format; a PHP dev's mental model of
"ship the whole app as one file you can `php app.phar`" is PHAR, not a native binary. Phorj already
transpiles to PHP and M5 emits brace-namespaced output; wrapping that output in a PHAR stub
(`phg package --phar` or a `transpile` flag) gives PHP-shops a deploy artifact they already know how
to run, sign, and put behind a web server. Strong fit, directly idiomatic-PHP, complements (does not
replace) the native binary. Note: the PHP `phar.readonly` ini and stub conventions are well-known
and stable.

**P-frontcontroller-gen** — Today `examples/web/server.php` is *hand-written* and the workflow is a
manual `phg transpile … | sed '$d' > web_app.php` then point `php -S` at a hand-maintained adapter.
For deploy this should be a command: `phg serve --emit-php out/` (or `phg build --target php`) that
generates both the transpiled handlers *and* the superglobal↔`Request` front-controller, so the PHP
deploy path is a single reproducible step, not a copy-paste recipe. Strong fit — it makes the "deploy
to any PHP host" promise real and turnkey, which is the whole point of the transpile bridge.

**P-release-cmd** — M12 (release automation) is on the GA roadmap but unbuilt. A `phg release`
that stamps the version, runs the gate, builds `--all`, generates checksums, and (with P-stub-
registry) publishes is the natural capstone. Strong fit: a single-developer project especially
benefits from a one-command, reproducible release that can't forget a target or a checksum.

**P-repro-builds** — The master M2.5 spec lists reproducible-build flags as a nice-to-have; the
Phase-3a design makes the sha256 manifest the *real* integrity guard but notes reproducibility is
weaker today. For a security-conscious, zero-dependency language whose pitch is "provably correct,"
byte-reproducible builds + an SBOM (trivial — std-only, one crate) + provenance (SLSA-style
attestation in CI) are a strong differentiator and cheap given the tiny dependency surface. Strong
fit with the "no supply-chain surface" VISION principle.

**P-install-script** — A `curl … | sh` installer that downloads the right release asset for the
host triple and verifies its checksum is table-stakes for adoption (rustup, deno, bun all ship one).
Small once P-stub-registry/release assets exist. Strong fit — lowers the on-ramp, which VISION names
as the adoption strategy.

**P-strip-meta** — Build ergonomics a PHP dev (and everyone) expects from a compiler: `--strip` for
size, `--release`/`--debug`, and a final size report. The native-binary payload is source + a VM
stub, so size matters for distribution. Small, additive flags on the existing `phg build`. Strong
fit, no surprise budget spent.

## Notes on defer / reject

- **P-codesign / P-macos-stub** — explicitly deferred to Phase 3b in the design because the
  maintainer has no signing certs and no Mac SDK; signing code would be unverifiable scaffolding
  (decision P3-1). Correctly `defer`.
- **P-frankenphp / P-faas** — FrankenPHP and RoadRunner worker mode are *runtime* targets for the
  transpiled PHP, not Phorj features; they `map` onto the existing transpile output and a thin
  worker-mode front-controller (defer), while bespoke FaaS adapters are vendor-specific glue that
  doesn't earn its keep on the philosophy lens (reject — a PHP dev deploys to FaaS via a standard
  PHP runtime image, not a language feature).
- **P-pkg-registry** — a hosted Packagist analogue is a v2+ ecosystem play, not a language gap; M5
  already chose git-based, vendored, offline deps deliberately. Reject for the foreseeable roadmap.
- **P-build-bytecode** — embedding compiled bytecode (not source) in built binaries is a perf/secrecy
  optimization the container's `payload_kind` already anticipates; defer to v2 (no current need, and
  shipping source keeps the parity spine trivially auditable).
- **P-container-img / P-win-runtime** — `defer`: an OCI build target and full Windows-host CI
  native-exec are real but lower-priority polish once the registry + vendor-merge land.

## Critic pass

Adversarial completeness + mis-listing re-check against FEATURES.md, KNOWN_ISSUES.md, ROADMAP.md,
and `src/main.rs`/`src/cli.rs`.

**Mis-listing re-check — none found.** Every `adopt` item was confirmed *not shipped*:
`src/main.rs` exposes only `run|runvm|check|parse|lex|transpile|disasm|bench|build|vendor|serve|
explain` — there is **no `release`, no `package`/`--phar`, no `serve --emit-php`/`build --target php`,
and no stub-registry** (`--sign` is reserved-only, errors). KNOWN_ISSUES explicitly states `phg
build` embeds one source file and does not merge `vendor/`, and that built binaries ignore argv +
always exit 0. So P-build-vendor / P-build-argv / P-phar / P-frontcontroller-gen / P-release-cmd /
P-stub-registry / P-strip-meta are all genuine gaps. (`phg serve` IS shipped, but no row claimed it
as a gap, so nothing to flag.) `removed_mislisted = 0`.

**Philosophy sanity-check on recommendations — all hold.** P-faas (reject) and P-pkg-registry
(reject) correctly fail the lens: FaaS adapters are vendor glue and a hosted registry reverses the
deliberate M5 git+vendor+offline decision (ADR-0005). P-codesign / P-macos-stub (defer) are
correctly credential-gated. One refinement: **P-phar should be `M6/M12`, not pure M12** — it is the
PHP dev's *primary* deploy mental model and complements the web story, so it belongs with the
front-controller work; left as-is since the row already reads M6/M12.

**Newly-found items (long-tail).** Three build/deploy items in this track's domain were missed:

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| P-shell-completion | Generated shell completion (bash/zsh/fish) for `phg` | port | strong | adopt | M2.5 P3 / M7 | S |
| P-build-assets | `phg build` embeds companion assets (templates/static/fixtures) | new | ok | defer | M6+ | M |
| P-self-update | `phg self-update` (pulls + verifies the latest release asset) | port | ok | defer | M12 | S |

- **P-shell-completion** (adopt) — `phg` is a hand-rolled CLI (`src/main.rs` matches subcommands by
  string); there is no completion generation. Every CLI a PHP dev installs from rustup/deno/bun ships
  bash/zsh/fish completion, and it is a pure DX on-ramp item (VISION names the on-ramp as the adoption
  strategy). Small and std-only (emit static completion scripts from the known subcommand/flag set —
  no `clap`, staying zero-dep). Strong fit: lowers the on-ramp, spends no surprise budget. Pairs
  naturally with the man-page/`--help` surface that already exists.
- **P-build-assets** (defer) — once M6 web + `phg build` converge, a real deployable app is *not* a
  single `.phg` — it has templates, static files, and committed fixtures (e.g. `Core.File.read` of a
  data file, as the `Core.File` guide already does). The container's `payload_kind` anticipates richer
  payloads, so embedding a companion-file set alongside the program source is the natural extension
  that makes "ship one binary" true for a web/CLI app, not just a pure-compute program. Defer behind
  P-build-vendor (merging packages is the prerequisite; assets are the next layer). `new` because PHP
  has no native-binary asset-embedding analogue, but it maps to the familiar PHAR "bundle everything"
  expectation.
- **P-self-update** (defer) — rustup/deno/bun all ship `self-update`; once P-stub-registry +
  P-install-script land (checksummed release assets exist), a `phg self-update` that downloads and
  *verifies* the latest host asset is a cheap, expected on-ramp capstone. `ok` fit (PHP-absent, but
  the verify-before-replace discipline mirrors the stub registry's parity-protecting guard). Defer to
  M12 with the rest of the release tooling — it is strictly downstream of the release-asset
  infrastructure existing.

`new_found = 3`, `removed_mislisted = 0`.
