# STAGE 2 — DESIGN: Persistent Cache (`Core.Cache`, Tier-B) + pure request-scoped `Core.Memo` (Tier-A)

Status: design-only. Topic: persistent PSR-16-shaped cache. Verdict: **mixed** — a **Tier-A pure
slice** (`Core.Memo` / request-scoped memoize over the shipped `Map`) ships first and is byte-identity
gated; the **Tier-B persistent `Core.Cache`** (APCu/file/Redis-backed, TTL) ships behind the existing
`pure:false` quarantine seam. Recommendation: **build the Tier-A slice in M4; admit the Tier-B
`Core.Cache` per-feature as part of the M-Batteries cluster, reusing the `Core.Process`/`Core.File`
machinery verbatim.**

Confidence: **high** on the Tier-A slice and on the Tier-B *mechanism* (every load-bearing primitive —
`pure:false` quarantine, `Op::CallNative` dispatch, the closure invoker, `Value::Map`/`map_set` kernels —
is already shipped and was read directly this session). **medium** on the exact Tier-B PHP adapter
surface (PSR-16 wording, APCu-vs-file-vs-Redis selection) and on the `getOrCompute` re-entrancy detail.

---

## 1. The reframing applied to "cache"

Per this session's reframing, "can a cache transpile to PHP?" is the wrong question — **everything
transpiles to PHP**. PHP has the richest cache story of the three legs: APCu (`apcu_fetch`/`apcu_store`
with TTL), the filesystem, and `Redis`/`Memcached` extensions. The real axis is the **three-leg
byte-identity** of `tests/differential.rs`:

- **Determinism**: a persistent cache's observable behavior depends on prior process state (was the key
  set in a *previous* run? has its TTL expired against the wall clock?). That is the textbook
  non-determinism trigger — exactly why `Core.Env`/`Core.Process` are `pure:false`.
- **Backend asymmetry**: the *Rust* legs (`run`/`runvm`) have no APCu, no shared-memory cache, and
  cannot open a Redis socket without a crate (zero-dep invariant) or TLS (the hard wall). The PHP leg
  can. So a persistent cache is **inherently a place where the Rust legs cannot mirror the PHP leg
  byte-for-byte.**
- **TLS wall**: a *Redis-over-TLS* adapter is unreachable from the Rust legs. But the Tier-B contract
  does not require the Rust legs to reach Redis at all (see §4.3) — they degrade to an in-process map,
  and the feature is non-gated, so the wall is not even load-bearing here.

Conclusion: a **persistent** cache is **Tier-B by construction** — it joins `Core.Process`/`Core.Env`
in the quarantine. But a large and genuinely useful slice — **request-scoped memoization** — is
deterministic, expressible identically on all three legs, and therefore **Tier-A**. Ship both, clearly
separated by module name so the `pure` flag (and thus the quarantine) is unambiguous per call.

---

## 2. Two-module split (the core design decision)

| Module | Tier | Purpose | `pure` | Gated? |
|---|---|---|---|---|
| `Core.Memo` | **A** | request/process-scoped memoize over the shipped `Map` — pure within one run | `true` | **yes**, in `differential.rs` |
| `Core.Cache` | **B** | persistent PSR-16-shaped cache (get/set/has/delete/getOrCompute, TTL) over APCu/file/Redis | `false` | **no** — fixture-tested in `tests/cache.rs` |

Why split rather than one module with mixed purity: the quarantine is keyed on **module import**
(`uses_impure_native` scans `src.contains("import {m}")` over impure modules — verified at
`tests/differential.rs:916–923`). Purity is a *per-native* flag, but the harness gate is *per-module-
import*. Mixing a pure `Memo.get` and an impure `Cache.get` under one module would force the whole
module impure (any program importing it would be quarantined), throwing away the gated coverage of the
pure slice. Two modules keep the pure slice gated and the impure slice quarantined — the cleanest cut,
and it mirrors the already-shipped `Core.File` (pure) vs `Core.Process`/`Core.Env` (impure) split.

---

## 3. Tier-A slice — `Core.Memo` (request-scoped memoize)

### 3.1 What it is

A pure, single-run memoization cache. "Persistent" only within the lifetime of one program execution —
it never survives a process, so its behavior is a deterministic function of the program text. This is
the strongest, free-est slice: it is essentially `Map`-with-a-compute-fallback, and `getOrCompute` is
the same higher-order shape as `List.map`/`reduce` (already shipped, `NativeEval::HigherOrder` +
`ClosureInvoker`, verified `src/native/list.rs:189–232`).

Because Phorj `Map` is **immutable / COW** (verified — `map_set` clones, `Map.set` returns a *new*
map), `Core.Memo` is purely functional: every op returns a *new* cache value. There is no hidden global
state, so it is trivially deterministic and byte-identical on all three legs.

### 3.2 Phorj-syntax API sketch

```phorj
package Main;
import Core.Console;
import Core.Memo;

function main() {
    // A Memo<K, V> is value-typed (a thin wrapper over Map<K, V>); ops are functional (COW).
    var cache = Memo.new();                    // Memo<K, V> empty

    // getOrCompute: return the cached value, or compute+store it (returns BOTH the value and the
    // updated cache, since Phorj has no mutation here — see §3.4 for the tuple-vs-pair decision).
    var r1 = Memo.getOrCompute(cache, "fib10", fn() => fib(10));
    cache = r1.cache;                          // r1.value : int, r1.cache : Memo<K, V>
    Console.println("fib10 = {r1.value}");

    var r2 = Memo.getOrCompute(cache, "fib10", fn() => fib(10));  // hit — closure NOT invoked
    Console.println("fib10 = {r2.value}");

    Console.println("has fib10 = {Memo.has(cache, \"fib10\")}");  // true
    Console.println("size = {Memo.size(cache)}");                 // 1
}
```

Surface (all `Core.Memo`, all `pure: true`):

| Native | Signature | Notes |
|---|---|---|
| `new` | `() -> Memo<K, V>` | empty memo |
| `get` | `(Memo<K, V>, K) -> V?` | safe lookup, `null` if absent (mirrors `Map.get`) |
| `has` | `(Memo<K, V>, K) -> bool` | |
| `set` | `(Memo<K, V>, K, V) -> Memo<K, V>` | functional update (COW) |
| `delete` | `(Memo<K, V>, K) -> Memo<K, V>` | functional removal |
| `size` | `(Memo<K, V>) -> int` | |
| `getOrCompute` | `(Memo<K, V>, K, () -> V) -> { value: V, cache: Memo<K, V> }` | **HigherOrder** |

### 3.3 PHP transpile target (Tier-A)

`Memo` erases to a plain PHP array (exactly as `Map` does — verified `Core.Map` natives erase to
`[k=>v]`). The pure ops map to array builtins:

```php
// Memo.new()              -> []
// Memo.get($m, $k)        -> ($m[$k] ?? null)
// Memo.has($m, $k)        -> array_key_exists($k, $m)
// Memo.set($m, $k, $v)    -> (function($m,$k,$v){ $m[$k]=$v; return $m; })($m,$k,$v)
// Memo.delete($m, $k)     -> (function($m,$k){ unset($m[$k]); return $m; })($m,$k)
// Memo.size($m)           -> count($m)
// Memo.getOrCompute($m,$k,$f) ->
//   (function($m,$k,$f){
//       if (array_key_exists($k,$m)) return ['value'=>$m[$k],'cache'=>$m];
//       $v = $f(); $m[$k] = $v; return ['value'=>$v,'cache'=>$m];
//   })($m,$k,$f)
```

All three legs see the same insertion-ordered map and invoke the closure **at most once per key** in
program order, so output is byte-identical by construction. (The closure-invocation order is fully
determined by the source — no clock, no scheduling.)

### 3.4 Open decision (Tier-A): the `getOrCompute` return shape

Phorj has no tuples yet (verified — no `Tuple` in the value set; structs/records are the idiom). Three
options, in preference order:

1. **Return a small struct** `{ value: V, cache: Memo<K, V> }` (sketch above). Requires the type to
   exist — either an *injected* type (the `Core.Json` `inject_json_prelude` precedent — verified in
   memory `[[core-json-and-injected-types]]`: inject an AST type before check, gated on import) or a
   generic `MemoResult<V, K>`. Cleanest ergonomics; **recommended**.
2. **Mutation-style** `Memo.getOrCompute(memo_ref, k, f) -> V` taking a *mutable* memo. Phorj has
   mutation (M-mut closed), but a native taking `&mut Value` is not in the current `NativeEval` shape
   (eval takes `&[Value]`). Would need a new native-eval variant — rejected (too much machinery).
3. **Two-call** `Memo.get` + `Memo.set` with the compute done in user code. No `getOrCompute` at all.
   Loses the one-shot-compute guarantee. Rejected as the *only* form, kept as the underlying primitive.

The injected-type pattern (option 1) is shipped and proven, so it is the recommended path. If the
developer prefers zero new injected types, ship the `get`+`set` primitives now and add `getOrCompute`
when a tuple/record-return convention is settled.

### 3.5 New Op / Value?

**None.** `Memo` reuses `Value::Map(Rc<Vec<(HKey, Value)>>)` and the `value::map_set`/`map_index`
kernels (verified shipped). `getOrCompute` reuses `NativeEval::HigherOrder` + the re-entrant
`ClosureInvoker` (verified `src/native/list.rs`). Dispatch is `Op::CallNative` (no new VM Op).

---

## 4. Tier-B slice — `Core.Cache` (persistent, PSR-16-shaped)

### 4.1 Why Tier-B (the precise byte-identity argument)

A persistent cache is **non-deterministic w.r.t. the program text** for two independent reasons:

1. **Cross-process state**: `Cache.get("k")` returns a value set by a *previous* run (or another
   process sharing the same APCu segment / Redis instance / file). The result is a function of process
   history, not the source — the exact `pure:false` criterion documented at `src/native/mod.rs:57`.
2. **TTL against the wall clock**: a key with TTL `expires` at `set_time + ttl`; whether `get` after
   `set` sees it depends on real elapsed time. Clock = the canonical non-determinism trigger.

Therefore a program importing `Core.Cache` is auto-dropped from `differential.rs` by `uses_impure_native`
(verified: it reads the `pure` flag off the registry, not a hardcoded list — `tests/differential.rs:914`),
exactly like `Core.Process`. Fixture-tested separately under a controlled environment in a new
`tests/cache.rs` (mirroring `tests/process.rs`).

### 4.2 Phorj-syntax API sketch (PSR-16-shaped)

```phorj
package Main;
import Core.Console;
import Core.Cache;

function main() {
    // Backend selection is a SET-UP call returning an opaque handle. Default = APCu if available,
    // else file under a temp dir (so a CLI run with no APCu still works).
    var c = Cache.open();                       // Cache (opaque handle); or Cache.openFile("/tmp/phg-cache")

    Cache.set(c, "user:42", "Ada", 3600);       // value (string), TTL seconds (0 = no expiry)
    var hit = Cache.get(c, "user:42");          // string?  -> "Ada" (or null if absent/expired)
    Console.println(hit ?? "miss");

    Console.println("has = {Cache.has(c, \"user:42\")}");
    Cache.delete(c, "user:42");

    // getOrCompute: PSR-16's "remember" — fetch, or compute+store with TTL. HigherOrder.
    var v = Cache.getOrCompute(c, "expensive", 60, fn() => slowQuery());
    Console.println("v = {v}");
}
```

Surface (all `Core.Cache`, all `pure: false`):

| Native | Signature | PSR-16 analogue |
|---|---|---|
| `open` | `() -> Cache` | auto-select APCu→file |
| `openFile` | `(string) -> Cache` | explicit file backend at a dir |
| `get` | `(Cache, string) -> string?` | `get` (null on miss/expired) |
| `set` | `(Cache, string, string, int) -> void` | `set` with TTL seconds |
| `has` | `(Cache, string) -> bool` | `has` |
| `delete` | `(Cache, string) -> void` | `delete` |
| `clear` | `(Cache) -> void` | `clear` |
| `getOrCompute` | `(Cache, string, int, () -> string) -> string` | "remember" (**HigherOrder**) |

Scope decision: **values are `string` only in v1** (a `bytes`/JSON value needs a serialization contract —
defer to a follow-up that composes with `Core.Json.stringify`/`parse`, both shipped). PSR-16 stores
arbitrary serializable values; Phorj v1 narrows to `string` to keep the Rust-leg degradation trivial
and the PHP erasure to a bare `apcu_store`/`file_put_contents` with no `serialize()` ambiguity.

### 4.3 The Rust-leg story (why a persistent cache is even runnable on `run`/`runvm`)

Tier-B natives still **execute** on the Rust legs — they are only excluded from the *byte-identity
oracle*. So `Core.Cache` needs a real Rust implementation. Options:

1. **In-process map (recommended)** — a `static CACHE: RwLock<HashMap<String, (String, Option<Instant>)>>`
   in `src/native/cache.rs`, mirroring `process.rs`'s `static PROCESS_ARGS: RwLock<…>` (verified that
   pattern is shipped and clippy-clean). `set` inserts with an optional expiry `Instant`; `get` checks
   expiry against `Instant::now()`. **Persistent within one process, lost on exit** — which is honest:
   the Rust legs have no cross-process cache and we do not pretend otherwise.
2. **File backend** — `openFile(dir)` writes one file per key (`std::fs`, already used by `Core.File`);
   `get` reads + checks an embedded expiry header. Gives genuine cross-process persistence on the Rust
   legs, deterministic enough for fixture tests. **Recommended for `openFile`; `open` defaults to the
   in-process map** (no temp-dir litter for the common case).
3. **No-op** — `get` always returns `null`. Rejected: it makes the Rust legs useless for any
   cache-dependent program and would surprise a developer running `phg run` on cache code.

The in-process `RwLock<HashMap>` is the closest Rust analogue to APCu (process-local shared memory) and
the cleanest fixture-test target. `Instant`-based TTL is the one **named determinism risk** on the Rust
legs (see §6) — handled in tests by a seam (a settable clock, §4.5).

**Why this is not gated**: because the Rust in-process map and the PHP APCu/file backend have different
*persistence scopes* (process-local vs cross-process) and different *clock sources*, their outputs
cannot be asserted equal across the three-process differential harness. That asymmetry is the definition
of Tier-B. The quarantine seam already handles this — no harness change needed beyond the `pure:false`
flag.

### 4.4 PHP transpile target (Tier-B)

`Cache` erases to a PHP adapter. Under `php -n` (the 8.5 oracle), **APCu is ABSENT** (it is a PECL
extension, confirmed in the project's `php -n` extension inventory). So the transpiled PHP must not
*hard-depend* on APCu at compile time — it selects at runtime:

```php
// Cache.open() -> a small adapter object/closure-set chosen at runtime:
//   if (function_exists('apcu_enabled') && apcu_enabled()) { /* APCu adapter */ }
//   else { /* file adapter under sys_get_temp_dir() . '/phg-cache' */ }
// Cache.openFile($dir) -> the file adapter rooted at $dir.

// APCu adapter:
//   get($k)        -> (($v = apcu_fetch($k, $ok)) , $ok ? $v : null)
//   set($k,$v,$t)  -> apcu_store($k, $v, $t)            // $t==0 => no expiry
//   has($k)        -> apcu_exists($k)
//   delete($k)     -> apcu_delete($k)
//   clear()        -> apcu_clear_cache()

// File adapter (the php -n-safe default — pure core, used by the oracle if it ever ran a Cache test):
//   set($k,$v,$t)  -> file_put_contents($dir.'/'.md5($k), json_encode(['v'=>$v,'exp'=>($t?time()+$t:0)]))
//   get($k)        -> read file; if exp && time()>exp -> unlink+null; else 'v'
//   has/delete/clear -> file_exists / unlink / array_map(unlink, glob(...))
```

Because the program is quarantined, the PHP leg's output is **not** asserted against the Rust legs — so
the APCu-vs-file selection diverging from the Rust in-process map is acceptable by design. The Redis
adapter (`phpredis` ext or a TLS socket) is **deferred**: it needs the `Redis` extension (absent under
`php -n`) and on the Rust side a real socket/TLS (the hard wall). Document it as a future adapter shape,
do not ship it in v1.

### 4.5 New Op / Value? + the handle representation

**No new VM Op.** Dispatch is `Op::CallNative` like every other native (verified the `Op::Print →
Op::CallNative` migration is the standing pattern).

**`Value` question (open)**: the `Cache` handle. Two choices:
1. **No new Value** — `open()` returns an `int` token (an index into a `RwLock<Vec<CacheBackend>>`),
   threaded as an opaque `Cache` *type alias for int* in the checker. Zero `Value` change. The handle
   is meaningless to arithmetic (checker types it as the opaque `Cache` named type, not `int`).
   **Recommended** — mirrors how a file descriptor would be modeled and avoids touching the value set.
2. **A new `Value::Handle(usize)`** — cleaner typing but a new `Value` variant ripples through the
   value kernels, `eq_val`, `Debug`, the bundle reader, etc. Rejected as overkill for one feature.

A clock seam for tests: a `static CLOCK_OVERRIDE: RwLock<Option<u64>>` (epoch seconds) so `tests/cache.rs`
can advance time deterministically across `set`/TTL/`get` without sleeping — the same settable-static
discipline as `PROCESS_ARGS`. Production reads `Instant::now()`/`SystemTime`.

---

## 5. Reuse map (everything load-bearing is already shipped)

| Need | Shipped mechanism | Verified at |
|---|---|---|
| impure quarantine | `pure: false` + `uses_impure_native` reading the flag | `src/native/mod.rs:57`, `tests/differential.rs:914–923` |
| settable process-global state | `static PROCESS_ARGS: RwLock<…>` + `set_process_args` | `src/native/process.rs:26–33` |
| optional return (`string?`) | `Ty::Optional` + `Value::Null`, `??`/if-let compose | `Core.File.read`, `Core.Map.get` |
| higher-order (closure) native | `NativeEval::HigherOrder` + re-entrant `ClosureInvoker` | `src/native/list.rs:189–232` |
| immutable map for `Memo` | `Value::Map` + `value::map_set` (COW) | `src/native/map.rs:53+` |
| generic native sig | `Ty::Param("T")` (S7b, erased pre-backend) | `src/native/list.rs:239` |
| per-leaf module file | one `src/native/<leaf>.rs` + `*_natives()` builder, no god-file | `src/native/mod.rs:30–43` |
| fixture test outside differential | `tests/process.rs` pattern | `tests/process.rs` |
| PHP arg helper | `parg(args, i)` | `src/native/mod.rs:242` |

New files: `src/native/cache.rs` (Tier-B), extend an existing leaf or add `src/native/memo.rs`
(Tier-A); `tests/cache.rs` (fixture); `examples/guide/memo.phg` (gated, Tier-A) + a non-gated
`examples/cache/` walkthrough README (Tier-B faults-can't-be-an-example rule).

---

## 6. Named determinism risks

1. **TTL clock (Tier-B, Rust legs)**: `Instant::now()`/`SystemTime` reads make `get`-after-TTL
   non-deterministic. Mitigated by the `CLOCK_OVERRIDE` test seam; production accepts it (the feature
   is non-gated). [Inferred: from the clock=non-determinism rule and the `process.rs` static precedent.]
2. **Cross-process state bleed (Tier-B)**: a file/APCu backend shares state across runs — a fixture
   test MUST clear the cache in setup (`Cache.clear`) or use a per-test temp dir, else tests are
   order-dependent. [Verified: this is exactly why the feature is quarantined.]
3. **APCu absence under `php -n` (Tier-B PHP leg)**: the oracle's `php -n` has no APCu — the
   transpiled adapter must runtime-select the file fallback, never compile-time-require `apcu_*`.
   [Verified: APCu listed ABSENT in the project's `php -n` inventory.]
4. **`getOrCompute` closure must invoke at most once (both tiers)**: a double-invocation on one backend
   (e.g. compute then re-check) would diverge from a single-invocation backend if the closure has an
   observable side effect (a `Console.println` inside it). The Tier-A example MUST exercise a
   side-effecting compute body so the differential catches a one-vs-two divergence — the standing
   "always add the operand/observable case" discipline (`[[cty-tracks-operand-types]]`, `[[null-op-scratch-slot]]`).
   [Inferred: from the documented "two-of-any-new-construct" gotcha pattern.]
5. **`Memo` insertion order (Tier-A)**: `Value::Map` is insertion-ordered (R1), so `Memo.size`/key
   iteration is stable across legs — no risk, but the example must not rely on hash order. [Verified:
   `Value::Map(Rc<Vec<(HKey,Value)>>)` is ordered.]

---

## 7. Effort & feasibility

- **Tier-A `Core.Memo`**: **small–medium**. Mostly assembling shipped pieces (Map kernels + one
  HigherOrder native). The only real decision is the `getOrCompute` return shape (§3.4). If `get`+`set`
  ship first and `getOrCompute` waits on a record-return convention, it is **small**. Feasibility ~92%.
- **Tier-B `Core.Cache`**: **medium**. New `src/native/cache.rs` (in-process `RwLock<HashMap>` + file
  backend + TTL clock seam), a runtime-selecting PHP adapter, and `tests/cache.rs`. No new Op, opaque
  `int` handle (no new Value). The medium cost is the PHP adapter (APCu/file runtime selection) and the
  clock seam, both novel-ish but precedented by `process.rs`. Feasibility ~80%.
- **Combined**: feasibility **~85%** — high on mechanism, the residual risk is the two open decisions
  (`getOrCompute` return shape; handle representation) and the PHP adapter wording.

std-only Rust feasibility: **yes** — `RwLock`/`HashMap`/`std::fs`/`SystemTime` cover the in-process and
file backends with zero crates. Redis/APCu live only on the PHP leg (deferred to a follow-up adapter).

---

## 8. Open questions for the developer

1. **`getOrCompute` return shape** (§3.4): inject a `MemoResult { value, cache }` type (cleanest), or
   ship only `get`+`set` first and add `getOrCompute` once a tuple/record-return convention exists?
2. **Tier-A admission timing**: ship `Core.Memo` now in M4, or fold it into the M-Batteries cluster
   alongside `Core.Cache`?
3. **Cache value type**: `string`-only in v1 (recommended), or compose with `Core.Json` for
   arbitrary-value caching from day one?
4. **Default Rust backend for `Cache.open()`**: in-process `RwLock<HashMap>` (no litter, lost on exit)
   vs a file backend under a temp dir (true persistence, needs cleanup)? Recommendation: in-process
   for `open()`, file only for explicit `openFile()`.
5. **Redis adapter**: confirm deferral (needs `phpredis` ext absent under `php -n`, plus the TLS wall
   on the Rust side) — document the shape, do not ship?
6. **Handle representation**: opaque `int` token (recommended, no `Value` change) vs a new
   `Value::Handle`?
