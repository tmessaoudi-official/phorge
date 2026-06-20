#!/usr/bin/env python3
"""M-RT Core-rename codemod: stdlib namespace `core.*` -> PascalCase `Core.*`.

`.phg` files are pure Phorge -> rewrite the whole file. `.rs` files mix Rust code with inline
Phorge program strings, and the leaves `file`/`bytes`/`text` collide with real Rust method calls
(`file.read`, `bytes.len`), so for `.rs` we rewrite ONLY inside string literals (normal "...",
raw r#"..."#). Char literals and lifetimes are skipped. The compiler + full test gate are the
safety net: any Rust corruption is a compile error, any missed Phorge string is a test failure.
"""
import re
import sys
import pathlib

MODS = {
    "console": "Console",
    "math": "Math",
    "text": "Text",
    "file": "File",
    "bytes": "Bytes",
    "html": "Html",
}


def rename(s: str) -> str:
    for low, pas in MODS.items():
        s = s.replace(f"core.{low}", f"Core.{pas}")
    for low, pas in MODS.items():
        s = re.sub(r"\b" + low + r"\.", pas + ".", s)
    return s


def process_phg(text: str) -> str:
    return rename(text)


def process_rs(src: str) -> str:
    out = []
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        # line comment: copy to end of line untouched
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            if j == -1:
                j = n
            out.append(src[i:j])
            i = j
            continue
        # block comment
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            j = src.find("*/", i + 2)
            j = n if j == -1 else j + 2
            out.append(src[i:j])
            i = j
            continue
        # raw string r"..." / r#"..."#
        if c == "r" and i + 1 < n and (src[i + 1] == '"' or src[i + 1] == "#"):
            j = i + 1
            hashes = 0
            while j < n and src[j] == "#":
                hashes += 1
                j += 1
            if j < n and src[j] == '"':
                start = j + 1
                term = '"' + "#" * hashes
                end = src.find(term, start)
                end = n if end == -1 else end
                out.append(src[i:start])
                out.append(rename(src[start:end]))
                i = end
                continue
        # normal string "..."
        if c == '"':
            j = i + 1
            buf = []
            while j < n:
                if src[j] == "\\" and j + 1 < n:
                    buf.append(src[j : j + 2])
                    j += 2
                    continue
                if src[j] == '"':
                    break
                buf.append(src[j])
                j += 1
            out.append('"')
            out.append(rename("".join(buf)))
            if j < n:
                out.append('"')
                i = j + 1
            else:
                i = j
            continue
        # char literal 'x' / '\n' / '"'  (vs lifetime 'a)
        if c == "'":
            m = re.match(r"'(\\.|[^'\\])'", src[i:])
            if m:
                out.append(m.group(0))
                i += m.end()
                continue
            out.append(c)
            i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def main():
    roots = [pathlib.Path(p) for p in sys.argv[1:]] or [pathlib.Path(".")]
    changed = 0
    for root in roots:
        files = [root] if root.is_file() else sorted(root.rglob("*"))
        for f in files:
            if not f.is_file():
                continue
            if "/target/" in str(f) or "/.git/" in str(f):
                continue
            if f.suffix == ".phg":
                fn = process_phg
            elif f.suffix == ".rs":
                fn = process_rs
            else:
                continue
            old = f.read_text()
            new = fn(old)
            if new != old:
                f.write_text(new)
                changed += 1
                print(f"  rewrote {f}")
    print(f"[core_rename] {changed} file(s) changed")


if __name__ == "__main__":
    main()
