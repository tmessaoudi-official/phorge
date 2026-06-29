//! M1 tree-walking evaluator. Walks the untyped AST against runtime `Value`s and
//! executes `main`. The type-checker (`crate::checker`) is the gate; this stage
//! assumes type-correct input and never panics on the faults types can't catch —
//! those become a runtime `Diagnostic`. See design spec `2026-06-15-m1-plan5-evaluator-design.md`.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ast::{
    BinaryOp, ClassDecl, ClassMember, CtorParam, Expr, FunctionDecl, Item, LambdaBody, MatchArm,
    Modifier, Param, Pattern, Program, Stmt, StrPart, UnaryOp,
};
use crate::diagnostic::Diagnostic;
use crate::value::{ClosureData, EnumVal, Instance, Value};
use std::rc::Rc;

/// Non-local control flow threaded through `Result::Err` (EV-3). The runtime fault carries a
/// unified [`Diagnostic`] (stage `Runtime`); the tree-walker tracks no source position, so the
/// diagnostic has none (line 0) — the VM, which knows `Chunk.lines`, is the backend that locates
/// runtime faults.
enum Signal {
    Return(Value),
    /// `break;` — unwinds to the innermost loop, which catches it and stops (M-mut.3).
    Break,
    /// `continue;` — unwinds to the innermost loop, which catches it and starts the next iteration.
    Continue,
    /// `throw e;` — a checked exception, unwinding to the innermost enclosing `try` whose `catch`
    /// matches it, or out of `main` (uncaught) if none does (M-faults 2b). Distinct from `Runtime`:
    /// only a `Throw` is catchable; a `Runtime` fault (panic/index-OOB/…) passes straight through
    /// every `catch` — panics are uncatchable by design.
    Throw(Value),
    Runtime(Diagnostic),
}

type R<T> = Result<T, Signal>;

/// Sentinel fault body used to carry a `Signal::Throw` *across a higher-order-native boundary*
/// without changing the backend-shared `ClosureInvoker` signature (`Result<_, String>`). The
/// invoker stashes the thrown value in `Interp::pending_throw` and returns this string; the
/// `CallNative` site recognises it and rebuilds the `Throw` (M-faults 2b). The same trick is used
/// by the VM. The token is not a valid source identifier, so it can never collide with a real fault.
/// Single-sourced with the VM via [`crate::chunk::THROW_SENTINEL`].
const THROW_SENTINEL: &str = crate::chunk::THROW_SENTINEL;

/// The source line of a statement, for runtime trace frames (error-handling slice 1).
fn stmt_line(s: &Stmt) -> u32 {
    match s {
        Stmt::VarDecl { span, .. }
        | Stmt::Assign { span, .. }
        | Stmt::Return { span, .. }
        | Stmt::If { span, .. }
        | Stmt::For { span, .. }
        | Stmt::While { span, .. }
        | Stmt::CFor { span, .. }
        | Stmt::Throw { span, .. }
        | Stmt::Try { span, .. }
        | Stmt::Destructure { span, .. } => span.line,
        Stmt::Break(s)
        | Stmt::Continue(s)
        | Stmt::Block(_, s)
        | Stmt::Expr(_, s)
        | Stmt::Discard(_, s) => s.line,
    }
}

fn rt<T>(msg: impl Into<String>) -> R<T> {
    Err(Signal::Runtime(Diagnostic::runtime(msg)))
}

/// Flatten a runtime `Signal` to its message body for the higher-order-native callback boundary (a
/// [`crate::native::ClosureInvoker`] returns `Result<_, String>`, the backend-shared fault contract).
/// A `Return` escaping `call_closure` would be an interpreter bug — a closure's return value is
/// consumed inside the call, never surfaced — so it maps to a defensive internal-error string.
fn signal_msg(sig: Signal) -> String {
    match sig {
        Signal::Runtime(d) => d.message,
        Signal::Return(_) => "internal error: closure return escaped".to_string(),
        Signal::Break | Signal::Continue => "internal error: loop control escaped".to_string(),
        // A `Throw` is intercepted before this point at the native boundary (it becomes the
        // sentinel + `pending_throw`); reaching here would be an interpreter bug.
        Signal::Throw(_) => "internal error: throw escaped to native boundary".to_string(),
    }
}

/// The literal text of a fault intrinsic's string-literal message argument (M-faults 2a). The checker
/// guarantees it is a single `StrPart::Literal`; defaults to empty (e.g. a bare `assert(cond)`).
fn lit_msg(e: Option<&Expr>) -> String {
    if let Some(Expr::Str(parts, _)) = e {
        if let [crate::ast::StrPart::Literal(s)] = &parts[..] {
            return s.clone();
        }
    }
    String::new()
}

fn as_bool(v: &Value) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => rt(format!("expected bool, got {}", other.type_name())),
    }
}

/// The lexical block-scope stack of the *currently executing* call — a `Vec` of scopes
/// (innermost last), pushed/popped as the tree-walker enters and leaves blocks. No closures in
/// M1, so it captures no enclosing environment. NB despite the holding field being named `frame`,
/// this is **not** a call frame: it is the opposite concept from `vm::Frame`
/// (`{func, ip, slot_base}`, a reified call record). The tree-walker keeps its call records on the
/// native Rust stack, so the only per-call state it reifies is this scope chain.
struct CallScopes {
    scopes: Vec<HashMap<String, Value>>,
}

impl CallScopes {
    fn new() -> Self {
        CallScopes {
            scopes: vec![HashMap::new()],
        }
    }
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    fn declare(&mut self, name: &str, v: Value) {
        self.scopes
            .last_mut()
            .expect("scope stack always has a base scope")
            .insert(name.to_string(), v);
    }
    fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
    /// Overwrite an existing binding in the innermost scope that declares it (M-mut.1
    /// reassignment). Returns `false` if no scope holds `name` (defensive — the checker guarantees
    /// the binding exists and is `mutable`). Does NOT create a new binding.
    fn assign(&mut self, name: &str, v: Value) -> bool {
        for s in self.scopes.iter_mut().rev() {
            if let Some(slot) = s.get_mut(name) {
                *slot = v;
                return true;
            }
        }
        false
    }
}

pub struct Interp<'c> {
    /// Free-function overload sets (M-RT overloading): a name maps to one *or more* declarations.
    /// Length 1 in the common case; dynamic dispatch selects among >1 by the argument values.
    funcs: HashMap<String, Vec<FunctionDecl>>,
    classes: HashMap<String, ClassDecl>,
    /// Transitively-flattened interface set each class implements — the `instanceof` table, built
    /// once via [`crate::ast::class_implements`] and shared verbatim with the checker + VM so the
    /// runtime test never diverges (M-RT S2). Interfaces themselves are erased: there are no
    /// interface values, only this lookup.
    class_implements: std::collections::BTreeMap<String, Vec<String>>,
    /// Static class hierarchy for the reflection enumeration natives (`Core.Reflect.interfaces`/…),
    /// built once from the program and shared verbatim with the VM + transpiler so reflection is
    /// byte-identical (M-Reflect Tier-2).
    class_tables: crate::native::ClassTables,
    /// The fully-resolved method-dispatch table — `(class, name) -> (declaring_class, method)` — built
    /// once via [`crate::ast::class_method_origins`] and shared with the compiler's pre-flatten so a
    /// multi-parent / resolution-clause / renamed call resolves to the *same* body the VM dispatches
    /// to (M-RT S6b). Subsumes the parent-chain walk: `call_method` is now a single table lookup.
    method_origins: std::collections::BTreeMap<(String, String), (String, String)>,
    /// variant name -> (enum name, arity)
    variants: HashMap<String, (String, usize)>,
    /// Program-lifetime `static` field storage (M-mut.7), keyed by `(class, field)`. Seeded once at
    /// load from each static's literal-const initializer; read/written via `ClassName.field`.
    statics: HashMap<(String, String), Value>,
    /// Class constants (Feature A), keyed by `(class, NAME)` → inlined literal `Value`. Seeded once at
    /// load from the shared [`crate::ast::class_consts`] table (inheritance + traits already
    /// flattened), and read — before `statics` — on a `ClassName.NAME` access.
    consts: HashMap<(String, String), Value>,
    /// Expression field initializers (Feature B), keyed by class → ordered `(field, init_expr)` list
    /// (base-first across ancestors, declaration order). Seeded once at load from the shared
    /// [`crate::ast::field_initializers`]; evaluated per-instance in `construct` after promotion.
    field_inits: HashMap<String, Vec<(String, Expr)>>,
    /// The shared `name → slot` field layout per class (M-perf S1b), built once at load from
    /// [`crate::ast::class_field_layout`] — the *same* source the compiler/VM build their layouts
    /// from, so an interpreter-built instance and a VM-built instance of one class agree on slots.
    /// Consulted by `construct` (allocate + populate) and every field read/write.
    layouts: HashMap<String, std::rc::Rc<crate::value::ClassLayout>>,
    frame: CallScopes,
    this: Option<Value>,
    /// The **lexical** class of the currently-executing method/constructor body (M-RT super/parent) —
    /// the class that *declares* the running body, used to resolve a `parent` call (which is lexical,
    /// not keyed on the receiver's runtime class). `None` in a free function. Saved/restored by
    /// `run_call` alongside `this`.
    cur_class: Option<String>,
    /// Inheritance tables for `parent`/super resolution, cached once at load (mirrors `method_origins`).
    parent_parents: std::collections::BTreeMap<String, Vec<String>>,
    parent_mro: std::collections::BTreeMap<String, Vec<String>>,
    out: String,
    /// Logical call stack for runtime stack traces (error-handling slice 1). A frame is pushed at each
    /// `run_call` entry (function name + current line) and popped **only on success** — an error path
    /// skips the pop, so at the top-level catch the stack still holds every active frame to snapshot.
    /// Names mirror the VM's compiled `Function.name` (`main`, `Class::method`, `Class::new`,
    /// `Class::name$set`) so `run`-traces are byte-identical to `runvm`-traces.
    trace_stack: Vec<crate::diagnostic::Frame>,
    /// Live call-frame depth, checked against [`crate::limits::MAX_CALL_DEPTH`] in `run_call`.
    /// Converts unbounded recursion into a clean `"stack overflow"` fault instead of a native
    /// stack abort — and uses the *same* limit as the VM, keeping the backends parity-identical.
    depth: usize,
    /// Carries a thrown value across a higher-order-native call boundary (M-faults 2b). When a
    /// closure passed to `List.map`/etc. throws, the invoker stows the value here and returns
    /// [`THROW_SENTINEL`]; the `CallNative` site rebuilds the `Throw` from it. `None` otherwise.
    pending_throw: Option<Value>,
    /// Green-thread coordination (M6 W4): the scheduler/id-allocator + finished-task results, mirroring
    /// the VM's `coop`. In the synchronous-degenerate path `spawn` runs eagerly and stores its result
    /// here (read by `join`); the cooperative driver shares one `Coop` across task interpreters.
    coop: std::rc::Rc<std::cell::RefCell<crate::green::exec::Coop>>,
    /// Green-thread cooperative cutover (S4.3): the suspension handle of the coroutine *this* task runs
    /// on. `Some` only while a task interpreter executes inside the cooperative driver
    /// ([`run_cooperative_interp`]); `None` on the ordinary synchronous run path, where `spawn` is eager
    /// and `recv`/`join` fault instead of blocking. The borrow is a closure-local of the driver's
    /// coroutine body (never escapes it), so it keeps `Interp` `'static`-movable into that `'static`
    /// closure — the deep-suspend mechanism the spike (`green::spike`) proved works without `unsafe`.
    coop_suspend: Option<&'c dyn crate::green::exec::Suspend>,
    /// The owning program AST, held only by a **cooperative** task interpreter so it can build a fresh
    /// child task interpreter when it evaluates a `spawn` (each green task = its own `Interp` over the
    /// same program, sharing only `coop`). `None` on the synchronous path (where `spawn` never defers).
    /// Read only by the native cooperative driver (`coop` module); legitimately dead when that module is
    /// absent (wasm, or a `--no-default-features` build without `green`).
    #[cfg_attr(
        not(all(feature = "green", not(target_arch = "wasm32"))),
        allow(dead_code)
    )]
    program: Option<std::rc::Rc<Program>>,
}

/// Run a whole program: collect declarations, locate `main`, call it, and return
/// the captured stdout buffer (the Plan 6 CLI prints it to real stdout).
///
/// The tree-walker recurses on the native Rust stack, so deep recursion needs a generous stack for
/// the `run_call` depth guard (not a native abort) to be what stops it. That stack is supplied by
/// the caller — `cli::cmd_run` runs the whole pipeline on a 256 MB worker thread — keeping this
/// function a plain recursive walk.
pub fn interpret(program: &Program) -> Result<String, Diagnostic> {
    interpret_main(program).map(|(out, _exit)| out)
}

/// Like [`interpret`], but also returns `main`'s exit code (Batch-1 B): the `int` it returns, or `0`
/// for a `void` `main` / a `main` that falls off the end. The CLI (`phg run`) maps this to the
/// process exit status; the stdout-only [`interpret`] wrapper preserves every existing caller and the
/// differential harness (which gates stdout identity).
pub fn interpret_main(program: &Program) -> Result<(String, i64), Diagnostic> {
    let mut interp = Interp {
        funcs: HashMap::new(),
        classes: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        class_tables: crate::native::ClassTables::default(),
        method_origins: std::collections::BTreeMap::new(),
        variants: HashMap::new(),
        statics: HashMap::new(),
        consts: HashMap::new(),
        field_inits: HashMap::new(),
        layouts: HashMap::new(),
        frame: CallScopes::new(),
        this: None,
        cur_class: None,
        parent_parents: std::collections::BTreeMap::new(),
        parent_mro: std::collections::BTreeMap::new(),
        out: String::new(),
        trace_stack: Vec::new(),
        depth: 0,
        pending_throw: None,
        coop: std::rc::Rc::new(std::cell::RefCell::new(crate::green::exec::Coop::new())),
        coop_suspend: None,
        program: None,
    };
    interp.collect(program);
    // Feature B-static: runtime static initializers run once, before `main`. A fault here surfaces
    // like any runtime fault (with the frames captured so far).
    if let Err(sig) = interp.eval_static_inits(program) {
        return Err(match sig {
            Signal::Runtime(e) => e.with_frames(interp.snapshot_frames()),
            Signal::Throw(v) => {
                Diagnostic::runtime(format!("uncaught exception `{}`", throw_what(&v)))
                    .with_frames(interp.snapshot_frames())
            }
            _ => Diagnostic::runtime("internal error: control escaped a static initializer"),
        });
    }
    // Batch-1 D: the entry is a top-level `function main` OR a class-static `main` method — the shared
    // `ast::entry_point` resolver picks the one (the checker's `E-MULTIPLE-MAIN` guarantees ≤1).
    let (entry_class, main) = match crate::ast::entry_point(program, "main") {
        Some(e) => e,
        None => return Err(Diagnostic::runtime(
            "no entry point: running needs a `main` function. A library or web file (no `main`) \
                 still type-checks and transpiles — use `phg check` / `phg transpile`",
        )),
    };
    let names: Vec<String> = main.params.iter().map(|p| p.name.clone()).collect();
    // Batch-1 B: a one-parameter `main` receives the program argv (the same `List<string>` value
    // `Core.Process.args()` exposes); a zero-parameter `main` gets none. The checker's
    // `E-MAIN-SIGNATURE` guarantees the arity is 0 or 1, so this never under/over-supplies.
    let args = if names.is_empty() {
        vec![]
    } else {
        vec![crate::native::process_args_value()]
    };
    // A class-static entry has no receiver (`this = None`); the trace name mirrors the VM's
    // `Class::main` for a static method, or bare `main` for a top-level one.
    let call_name = match entry_class {
        Some(c) => format!("{c}::main"),
        None => "main".to_string(),
    };
    match interp.run_call(&call_name, &names, &main.body, args, None, None) {
        // `run_call` converts `main`'s `return n` into `Ok(Value::Int(n))` (and a fall-off-the-end
        // `void` `main` into `Ok(Value::Unit)`); `exit_code_of` maps both to the exit status.
        Ok(v) => Ok((interp.out, exit_code_of(&v))),
        // Defensive: a `Return` that escapes `run_call` uncaught carries the same exit value.
        Err(Signal::Return(v)) => Ok((interp.out, exit_code_of(&v))),
        Err(Signal::Runtime(e)) => Err(e.with_frames(interp.snapshot_frames())),
        // An exception that escapes `main` uncaught (defensive — the checker's `E-UNCAUGHT-THROW`
        // guarantees `main` handles every throw, so this is unreachable for a checked program).
        Err(Signal::Throw(v)) => Err(Diagnostic::runtime(format!(
            "uncaught exception `{}`",
            throw_what(&v)
        ))
        .with_frames(interp.snapshot_frames())),
        // Checker-unreachable: `break`/`continue` are rejected outside a loop and caught by their
        // enclosing loop, so they never escape `main`'s body. Defensive (EV-7 parity).
        Err(Signal::Break | Signal::Continue) => {
            Err(Diagnostic::runtime("internal error: loop control escaped"))
        }
    }
}

/// The process exit code carried by `main`'s return value (Batch-1 B): the `int` it returned, or `0`
/// for anything else (a `void` `main` returns `Value::Unit`). The checker's `E-MAIN-SIGNATURE`
/// constrains `main`'s return to `void`/`int`, so the non-`Int` case is only the `void` path.
fn exit_code_of(v: &Value) -> i64 {
    match v {
        Value::Int(n) => *n,
        _ => 0,
    }
}

/// The display name of a thrown value for an uncaught-exception message — its class for an
/// instance, else its type name (M-faults 2b).
fn throw_what(v: &Value) -> String {
    match v {
        Value::Instance(inst) => inst.class.clone(),
        other => other.type_name().to_string(),
    }
}

/// The catchable type name(s) of a `catch` clause type (M-faults 2b): a single name for a class /
/// interface, or one per member for a union `catch (A | B e)`. The checker has already rejected any
/// non-`Error` member (`E-CATCH-TYPE`), so other `Type` shapes never reach here.
fn catch_type_names(ty: &crate::ast::Type) -> Vec<String> {
    match ty {
        crate::ast::Type::Named { name, .. } => vec![name.clone()],
        crate::ast::Type::Union(members, _) => members.iter().flat_map(catch_type_names).collect(),
        _ => Vec::new(),
    }
}

/// Call a single named top-level function with pre-built `args`, returning its value plus the
/// captured stdout. The serve runtime (M6 W3, `crate::serve`) uses this to invoke
/// `respond(bytes) -> bytes` once per request — the one entry the socket bridge needs. The
/// interpreter is the reference backend; `run` ≡ `runvm` (the differential harness) guarantees the
/// VM would compute identical bytes, so the spike does not need a VM `call_named` (deferred — the
/// VM has no return-value capture today). Mirrors [`interpret`] exactly, but enters an arbitrary
/// named function with caller-supplied arguments instead of an argument-less `main`.
pub fn call_named(
    program: &Program,
    name: &str,
    args: Vec<Value>,
) -> Result<(Value, String), Diagnostic> {
    let mut interp = Interp {
        funcs: HashMap::new(),
        classes: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        class_tables: crate::native::ClassTables::default(),
        method_origins: std::collections::BTreeMap::new(),
        variants: HashMap::new(),
        statics: HashMap::new(),
        consts: HashMap::new(),
        field_inits: HashMap::new(),
        layouts: HashMap::new(),
        frame: CallScopes::new(),
        this: None,
        cur_class: None,
        parent_parents: std::collections::BTreeMap::new(),
        parent_mro: std::collections::BTreeMap::new(),
        out: String::new(),
        trace_stack: Vec::new(),
        depth: 0,
        pending_throw: None,
        coop: std::rc::Rc::new(std::cell::RefCell::new(crate::green::exec::Coop::new())),
        coop_suspend: None,
        program: None,
    };
    interp.collect(program);
    let set = match interp.funcs.get(name) {
        Some(v) => v.clone(),
        None => return Err(Diagnostic::runtime(format!("no `{name}` function"))),
    };
    // M-RT overloading: select the overload by the supplied argument values (single-overload sets
    // return directly). A selection fault surfaces as a runtime diagnostic.
    let f = match interp.select_free_overload(name, &set, &args) {
        Ok(f) => f,
        Err(Signal::Runtime(d)) => return Err(d),
        Err(_) => return Err(Diagnostic::runtime(format!("cannot resolve `{name}`"))),
    };
    if f.params.len() != args.len() {
        return Err(Diagnostic::runtime(format!(
            "`{name}` expects {} argument(s), got {}",
            f.params.len(),
            args.len()
        )));
    }
    let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
    match interp.run_call(&f.name, &names, &f.body, args, None, None) {
        Ok(v) => Ok((v, interp.out)),
        Err(Signal::Return(v)) => Ok((v, interp.out)),
        Err(Signal::Runtime(e)) => Err(e.with_frames(interp.snapshot_frames())),
        Err(Signal::Throw(v)) => Err(Diagnostic::runtime(format!(
            "uncaught exception `{}`",
            throw_what(&v)
        ))
        .with_frames(interp.snapshot_frames())),
        Err(Signal::Break | Signal::Continue) => {
            Err(Diagnostic::runtime("internal error: loop control escaped"))
        }
    }
}

// cohesion split (M-Decomp W4): stmt/expr/call/construct clusters.
mod call;
mod construct;
mod expr;
mod stmt;

// Cooperative green-thread driver (M6 W4 / S4.3) — native + `green` only (uses stackful coroutines,
// which corosensei cannot provide on wasm32; the wasm interpreter keeps the eager model).
#[cfg(all(feature = "green", not(target_arch = "wasm32")))]
mod coop;
#[cfg(all(feature = "green", not(target_arch = "wasm32")))]
pub use coop::run_cooperative_interp;

impl<'c> Interp<'c> {
    fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(f) => {
                    // M-RT overloading: a same-named function joins an overload set (the checker has
                    // already validated legality). Declaration order is preserved.
                    self.funcs
                        .entry(f.name.clone())
                        .or_default()
                        .push(f.clone());
                }
                Item::Enum(e) => {
                    for v in &e.variants {
                        self.variants
                            .insert(v.name.clone(), (e.name.clone(), v.fields.len()));
                    }
                }
                Item::Class(c) => {
                    // Seed `static` field storage once at load from the literal-const initializer
                    // (M-mut.7) — the same `const_literal` kernel the VM's `static_inits` uses (F3).
                    for m in &c.members {
                        if let crate::ast::ClassMember::Field {
                            modifiers,
                            name,
                            init,
                            ..
                        } = m
                        {
                            if modifiers.contains(&crate::ast::Modifier::Static) {
                                let v = init
                                    .as_ref()
                                    .and_then(crate::value::const_literal)
                                    .unwrap_or(Value::Unit);
                                self.statics.insert((c.name.clone(), name.clone()), v);
                            }
                        }
                    }
                    self.classes.insert(c.name.clone(), c.clone());
                }
                // Interfaces have no runtime instances; they contribute only to the
                // `class_implements` table built below (used by `instanceof`).
                Item::Interface(_) => {}
                // M-RT S8: a trait is registered as a synthetic class so `call_method` can resolve a
                // trait-supplied method body — the shared `class_method_origins` maps a using class's
                // method to its `(trait, m)` origin, and the body is looked up via `self.classes`. A
                // trait is never instantiated and never enters the subtype table, so this entry only
                // ever serves method-body lookup.
                Item::Trait(t) => {
                    self.classes.insert(
                        t.name.clone(),
                        crate::ast::ClassDecl {
                            vis: crate::ast::Visibility::Public,
                            name: t.name.clone(),
                            type_params: Vec::new(),
                            extends: Vec::new(),
                            implements: Vec::new(),
                            open: false,
                            is_abstract: true,
                            resolutions: Vec::new(),
                            uses: Vec::new(),
                            members: t.members.clone(),
                            foreign: false,
                            span: t.span,
                        },
                    );
                }
                Item::Import { .. } => {}
                // Aliases are expanded out of the AST before any backend runs (checker::
                // expand_aliases); this arm only satisfies the exhaustive match.
                Item::TypeAlias { .. } => {}
                // M-Test: `test` items declare no callable symbol; the `phg test` runner executes
                // each test body directly (M-Test T3). Nothing to hoist here.
                Item::Test { .. } => {}
            }
        }
        // M-RT S8: seed a `use`d trait's `static` field as a per-using-class copy (PHP `use` semantics)
        // — keyed `(class, field)`, mirroring the compiler's per-using-class slot.
        for it in &program.items {
            let Item::Class(c) = it else { continue };
            for u in &c.uses {
                for t in &program.items {
                    let Item::Trait(td) = t else { continue };
                    if td.name != u.name {
                        continue;
                    }
                    for m in &td.members {
                        if let crate::ast::ClassMember::Field {
                            modifiers,
                            name,
                            init,
                            ..
                        } = m
                        {
                            if modifiers.contains(&crate::ast::Modifier::Static) {
                                let v = init
                                    .as_ref()
                                    .and_then(crate::value::const_literal)
                                    .unwrap_or(Value::Unit);
                                self.statics.insert((c.name.clone(), name.clone()), v);
                            }
                        }
                    }
                }
            }
        }
        // The single shared runtime subtype oracle (M-RT S6c.3): parent classes AND interfaces, so
        // `instanceof`/match-patterns/overload-subtyping see a class ancestor too. Same algorithm as the
        // VM (the BytecodeProgram builds the identical table), no divergence.
        self.class_implements = crate::ast::instanceof_table(program);
        self.class_tables = crate::native::ClassTables::from_program(program);
        // M-perf S1b: the shared `name → slot` layout per class — same source as the compiler/VM, so
        // both backends allocate slot-aligned instances. Stored as one `Rc` per class, cloned onto
        // every instance of that class in `construct`.
        self.layouts = crate::ast::class_field_layout(program)
            .into_iter()
            .map(|(class, names)| (class, crate::value::ClassLayout::new(names)))
            .collect();
        // The single shared method-dispatch table (M-RT S6b): `call_method` resolves `(class, name)`
        // to its `(declaring_class, method)` — the same table the compiler pre-flattens into the VM's
        // method table, so multi-parent / resolution-clause / renamed dispatch can never diverge. The
        // conflict list is checker-only (E-MI-CONFLICT); a clean program reaches here conflict-free.
        let (origins, _conflicts) = crate::ast::class_method_origins(program);
        self.method_origins = origins;
        // M-RT super/parent: cache the inheritance tables for lexical `parent` resolution (shared with
        // the checker + compiler via `ast::resolve_parent_method`).
        self.parent_parents = crate::ast::class_parents(program);
        self.parent_mro = crate::ast::class_mro(program);
        // Class constants (Feature A): the shared table already flattens inheritance + traits. Drop the
        // declared `Type` — the interpreter inlines only the literal `Value` at each `ClassName.NAME`.
        self.consts = crate::ast::class_consts(program)
            .into_iter()
            .map(|(key, (v, _ty))| (key, v))
            .collect();
        // Expression field initializers (Feature B): the shared ordered list per class.
        for it in &program.items {
            if let Item::Class(c) = it {
                self.field_inits.insert(
                    c.name.clone(),
                    crate::ast::field_initializers(program, &c.name),
                );
            }
        }
    }

    /// Feature B-static: evaluate every **non-literal** static-field initializer once, in declaration
    /// order across classes, BEFORE `main` — overwriting the `Unit` placeholder `collect` seeded.
    /// Evaluated with no `this` (statics are class-level); a later static may read an earlier one
    /// (already stored). Literal statics are already seeded by `collect`, so they are skipped here —
    /// matching the VM, which keeps literals in `static_inits` and emits a `SetStatic` prelude only for
    /// the non-literals. Runs after `collect`, so every function/static is available.
    fn eval_static_inits(&mut self, program: &Program) -> R<()> {
        for item in &program.items {
            let Item::Class(c) = item else { continue };
            for m in &c.members {
                if let ClassMember::Field {
                    modifiers,
                    name,
                    init: Some(e),
                    ..
                } = m
                {
                    if modifiers.contains(&Modifier::Static)
                        && !modifiers.contains(&Modifier::Const)
                        && crate::value::const_literal(e).is_none()
                    {
                        let saved_frame = std::mem::replace(&mut self.frame, CallScopes::new());
                        let saved_this = self.this.take();
                        let v = self.eval(e);
                        self.frame = saved_frame;
                        self.this = saved_this;
                        let v = v?;
                        self.statics.insert((c.name.clone(), name.clone()), v);
                    }
                }
            }
        }
        Ok(())
    }

    /// Run a callable body in a fresh frame: bind `args` to `names` in the base
    /// scope, set `this`, execute, restore caller state. A `Return` becomes the
    /// value; falling off the end yields `Unit`.
    fn run_call(
        &mut self,
        fn_name: &str,
        names: &[String],
        body: &[Stmt],
        args: Vec<Value>,
        this: Option<Value>,
        lexical_class: Option<&str>,
    ) -> R<Value> {
        // Mirror the VM's frame cap: past the shared limit, fault cleanly instead of letting
        // native recursion abort the process. Checked before incrementing, so the guard path
        // leaves `depth` untouched and every non-guard exit below decrements exactly once.
        if self.depth >= crate::limits::MAX_CALL_DEPTH {
            return rt("stack overflow");
        }
        self.depth += 1;
        // Push a trace frame (line is filled in by `exec_stmt` as the body runs). Popped only on the
        // success arms below — an error leaves it on `trace_stack` for the top-level snapshot.
        self.trace_stack.push(crate::diagnostic::Frame {
            function: fn_name.to_string(),
            file: None,
            line: 0,
            col: 0,
        });
        let saved_frame = std::mem::replace(&mut self.frame, CallScopes::new());
        let saved_this = std::mem::replace(&mut self.this, this);
        // M-RT super/parent: the running body's lexical class (the declaring class, for `parent`
        // resolution). `None` for a free function. Saved/restored exactly like `this`.
        let saved_class = std::mem::replace(&mut self.cur_class, lexical_class.map(str::to_string));
        for (n, a) in names.iter().zip(args) {
            self.frame.declare(n, a);
        }
        let result = self.exec_stmts(body);
        self.frame = saved_frame;
        self.this = saved_this;
        self.cur_class = saved_class;
        self.depth -= 1;
        match result {
            Ok(()) => {
                self.trace_stack.pop();
                Ok(Value::Unit)
            }
            Err(Signal::Return(v)) => {
                self.trace_stack.pop();
                Ok(v)
            }
            Err(other) => Err(other),
        }
    }

    /// Snapshot the live trace stack as ordered frames (innermost → outermost) for a fault diagnostic.
    fn snapshot_frames(&self) -> Vec<crate::diagnostic::Frame> {
        self.trace_stack.iter().rev().cloned().collect()
    }

    fn exec_stmts(&mut self, stmts: &[Stmt]) -> R<()> {
        for s in stmts {
            self.exec_stmt(s)?;
        }
        Ok(())
    }

    fn exec_scoped(&mut self, stmts: &[Stmt]) -> R<()> {
        self.frame.push_scope();
        let r = self.exec_stmts(stmts);
        self.frame.pop_scope();
        r
    }
}

fn arith(op: BinaryOp, l: Value, r: Value) -> R<Value> {
    use BinaryOp::*;
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => {
            // Checked ops via the single-sourced `value` kernels: overflow / div-zero / mod-zero are
            // faults the type system can't catch, so they become a Diagnostic, never a panic
            // (EV-7). The VM dispatches into the *same* kernels, so the fault path can't diverge.
            let v = match op {
                Add => crate::value::int_add(a, b),
                Sub => crate::value::int_sub(a, b),
                Mul => crate::value::int_mul(a, b),
                Pow => crate::value::int_pow(a, b),
                Div => crate::value::int_div(a, b),
                Rem => crate::value::int_rem(a, b),
                _ => unreachable!("arith only called with +-*/%**"),
            };
            match v {
                Ok(n) => Ok(Value::Int(n)),
                Err(msg) => rt(msg),
            }
        }
        (Value::Float(a), Value::Float(b)) => {
            // `+ - * **` are infallible on `f64`; `/` and `%` fault on a zero divisor (the same
            // kernels the VM's `DivF`/`RemF` call, so the fault path can't diverge).
            match op {
                Add => Ok(Value::Float(crate::value::float_add(a, b))),
                Sub => Ok(Value::Float(crate::value::float_sub(a, b))),
                Mul => Ok(Value::Float(crate::value::float_mul(a, b))),
                Pow => Ok(Value::Float(crate::value::float_pow(a, b))),
                Div => match crate::value::float_div(a, b) {
                    Ok(n) => Ok(Value::Float(n)),
                    Err(msg) => rt(msg),
                },
                Rem => match crate::value::float_rem(a, b) {
                    Ok(n) => Ok(Value::Float(n)),
                    Err(msg) => rt(msg),
                },
                _ => unreachable!("arith only called with +-*/%**"),
            }
        }
        // `decimal` arithmetic (M-NUM S1): `+ - *` over a decimal — including a mixed `decimal`/`int`
        // pair (the kernel widens the int to scale 0) — dispatches into the single-sourced
        // `value::decimal_*` kernels the VM's `AddD/SubD/MulD` ops also call, so the exact result and
        // the i128-overflow fault are byte-identical. The checker rejects decimal `/`/`%` (S2), so
        // only `Add/Sub/Mul` reach here; a stray `Div/Rem` is a checker-unreachable defensive error.
        (l @ Value::Decimal { .. }, r) | (l, r @ Value::Decimal { .. })
            if matches!(r, Value::Decimal { .. } | Value::Int(_))
                && matches!(l, Value::Decimal { .. } | Value::Int(_)) =>
        {
            let res = match op {
                Add => crate::value::decimal_add(&l, &r),
                Sub => crate::value::decimal_sub(&l, &r),
                Mul => crate::value::decimal_mul(&l, &r),
                Rem => crate::value::decimal_rem(&l, &r),
                Div => crate::value::decimal_div_exact(&l, &r),
                _ => unreachable!("decimal arith only +-*/%"),
            };
            match res {
                Ok(v) => Ok(v),
                Err(msg) => rt(msg),
            }
        }
        // `string + string` → concatenation (Phase 1 string slice). The checker guarantees `+` is
        // the only op and both sides are `string`; the VM lowers this to `Op::Concat(2)`, whose
        // two-`Str` result is exactly `a + b`, so the backends stay byte-identical.
        (Value::Str(a), Value::Str(b)) if matches!(op, Add) => Ok(Value::Str(a + &b)),
        (l, r) => rt(format!(
            "cannot apply {op:?} to {} and {}",
            l.type_name(),
            r.type_name()
        )),
    }
}

/// Bitwise binaries on ints (primitives P2) — the same single-sourced `value` kernels the VM uses,
/// so a negative-shift fault can't diverge between backends.
fn bitwise(op: BinaryOp, l: Value, r: Value) -> R<Value> {
    use BinaryOp::*;
    match (l, r) {
        (Value::Int(a), Value::Int(b)) => match op {
            BitAnd => Ok(Value::Int(crate::value::int_bitand(a, b))),
            BitOr => Ok(Value::Int(crate::value::int_bitor(a, b))),
            BitXor => Ok(Value::Int(crate::value::int_bitxor(a, b))),
            Shl => match crate::value::int_shl(a, b) {
                Ok(n) => Ok(Value::Int(n)),
                Err(msg) => rt(msg),
            },
            Shr => match crate::value::int_shr(a, b) {
                Ok(n) => Ok(Value::Int(n)),
                Err(msg) => rt(msg),
            },
            _ => unreachable!("bitwise only called with & | ^ << >>"),
        },
        (l, r) => rt(format!(
            "cannot apply {op:?} to {} and {}",
            l.type_name(),
            r.type_name()
        )),
    }
}

fn compare(op: BinaryOp, l: Value, r: Value) -> R<Value> {
    use BinaryOp::*;
    // The ordering + comparability fault is single-sourced in `value::compare_ord` (the VM calls the
    // same fn); only the op→bool projection below is backend-local (the op enums differ).
    let ord = match crate::value::compare_ord(&l, &r) {
        Ok(o) => o,
        Err(msg) => return rt(msg),
    };
    let res = match ord {
        Some(o) => match op {
            Lt => o.is_lt(),
            Gt => o.is_gt(),
            Le => o.is_le(),
            Ge => o.is_ge(),
            _ => unreachable!("compare only called with < > <= >="),
        },
        None => false, // NaN compares false
    };
    Ok(Value::Bool(res))
}

/// Try to match `pat` against `value`, pushing any bindings. Returns whether it matched. `implements`
/// is the shared `class_implements` table (needed by a type pattern to test an interface RHS — the
/// same data the `instanceof` evaluation uses, so the two can't diverge).
#[allow(clippy::float_cmp)] // intentional: literal float patterns match exactly
fn match_pattern(
    pat: &Pattern,
    value: &Value,
    implements: &std::collections::BTreeMap<String, Vec<String>>,
    out: &mut Vec<(String, Value)>,
) -> bool {
    match pat {
        Pattern::Wildcard(_) => true,
        Pattern::Binding { name, .. } => {
            out.push((name.clone(), value.clone()));
            true
        }
        Pattern::Int(n, _) => matches!(value, Value::Int(v) if v == n),
        Pattern::Float(x, _) => matches!(value, Value::Float(v) if v == x),
        // A decimal literal pattern matches numerically (scale-insensitive, like `==`): `1.5d` matches
        // a `1.50d` scrutinee. Reuse the value-equality kernel via a fresh `Value::Decimal` (M-NUM S1).
        Pattern::Decimal {
            unscaled, scale, ..
        } => value.eq_val(&Value::Decimal {
            unscaled: *unscaled,
            scale: *scale,
        }),
        Pattern::Str(s, _) => matches!(value, Value::Str(v) if v == s),
        Pattern::Bool(b, _) => matches!(value, Value::Bool(v) if v == b),
        Pattern::Null(_) => matches!(value, Value::Null), // M3 S2.6: `null` arm over a `T?`
        Pattern::Variant { name, fields, .. } => {
            if let Value::Enum(ev) = value {
                if &ev.variant == name && ev.payload.len() == fields.len() {
                    return fields
                        .iter()
                        .zip(&ev.payload)
                        .all(|(fp, fv)| match_pattern(fp, fv, implements, out));
                }
            }
            false
        }
        // M-RT S4 type pattern: matches iff `value` is an instance whose class equals `type_name` or
        // implements interface `type_name` — exactly the `instanceof` test (`eval` arm above), so the
        // backends agree. Binds the matched value (if a binder is present).
        Pattern::Type {
            type_name, binding, ..
        } => {
            let is = matches!(value, Value::Instance(inst)
                if inst.class == *type_name
                    || implements
                        .get(&inst.class)
                        .is_some_and(|ifaces| ifaces.iter().any(|i| i == type_name)));
            if is {
                if let Some(name) = binding {
                    out.push((name.clone(), value.clone()));
                }
            }
            is
        }
        // S5.2 struct pattern: matches iff `value` is an instance of `type_name` (same `instanceof`
        // test as a type pattern), then each named field's sub-pattern matches that field's value.
        // A field absent at runtime (a declared-but-uninitialized explicit field) is a no-match here;
        // struct patterns are intended for classes whose fields are all initialized — promoted ctor
        // params — exactly like a direct `obj.field` read (KNOWN_ISSUES).
        Pattern::Struct {
            type_name, fields, ..
        } => {
            let is = matches!(value, Value::Instance(inst)
                if inst.class == *type_name
                    || implements
                        .get(&inst.class)
                        .is_some_and(|ifaces| ifaces.iter().any(|i| i == type_name)));
            if !is {
                return false;
            }
            if let Value::Instance(inst) = value {
                for fp in fields {
                    let fv = inst.get_field(&fp.field);
                    match fv {
                        Some(v) => {
                            if !match_pattern(&fp.pat, &v, implements, out) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
            }
            true
        }
    }
}

#[cfg(test)]
mod tests;
