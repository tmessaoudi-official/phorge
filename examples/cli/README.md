# The `phorge` CLI — source forms, inspection, and diagnostics

Beyond `run` / `runvm`, the CLI takes a program three ways, exposes the front-end stages, and ships a
diagnostic dictionary. `demo.phg` is the program used below (and, like every example, it is in the
byte-identity sweep). Run `phorge <command> --help` for per-command help with worked examples.

## Three ways to give it a program

```bash
phorge run demo.phg                                              # a file
echo 'function main() { console.println("from stdin"); }' | phorge run -   # stdin
phorge run -e 'function main() { console.println("inline program"); }'     # inline
```

```
$ phorge run demo.phg
phorge CLI demo
n doubled = 12
```

`run -- <file>` forces a literal path (for a filename that would otherwise look like a flag). The
same source forms work for `runvm`, `check`, `parse`, `lex`, and `transpile`.

## Inspecting the front end

```bash
phorge check demo.phg     # lex + parse + type-check, no execution
phorge lex   demo.phg     # the token stream
phorge parse demo.phg     # the AST
```

```
$ phorge check demo.phg
OK (type-checks clean)

$ phorge lex demo.phg
Import @ 1:1
Ident("std") @ 1:8
Dot @ 1:11
Ident("io") @ 1:12
Semicolon @ 1:14
Function @ 6:1
...

$ phorge parse demo.phg
Program {
    items: [
        Import { path: ["std", "io"], .. },
        Function(FunctionDecl { name: "main", ret: None, body: [ .. ] }),
    ],
}
```

(`lex` and `parse` print the full token / AST dump — abbreviated here.)

## Diagnostics

Front-end errors carry a caret-underlined span, a stable code, and a did-you-mean hint when a close
name is in scope:

```
$ phorge run -e 'function main() { int count = 1; int y = conut + 1; console.println("{y}"); }'
type error at 1:42: unknown identifier `conut`
function main() { int count = 1; int y = conut + 1; console.println("{y}"); }
                                         ^
  [E-UNKNOWN-IDENT]
  hint: did you mean `count`?
```

Look any code up in the dictionary with `explain`:

```
$ phorge explain E-UNKNOWN-IDENT
E-UNKNOWN-IDENT — a name was used that is not in scope.

Phorge resolves identifiers lexically: block-scope locals (including `var` bindings
and `for` loop variables), parameters, top-level functions, and — inside a method —
the current class's fields. ...
```

## Faults never panic

Phorge never panics on input — runtime faults are clean, one-line errors with exit code 1:

```
$ phorge run   -e 'function main() { int a = 10; int b = 0; int x = a / b; }'
runtime error: division by zero

$ phorge runvm -e 'function main() { int a = 10; int b = 0; int x = a / b; }'
runtime error at 1: division by zero

$ phorge run   -e 'function main() { List<int> xs = [1, 2]; int v = xs[5]; }'
runtime error: list index out of range
```

Both backends fault on the same condition with the same message *body*; the VM also reports the line
(`at 1`). The differential harness (`tests/differential.rs`) gates that `run` and `runvm` fault on
exactly the same inputs — the same checked-arithmetic / bounds-checking guarantee (integer overflow,
division by zero, out-of-range indexing) that `guide/operators.phg` describes.
