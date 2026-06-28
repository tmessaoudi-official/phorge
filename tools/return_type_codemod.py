#!/usr/bin/env python3
"""S0b return-type mandate codemod.

Insert `-> void` into every `function NAME(...)` declaration that lacks a return type, across
`.phg` sources AND inline Phorj programs embedded in Rust string literals.

Rule (uniform): find the `function` keyword followed by `NAME(` ; depth-match the parameter list's
parens (function-typed params like `(int) -> bool f` contain parens, so a regex won't do); then look
at the first significant token after the matching `)`:
  - `->`              -> already annotated, leave it
  - `{` / `;` / `throws` -> a declaration with no return type: insert `-> void ` right after `)`
  - anything else     -> not a declaration continuation (e.g. prose in a comment), leave it

Constructors (`constructor(...)`), property hooks (`get =>`/`set(...)`), and lambdas (`fn ...`) never
use the `function` keyword, so they are naturally exempt. Expression-body lambdas keep inference.

Usage: return_type_codemod.py [--apply] FILE...   (default: dry-run, report insertions with context)
"""
import sys

KW = "function"


def is_ident(c: str) -> bool:
    return c.isalnum() or c == "_"


def transform(s: str):
    """Return (new_text, [ (offset, context) ]) — the rewritten text and a list of insertion points."""
    out = []
    inserts = []
    i = 0
    n = len(s)
    while True:
        j = s.find(KW, i)
        if j == -1:
            out.append(s[i:])
            break
        out.append(s[i:j])  # text before the keyword

        after = j + len(KW)
        # Must be a standalone keyword: boundary before, whitespace after. In a Rust string literal
        # `function` may sit right after a `\n`/`\t`/`\r` escape, so the preceding char is the escape
        # letter (`n`/`t`/`r`) — treat that as a boundary too.
        before_ok = (
            (j == 0)
            or (not is_ident(s[j - 1]))
            or (j >= 2 and s[j - 2] == "\\" and s[j - 1] in "ntr")
        )
        if not before_ok or after >= n or not s[after].isspace():
            out.append(s[j:after])
            i = after
            continue

        # skip ws, read the function name
        k = after
        while k < n and s[k].isspace():
            k += 1
        id_start = k
        while k < n and is_ident(s[k]):
            k += 1
        if k == id_start:
            out.append(s[j:after])
            i = after
            continue

        # skip ws to '('
        m = k
        while m < n and s[m].isspace():
            m += 1
        if m >= n or s[m] != "(":
            out.append(s[j:after])
            i = after
            continue

        # depth-match the parameter parens
        depth = 0
        p = m
        while p < n:
            c = s[p]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    break
            p += 1
        if p >= n:
            out.append(s[j:])  # unbalanced — bail, emit the rest verbatim
            break

        # q = first significant char after ')', skipping ws + rust string line-continuations
        q = p + 1
        while q < n and s[q] in " \t\r\n\\":
            q += 1

        if s[q : q + 2] == "->":
            out.append(s[j:q])  # already annotated
            i = q
            continue

        throws_kw = (
            s[q : q + 6] == "throws" and (q + 6 >= n or not is_ident(s[q + 6]))
        )
        if (q < n and s[q] in "{;") or throws_kw:
            out.append(s[j:q])
            out.append("-> void ")
            ctx = s[max(0, j) : min(n, q + 12)].replace("\n", "\\n")
            inserts.append((j, ctx))
            i = q
            continue

        # not a recognizable declaration continuation — leave unchanged
        out.append(s[j:q])
        i = q

    return "".join(out), inserts


def main():
    args = sys.argv[1:]
    apply = "--apply" in args
    files = [a for a in args if a != "--apply"]
    total = 0
    for path in files:
        with open(path, "r", encoding="utf-8") as f:
            src = f.read()
        new, inserts = transform(src)
        if not inserts:
            continue
        total += len(inserts)
        print(f"\n{path}: {len(inserts)} insertion(s)")
        for _off, ctx in inserts:
            print(f"    + {ctx}")
        if apply:
            with open(path, "w", encoding="utf-8") as f:
                f.write(new)
    print(f"\n{'APPLIED' if apply else 'DRY-RUN'}: {total} total insertion(s) across {len(files)} file(s)")


if __name__ == "__main__":
    main()
