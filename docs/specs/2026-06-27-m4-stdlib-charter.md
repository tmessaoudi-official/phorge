# M4 — Stdlib Charter

> The conventions every `Core.*` module obeys. **Descriptive** — it codifies the conventions already
> shared by the 18 shipped native modules — and **binding** on all new stdlib (M-Test, M-text,
> breadth, M-NUM). Written charter-first (2026-06-27) so new modules don't mint inconsistent APIs that
> later need a breaking codemod. Mechanizable rules are enforced by `src/native/tests.rs` guards;
> the rest are reviewed per module. See [[philosophy-of-phorj]]: legible, surprise-free PHP upgrade.

## Rule 1 — Naming
- **Modules:** `Core.<PascalCase>` (`Core.Text`, `Core.List`, `Core.Json`). `Core` is reserved.
- **Functions:** `lowerCamelCase` (`parseInt`, `base64Encode`, `splitOnce`, `startsWith`).
- **Constructors:** `of(...)` from components (`Set.of`, `Decimal.of`); `from<Source>(...)` from one
  foreign representation (`Bytes.fromString`). **Converters out:** `to<Target>` (`toInt`, `toFloat`,
  `toString`) — cross-type ones live in `Core.Convert`, same-family ones on the owning module.
- **Predicates:** `is<X>` / `has` / `contains` / `startsWith` … return `bool`.

## Rule 2 — Subject-first argument order
The value operated on is **parameter 0**: `Text.length(s)`, `List.map(xs, f)`, `Map.get(m, k)`.
This is load-bearing — UFCS (`x.f(a) ≡ f(x, a)`) only works if the receiver is param 0. Options /
config come **last**, usually with a default: `Text.parseFloat(s, permissive = false)`.

## Rule 3 — Failure discipline (optional vs fault)
- **Recoverable / expected-absent → `T?`** (Optional), never a sentinel: parse failure
  (`parseInt → int?`), missing key (`Map.get → V?`), not-found (`indexOf → int?`), empty
  (`first`/`last → T?`), decode failure (`base64Decode → bytes?`), file-read miss (`File.read → string?`).
- **Programmer error / contract violation → fault** (panic + Slice-1 stack trace), never swallowed:
  index OOB, force-unwrap of null, missing-key on the `m[k]` **indexing** form (vs the soft `Map.get`).
- A native never returns PHP-style `false`-for-error or `-1`-for-not-found — map PHP `false` → `null`.

## Rule 4 — Determinism tiers
- **Pure** (`pure: true`, default): result is a function of argument values only; participates in the
  `run ≡ runvm ≡ real PHP` byte-identity oracle.
- **Quarantined** (`pure: false`): depends on ambient state (env, args, clock, RNG). Excluded from
  `tests/differential.rs` via `program_uses_impure_native`; tested under a controlled environment
  (`tests/process.rs`, `tests/random.rs`). Today: `Core.Process`, `Core.Env`, `Core.Random`. New
  impure modules **must** be added to the `every_other_native_is_pure` seam guard's allowlist.
- A *pure* native that touches the filesystem may read only committed, deterministic inputs
  (`Core.File` reads a fixture); anything else is quarantined.
- **Determinism beats the dependency:** a feature that breaks byte-identity (network, wall clock) is
  quarantined or deferred — never forced into the oracle.

## Rule 5 — Native vs `.phg`, and PHP erasure
- A **native** is a Rust `NativeEval` body **plus** a `php:` emission byte-identical to the Rust kernel
  under `php -n`. **Tier-A only** if it maps to a PHP **core** function under `-n` (no mbstring;
  PCRE / hash / base64 / bin2hex are core — see [[transpile-no-ini-extensions]]).
- A stdlib **type** (not a function) ships as an **injected `.phg` prelude AST** (`Core.Json`'s `Json`
  enum, `Core.Http`'s `Request`/`Response`, `RoundingMode`), gated on its `import`, flowing through all
  backends as ordinary user code — zero backend machinery.

## Grandfathered (documented, not broken — never-remove-capability)
- Constructors mix `of` (Set/Decimal) and `fromString` (Bytes) — both kept; Rule 1 codifies which
  applies *going forward*.
- `Core.Convert` holds cross-type converters while same-family converters live on their owning module
  (`Decimal.intToDecimal`, …) — intentional.

## Enforcement
`src/native/tests.rs`: `charter_module_names_are_core_pascalcase`, `charter_function_names_are_lowercamel`
(mechanized). Failure discipline (Rule 3) + determinism tiers (Rule 4) are reviewed per module — the
`every_other_native_is_pure` guard partially mechanizes Rule 4.
