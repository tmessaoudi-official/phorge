# Examples Full-Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans (inline — subagents
> deadlock on the ask-human gate in this repo). Steps use checkbox (`- [ ]`) syntax. Phorge git
> autonomy applies (commit green, self-contained; `feat:`/`docs:`/`test:` prefixes, no
> `Co-Authored-By`). Read `docs/specs/2026-06-16-examples-coverage-design.md` and
> `docs/INVARIANTS.md` first.

**Goal:** Add a complete, honest, runnable example set — four real-world programs, six focused
guide programs, and a Phorge→PHP transpile demo — every runnable `.phg` byte-identical on `run`
and `runvm`, auto-gated by a globbing differential test.

**Architecture:** Examples-only change plus one test refactor (explicit example list → recursive
`examples/**/*.phg` glob, std-only walker). No `src/` change. Each example uses only the verified
runnable surface (spec §2) and obeys two rules: zero-payload enum variants are constructed with
call form `V()`, and state evolves by fresh bindings/recursion (no reassignment, no list folding).

**Tech Stack:** Rust (std-only), the `phorge` CLI (`run`/`runvm`/`transpile`), `tests/differential.rs`.

**Hard constraints baked into every program (verified 2026-06-16):**
- No reassignment, no field mutation, no list indexing/destructuring → accumulate via recursion on
  a counter or chain fresh bindings; `for…in` is for side effects (`println`) only.
- Zero-payload enum variants need call form `V()` **both** to construct AND as a match pattern:
  `Defend()`, not `Defend`. Bare `Defend =>` in a match is a *catch-all binding* (silent logic bug
  — both backends agree on wrong output, so the glob test won't catch it). Always `Defend() =>`.
- Only builtin is `println(string)`. Excluded (checker-rejected): `null`, `T?`, `Map`/`Set` values,
  `|>`, exceptions, traits, overloading, sized ints, `decimal`, real imports. `is` omitted (it is a
  deep-`==` alias).
- Verify EVERY new `.phg` with both backends before committing:
  `phg run <f>` and `phg runvm <f>` must print identically (the glob test enforces this).

---

## Wave A — Glob the sweep + four real-world examples

### Task A1: Replace the explicit example sweep with a recursive glob

**Files:**
- Modify: `tests/differential.rs` (the `examples_match_between_backends` test)

- [ ] **Step 1: Replace the test.** Find `fn examples_match_between_backends()` and replace the
  whole function with a recursive `examples/` walker (std-only — no `glob` crate) that runs `agree`
  on every `.phg`:

```rust
/// Recursively collect every `*.phg` under `dir`.
fn collect_phg(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read_dir examples/") {
        let path = entry.expect("examples dir entry").path();
        if path.is_dir() {
            collect_phg(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("phg") {
            out.push(path);
        }
    }
}

/// Every runnable example under `examples/` must produce byte-identical stdout on both backends.
/// Globbing (not an explicit list) means a newly-added example is gated with no test edit.
#[test]
fn all_examples_match_between_backends() {
    let mut files = Vec::new();
    collect_phg(std::path::Path::new("examples"), &mut files);
    files.sort();
    assert!(
        files.len() >= 3,
        "expected at least the seed examples, found {}",
        files.len()
    );
    for path in &files {
        eprintln!("differential: {}", path.display()); // names the file if agree() panics
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        agree(&src);
    }
}
```

- [ ] **Step 2: Run it (currently only hello/fib/grades exist).**
  Run: `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --test differential all_examples -- --nocapture`
  Expected: PASS, lists `examples/fib.phg`, `examples/grades.phg`, `examples/hello.phg`.

- [ ] **Step 3: Full gate + commit.**
  Run: `cargo test && cargo clippy --all-targets && cargo fmt --check`
```bash
git add tests/differential.rs
git commit -m "test: glob examples/ into the differential sweep (auto-gate new examples)"
```

### Task A2: `examples/realworld/ledger.phg`

**Files:** Create: `examples/realworld/ledger.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

// A bank ledger. Phorge values are immutable, so `apply` returns a *new* Account
// rather than mutating in place — state evolves by binding fresh names.

enum Tx {
    Deposit(int cents),
    Withdraw(int cents),
    Transfer(int cents, string to),
}

class Account {
    constructor(private string owner, private int cents) {}

    function owner_name() -> string { return owner; }
    function balance() -> int { return cents; }

    function apply(Tx t) -> Account {
        return match t {
            Deposit(c)      => Account(owner, cents + c),
            Withdraw(c)     => Account(owner, cents - c),
            Transfer(c, to) => Account(owner, cents - c),
        };
    }
}

function describe(Tx t) -> string {
    return match t {
        Deposit(c)      => "deposit {c}c",
        Withdraw(c)     => "withdraw {c}c",
        Transfer(c, to) => "transfer {c}c to {to}",
    };
}

// Recursive integer compound interest: 5% per year, floored (no list folding needed).
function grow(int cents, int years) -> int {
    if (years <= 0) {
        return cents;
    }
    return grow(cents + cents * 5 / 100, years - 1);
}

function main() {
    Account a0 = Account("Ada", 1000);
    Account a1 = a0.apply(Deposit(500));
    Account a2 = a1.apply(Withdraw(200));
    Account a3 = a2.apply(Transfer(100, "Bob"));

    println("{a3.owner_name()} balance = {a3.balance()}c");

    List<Tx> log = [Deposit(500), Withdraw(200), Transfer(100, "Bob")];
    for (Tx t in log) {
        println(describe(t));
    }

    println("1000c after 3y at 5% = {grow(1000, 3)}c");
}
```

- [ ] **Step 2: Verify both backends identical.**
  Run: `phg run examples/realworld/ledger.phg` then `phg runvm examples/realworld/ledger.phg`
  Expected: identical stdout. If `runvm` errors/diverges, fix the program (likely a CTy/operand
  issue) until both match.

### Task A3: `examples/realworld/library.phg`

**Files:** Create: `examples/realworld/library.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

// A library catalogue. Note zero-payload variants (`Available`, `Lost`) are
// *constructed* with call form — `Available()` — but matched bare.

enum Availability {
    Available,
    Borrowed(string by),
    Lost,
}

class Book {
    constructor(private string title, private Availability status) {}

    function title_of() -> string { return title; }
    function status_of() -> Availability { return status; }
}

function describe(Availability a) -> string {
    return match a {
        Available    => "available",
        Borrowed(by) => "borrowed by {by}",
        Lost         => "lost",
    };
}

// Late fee: 25 cents per day (float arithmetic).
function fee(float days) -> float {
    return days * 0.25;
}

function main() {
    List<Book> shelf = [
        Book("The Rust Book", Available()),
        Book("Crafting Interpreters", Borrowed("Ada")),
        Book("SICP", Lost()),
    ];
    for (Book b in shelf) {
        println("{b.title_of()}: {describe(b.status_of())}");
    }
    println("late fee for 4.0 days = {fee(4.0)}c");
}
```

- [ ] **Step 2: Verify both backends identical** (as A2). Note the float display format — whatever
  `run` prints, `runvm` must match (shared `value.rs` display, so it will).

### Task A4: `examples/realworld/shop.phg`

**Files:** Create: `examples/realworld/shop.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

// A shopping cart with per-item discounts.

enum Discount {
    None,
    Percent(int pct),
    Flat(int cents),
}

class Item {
    constructor(private string name, private int cents, private Discount disc) {}

    function name_of() -> string { return name; }

    function price() -> int {
        return match disc {
            None       => cents,
            Percent(p) => cents - cents * p / 100,
            Flat(c)    => cents - c,
        };
    }
}

// Recursive bulk price for `n` identical units (no list folding needed).
function bulk(int unit, int n) -> int {
    if (n <= 0) {
        return 0;
    }
    return unit + bulk(unit, n - 1);
}

function main() {
    List<Item> cart = [
        Item("Widget", 1000, None()),
        Item("Gadget", 2000, Percent(10)),
        Item("Gizmo", 1500, Flat(300)),
    ];
    for (Item it in cart) {
        println("{it.name_of()} = {it.price()}c");
    }
    println("5x Widget = {bulk(1000, 5)}c");
}
```

- [ ] **Step 2: Verify both backends identical** (as A2).

### Task A5: `examples/realworld/rpg.phg`

**Files:** Create: `examples/realworld/rpg.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

// A turn-based RPG. Characters are immutable: `act` returns a new Character.

enum Action {
    Attack(int dmg),
    Heal(int hp),
    Defend,
}

class Character {
    constructor(private string name, private int hp) {}

    function name_of() -> string { return name; }
    function hp_of() -> int { return hp; }

    function act(Action a) -> Character {
        return match a {
            Attack(d) => Character(name, hp - d),
            Heal(h)   => Character(name, hp + h),
            Defend    => Character(name, hp),
        };
    }
}

function resolve(Action a) -> string {
    return match a {
        Attack(d) => "attacks for {d}",
        Heal(h)   => "heals {h}",
        Defend    => "defends",
    };
}

function main() {
    Character hero = Character("Hero", 30);
    Character hurt = hero.act(Attack(12));
    Character healed = hurt.act(Heal(5));
    println("{healed.name_of()} hp = {healed.hp_of()}");

    List<Action> turns = [Attack(12), Heal(5), Defend()];
    for (Action a in turns) {
        println(resolve(a));
    }
}
```

- [ ] **Step 2: Verify both backends identical** (as A2).

### Task A6: Gate + commit Wave A

- [ ] **Step 1:** `cargo test && cargo clippy --all-targets && cargo fmt --check` (the glob test now
  covers the four new files). Expected: green.
- [ ] **Step 2: Commit.**
```bash
git add examples/realworld
git commit -m "docs: four real-world examples (ledger, library, shop, rpg) — full runnable surface"
```

---

## Wave B — Six focused guide examples

For each: write the file, run `phg run` + `phg runvm` to confirm identical output, then
proceed. Commit once at the end of the wave (Task B7).

### Task B1: `examples/guide/operators.phg`

**Files:** Create: `examples/guide/operators.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

// Arithmetic, comparison, logical, and unary operators.
// Integer arithmetic is overflow-checked (a real overflow faults cleanly on both backends).

function main() {
    int a = 7;
    int b = 3;
    println("a + b = {a + b}");
    println("a - b = {a - b}");
    println("a * b = {a * b}");
    println("a / b = {a / b}");   // integer division
    println("a % b = {a % b}");

    float x = 7.0;
    float y = 2.0;
    println("x / y = {x / y}");   // float division

    println("a < b      = {a < b}");
    println("a == 7     = {a == 7}");
    println("a != b     = {a != b}");
    println("and        = {(a > b) && (x > y)}");
    println("or         = {(a < b) || (x > y)}");
    println("not        = {!(a < b)}");
    println("neg        = {-a}");
}
```

- [ ] **Step 2: Verify both backends identical.**

### Task B2: `examples/guide/control-flow.phg`

**Files:** Create: `examples/guide/control-flow.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

// recursion
function fact(int n) -> int {
    if (n <= 1) {
        return 1;
    }
    return n * fact(n - 1);
}

// mutual recursion
function is_even(int n) -> bool {
    if (n == 0) {
        return true;
    }
    return is_odd(n - 1);
}

function is_odd(int n) -> bool {
    if (n == 0) {
        return false;
    }
    return is_even(n - 1);
}

function main() {
    if (fact(5) > 100) {
        println("5! is big: {fact(5)}");
    } else {
        println("5! is small");
    }

    for (int i in [0, 1, 2, 3, 4]) {
        println("is_even({i}) = {is_even(i)}");
    }
}
```

- [ ] **Step 2: Verify both backends identical.**

### Task B3: `examples/guide/collections.phg`

**Files:** Create: `examples/guide/collections.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

class Point {
    constructor(private int x, private int y) {}
    function show() -> string { return "({x}, {y})"; }
}

function main() {
    List<int> nums = [10, 20, 30];
    for (int n in nums) {
        println("n = {n}");
    }

    List<List<int>> grid = [[1, 2], [3, 4]];
    for (List<int> row in grid) {
        for (int v in row) {
            println("v = {v}");
        }
    }

    List<Point> pts = [Point(0, 0), Point(1, 2)];
    for (Point p in pts) {
        println(p.show());
    }
}
```

- [ ] **Step 2: Verify both backends identical.** (If nested `for…in` or `List<List<int>>`
  diverges, fall back to a flat list and note the limitation in the README.)

### Task B4: `examples/guide/classes.phg`

**Files:** Create: `examples/guide/classes.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

class Engine {
    constructor(private int power) {}
    function power_of() -> int { return power; }
}

class Car {
    // composition: a Car *has an* Engine
    constructor(private string make, private Engine engine) {}

    function make_of() -> string { return make; }
    // a method that calls a method on a field (the Wave-4 class-aware path)
    function horsepower() -> int { return engine.power_of(); }
}

function main() {
    Car c = Car("Phorge", Engine(250));
    println("{c.make_of()} has {c.horsepower()} hp");
}
```

- [ ] **Step 2: Verify both backends identical.** (Confirms `engine.power_of()` — a method call on
  a class-typed field — compiles on the VM.)

### Task B5: `examples/guide/enums-match.phg`

**Files:** Create: `examples/guide/enums-match.phg`

- [ ] **Step 1: Write the file.**

```
import std.io;

// Variant patterns (with bindings) over a payload+zero-payload enum.
enum Json {
    JNull,
    JBool(bool b),
    JNum(int n),
    JStr(string s),
}

function show(Json j) -> string {
    return match j {
        JNull()  => "null",
        JBool(b) => "bool {b}",
        JNum(n)  => "num {n}",
        JStr(s)  => "str {s}",
    };
}

// Literal pattern + binding pattern (catch-all) over a primitive scrutinee.
function sign(int n) -> string {
    return match n {
        0 => "zero",
        x => "nonzero {x}",
    };
}

function main() {
    List<Json> vals = [JNull(), JBool(true), JNum(42), JStr("hi")];
    for (Json j in vals) {
        println(show(j));
    }
    println(sign(0));
    println(sign(7));
}
```

- [ ] **Step 2: Verify both backends identical.**

### Task B6: `examples/guide/strings.phg`

**Files:** Create: `examples/guide/strings.phg`

- [ ] **Step 1: Write the file.** (Avoid nested string literals inside `{…}` — bind first; the
  interpolation sub-lexer does not nest quotes/`match`.)

```
import std.io;

function greet(string who) -> string {
    return "Hello, {who}!";
}

function main() {
    string name = "Phorge";
    int n = 3;
    float pi = 3.14;
    println("name = {name}, n = {n}, pi = {pi}");
    println("n + 1 = {n + 1}");
    println(greet(name));

    string w = "world";
    println("nested call: {greet(w)}");
}
```

- [ ] **Step 2: Verify both backends identical.**

### Task B7: Gate + commit Wave B

- [ ] **Step 1:** `cargo test && cargo clippy --all-targets && cargo fmt --check`. Expected: green.
- [ ] **Step 2: Commit.**
```bash
git add examples/guide
git commit -m "docs: six focused guide examples (operators, control-flow, collections, classes, enums-match, strings)"
```

---

## Wave C — Transpile/PHP bridge + README index

### Task C1: `examples/transpile/demo.phg` + generated `demo.php`

**Files:** Create: `examples/transpile/demo.phg`, `examples/transpile/demo.php`

- [ ] **Step 1: Write `examples/transpile/demo.phg`.**

```
import std.io;

// This program is also in the byte-identity sweep. `phg transpile` turns it into
// runnable PHP 8.x — the only Phorge↔PHP-ecosystem path (output, not input).

enum Shape {
    Circle(float r),
    Square(float side),
}

class Named {
    constructor(private string label) {}
    function label_of() -> string { return label; }
}

function area(Shape s) -> float {
    return match s {
        Circle(r)    => 3.14159 * r * r,
        Square(side) => side * side,
    };
}

function main() {
    Named n = Named("demo");
    println("{n.label_of()}: circle area = {area(Circle(2.0))}");
}
```

- [ ] **Step 2: Verify run/runvm identical.**
  Run: `phg run examples/transpile/demo.phg` and `phg runvm examples/transpile/demo.phg`.

- [ ] **Step 3: Generate the committed PHP.**
  Run: `phg transpile examples/transpile/demo.phg > examples/transpile/demo.php`
  Then inspect it (`cat`) to confirm it is valid-looking PHP (`<?php`, a `main()` call at the end).

### Task C2: Transpile snapshot test

**Files:** Modify: `tests/cli.rs` (add one test, mirroring the existing `transpile_*` tests)

- [ ] **Step 1: Add the snapshot test.** (Use the same CLI-invocation helper the other `tests/cli.rs`
  tests use; capture stdout of `transpile examples/transpile/demo.phg` and compare to the committed
  file.)

```rust
#[test]
fn transpile_demo_matches_committed_php() {
    let expected = std::fs::read_to_string("examples/transpile/demo.php")
        .expect("read committed demo.php");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_phg"))
        .args(["transpile", "examples/transpile/demo.phg"])
        .output()
        .expect("run phg transpile");
    assert!(output.status.success(), "transpile exited non-zero");
    let actual = String::from_utf8(output.stdout).expect("utf-8 php");
    assert_eq!(
        actual, expected,
        "generated PHP drifted from examples/transpile/demo.php — regenerate it"
    );
}
```

- [ ] **Step 2: Run it.**
  Run: `cargo test --test cli transpile_demo_matches_committed_php`
  Expected: PASS. (If the existing tests use `assert_cmd`/a helper instead of raw `Command`, match
  that style — read the top of `tests/cli.rs` first and reuse its pattern.)

### Task C3: `examples/transpile/README.md`

**Files:** Create: `examples/transpile/README.md`

- [ ] **Step 1: Write it** — show the transpile command, how to run the PHP, and the honest scope:

```markdown
# Phorge → PHP

Phorge can transpile to runnable **PHP 8.x**. This is the only Phorge↔PHP-ecosystem path: the
transpiler *produces* PHP source; Phorge does not consume composer/PHP packages.

```bash
phg transpile demo.phg > demo.php   # regenerate the committed output
php demo.php                            # run it under any PHP 8.x
```

`demo.php` in this directory is the committed output of `phg transpile demo.phg`, kept in sync by
a snapshot test (`tests/cli.rs::transpile_demo_matches_committed_php`). `demo.phg` itself also runs
on both native backends (`phg run` / `phg runvm`).
```

### Task C4: `examples/README.md` index + coverage matrix

**Files:** Create: `examples/README.md`

- [ ] **Step 1: Write the index** — one-line-per-example listing, the coverage matrix (spec §3), the
  honest notes (`import` is decorative until M5; PHP is via transpile only), and the explicit
  not-yet-supported list (`null`, `T?`, `Map`/`Set` values, `|>`, exceptions, traits, overloading,
  sized ints, `decimal`). Mark it as the "what Phorge can do today" page, updated as examples are
  added. (Content drafted at execution against the final file set so the listing is accurate.)

### Task C5: CHANGELOG + pointers, gate + commit

**Files:** Modify: `CHANGELOG.md`, `docs/MILESTONES.md` (or `CLAUDE.md`) — add an "Examples" pointer.

- [ ] **Step 1:** Add a `CHANGELOG.md` `[Unreleased]` entry describing the example set + the glob
  sweep + the transpile snapshot. Add one line to `docs/MILESTONES.md` (under M1/M2) pointing to
  `examples/README.md` as the living language-surface showcase.
- [ ] **Step 2: Gate.** `cargo test && cargo clippy --all-targets && cargo fmt --check`. Green.
- [ ] **Step 3: Commit.**
```bash
git add examples/transpile examples/README.md tests/cli.rs CHANGELOG.md docs/MILESTONES.md
git commit -m "docs: Phorge→PHP transpile example + examples README index + coverage matrix"
```

---

## Acceptance criteria (spec §10)

- [ ] Every `.phg` under `examples/` runs byte-identically on `run` and `runvm` (glob test green).
- [ ] `examples/README.md` coverage matrix maps every runnable feature to ≥1 example and lists
  excluded features honestly.
- [ ] `examples/transpile/demo.php` matches freshly-generated output (snapshot test green).
- [ ] `cargo test` green, `cargo clippy --all-targets` clean, `cargo fmt --check` clean.

## Risks & rollback

- **Risk — a designed program hits an unanticipated parity gap** (e.g. nested `for…in`, a method
  call on a field, enum-typed class field). Mitigation: each example has a "verify both backends"
  step *before* commit; the glob test fails loudly otherwise. Fall back to a simpler form and note
  the limitation in the README. **Risk — float display format surprise:** both backends share
  `value.rs` display, so they cannot disagree; whatever prints is the contract.
- **Rollback:** examples-only + one test; `git revert` the wave commit. No `src/` runtime change.

## Decisions Log

- [2026-06-16] AGREED: examples set = 4 real-world + 6 guide + 1 transpile demo; glob `examples/**/
  *.phg` into the differential harness; `import`/PHP documented honestly (no fake examples). See
  `docs/specs/2026-06-16-examples-coverage-design.md`.
- [2026-06-16] AGREED: zero-payload enum variants constructed with `()`; `is` omitted; state evolves
  by fresh bindings/recursion (no reassignment/list-folding) — all verified against the surface.
