# Changelog

All notable changes to Phorge. Format follows [Keep a Changelog](https://keepachangelog.com/);
the project is pre-1.0 and unpublished, so versions track milestone progress, not a release
cadence. Milestones and their status live in `docs/MILESTONES.md`.

## [Unreleased]

### M3 slice S0 — developer experience

- **`var` local type inference** — `var x = expr;` infers the binding's type from its initializer
  (still fully static + immutable). The VM derives the local's operand type from the initializer, so
  arithmetic on a `var` still specializes (`AddI`/`AddF`); `ctype` now also resolves a `match` value.
- **`type` aliases** — `type Name = T;`, compile-time only. The checker resolves aliases (with cycle,
  built-in-shadow, and duplicate detection); a post-check pass (`checker::expand_aliases`) expands
  them out of the AST so the interpreter, VM, and transpiler all see alias-free types and the PHP
  output never mentions the alias.
- **Sharper diagnostics** — front-end (lex/parse/type) errors render the offending source line with a
  caret, attach a "did you mean `…`?" hint (nearest in-scope name, Levenshtein ≤ 2), and carry a
  stable code. `Diagnostic` gains `code`/`hint` fields + a `render` method; all construction is
  centralized through `Diagnostic::new`. Runtime-error strings are unchanged (differential parity).
- **`phorge explain <CODE>`** — print the explanation for a diagnostic code (`E-UNKNOWN-IDENT`,
  `E-UNKNOWN-TYPE`, `E-INFER-NULL`, `E-ALIAS-CYCLE`).
- **Per-command help** — `phorge <command> --help` / `-h` prints a description, the source/flag forms,
  and 1–2 worked examples.
- New guide example `examples/guide/inference.phg` (auto byte-identity-gated by the differential
  harness).

_Next: M3 S1 (indexing `xs[i]`, ranges `0..n`, expression `if`), then S2 (null-safety: `T?`, `??`,
`?.`). M2.5 Phase 3 (CI stub registry; opt-in `--sign`) remains parked._

## [0.4.0] — 2026-06-17

The first fully-documented release: CLI UX, profiling, a disassembler, cross-OS standalone builds,
and a complete OSS doc set.

### Profiling & introspection

- `phorge bench` now reports **memory** alongside timing: peak-RSS growth of one cold execution plus
  the process `VmHWM`/`VmRSS`, via a std-only, Linux-only `src/mem.rs` (`/proc/self/status` +
  `/proc/self/clear_refs`). Non-Linux hosts print `memory: unavailable on this platform`.
- `phorge disasm <source>` — print the compiled bytecode: per-function instruction listings (index,
  source line, op, and a resolved annotation for index-carrying ops) plus the program-level
  enum/class/method descriptor tables.
- New profiling example `examples/bench/workload.phg` (CPU recursion + heap allocation) with
  `examples/bench/README.md` documenting how the time and memory numbers are collected.

### CLI UX

- `-v` / `--version` — print `phorge <version>` and exit; `-h` / `--help` — full usage banner.
- Flexible program source for the run-family commands
  (`run`/`runvm`/`check`/`parse`/`lex`/`transpile`/`disasm`/`bench`): `<file>` | `-` (read from **stdin**) |
  `-e <code>` / `--eval <code>` (run **inline** source) | `--` (next arg is a path even if it starts
  with `-`).

### M2.5 Phase 2 — cross-OS standalone builds

- `phorge build --target <triple>` / `--all` cross-compiles a runtime stub via
  [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) (zig as the linker) and embeds the
  program as a named object-file section. Targets: `x86_64-unknown-linux-musl`,
  `aarch64-unknown-linux-{gnu,musl}`, `x86_64-pc-windows-gnu`.
- `src/bundle.rs` → a `bundle/` module: CRC-guarded `container`, per-format readers `elf`/`pe`/`macho`
  (thin + fat), a magic-sniffing `section::find_section` dispatcher, and a `cross` orchestrator. The
  hand-rolled, std-only **PE/COFF**, **Mach-O 64**, and **fat/universal** readers use checked arithmetic
  (EV-7: adversarial input → `None`, never a panic) so a produced binary self-reads its own format.
- Stub cache keyed on an FNV-1a-64 of the phorge binary's own bytes (a rebuilt phorge invalidates stale
  stubs, protecting the parity spine). Precise "missing rustup target" / "needs a source checkout"
  errors. apple/darwin targets are rejected with a clear message (macOS stub deferred to Phase 3; the
  Mach-O reader ships and is tested). `--sign` reserved for Phase 3.
- Cross-parity tests (toolchain-gated): `x86_64-musl` native-execution parity vs `runvm`, and a real
  windows-PE section round-trip.

### Documentation

- Full OSS project doc set: rewritten README, dual **MIT OR Apache-2.0** license, CONTRIBUTING,
  CODE_OF_CONDUCT, SECURITY, SUPPORT, GOVERNANCE, AUTHORS, ROADMAP, VISION, FEATURES, KNOWN_ISSUES,
  THIRD-PARTY-NOTICES, CITATION.cff, `.editorconfig`, and `.github/` templates.

Built standalone binaries are unchanged: they run their embedded program and ignore argv.

## [0.3.0] — 2026-06-16

First tagged POC. Usable end-to-end on `x86_64-linux-gnu`: the full M1 language on two
byte-identical backends (`run` interpreter + `runvm` bytecode VM), a Phorge→PHP transpiler, and
`phorge build` producing a standalone native Linux executable. Bundles all post-M2-P3 work — the
P3.5 hardening pass, M2 P4 (classes/enums/match/methods), Wave 4 (class-aware compiler types), P5a
(`Rc`-shared heap), the full-coverage example set, and M2.5 Phase 1 (standalone build). Known v1
limits: `build` is host-only; the artifact ignores argv and always exits 0; the language has no
indexing/`Map`/`Set`/optionals/`|>`/exceptions/mutation (all M3).

### M2.5 Phase 1 — `phorge build` (x86_64-linux-gnu) (2026-06-16) — **distribution**
`phorge build foo.phg` produces a standalone host executable that runs `foo.phg` on the VM with no
Phorge install — by copying the running phorge binary, embedding the program **source** in a
`.phorge` ELF section, and self-detecting + running that payload at startup. Same section+container
mechanism as the cross-OS end state (design §7). See
`docs/specs/2026-06-16-m2.5-phorge-build-design.md` + `docs/plans/2026-06-16-m2.5-phase1-build-linux-gnu.md`.

- **Added**
  - `src/bundle.rs` (std-only, zero new deps): a bitwise CRC-32, a versioned CRC-guarded payload
    **container** (`magic | version | header_len | kind | comp | enc | flags | len | payload_crc32 |
    header_crc32`), a hand-rolled **ELF64 section reader** (no `object`/`goblin` — it links into the
    produced binary, so it must stay zero-dep), and `embedded_source()` (graceful `None` on every
    malformed/tampered/absent input).
  - `cli::cmd_build` — validates the program (no broken binary is ever emitted), copies `current_exe`,
    and shells `llvm-objcopy --add-section .phorge=…` (override via `PHORGE_OBJCOPY`).
  - `phorge build <file> [-o out]` CLI command; `main()` runs an embedded payload at startup before
    any arg parsing.
  - `tests/build.rs` — the parity spine extended to distribution: a built binary's output is
    byte-identical to `runvm`; argv is ignored (v1); ill-typed programs fail with diagnostics and
    emit no binary.
  - **Hardening (post-review):** the ELF64 reader uses fully-checked offset arithmetic — adversarial/
    malformed input returns `None`, never overflow-panics under the debug/test profile
    (regression-tested per EV-7); `phorge build` rejects a dangling `-o`, an unrecognized flag, or any
    extra argument with a usage error (exit 2) instead of a silent default-named build. `docs/INVARIANTS.md`
    #1 now records the build binary as the third `cmd_runvm` parity surface.
- **Notes** (v1 limits) — host-only (`x86_64-linux-gnu`); the embedded program ignores argv and
  cannot set a custom exit code; the source is recoverable from the artifact (not obfuscated).
  Cross-targets (zig), PE/Mach-O reader arms + stub cache = Phase 2; CI stub registry + signing/
  notarization (rcodesign-from-Linux) = Phase 3.

### Examples — full-coverage showcase (2026-06-16) — **docs/tests**
A living example set covering the entire runnable language surface, plus the Phorge→PHP bridge. See
`docs/specs/2026-06-16-examples-coverage-design.md` + `docs/plans/2026-06-16-examples-coverage.md`.

- **Added**
  - Four real-world programs (`examples/realworld/{ledger,library,shop,rpg}.phg`) and six focused
    guide programs (`examples/guide/{operators,control-flow,collections,classes,enums-match,strings}.phg`),
    each exercising a different slice of the surface; an `examples/README.md` index + coverage matrix.
  - `examples/transpile/{demo.phg,demo.php,README.md}` — the Phorge→PHP transpile bridge (the only
    PHP-ecosystem path: output, not input), with a `tests/cli.rs::transpile_demo_matches_committed_php`
    snapshot test that fails on transpiler drift.
- **Changed**
  - `tests/differential.rs` now **globs `examples/**/*.phg`** instead of listing examples explicitly,
    so every current and future example is byte-identity-gated with no test edit.
- **Notes** (honest boundary, documented in `examples/README.md`)
  - Zero-payload enum variants need call form `V()` to construct **and** in a `match` pattern — a
    bare `V =>` arm is a catch-all binding (a silent logic bug both backends agree on).
  - `import` is decorative (no module resolution until M5); `null`/`T?`/`Map`/`Set`/`|>`/exceptions
    /traits/overloading remain M3+ and are deliberately absent.

### M2 P5a — `Rc`-shared heap objects (2026-06-16) — **object-path perf**
Makes compound heap objects *shared* instead of *deep-cloned*. The M1 heap is immutable + acyclic
(no reassignment, no field mutation, args evaluated before the instance exists), so `Rc` is both
sufficient and complete for reclamation — `Drop` frees everything, no cycle can leak, no tracing
collector is needed (that stays deferred to M3). See
`docs/specs/2026-06-16-m2-p5-object-model-design.md` + `docs/plans/2026-06-16-m2-p5a-rc-shared-heap.md`.

- **Changed**
  - `Value::Instance(Rc<Instance>)`, `Value::Enum(Rc<EnumVal>)`, `Value::List(Rc<Vec<Value>>)`
    (were `Box`/`Vec`). Cloning a `Value` — the `Op::GetLocal` hot path and every interpreter
    var-read — is now an O(1) refcount bump instead of a deep `HashMap`/`Vec` copy. The constructor
    now shares one `Rc` between the `this` receiver and the returned instance (no double build).
  - Three move-out sites adjusted (can't move out of an `Rc`): `vm.rs` `GetEnumField`
    (`into_iter().nth` → `.get().cloned()`), the interpreter's list `for` (iterate by ref + clone),
    and the ctor double-build (folded into one shared `Rc`). No `Op`/bytecode/AST/checker change.
- **Perf** (`phorge bench`, median of 101, `fib(28)`)
  - Object-heavy VM run **1537 ms → 634 ms (2.4× faster)**; the VM's advantage over the tree-walker
    recovered from **4.73× → 9.35×**, essentially on par with the scalar baseline (10.92×) — i.e.
    the object-path penalty (deep-clone-on-load) is largely eliminated.
  - **Phase B deferred (bench-gated, not opened):** slot-indexed `Vec` field layout. With the object
    path now ~within scalar's advantage, field access (HashMap lookup) is no longer dominating, so
    there is no evidence to justify the larger interpreter-touching change.
- **Parity** — behavior-preserving refactor; the full differential suite + examples sweep stay
  byte-identical (244 tests green), clippy + fmt clean, `#![forbid(unsafe_code)]` intact.

### M2 Wave 4 — Class-aware compiler types (2026-06-16) — **closes the last `num_ty` parity gap**
Makes the compiler's operand-type inference class-aware, so the VM no longer rejects checker-valid
programs that read a field of an arbitrary instance, a method-call result, or a nested member as an
arithmetic operand. `runvm` is now a faithful drop-in across the full checker-valid surface. See
`docs/plans/2026-06-16-m2-wave4-compiler-types.md`.

- **Changed**
  - The compiler's coarse `enum TyTag { Int, Float, Other }` became `enum CTy { Int, Float,
    Class(String), Other }` — an instance now carries *which class* it is, derived structurally from
    the AST's declared `Type` annotations (`type_tag` → `resolve_cty`); the AST, the `Op` set, the
    VM, and `value.rs` are untouched.
  - `num_ty` is now the numeric projection (`as_num`) of a new recursive `ctype(&Expr)` resolver
    that walks `Ident`/`This`/`Member`/`Call` to a class-aware type. New per-program tables —
    `class_field_ctys` (class → field → type) and `method_rets` (`(class, method)` → return type) —
    plus a `cur_class` on the compiler back the `Member`/method-call/`this` resolution. The
    P4c-era `this.field`-only `num_ty` `Member` arm is subsumed by the general resolver.
- **Parity**
  - Five programs that ran on the interpreter but failed to *compile* on the VM now agree
    byte-identically (`tests/differential.rs::WAVE4_PROGRAMS`): a field of an arbitrary instance
    (`p.x + 1`), a method result (`c.get() + 1`), a nested field (`a.inner.x + 1`), a class-typed
    enum payload bound in `match` (`Some(p) => p.x + 1`), and a free function returning an instance
    (`mk().x + 1`).
  - The only remaining coarse-type note is the deliberately out-of-M1-surface `Index` (`xs[i]`
    arithmetic faults on both backends — M1 has no user indexing).

### M2 P4c — Methods + `this` on the VM (2026-06-16) — **M2 P4 complete**
Brings instance methods and `this` to the bytecode VM. With this, **`runvm` covers the full M1
language surface** and `examples/grades.phg` runs on both backends. See
`docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::CallMethod(name_idx, argc)` — runtime method dispatch off the receiver instance's class,
    via a program-level `(class, method) → function index` table; the frame opens with the
    receiver at slot 0 (`this`).
  - Methods compile to functions (receiver at slot 0, params at `1..=argc`); `this` and bare field
    reads inside a method/ctor body resolve against the receiver.
  - `examples/grades.phg` joined the differential examples sweep; `phorge bench examples/grades.phg`
    runs (VM ≈3.2× the tree-walker on it).
- **Removed**
  - The last two `(M2 P4)` compile-error stubs (`Expr::This`, method calls) — `grep "M2 P4"` in
    `compiler.rs`/`vm.rs` is now clean.
- **Parity notes**
  - Method existence is checker-enforced, so the VM's method-not-found fault is a defensive
    backstop (no `agree_err` case, like P4a's exhaustiveness).
  - `num_ty` now classifies a `this.field`/bare-field arithmetic operand (via the class's field
    tags). At this commit a field read on an *arbitrary* instance was still the coarse-`TyTag` gap;
    **closed in M2 Wave 4** (see the Wave 4 entry above) by making the type class-aware (`CTy`).

### M2 P4b — Classes on the VM (2026-06-16)
Brings class construction (with constructor promotion + body side effects) and field reads to the
bytecode VM. See `docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::MakeInstance` (build a `Value::Instance` from promoted-field values) and `Op::GetField`
    (runtime field lookup, with a `no field` fault byte-identical to the interpreter).
  - A program-level `ClassDesc` table (per-class promoted-field names) and an interned
    field-name pool, both validated by `BytecodeProgram::validate`.
  - Each constructor compiles to a synthetic `<Class>::new` function: it promotes its params into
    fields via `MakeInstance`, runs the body for side effects with the instance in scope, and
    returns the instance. `ClassName(args)` resolves to a `Call` into it.
- **Object model**
  - Instances are value-native: the VM reuses the shared `Value::Instance`, clone-on-use,
    mirroring the interpreter (decision P4-1). No arena.
- **Parity notes**
  - A ctor body's `return` is discarded and the promoted instance is always returned (interpreter
    parity): the synthetic ctor redirects body `return`s to an epilogue that loads + returns the
    instance, so an early `return;` cannot change the result.
  - Reading an explicit (uninitialized) `Field` member type-checks but faults `no field` at
    runtime on **both** backends — construction populates only promoted ctor params.
- **Known limitation at this commit (coarse-type gap — since closed in M2 Wave 4)**
  - A field read used as the *direct left operand* of arithmetic (`p.x + …`) couldn't be classified
    by the compiler's coarse `TyTag`. Field reads worked everywhere else: interpolation, equality,
    call arguments, arithmetic right-operand, or bound through a typed local first. **M2 Wave 4
    closed this** by making the compiler's type class-aware (`CTy`); see the Wave 4 entry above.
  - `examples/grades.phg` still needs P4c (it calls an instance method).

### M2 P4a — Enums + `match` on the VM (2026-06-16)
Brings single-payload enums and exhaustive `match` to the bytecode VM (already in the
interpreter since M1). See `docs/plans/2026-06-16-m2-p4-classes-enums-match.md`.

- **Added**
  - `Op::MakeEnum`/`MatchTag`/`GetEnumField` (enum construction, variant tag test, payload
    extraction) + `Op::MatchFail` (checker-unreachable non-exhaustive backstop, byte-identical
    to the interpreter's fault).
  - A program-level `EnumDesc` table (the enum analogue of the constant pool), validated by
    `BytecodeProgram::validate`.
  - Compiler operand-height tracking, so a `match` used mid-expression (e.g. as a binary
    operand, or nested in another arm) spills its scrutinee to the correct stack slot.
- **Object model**
  - Enums are value-native: the VM reuses the shared `Value::Enum`, clone-on-use, mirroring the
    interpreter (decision P4-1). No arena — deferred to a bench-gated perf milestone.
- **Known limitation (pre-existing, shared by both backends)**
  - `match` cannot appear inside string interpolation — the lexer's `{…}` interpolation does not
    nest a `match`'s braces. Not a parity issue (both backends reject it identically).

### M2 P3.5 — Hardening (in progress, 2026-06-16)
Closing the parity/no-crash contract gaps before P4 widens the surface. See
`docs/plans/2026-06-16-m2-p3.5-hardening-roadmap.md`.

- **Added**
  - `phorge bench <file>` — median-of-N timing of both backends, output-identity gated; measures
    the "VM faster than tree-walker" thesis (≈10× on `examples/fib.phg`) instead of asserting it.
  - `agree_err` error-parity oracle in the differential harness (faults classified by semantic
    `FaultKind`).
  - Central `src/limits.rs` (recursion/nesting caps + numeric-width policy); unified
    `diagnostic::Diagnostic` for all stages; `BytecodeProgram::validate`; `docs/INVARIANTS.md`,
    `docs/ARCHITECTURE.md`; `rust-toolchain.toml`.
- **Changed**
  - Arithmetic/comparison single-sourced into `value.rs` (both backends call the same kernels).
  - VM runtime errors now carry the source line (`Chunk.lines`).
  - Constant pool interns scalar duplicates.
  - `interpreter::Frame` → `CallScopes` (removes the name collision with `vm::Frame`); scope-verbs
    unified (`push_scope`/`pop_scope`).
  - Quality gate is now compile-time (`warnings = "deny"`, `clippy.all = "deny"`,
    `#![forbid(unsafe_code)]`) + a tracked pre-commit hook.
- **Fixed**
  - `Op::Neg` on `i64::MIN` aborted the VM (P0) — now a clean `integer overflow` fault, matching
    the interpreter.
  - Interpreter/parser/checker no longer SIGABRT on deep recursion/nesting — explicit limits fault
    cleanly.
  - Determinism: checker's non-exhaustive-`match` error sorts its missing-variant list.

## M2 — Bytecode + VM (P1–P3, 2026-06-16)
- **P1** — `Chunk` + typed `Op` enum + stack VM dispatch loop.
- **P2** — AST→bytecode compiler for the `main`-only surface + `phorge runvm` + the differential
  harness (`runvm` byte-identical to `run`).
- **P3** — user function calls, clox-style call frames, recursion/mutual recursion; `examples/fib.phg`
  runs on the VM.

## M1 — Tree-walking interpreter + transpiler — 2026-06-15 (`9da6e56`)
- Full pipeline: lexer → parser → type-checker → tree-walking evaluator.
- Phorge → PHP transpiler, round-trip-verified against real PHP.
- CLI: `phorge <run|check|parse|lex|transpile>`.
- Language surface: static types, immutable-by-default bindings, functions, classes + constructor
  promotion, single-payload enums + exhaustive `match`, string interpolation, `List<T>` literals,
  `for…in`, checked int/float arithmetic. 162 tests green at the tag.
