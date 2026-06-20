#!/usr/bin/env python3
"""Core-rename codemod v2 (robust): rewrite stdlib qualifiers ONLY inside Rust string literals.

v1's hand-rolled scanner desynced on multi-line raw strings. v2 matches Rust string literals with
regexes (raw r#"..."# and normal "...") and applies the rename to each match's text. `rename` is
idempotent (Core.X has no core.X left; Console. has no console. left), so re-processing already-fixed
literals is a no-op. Rust code outside string literals is never touched, so `file.read()` /
`bytes.len()` in real Rust are safe. The compiler + full gate are the safety net.
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


RAW = re.compile(r'r(#*)"(?:.*?)"\1', re.DOTALL)
NORMAL = re.compile(r'"(?:\\.|[^"\\])*"')


def process_rs(src: str) -> str:
    src = RAW.sub(lambda m: rename(m.group(0)), src)
    src = NORMAL.sub(lambda m: rename(m.group(0)), src)
    return src


def main():
    roots = [pathlib.Path(p) for p in sys.argv[1:]] or [pathlib.Path(".")]
    changed = 0
    for root in roots:
        files = [root] if root.is_file() else sorted(root.rglob("*.rs"))
        for f in files:
            if "/target/" in str(f):
                continue
            old = f.read_text()
            new = process_rs(old)
            if new != old:
                f.write_text(new)
                changed += 1
                print(f"  rewrote {f}")
    print(f"[core_rename2] {changed} file(s) changed")


if __name__ == "__main__":
    main()
