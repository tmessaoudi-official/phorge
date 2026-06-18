---
name: Bug report
about: Report a reproducible bug — especially any crash, panic, hang, or wrong output
title: "[bug] "
labels: bug
assignees: ""
---

## What happened

<!-- A clear description of the bug. Wrong output? A crash/panic? A hang? -->

## Minimal reproduction

<!-- The smallest .phg program (or stdin/-e snippet) that triggers it. -->

```phorge
function main() {
    // ...
}
```

## Command

<!-- e.g. `phg run repro.phg`, `phg runvm -e '...'`, `phg build repro.phg` -->

```sh
phorge ...
```

## Expected vs. actual

- **Expected:**
- **Actual:** <!-- paste stdout/stderr; for a crash, paste the full message -->

## Does it differ between backends?

<!-- Important for parity bugs: do `phg run` and `phg runvm` produce DIFFERENT output? -->

- [ ] `run` and `runvm` disagree
- [ ] only `run` is wrong
- [ ] only `runvm` is wrong
- [ ] both agree (output is just wrong, or it's a crash)

## Environment

- phorge version (`phg --version`):
- OS / arch:
- built from source / prebuilt binary:
