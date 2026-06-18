# `withdeps` — a project with a vendored git dependency (M5 S3)

This project depends on an external library package, `acme/strutil`, fetched as a **git dependency**
and **vendored** for offline, deterministic builds. It is the companion showcase for M5 S3
(git deps + `phorge.lock` + `phg vendor` + auto-offline).

## Layout

```
withdeps/
├── phorge.toml                     # name + [require] git dependency
├── phorge.lock                     # resolved commit SHA + content hash (generated)
├── src/
│   └── main.phg                    # package main — imports & calls acme.strutil
└── vendor/                         # committed offline dependency tree (generated)
    └── acme/strutil/               #   vendor/<vendor>/<package>/ — this dep's own root
        └── acme/strutil/
            └── text.phg            #   package acme.strutil
```

## Run it

```sh
phg run   src/main.phg          # tree-walking interpreter
phg runvm src/main.phg          # bytecode VM (byte-identical)
phg transpile src/main.phg | php
```

All three print the same two lines — the vendored dependency is consumed exactly like a first-party
package:

```
== Phorge deps ==
vendored offline!
```

## How dependencies work (Go's vendoring model, Composer's vocabulary)

`phorge.toml` declares the dependency under `[require]`, pinned to a tag or rev — **never a moving
branch** (determinism):

```toml
[require]
"acme/strutil" = { git = "https://github.com/phorge-lang/example-strutil.phg", tag = "v0.1.0" }
```

`phg vendor` is the **only** command that touches the network. It clones each dependency at its
pin, copies the dependency's source into `vendor/<vendor>/<package>/`, and writes `phorge.lock`
pinning the **resolved commit SHA** plus a content hash:

```sh
phg vendor            # fetch [require] deps into vendor/ + (re)write phorge.lock
```

`vendor/` and `phorge.lock` are then **committed**. At run time `phg run`/`runvm`/`transpile`
resolve dependencies **entirely offline** from the committed `vendor/` — they never fetch. This is
what keeps every example (this one included) byte-identical on both backends and reproducible with
zero network, the same determinism rule that defers URL/network features to M6.

## Notes

- **Illustrative dependency.** `acme/strutil`'s source is committed under `vendor/` (Go's vendoring
  model). The `git` URL is a documented coordinate; its source is right here, so the example runs
  with no network. `rev` and `hash` in `phorge.lock` are the real values for the vendored source.
- **A dependency is a library:** it exports dotted packages (here `package acme.strutil;`), never
  `package main` — that is reserved for the consuming program's entry.
- **Transpiled PHP:** the vendored package becomes a `namespace Acme\Strutil { … }` block in the
  emitted single-file PHP, called as `\Acme\Strutil\banner(...)` — and runs under stock `php`.
- **Not yet:** transitive dependencies (a dependency's own `[require]`) are resolved in a follow-up;
  `phg vendor` currently vendors the direct `[require]` set.
