# The `phorge` CLI — source forms, inspection, and diagnostics

Beyond `run` / `runvm`, the CLI takes a program three ways, exposes the front-end stages, and ships a
diagnostic dictionary. `demo.phg` is the program used below (and, like every example, it is in the
byte-identity sweep). Run `phg <command> --help` for per-command help with worked examples.

## Three ways to give it a program

```bash
phg run demo.phg                                              # a file
echo 'package main; import core.console; function main() { console.println("from stdin"); }' | phg run -   # stdin
phg run -e 'package main; import core.console; function main() { console.println("inline program"); }'     # inline
```

```
$ phg run demo.phg
phorge CLI demo
n doubled = 12
```

`run -- <file>` forces a literal path (for a filename that would otherwise look like a flag). The
same source forms work for `runvm`, `check`, `parse`, `lex`, and `transpile`.

## Inspecting the front end

```bash
phg check demo.phg     # lex + parse + type-check, no execution
phg lex   demo.phg     # the token stream
phg parse demo.phg     # the AST
```

```
$ phg check demo.phg
OK (type-checks clean)

$ phg lex demo.phg
Package @ 1:1
Ident("main") @ 1:9
Semicolon @ 1:13
Import @ 2:1
Ident("core") @ 2:8
Dot @ 2:12
Ident("console") @ 2:13
Semicolon @ 2:20
...

$ phg parse demo.phg
Program {
    package: ["main"],
    items: [
        Import { path: ["core", "console"], .. },
        Function(FunctionDecl { name: "main", ret: None, body: [ .. ] }),
    ],
}
```

(`lex` and `parse` print the full token / AST dump — abbreviated here.)

## Diagnostics

Front-end errors carry a caret-underlined span, a stable code, and a did-you-mean hint when a close
name is in scope:

```
$ phg run -e 'package main; import core.console; function main() { int count = 1; int y = conut + 1; console.println("{y}"); }'
type error at 1:77: unknown identifier `conut`
package main; import core.console; function main() { int count = 1; int y = conut + 1; console.println("{y}"); }
                                                                            ^
  [E-UNKNOWN-IDENT]
  hint: did you mean `count`?
```

Look any code up in the dictionary with `explain`:

```
$ phg explain E-UNKNOWN-IDENT
E-UNKNOWN-IDENT — a name was used that is not in scope.

Phorge resolves identifiers lexically: block-scope locals (including `var` bindings
and `for` loop variables), parameters, top-level functions, and — inside a method —
the current class's fields. ...
```

## Faults never panic

Phorge never panics on input — runtime faults are clean, one-line errors with exit code 1:

```
$ phg run   -e 'package main; function main() { int a = 10; int b = 0; int x = a / b; }'
runtime error: division by zero

$ phg runvm -e 'package main; function main() { int a = 10; int b = 0; int x = a / b; }'
runtime error at 1: division by zero

$ phg run   -e 'package main; function main() { List<int> xs = [1, 2]; int v = xs[5]; }'
runtime error: list index out of range
```

Both backends fault on the same condition with the same message *body*; the VM also reports the line
(`at 1`). The differential harness (`tests/differential.rs`) gates that `run` and `runvm` fault on
exactly the same inputs — the same checked-arithmetic / bounds-checking guarantee (integer overflow,
division by zero, out-of-range indexing) that `guide/operators.phg` describes.
