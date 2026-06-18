# Multi-file projects (M5)

Single `.phg` files are great for scripts, but real programs span many files and packages. Phorge's
**project model** (milestone M5) is Go-shaped: every file declares a `package`, the folder layout
*is* the package path, and cross-package functions are imported and called leaf-qualified — and it
all transpiles to idiomatic namespaced PHP.

Each subdirectory here is a self-contained project, discovered by its `phorge.toml`. Like every
other example, each one runs byte-identically on both backends — `tests/differential.rs` finds every
project root and asserts `run` ≡ `runvm` (and that it runs at all).

## `tempconv/` — a two-package Celsius→Fahrenheit converter

```
tempconv/
├── phorge.toml                     # name = "acme/tempconv", source = "src"
└── src/
    ├── main.phg                    # package main   — the runnable entry
    └── acme/
        ├── convert/                # package acme.convert  (folder = path)
        │   ├── temp.phg            #   c_to_f(c) = scale(c) + 32
        │   └── scale.phg           #   scale(c)  = c * 9 / 5
        └── label/                  # package acme.label
            └── label.phg           #   tag(name, v) -> "{name} = {v}F"
```

Run it (the CLI walks up to `phorge.toml`, loads the whole project, and runs `package main`):

```console
$ phg run examples/project/tempconv/src/main.phg
freezing = 32F
boiling = 212F

$ phg runvm examples/project/tempconv/src/main.phg   # byte-identical
freezing = 32F
boiling = 212F
```

### What it demonstrates

1. **Mandatory packages + folder = path.** Each file's first line is a `package` declaration, never
   inferred. A dotted library package must live in the matching directory under the source root:
   `package acme.convert;` ⇒ `src/acme/convert/`. The reserved `package main;` is the runnable entry
   and is folder-exempt. A mismatch is a load error (`E-PKG-PATH`).
2. **Cross-package qualified calls + aliasing.** `main` imports a package and calls its functions
   *leaf-qualified* — `import acme.convert;` then `convert.c_to_f(0)` (Go's `import "fmt"` →
   `fmt.Println`). An import can be renamed with `as`: `import acme.label as fmt;` binds the leaf
   `fmt`, so the call is `fmt.tag(...)`.
3. **Same-package calls across files.** A package may span multiple files. In `acme.convert`,
   `c_to_f` (temp.phg) calls `scale` (scale.phg) by its **bare** name — same package, no
   qualification — and the loader resolves both consistently.

### The PHP it transpiles to

`phg transpile examples/project/tempconv/src/main.phg` emits one PHP `namespace` block per package
plus a bootstrap that invokes `main` last (so every function is declared before it runs):

```php
<?php
namespace Acme\Convert {
    function scale(int $c): int { return $c * 9 / 5; }
    function c_to_f(int $c): int { return \Acme\Convert\scale($c) + 32; }
}
namespace Acme\Label {
    function tag(string $name, int $value): string { return ($name) . " = " . ($value) . "F"; }
}
namespace Main {
    function main(): void { /* … */ }
}
namespace {
    \Main\main();
}
```

Package segments PascalCase into PHP namespaces (`acme.convert` ⇒ `Acme\Convert`); cross-package
calls emit fully-qualified (`\Acme\Convert\c_to_f`). It runs under a bare `php out.php` — no Composer
and no autoloader (PSR-4 can't autoload free functions, and Phorge is function-heavy).

> The conversions use **exact integer arithmetic** (0→32, 100→212) on purpose: a non-whole result
> would render differently under PHP's float `/` than under Phorge's integer `/`, so the example
> sticks to values that are identical across all three. The `run` ≡ `runvm` spine is always identical
> regardless.

## Scope (M5 S2c/S2d)

Library packages export **functions only** for now — a `class`/`enum` in a non-`main` package is
rejected (`E-PKG-TYPE`); cross-package types are an M5 follow-up. Git-based dependencies
(`[require]` in `phorge.toml`), `phorge.lock`, and vendoring arrive in **M5 S3**.
