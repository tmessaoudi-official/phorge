#!/usr/bin/env python3
"""Generate web/examples.js from examples/guide/*.phg.

Each guide program becomes a named entry the playground's picker loads. Programs that touch the
filesystem (`Core.File`) are skipped — the browser has no filesystem, so they cannot run in the
playground (the build logs which were dropped; no silent truncation). Run from anywhere; paths are
resolved relative to this script.
"""

import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
GUIDE = HERE.parent.parent / "examples" / "guide"
OUT = HERE / "examples.js"

DEFAULT = """package Main;
import Core.Console;

function main(): void {
    List<string> who = ["world", "Phorj"];
    for (string w in who) {
        Console.println("Hello, {w}!");
    }
}
"""


def main() -> int:
    if not GUIDE.is_dir():
        print(f"error: guide dir not found: {GUIDE}", file=sys.stderr)
        return 1

    examples = {"hello (default)": DEFAULT}
    skipped = []
    for phg in sorted(GUIDE.glob("*.phg")):
        src = phg.read_text(encoding="utf-8")
        if "Core.File" in src:
            skipped.append(phg.name)
            continue
        examples[phg.stem] = src

    body = "window.PHORJ_EXAMPLES = " + json.dumps(examples, indent=2, ensure_ascii=False) + ";\n"
    OUT.write_text(body, encoding="utf-8")

    print(f"wrote {OUT} with {len(examples)} examples")
    if skipped:
        print(f"skipped {len(skipped)} filesystem example(s): {', '.join(skipped)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
