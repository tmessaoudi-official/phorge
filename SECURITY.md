# Security Policy

## Supported versions

Phorge is pre-1.0 and developed by a single maintainer. Only the latest tagged release and the
`master` branch receive fixes.

| Version | Supported |
|---|---|
| latest release / `master` | ✅ |
| older tags | ❌ |

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Report privately through GitHub's **[private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)**
on the repository (Security → Report a vulnerability), or contact the maintainer through their GitHub
profile: **[@tmessaoudi-official](https://github.com/tmessaoudi-official)**.

Please include: a description of the issue, steps to reproduce (a minimal `.phg` program or input is
ideal), the affected version/commit, and the impact you observed. You can expect an initial
acknowledgement within a reasonable time; fixes are coordinated before public disclosure.

## Threat model & hardening notes

Phorge is a language toolchain, so the relevant attack surface is **untrusted input**:

- **Untrusted source programs.** The lexer, parser, and type-checker must reject malformed or
  adversarial programs cleanly (a diagnostic + non-zero exit), never with a panic, infinite loop, or
  unbounded memory growth. Recursion and nesting are explicitly depth-limited (`src/limits.rs`) on a
  fixed-size worker stack so pathological nesting faults cleanly.
- **Untrusted binaries (`phg build`).** The hand-rolled ELF / PE / Mach-O section readers used to
  detect an embedded program parse attacker-controlled object files. They perform **minimal section
  lookup with checked arithmetic on every offset** — malformed or hostile headers return `None`, never
  an overflow panic or out-of-bounds read (invariant **EV-7**). `#![forbid(unsafe_code)]` is set
  crate-wide.
- **No third-party runtime dependencies.** Phorge links zero external crates, which removes the
  supply-chain surface for the runtime (see [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)).

If you find input that causes a panic, crash, hang, or unbounded resource use, that is a bug we want
to hear about — please report it as above.
