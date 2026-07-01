//! Value-dump on fault (M-DX S3) — an opt-in, Dev-only post-mortem. When a runtime fault occurs and
//! dumping is enabled, the interpreter snapshots the faulting frame's locals and renders them (via the
//! secure [`crate::inspect`] renderer) into the fault [`Diagnostic`]'s `dump` section, printed to
//! stderr after the stack trace. Deep at the fault (locals), shallow elsewhere (the backtrace is just
//! function + line).
//!
//! # Security posture
//!
//! - **Opt-in even in Dev.** Off by default; enabled only by `--dump-on-fault`.
//! - **Never in Release.** [`should_dump`] additionally requires the Dev profile
//!   ([`crate::profile::active`]), so a Release artifact never emits locals regardless of the flag.
//! - **Secret-safe + bounded.** Rendering goes through [`crate::inspect`], which redacts `Secret<T>`
//!   and caps depth/size.
//! - **Side-channel only.** The dump is appended to the stderr fault render; it never touches stdout,
//!   so it is outside the `run ≡ runvm ≡ PHP` correctness spine.
//!
//! # Backend scope
//!
//! Rich named locals are produced on the **interpreter**, which holds live `name → Value` scopes at
//! fault time. The VM stores slot-indexed locals with no name mapping, so a byte-identical *named*
//! dump would need a per-scope debug-symbol table — deliberately not built, mirroring the debugger
//! (S5), which is interpreter-only for the same reason: the spine guarantees `run ≡ runvm ≡ PHP`, so
//! a dump taken on the interpreter provably reflects the VM's state. The stack-trace backtrace is
//! byte-identical on both backends (error-handling slice 1).

use std::sync::atomic::{AtomicBool, Ordering};

use crate::value::Value;

/// Whether `--dump-on-fault` was requested. Process-global (like the active profile) — set once at the
/// CLI entry, read on the fault path. It is a *side-channel* switch: it changes only stderr output, so
/// reading it never affects `run ≡ runvm` (which compares stdout).
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Request value-dumps on fault (from `--dump-on-fault`).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// Whether a dump should be produced: requested **and** under the Dev profile (never in Release).
#[must_use]
pub fn should_dump() -> bool {
    gate(ENABLED.load(Ordering::Relaxed), crate::profile::active())
}

/// The pure enablement rule (testable without touching globals): enabled AND Dev.
#[must_use]
fn gate(enabled: bool, profile: crate::profile::Profile) -> bool {
    enabled && profile.is_dev()
}

/// Render a faulting frame's locals into the `dump` section string. `locals` is expected already
/// deterministic (sorted by name). Empty locals still emit the header so the dump is unambiguous.
#[must_use]
pub fn format_locals(locals: &[(String, Value)]) -> String {
    let mut s = String::from("faulting frame locals:");
    if locals.is_empty() {
        s.push_str("\n  <none>");
        return s;
    }
    for (name, value) in locals {
        s.push_str(&format!("\n  {name} = {}", crate::inspect::render(value)));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;

    #[test]
    fn gate_requires_enabled_and_dev() {
        assert!(gate(true, Profile::Dev));
        assert!(!gate(false, Profile::Dev)); // not requested
        assert!(!gate(true, Profile::Release)); // never in Release, even if requested
        assert!(!gate(false, Profile::Release));
    }

    #[test]
    fn format_locals_renders_name_value_lines() {
        let locals = vec![
            ("count".to_string(), Value::Int(3)),
            ("name".to_string(), Value::Str("ok".into())),
        ];
        let out = format_locals(&locals);
        assert_eq!(out, "faulting frame locals:\n  count = 3\n  name = \"ok\"");
    }

    #[test]
    fn format_locals_redacts_secrets() {
        use crate::value::{ClassLayout, Instance};
        use std::rc::Rc;
        let layout = ClassLayout::new(vec!["value".to_string()]);
        let inst = Instance::new("Secret".to_string(), layout);
        inst.set_field("value", Value::Str("hunter2".into()));
        let locals = vec![("token".to_string(), Value::Instance(Rc::new(inst)))];
        let out = format_locals(&locals);
        assert!(out.contains("token = Secret(<redacted>)"), "{out}");
        assert!(!out.contains("hunter2"), "secret leaked: {out}");
    }

    #[test]
    fn format_locals_empty_is_explicit() {
        assert_eq!(format_locals(&[]), "faulting frame locals:\n  <none>");
    }
}
