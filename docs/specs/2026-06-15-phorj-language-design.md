# Phorj — Language Design Spec (v0.1)

- **Date:** 2026-06-15
- **Status:** Design frozen — ready for implementation planning
- **Codename:** Phorj · **File extension:** `.phg`
- **Implementation language:** Rust
- **Repo:** `/stack/projects/phorj` (own git repo; `/stack/projects` is gitignored by /stack)

---

## 1. Vision & Intent

Phorj is a **new general-purpose programming language inspired by PHP**, built as a
**learning journey that produces a real, runnable socle** while **fixing specific,
well-known PHP limitations**.

Explicit non-goal: "dethrone Java and Rust." The honest target is to **borrow the best
ideas from Java and Rust to fix PHP's worst weaknesses**, and prove it with a working
compiler. The Java/Rust "rivalry" is reframed as *stealing their good parts*, not beating
their ecosystems (which took decades and thousands of contributors).

Two intents combined:
1. **Learning** — deeply understand language/compiler design by building one end-to-end.
2. **Fix PHP pains** — give the language a concrete reason to exist.

---

## 2. Design Philosophy

- **Familiar + explicit wins.** Across every syntax decision the owner chose the
  PHP/Java-familiar, explicit option (`function`, semicolons, type-first, always-typed).
- **Managed now, systems later.** GC first (fast path to a runnable language); ownership /
  no-GC is a deliberate **v2** research branch — mirrors "rival Java now, rival Rust later."
- **Sound over convenient.** No type juggling, no truthiness, no implicit coercion.

---

## 3. Frozen v0.1 Language Specification

### Surface syntax
| Concern | Decision |
|---|---|
| Variable sigil | **none** (no `$`) |
| Member + static access | `.` for **everything** (`user.name`, `Foo.bar`) — `::` dropped |
| Function keyword | `function` |
| Statement terminator | **semicolons required** |
| Modules | `import a.b.c` (dotted paths); PHP `\` namespaces dropped |
| String concat | **interpolation** `"Hello {name}"` (`.` is taken by member access) |
| Pipe operator | `|>` (adopted from PHP 8.5) |

### Variables
| Concern | Decision |
|---|---|
| Declaration | **type-first**: `int n = 5;` (matches `private int age` members) |
| Implicit vars | forbidden — every variable explicitly declared |
| Types | **always explicit** (no inference in v0.1) |
| Mutability | **mutable by default**; `const` / `final` to lock |
| Scope | block-scoped (PHP is function-scoped) |

### Type system
- **Sound static typing**, no juggling, no implicit coercion.
- **True generics**, monomorphized: `List<User>`.
- **Null safety**: `T?` optional + `Option`; must be unwrapped before use.
- **Algebraic data types**: `enum` with associated data + **exhaustive `match`** (with
  destructuring) the compiler verifies.
- **Equality**: `==` = value/structural equality (field-by-field for objects, auto-derived,
  customizable via `Equatable`); `is` = identity. Cross-type compare requires explicit
  conversion (`n == int(x)`).
- **Booleans**: strict `bool` only — **no truthiness** (`0`, `""`, `null` are not false).
- **Numbers**: `int` (64-bit signed) + sized `i8..i64` / `u8..u64` (PHP has no unsigned);
  `decimal` (exact money math, PHP needs bcmath); `float`/`double`.
- **Strings**: UTF-8 native, single `string` type.

### Collections (fixes PHP's biggest wart — `array` is list+map fused)
- `List<T>`, `Map<K,V>`, `Set<T>`, tuples. (`List<T>` is "the array".)
- Deferred: fixed-size `Array<T, N>` (stack-friendly) → v2.

### OOP & composition
- Single inheritance + **traits** with explicit conflict resolution — the safe answer to
  "multiple inheritance" (no C++ diamond problem).
- **Method overloading** (ad-hoc polymorphism): same name, different params; resolved by
  **arity + exact type**, no silent numeric widening.
- **Constructor**: `constructor(...)` keyword with parameter promotion + visibility.
- **Asymmetric visibility**: `public private(set)` (adopted from PHP 8.4).
- **Property accessors** (get/set) — statically typed (PHP 8.4 has these as "property hooks";
  ours is the type-checked version — parity, not a novel win).
- **Value types / structs** — value semantics, not heap objects (perf lever PHP lacks).
- **Operator overloading** — types can define `+`, `==`, etc.
- `this` (no `$this`).

### Error handling
- **Exceptions only** (try/catch/throw). (`Result<T,E>` was considered and declined in favor
  of PHP/Java familiarity.)

### Concurrency
- First-class concurrency model — **specific model (async/await vs goroutine-style) TBD**.

### Runtime
- Compiles to a **native binary**, **managed GC**.
- Ownership / no-GC → **v2**.

### Removed PHP footguns (folded in, no debate)
`@` error suppression · variable-variables `$$x` · loose `switch` + fallthrough (use `match`) ·
verbose `use(...)` closure capture (auto-capture instead) · function-scoping.

---

## 4. Improvements vs PHP 8.5 (verified against current PHP)

> Latest PHP is **8.5** (released 2025-11-20). PHP 8.4 shipped property
> hooks + asymmetric visibility; 8.5 shipped the pipe operator `|>` + `clone()` overrides.
> Verified via web search 2026-06-15.

**🏆 Genuine wins — NOT in PHP 8.5:**
true generics · method overloading · ADTs with associated data + destructuring `match` ·
value types/structs · userland operator overloading · native AOT binary · real
threads/parallelism · sound static typing (no juggling) · block scoping · unsigned ints ·
native `decimal` · ownership/no-GC (v2).

**Parity (already in modern PHP — kept, not marketed as innovation):**
property accessors (8.4 hooks) · constructor promotion (8.0) · `match` value-form (8.0) ·
basic enums (8.1) · readonly (8.1/8.2) · nullable/`?->` (8.0) · union/intersection types ·
asymmetric visibility (8.4) · pipe `|>` (8.5).

---

## 5. Execution Architecture & Roadmap

```
M1: Tree-walking interpreter  ← THE SOCLE (first runnable artifact)
     lexer → parser → type-checker → evaluator
     scope: AMBITIOUS — core + generics + ADTs + match, sequenced so each lands runnable

M2: Bytecode + VM  (the "rival Java" phase)
     AST → bytecode → stack VM → GC
     single self-contained binary via bundling (GraalVM-native-image / bun-compile style)

v2: Native + systems  (the "rival Rust" phase)
     native-AOT (compile-to-C or LLVM) or JIT
     ownership / borrow checker · no-GC · sized-int perf
     desktop/mobile UI exploration
```

### M1 internal sequence (each step runnable before the next)
1. **Lexer** — tokens: no `$`, `.` access, `;`, string interpolation, keywords, literals.
2. **Parser → AST** — expressions, statements, `function`, `class`, `enum`.
3. **Type checker** — sound static types, null safety, generics, overload resolution (arity+type).
4. **Tree-walking evaluator** — programs run.
5. **Layer in** — core types → generics → ADTs + exhaustive `match`.

**M1 deliverable:** real Phorj programs (e.g. the `Shape`/`area`/`match` sample) run end-to-end.

### Implementation language: Rust (decision rationale in §7)

### Server/deployment model (the "Go model")
Phorj compiles to **one binary that *is* the web server** — listens on a port, handles
requests concurrently in-process, state persists in memory. No FPM (PHP's per-request model),
no app server (Java's resident-VM model). `scp` one binary and run it. Native-resident, like Go/Rust.

---

## 6. Sample program (frozen syntax)

```phorj
import std.io;

enum Shape {
    Circle(float radius),
    Rect(float w, float h),
}

function area(Shape s) -> float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}

class Greeter {
    private string name;

    constructor(private string name) {}

    function greet() -> string {
        return "Hello {name}";
    }
}

function main() {
    Greeter g = Greeter("Tak");
    println(g.greet());

    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        println("area = {area(s)}");
    }
}
```

---

## 7. Decisions Log

| # | Decision | Choice | Rationale |
|---|---|---|---|
| 1 | Project intent | Learning journey + real socle, scoped to fix PHP pains | Best ROI for a solo dev; gives the language a reason to exist |
| 2 | Memory model | Managed GC first; ownership/no-GC = v2 | Fastest path to a runnable language; defers the hardest part |
| 3 | PHP lineage | Clean break + one-way migration tool | Syntax changes (no `$`, `.` access) make a strict superset impossible |
| 4 | Sigil | Remove `$` | Owner preference |
| 5 | Member access | `.` for all (drop `::`) | One operator; forces interpolation for concat |
| 6 | Concat | String interpolation `{}` | `.` is now member access |
| 7 | Function keyword | `function` | Familiarity over `fn` |
| 8 | Terminator | Semicolons required | Explicit |
| 9 | Modules | `import a.b.c` | Clean dotted paths |
| 10 | Local decl | Type-first `int n = 5;` | Consistent with type-first members |
| 11 | Mutability | Mutable by default, `const`/`final` to lock | PHP/Java familiarity |
| 12 | Type inference | None — always explicit | Owner's "every var typed" rule |
| 13 | Collections | Split: `List`/`Map`/`Set`/tuples | Fixes PHP `array` wart |
| 14 | Constructor | `constructor(...)` + promotion + visibility | Decoupled from class name |
| 15 | Overloading | Yes — arity + exact type | Adds ad-hoc polymorphism PHP lacks |
| 16 | Error handling | Exceptions only | PHP/Java familiarity |
| 17 | Equality | `==` value · `is` identity | No juggling |
| 18 | Truthiness | Strict bool only | Kills a class of bugs |
| 19 | Ints | `int` + sized `i8..u64` | Systems/value-type/FFI needs; unsigned |
| 20 | Decimal | Native `decimal` type | Money math without bcmath |
| 21 | "MI" | Traits (not C++ MI) | Power of MI without the diamond problem |
| 22 | Power feats | Value types/structs, property accessors, operator overloading | Selected for v0.1 |
| 23 | Exec model | Tree-walking interpreter first | Fastest to "it runs"; Crafting-Interpreters path |
| 24 | Backend | Bytecode+VM now, native-AOT in v2 | Max learning; matches "GC now, ownership later" |
| 25 | POC scope | Ambitious: core + generics + ADTs + match | Owner's choice (eyes-open on stall risk) |
| 26 | Impl language | Rust | Greenfield + extensibility + AST fit + learning synergy + native-v2 alignment |
| 27 | Codename | Phorj (`.phg`) | PH + forge |
| 28 | Location | `/stack/projects/phorj` (own repo) | `/stack/projects` is gitignored — clean separation |

---

## 8. Open Questions / Deferred to v2

- Concurrency model: async/await vs goroutine-style + channels (decide before M2).
- Native-AOT target: compile-to-C vs LLVM (decide at v2).
- Ownership/borrow-checker design (v2).
- Standard library scope & naming (clean, consistent — large effort, v2 track).
- **Source-position column semantics** for diagnostics/LSP: the M1 lexer counts columns per *byte*, so error columns after a multi-byte UTF-8 char on a line are offset by N. Decide the unit (UTF-16 code units for LSP, Unicode scalars, or graphemes) when building the diagnostics layer (Plan 3+), then make `bump()` column-counting match. Byte offsets in `Span.start`/`len` stay byte-based (correct for slicing).
- Desktop/mobile UI story (Flutter's hard part is the rendering engine, not the language).
- PHP→Phorj migration tool (separate sub-project).

---

## 9. Prior Art to Study

- **Hack** (Meta's statically-typed PHP) — Phorj's closest older sibling.
- **Crafting Interpreters** (Nystrom) — the M1→M2 path (tree-walker → bytecode VM).
- **Rust** itself — the implementation language *and* a reference for traits/ADTs/ownership.
- **Go** — the server/deployment model and concurrency inspiration.
