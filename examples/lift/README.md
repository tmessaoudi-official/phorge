# `phg lift` — PHP → Phorge

`lift` is the **inverse of `transpile`**: it reads PHP and emits a Phorge **draft**.

Where `transpile` is *total and byte-identity-verified* (every Phorge program has one correct PHP
translation), `lift` is **best-effort and review-required** — PHP is larger and dynamic, Phorge is
smaller and typed, so the map is partial by nature. The output is a scaffold a human checks, prefixed
`// lifted (verify)`. Anything outside the supported subset is refused with a clear `lift …` error
rather than guessed at — lift **never** silently produces wrong Phorge.

## Try it

```console
$ phg lift sample.php
```

Input — [`sample.php`](sample.php), ordinary typed PHP (note the double-quoted **interpolation**):

```php
function greet(string $name): string {
    return "Hello, $name!";
}

class Counter {
    public function __construct(public int $start) {}
    public function next(): int { return $this->start + 1; }
}

$c = new Counter(41);
echo greet("Phorge");
echo " Counter starts at $c->start, next is {$c->next()}.";
```

Output — [`sample.phg`](sample.phg), idiomatic Phorge (PHP is the *floor*, not the ceiling — lift
emits clean Phorge, it doesn't mirror PHP's quirks). PHP interpolation maps straight to Phorge holes:
`"$name"` → `"{name}"`, `"$c->start"` → `"{c.start}"`, `"{$c->next()}"` → `"{c.next()}"`:

```phorge
package Main;
import Core.Console;

function greet(string name) -> string {
    return "Hello, {name}!";
}

open class Counter {
    constructor(public mutable int start) {}
    public open function next() -> int {
        return this.start + 1;
    }
}

function main() -> void {
    mutable var c = new Counter(41);
    Console.print(greet("Phorge"));
    Console.print(" Counter starts at {c.start}, next is {c.next()}.");
}
```

Both print `Hello, Phorge! Counter starts at 41, next is 42.` The lifted `sample.phg` is part of the
example suite, so it is byte-identity-gated on `run`, `runvm`, **and** real PHP like every other
example.

## What lift does (idiomatic, not a mirror)

| PHP | Phorge |
|---|---|
| top-level statements | a synthesized `function main()` (the runnable entry) |
| the whole file | `package Main;` (PHP has no packages) |
| `$x = e` | `mutable var x = e;` (PHP locals are freely reassignable) |
| `.` string concat / `===` / `!==` | `+` / `==` / `!=` (Phorge is typed) |
| `echo e;` | `Console.print(e);` (+ an automatic `import Core.Console;`) |
| `__construct` + promoted params | a `constructor` with promoted (mutable) fields |
| a non-`final` PHP class | an `open` class (Phorge is final-by-default) |
| `[a, b]` / `[k => v]` | a `List` / a `Map` |
| ternary `c ? a : b` / `match` | an expression `if` / a Phorge `match` |
| `"$name"` / `"$o->prop"` / `"{$o->m()}"` interpolation | Phorge `"{name}"` / `"{o.prop}"` / `"{o.m()}"` holes |
| `foreach ($xs as $x)` (keyless) | Phorge `foreach (xs as x)` — element type inferred (A-6) |

## What lift refuses (loudly — the Tier-2 frontier)

Lift errors rather than guess when there is no faithful Phorge form *yet*: an `array` **type**
annotation (needs `List`/`Map`/`Set` inference), a **key/value** `foreach ($xs as $k => $v)` (Phorge's
`foreach` has no key binding yet), backed enums and enum methods, default parameter values, untyped
parameters, the elvis `?:`, an assignment used as a sub-expression, and a non-literal `match` arm.
Each is a clear `lift …` message naming what to do by hand.

Interpolation is lifted only within PHP's *actual* grammar — a `$`-rooted access chain (`$x`,
`$o->p`, `$a[$k]`, `$o->m()`). The forms PHP itself rejects or that coerce silently are refused
loudly: a top-level operator inside `{$…}` (a PHP parse error too), the removed `${…}`
variable-variable form, and a simple-syntax bareword subscript `"$a[key]"` (whose key silently
becomes the string `'key'` — use the explicit `"{$a['key']}"` form).

> **Review the draft.** A lifted program that type-checks is *structurally* sound, but `lift` cannot
> prove it preserves the original PHP's behavior — that is the `// lifted (verify)` contract.
