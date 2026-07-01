//! Interactive debugger engine (M-DX S5) — an **interpreter-only** pause/step/inspect state machine.
//!
//! The tree-walking interpreter consults a [`Debugger`] before each statement ([`should_pause`]); on a
//! pause it hands a [`PauseCtx`] (line, call depth, faulting-frame-style locals, backtrace) to a
//! [`DebugFrontend`] and applies the returned [`DebugCommand`]. Two frontends share this one engine:
//! the terminal REPL and the DAP server (VS Code / JetBrains) — "shared engine, two thin adapters".
//!
//! **Interpreter-only, by design.** The bytecode VM has no source-line/local-name table, so stepping
//! it would need a debug-symbol subproject; the parity spine guarantees `run ≡ runvm ≡ PHP`, so a
//! debug session on the interpreter provably reflects the other backends. Dev-only (`phg debug`), and
//! entirely a side-channel: it never touches stdout or the `tests/differential.rs` correctness spine.
//!
//! [`should_pause`]: Debugger::should_pause

use std::collections::BTreeSet;

use crate::diagnostic::Frame;
use crate::value::Value;

/// How execution advances after a pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StepMode {
    /// Run until a breakpoint line is hit.
    Continue,
    /// Pause at the very next statement, at any call depth (step *into* calls). The default so a fresh
    /// session pauses at the first statement, letting the user set breakpoints before running.
    #[default]
    StepInto,
    /// Pause at the next statement at the same depth or shallower (step *over* calls). The `usize` is
    /// the call depth at which `next` was issued.
    StepOver(usize),
    /// Pause at the next statement strictly shallower than the recorded depth (step *out* / finish).
    StepOut(usize),
}

/// A command a [`DebugFrontend`] returns at a pause. Breakpoint edits keep the session paused (the
/// engine re-prompts); the movement commands and `Quit` resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugCommand {
    Continue,
    StepInto,
    StepOver,
    StepOut,
    SetBreakpoint(u32),
    ClearBreakpoint(u32),
    /// Detach the debugger and let the program run to completion (v1 "quit" semantics — no new
    /// interpreter `Signal` variant, and testable). The interpreter drops the session.
    Quit,
}

/// The state handed to a frontend at each pause. `locals` is already deterministic (sorted by name);
/// `frames` is innermost → outermost, the same backtrace a fault renders.
pub struct PauseCtx {
    pub line: u32,
    pub depth: usize,
    pub locals: Vec<(String, Value)>,
    pub frames: Vec<Frame>,
}

/// A debugger frontend: the REPL reads a command from the user; a test returns a scripted command;
/// the DAP adapter blocks on a channel fed by the editor. Returning [`DebugCommand::Quit`] detaches.
pub trait DebugFrontend {
    fn on_pause(&mut self, ctx: &PauseCtx) -> DebugCommand;
}

/// The pause/step decision engine — pure and deterministic (unit-testable without any I/O).
#[derive(Debug, Default)]
pub struct Debugger {
    breakpoints: BTreeSet<u32>,
    mode: StepMode,
}

impl Debugger {
    #[must_use]
    pub fn new(breakpoints: BTreeSet<u32>) -> Self {
        Debugger {
            breakpoints,
            mode: StepMode::default(),
        }
    }

    /// Whether execution should pause before the statement on `line`, executing at call `depth`.
    #[must_use]
    pub fn should_pause(&self, line: u32, depth: usize) -> bool {
        if self.breakpoints.contains(&line) {
            return true;
        }
        match self.mode {
            StepMode::Continue => false,
            StepMode::StepInto => true,
            StepMode::StepOver(d) => depth <= d,
            StepMode::StepOut(d) => depth < d,
        }
    }

    /// Apply a movement/breakpoint command at the given current `depth`. Returns `true` if the command
    /// **resumes** execution (movement or quit); `false` if it only edited breakpoints (stay paused).
    pub fn apply(&mut self, cmd: DebugCommand, depth: usize) -> bool {
        match cmd {
            DebugCommand::Continue => {
                self.mode = StepMode::Continue;
                true
            }
            DebugCommand::StepInto => {
                self.mode = StepMode::StepInto;
                true
            }
            DebugCommand::StepOver => {
                self.mode = StepMode::StepOver(depth);
                true
            }
            DebugCommand::StepOut => {
                self.mode = StepMode::StepOut(depth);
                true
            }
            DebugCommand::SetBreakpoint(l) => {
                self.breakpoints.insert(l);
                false
            }
            DebugCommand::ClearBreakpoint(l) => {
                self.breakpoints.remove(&l);
                false
            }
            DebugCommand::Quit => true,
        }
    }
}

/// A live debug session held by the interpreter: the engine plus its frontend.
pub struct DebugSession {
    dbg: Debugger,
    frontend: Box<dyn DebugFrontend>,
}

impl DebugSession {
    #[must_use]
    pub fn new(dbg: Debugger, frontend: Box<dyn DebugFrontend>) -> Self {
        DebugSession { dbg, frontend }
    }

    /// Should the interpreter pause here?
    #[must_use]
    pub fn should_pause(&self, line: u32, depth: usize) -> bool {
        self.dbg.should_pause(line, depth)
    }

    /// Run one pause interaction: prompt the frontend (repeatedly for breakpoint edits) until it
    /// returns a resuming command. Returns `true` if the user asked to **quit** (detach).
    pub fn pause(
        &mut self,
        line: u32,
        depth: usize,
        locals: Vec<(String, Value)>,
        frames: Vec<Frame>,
    ) -> bool {
        let ctx = PauseCtx {
            line,
            depth,
            locals,
            frames,
        };
        loop {
            let cmd = self.frontend.on_pause(&ctx);
            let resumes = self.dbg.apply(cmd, depth);
            if resumes {
                return cmd == DebugCommand::Quit;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted frontend: returns queued commands in order (for deterministic engine tests).
    struct ScriptedFrontend {
        cmds: std::collections::VecDeque<DebugCommand>,
    }
    impl ScriptedFrontend {
        fn new(cmds: Vec<DebugCommand>) -> Self {
            ScriptedFrontend {
                cmds: cmds.into_iter().collect(),
            }
        }
    }
    impl DebugFrontend for ScriptedFrontend {
        fn on_pause(&mut self, _ctx: &PauseCtx) -> DebugCommand {
            self.cmds.pop_front().unwrap_or(DebugCommand::Continue)
        }
    }

    /// Build a debugger with explicit breakpoints + mode (avoids the default-then-assign pattern).
    fn dbg(breakpoints: &[u32], mode: StepMode) -> Debugger {
        Debugger {
            breakpoints: breakpoints.iter().copied().collect(),
            mode,
        }
    }

    #[test]
    fn continue_pauses_only_on_breakpoints() {
        let d = dbg(&[5, 9], StepMode::Continue);
        assert!(d.should_pause(5, 1));
        assert!(d.should_pause(9, 3));
        assert!(!d.should_pause(6, 1));
    }

    #[test]
    fn step_into_pauses_everywhere() {
        let d = dbg(&[], StepMode::StepInto);
        assert!(d.should_pause(1, 0));
        assert!(d.should_pause(42, 7));
    }

    #[test]
    fn step_over_pauses_at_same_or_shallower_depth() {
        let d = dbg(&[], StepMode::StepOver(2));
        assert!(d.should_pause(1, 2)); // same depth
        assert!(d.should_pause(1, 1)); // shallower (returned)
        assert!(!d.should_pause(1, 3)); // deeper (inside a call) — skip
    }

    #[test]
    fn step_out_pauses_only_shallower() {
        let d = dbg(&[], StepMode::StepOut(2));
        assert!(!d.should_pause(1, 2)); // same depth — keep running
        assert!(d.should_pause(1, 1)); // shallower — paused in the caller
    }

    #[test]
    fn breakpoint_overrides_continue_even_when_deeper() {
        let d = dbg(&[7], StepMode::StepOut(1));
        assert!(d.should_pause(7, 9)); // a breakpoint fires regardless of step mode/depth
    }

    #[test]
    fn apply_movement_resumes_breakpoint_edit_does_not() {
        let mut d = Debugger::default();
        assert!(d.apply(DebugCommand::StepOver, 3)); // resumes
        assert_eq!(d.mode, StepMode::StepOver(3));
        assert!(!d.apply(DebugCommand::SetBreakpoint(12), 3)); // stays paused
        assert!(d.should_pause(12, 99));
        assert!(!d.apply(DebugCommand::ClearBreakpoint(12), 3));
        d.apply(DebugCommand::Continue, 3);
        assert!(!d.should_pause(12, 99));
    }

    #[test]
    fn session_pause_loops_over_breakpoint_edits_then_resumes() {
        // A frontend that sets a breakpoint (stay paused) then continues (resume) — the loop applies
        // the edit and re-prompts before resuming.
        let fe = ScriptedFrontend::new(vec![
            DebugCommand::SetBreakpoint(20),
            DebugCommand::Continue,
        ]);
        let mut session = DebugSession::new(Debugger::default(), Box::new(fe));
        let quit = session.pause(3, 0, vec![], vec![]);
        assert!(!quit);
        assert!(
            session.should_pause(20, 5),
            "breakpoint set during the pause takes effect"
        );
    }

    #[test]
    fn session_quit_detaches() {
        let fe = ScriptedFrontend::new(vec![DebugCommand::Quit]);
        let mut session = DebugSession::new(Debugger::default(), Box::new(fe));
        assert!(
            session.pause(1, 0, vec![], vec![]),
            "quit returns true (detach)"
        );
    }
}
