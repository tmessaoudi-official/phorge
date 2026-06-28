# ADR-0004: PHP emission is a single self-contained brace-namespace file

- **Status:** Accepted (2026-06-19)
- **Deciders:** project author
- **Fuller design:** `docs/specs/2026-06-18-m5-project-model-design.md` — decision **M5-7** (and §4,
  the free-function nuance that forces it).

## Context

The conventional multi-file PHP target is PSR-4: one class per file, autoloaded by directory. But
**PSR-4 autoloads classes, not free functions**, and Phorj is **function-heavy** (a `package` is a
bag of functions, not exclusively classes). A faithful multi-file PSR-4 emission therefore cannot
autoload most of what Phorj emits.

## Decision

Emit **one self-contained, `php out.php`-runnable file**: a `namespace App\Util { … }` brace-block
per package (each path segment PascalCased), plus a nameless `namespace { \App\Main\main(); }`
bootstrap that invokes the entry point. **Zero Composer, zero autoloader, deterministic.** Native
calls are emitted fully-qualified with a leading `\` so an unqualified builtin still resolves to the
global namespace from inside a `namespace` block.

## Consequences

- **Deterministic, byte-identical output** — a single file with no load-order ambiguity keeps the
  `run ≡ runvm ≡ php` spine intact (no autoloader nondeterminism to reason about).
- The transpiled program runs under stock `php` with **no external tooling** to install or configure.
- One artifact to ship, read, and diff — aligns with the "honest to read" GA goal.

## Alternatives rejected

- **Multi-file PSR-4 emission** — cannot autoload free functions, which are the bulk of Phorj code.
- **Literal `composer.json`** — a file the `composer` tool **cannot actually process** (no Packagist,
  no autoloader Phorj uses); shipping it would be a false promise. The manifest instead borrows
  Composer's *vocabulary* (`[require]`/`vendor/package`) inside an honest TOML container (see
  [ADR-0005](0005-offline-only-vendor.md)).
