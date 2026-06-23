# `project/visibility/` — declaration visibility (`public` / `internal` / `private`)

Phorge gives every **top-level declaration** (class, enum, interface, free function) a visibility
level. It is the declaration analog of member visibility, and it follows Phorge's strictest-sensible
default — a `private` declaration leaks *nothing*, not even to a sibling file, unless you widen it.

## The three levels — a lattice `file ⊂ package ⊂ public`

| Keyword | Visible to | Note |
|---|---|---|
| `public` (default — omit the keyword) | any package that imports it | cross-package surface |
| `internal` | every file of **this package** | not other packages |
| `private` | **this `.phg` file** only | not even sibling files of the same package |

A reference from site **R** to declaration **D** is legal iff: **same file** → always; **same package,
other file** → `D` must be `≥ internal`; **other package** → `D` must be `public`.

Visibility is **loader-enforced and erased from the backend** — PHP has no file/package-private
declarations, so the transpiler emits a normal `class`/`function`. The `run ≡ runvm ≡ real PHP`
byte-identity spine is unaffected.

## This project

```
src/
  main.phg                  package Main      — imports the public Rect across packages
  Acme/Shapes/shapes.phg    package Acme.Shapes — public class Rect; internal fn scale
  Acme/Shapes/helpers.phg   package Acme.Shapes — internal fn factor; private fn clamp
```

Run it (byte-identical on both backends and real PHP):

```
phg run     src/main.phg     # area: 12
phg runvm   src/main.phg     # area: 12
phg transpile src/main.phg | php   # area: 12
```

The legal references it exercises:

- `main` imports `Rect` — **public**, so the cross-package `import type Acme.Shapes.Rect;` is allowed.
- `scale` (in `shapes.phg`) calls `factor` (in `helpers.phg`) — both **internal**, same package, different
  file — allowed.
- `factor` calls `clamp` — `clamp` is **private** but the call is in the *same file* — allowed.

## The rejected cases (why these can't be runnable examples)

Every shipped example must produce identical *Ok* output, so a compile error can't be one — it is
documented here instead. Each of the following, added to `main.phg`, is a **compile error**:

```phorge
// scale is `internal` to Acme.Shapes — not exportable to another package:
import type Acme.Shapes.Scale;   //  no such public type
Shapes.scale(12);                  //  E-VIS-INTERNAL: scale is internal to Acme.Shapes

// clamp is `private` to helpers.phg — not visible to any other file:
//   (referenced from shapes.phg or main.phg)
clamp(1);                        //  E-VIS-PRIVATE: clamp is private to helpers.phg
```

Run `phg explain E-VIS-INTERNAL` or `phg explain E-VIS-PRIVATE` for the full guidance.
