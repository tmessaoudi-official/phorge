# Phorj Playground

A free, zero-backend, browser playground for Phorj. Edit code on the left; on the right you get the
**interpreter** (`run`), the **bytecode VM** (`runvm`), the **transpiled PHP source**, and that PHP
**executed in-browser** (php-wasm, PHP 8.4) — with a badge confirming all three backends produced
byte-identical output (and a diff banner if they ever don't). Everything runs client-side; nothing is
sent to a server.

The **⬆ Lift PHP** button runs the inverse direction: it treats the editor contents as PHP and lifts
them to a Phorj draft (the same engine as `phg lift`), opening the result with a `// lifted (verify)`
banner — a best-effort, review-required scaffold for the Tier-1 PHP subset.

It is auto-deployed to GitHub Pages on every push to `master`, so the live site always runs the latest
`phg`.

## How it works

- The Phorj pipeline is compiled to WebAssembly. The core `phorj` crate is unchanged and stays
  dependency-free + `#![forbid(unsafe_code)]`; this `playground/` crate is a **separate workspace
  member** that adds the only external dependency in the project (`wasm-bindgen`, wasm32-only).
- The wasm runs in a **Web Worker** with a per-call timeout — a runaway program terminates the worker
  instead of freezing the tab (wasm is single-threaded and non-interruptible).
- The wrapper functions bypass the CLI's 256 MB `std::thread` worker (`on_deep_stack`, unavailable on
  wasm) and call the public pipeline directly: `parse → check → interpret / compile+VM / transpile`.
- The transpiled PHP is executed by [`php-wasm`](https://github.com/seanmorris/php-wasm) (defaults to
  PHP 8.4 — matching Phorj's transpile floor), loaded lazily from a CDN on first run.

The wrapper *logic* lives in plain `*_json(&str) -> String` functions in `src/lib.rs` and is unit-tested
on the native target (`cargo test -p phorj-playground`); only the thin `#[wasm_bindgen]` exports are
wasm-gated. Byte-identity itself stays gated by `tests/differential.rs` — the playground only *surfaces*
agreement.

## Build & run locally

Prerequisites: the pinned Rust toolchain (`rust-toolchain.toml`), `wasm-pack`, and Python 3.

```bash
# from the repo root
rustup target add wasm32-unknown-unknown                 # once
cargo install wasm-pack                                  # once (or use the official installer)

# bump the wasm stack (MAX_CALL_DEPTH is 4096 — see src/limits.rs)
RUSTFLAGS="-C link-arg=-zstack-size=33554432" \
  wasm-pack build playground --target web --release --out-dir web/pkg

python3 playground/web/gen_examples.py                   # regenerate examples.js

# serve the static dir (any static server works); a module worker needs http://, not file://
python3 -m http.server -d playground/web 8000
# open http://localhost:8000
```

## Deploy

GitHub Pages, via `.github/workflows/playground.yml`:

1. In the repo: **Settings → Pages → Build and deployment → Source = GitHub Actions** (one-time).
2. Push to `master` (or run the `playground` workflow manually via *Actions → playground → Run
   workflow*). The workflow builds the wasm, regenerates `examples.js`, assembles `dist/`, and deploys.
3. The site is published at `https://<owner>.github.io/phorj/`.

## Tests

```bash
cargo test -p phorj-playground          # native unit tests of the wrapper logic
```

## Known limitations (v1)

- **php-wasm CDN import** is the one path not covered by the Rust test suite — validate it on first
  deploy and pin a specific php-wasm version once confirmed.
- **Very deep recursion** can hit the wasm engine's call-stack limit before Phorj's `MAX_CALL_DEPTH`
  guard; it surfaces as an "execution crashed/timed out" message rather than a clean fault.
- Single-snippet `package Main;` programs only — no multi-file projects, vendored deps, `phg build`,
  or real `Core.File` I/O. Filesystem guide examples are excluded from the picker.
