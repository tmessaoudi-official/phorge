# Import roots (PSR-4 mapping + `vendor:`) — design

> Status: designed, not implemented. M-DOGFOOD follow-on (developer-driven). Goal: know **by eye and
> for the resolver** exactly where every import comes from, and **decouple a package's namespace from
> its folder** (PSR-4). Breaking change to the M5 import/package model — needs a migration codemod.

## Two orthogonal axes (the core clarification)

- **Namespace** — the logical package name written in code and emitted as the PHP namespace
  (`App.Data` → `namespace App\Data`).
- **Root / origin** — which *directory* the files physically live in (`src/`, `bin/`, `vendor/`).

The design keeps these separate. Origin is conveyed **by the namespace root + the `vendor:` marker**,
not by a per-import prefix on everything.

## The model

- **`Core.` — reserved stdlib** (unchanged). Import-free-*types* stay builtin; stdlib *functions* import
  as today (`import Core.String;`).
- **First-party — bare, resolved by convention or `[packages]`.** `import App.Data;` — no prefix.
- **Vendored deps — `vendor:` prefix, required.** `import vendor:Acme.Strutil;` — outside your control,
  could collide with a first-party root, so the prefix both disambiguates and signals "external" by eye.

By eye: `Core.` = stdlib · `vendor:` = dependency · everything else = first-party (per `[packages]`/
convention). Less ceremony than prefixing everything; the namespace root already carries origin for
first-party code.

## Resolution rule (answers "what if I don't declare `App = "src"`?")

```toml
# phorj.toml — OPTIONAL; overrides/extends the default convention
[packages]
App     = "src"      # App.*  ⇒ files directly under src/   (App.Data = src/Data.phg)
Console = "bin"      # Console.* ⇒ bin/
Migrations = "migrations"
```

1. **No `[packages]` entry for a root → default convention:** the source root is **`src/`** and
   **folder = namespace path**. `import App.Data` ⇒ `src/App/Data.phg` (the `App` segment is a real
   subfolder of `src/`). Zero-config works for a conventional project.
2. **A `[packages]` entry `App = "src"` → the root aliases that directory:** `App.` maps to `src/`
   *itself*, so `App.Data` ⇒ `src/Data.phg` (files flat in `src/`, no `App/` subfolder) — the
   "src folder, `App.` namespace" case. Decouples namespace name from folder name.
3. **A `[packages]` entry for another folder → an additional root:** `Console = "bin"` ⇒
   `import Console.Run` resolves `bin/Run.phg`, emitted as `namespace Console\Run`.
4. **`vendor:` imports** resolve from the vendored source under `vendor/` (existing M5 S3 machinery);
   the dep must be in `[require]` and vendored (`E-VENDOR-MISSING` otherwise). Namespace = the path
   after `vendor:` (`vendor:Acme.Strutil` ⇒ `namespace Acme\Strutil`).

**Emitted PHP namespace is always the namespace root path, never the folder** — `App.Data` → `App\Data`
even when `App = "src"`. Folder mapping is a loader concern; PHP output is folder-independent (PSR-4).

## Surfaces

- **Manifest** (`src/manifest.rs`) — parse `[packages]` → `Map<NamespaceRoot, Dir>`. Validate: root is
  PascalCase, not `Core`/`Main`/`vendor`; dir exists; no duplicate root; no two roots on the same dir
  clash. New codes `E-PKG-ROOT-*`.
- **Parser** — allow an optional `vendor:` marker on an import path: `import [vendor:] Path [as Alias];`.
  `vendor` + `:` at import position (contextual — `vendor` stays a legal identifier elsewhere). Only
  `vendor:` is accepted this slice (the sole non-first-party, non-`Core` origin).
- **Loader** (`src/loader.rs`) — build the root→dir table (defaults + `[packages]`); resolve a bare
  import through it (rule 1/2/3), a `vendor:` import through the vendored tree (rule 4). Folder=path
  validation runs *within* each root. Cross-package mangling/resolution (M5 S2c) keys on the resolved
  namespace, unchanged after this front-matter.
- **Checker** — the import map (leaf → module) distinguishes first-party vs `vendor:`; a bare import of
  an undeclared/unfound root is `E-UNKNOWN-ROOT` (with a did-you-mean over declared roots + `vendor:`);
  `E-SHADOW-IMPORT` still guards a local shadowing an imported qualifier.
- **Transpiler** — emit `namespace <Root>\<Path>` from the namespace (folder-independent); vendored
  packages de-mangle to their `Acme\…` block as today. Single-package first-party output stays flat.

## Scope / decisions

- **Default root = `src/`, folder=path** (rule 1) — LOCKED (developer, 2026-07-01): `[packages]` is
  optional and purely additive/overriding; zero-config projects keep working (unmapped first-party
  resolves as `src/` + folder=path). Rejected the "mandatory, no default" alternative as too heavy for
  small projects.
- **`vendor:` is the only prefix** this slice. A future slice could add user prefixes if a real need
  appears, but the namespace-root already disambiguates first-party, so none is planned.
- **Breaking** — existing examples/projects (`package Main;` + bare `import Acme.Util` for a vendored
  dep) must migrate: vendored imports gain `vendor:`, and any decoupled layout gains a `[packages]`
  entry. A codemod (`tools/import_roots_migrate.py`, dry-run first) mirrors the naming-overhaul model.
  Distributable coordinates (`[require]` keys, vendor dir, lockfile) stay lowercase.
- **`package Main;`** (runnable entry) is unchanged; `Main` is a reserved root resolved at the project
  source root.

## Testing / byte-identity

- Manifest unit tests (`[packages]` parse + `E-PKG-ROOT-*`).
- Loader tests: default-convention resolution, `App = "src"` alias, extra root (`bin`), `vendor:`
  resolution + `E-VENDOR-MISSING`, `E-UNKNOWN-ROOT`.
- A migrated multi-root example project under `examples/project/` (first-party `[packages]` alias + a
  `bin/` root + a vendored `vendor:` dep) gated by the project-aware differential (`run≡runvm≡real PHP`).
- Output-preserving: single-package first-party programs stay byte-identical (namespace emission
  unchanged); the reshape is front-matter + loader only.
