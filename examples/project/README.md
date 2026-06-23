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
├── phorge.toml                     # module = "acme/tempconv", source = "src"
└── src/
    ├── main.phg                    # package Main   — the runnable entry
    └── Acme/
        ├── Convert/                # package Acme.Convert  (folder = path)
        │   ├── temp.phg            #   cToF(c) = scale(c) + 32
        │   └── scale.phg           #   scale(c)  = c * 9 / 5
        └── Label/                  # package Acme.Label
            └── label.phg           #   tag(name, v) -> "{name} = {v}F"
```

Run it (the CLI walks up to `phorge.toml`, loads the whole project, and runs `package Main`):

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
   `package Acme.Convert;` ⇒ `src/Acme/Convert/`. The reserved `package Main;` is the runnable entry
   and is folder-exempt. A mismatch is a load error (`E-PKG-PATH`).
2. **Cross-package qualified calls + aliasing.** `main` imports a package and calls its functions
   *leaf-qualified* — `import Acme.Convert;` then `Convert.cToF(0)` (Go's `import "fmt"` →
   `fmt.Println`). An import can be renamed with `as`: `import Acme.Label as Fmt;` binds the leaf
   `Fmt`, so the call is `Fmt.tag(...)`.
3. **Same-package calls across files.** A package may span multiple files. In `Acme.Convert`,
   `cToF` (temp.phg) calls `scale` (scale.phg) by its **bare** name — same package, no
   qualification — and the loader resolves both consistently.

### The PHP it transpiles to

`phg transpile examples/project/tempconv/src/main.phg` emits one PHP `namespace` block per package
plus a bootstrap that invokes `main` last (so every function is declared before it runs):

```php
<?php
namespace Acme\Convert {
    function scale(int $c): int { return $c * 9 / 5; }
    function cToF(int $c): int { return \Acme\Convert\scale($c) + 32; }
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

Package segments map **1:1** to PHP namespaces (`Acme.Convert` ⇒ `Acme\Convert`) — segments are
PascalCase at the source, so there is no casing transform; cross-package calls emit fully-qualified
(`\Acme\Convert\cToF`). It runs under a bare `php out.php` — no Composer and no autoloader (PSR-4
can't autoload free functions, and Phorge is function-heavy).

> The conversions use **exact integer arithmetic** (0→32, 100→212) on purpose: a non-whole result
> would render differently under PHP's float `/` than under Phorge's integer `/`, so the example
> sticks to values that are identical across all three. The `run` ≡ `runvm` spine is always identical
> regardless.

## Scope

Library packages export **functions and types** — a `class`/`enum`/`interface` in a library package
is consumed cross-package via `import type Pkg.Path.TypeName;` (see `shapes/`). Git-based
dependencies (`[require]` in `phorge.toml`), `phorge.lock`, and vendoring ship in M5 S3 (see
`withdeps/`). Casing is enforced: package/folder segments are PascalCase (`E-PKG-CASE`), types are
PascalCase, functions/variables are camelCase.
