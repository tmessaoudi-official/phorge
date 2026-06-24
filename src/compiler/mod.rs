//! AST → bytecode compiler (M2 P1–P3). A dedicated pass over the type-checked AST,
//! emitting a `Chunk` the VM executes. Mirrors the tree-walker's semantics so
//! `runvm` output is byte-identical to `run` (the differential oracle).
//!
//! P2 scope: `main`-only programs — literals, arithmetic, comparison, logical
//! short-circuit, unary, interpolation, `println`, list literals, locals, `if`/`else`,
//! `for…in`, blocks. P3 added user function calls + call frames + recursion (multi-function
//! compile → `BytecodeProgram`). P4a adds single-payload enums (`Variant(args)` construction)
//! and exhaustive `match` (lowered to scrutinee-spill + per-arm tag/literal tests + payload
//! re-extraction; decision P4-7). P4b adds classes: each constructor compiles to a synthetic
//! function (promoted-field `MakeInstance` + body with the instance in scope), `ClassName(args)`
//! resolves to a `Call` into it, and `obj.field` lowers to `GetField` (decisions P4-4/P4-5). P4c
//! adds methods + `this`: each method compiles to a function with the receiver at slot 0,
//! `obj.m(args)` lowers to `CallMethod` (runtime dispatch on the receiver's class), and `this` /
//! bare field reads resolve against the receiver (decision P4-6). The compiler now covers the full
//! M1 surface. Enums, instances, and lists are value-native `Value` (no heap; P4-1).

use crate::ast::{
    free_vars, BinaryOp, ClassDecl, ClassMember, CtorParam, Expr, FunctionDecl, Item, LambdaBody,
    MatchArm, Modifier, Param, Pattern, Program, Stmt, StrPart, Type, UnaryOp, Visibility,
};
use crate::chunk::{BytecodeProgram, Chunk, ClassDesc, EnumDesc, FaultMsg, Function, Op};
use crate::diagnostic::Diagnostic;
use crate::value::Value;
use std::collections::HashMap;

/// Numeric operand kind, inferred just enough to pick int- vs float-specialized
/// arithmetic ops (decision P2-6).
#[derive(Clone, Copy, PartialEq)]
enum NumTy {
    Int,
    Float,
}

/// The compiler's class-aware view of a declared type (M2 Wave 4). Derived *structurally* from the
/// AST's declared `Type` annotations — the checker has already verified full types, so the compiler
/// only re-derives the little it needs: the numeric head (to pick int- vs float-specialized
/// arithmetic) and, for an instance, *which class* it is. Knowing the class lets `ctype` walk
/// `obj.field` / `c.method()` / a class-typed enum payload to the underlying numeric type — closing
/// the pre-Wave-4 gap where a field read on an arbitrary instance or a method result was
/// unclassifiable. `Other` stays the catch-all for everything non-numeric/non-class (bool, string,
/// unit, list, map, set, optional) — the compiler only needs to *reject* those as arithmetic
/// operands, not tell them apart — except a **list**, whose element type *is* reachable as an
/// operand via indexing (`xs[i] + 1`, since M3 S1.1), so `List(elem)` carries it; everything else
/// non-numeric/non-class (bool, string, unit, map, set, optional) stays `Other`.
#[derive(Clone, PartialEq)]
enum CTy {
    Int,
    Float,
    /// A `string` — not a numeric operand, but tracked distinctly so the compiler can lower
    /// `string + string` to `Op::Concat(2)` (Phase 1 string slice) instead of an `AddI`/`AddF`. A
    /// string operand that collapsed to `Other` would make the VM reject what the interpreter
    /// accepts (the CTy-operand trap), so every string-producing operand must resolve here.
    Str,
    /// A class instance, carrying its class name so `ctype` can resolve `obj.field` / `obj.m()`.
    Class(String),
    /// A `List<elem>`, carrying its element type so `ctype(Index)` (`xs[i]`) resolves to the element
    /// — which can be an arithmetic operand since M3 S1.1 (e.g. `xs[0] + 1` → `AddI`).
    List(Box<CTy>),
    /// A `Map<key, val>`, carrying both so `ctype(Index)` (`m[k]`) resolves to the **value** type —
    /// which can be an arithmetic operand (e.g. `m["a"] + 1` → `AddI`). Without this, a map-index
    /// operand collapses to `Other` and `num_ty` errors on the VM only — a `run`↔`runvm` break
    /// (M-RT S3, the same reason `List` carries its element type).
    Map(Box<CTy>, Box<CTy>),
    /// A function type `(params) -> ret` — not a numeric operand; carried for future lambda support.
    Fn {
        params: Vec<CTy>,
        ret: Box<CTy>,
    },
    Other,
}

/// A declared local: its name, its class-aware type (for `num_ty`/`ctype`), and the lexical depth
/// it lives at (for scope cleanup). Its stack slot is its index in `locals`.
struct Local {
    name: String,
    ty: CTy,
    depth: u32,
}

/// Per-function metadata gathered in the pre-pass: its index in `BytecodeProgram.functions`
/// and its declared return type (for `ctype` of a call result — decision P3-6). A class return
/// type lets `f().field` resolve.
struct FnMeta {
    index: usize,
    ret: CTy,
    /// Class-aware param types, so a bare named-function reference in value position resolves to a
    /// `CTy::Fn` (lets `var f = namedfn; f(x)` dispatch through `CallValue` like a lambda local).
    params: Vec<CTy>,
    /// `Some(set_id)` when this name is overloaded (M-RT): a call emits `Op::CallOverload(set_id,
    /// argc)` (runtime dynamic dispatch) instead of a direct `Op::Call(index)`. `None` — the common
    /// single-overload case — keeps using `index`. Filled in a post-pass once all overloads are seen.
    overload: Option<usize>,
}

/// Per-variant metadata gathered in the pre-pass: its index into the `enum_descs` table (for
/// `MakeEnum`/`MatchTag`) and the class-aware type of each payload field (so a payload binding —
/// including a class-typed one — resolves through `ctype`). Decision P4-2.
struct VariantMeta {
    index: usize,
    field_tags: Vec<CTy>,
}

/// One step from the `$match` scrutinee to a sub-value: an enum-payload index (`Op::GetEnumField`)
/// or a named instance field (`Op::GetField` into the names pool). The mixed sequence lets a binding
/// reach, e.g., `Wrapper(Point { x })` — an enum field then a struct field — with no new `Op` (S5.2).
#[derive(Clone)]
enum PathSeg {
    Enum(usize),
    Field(usize),
}

/// A `match`-arm payload binding: the name, the slot of the hidden `$match` scrutinee local, and
/// the `path` from the scrutinee to the bound value. Bindings are *re-extracted* at each use
/// (`GetLocal $match` + a `GetEnumField`/`GetField` per path step) rather than stored as stack
/// locals, which keeps arm bodies stack-neutral and sidesteps mid-expression slot bookkeeping (P4-7).
struct MatchBinding {
    name: String,
    match_slot: usize,
    path: Vec<PathSeg>,
    ty: CTy,
}

struct Compiler<'a> {
    chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: u32,
    fns: &'a HashMap<String, FnMeta>,
    /// Function arities, indexed parallel to `BytecodeProgram.functions` — lets `stack_effect`
    /// account for `Op::Call` (which pops `arity` args and pushes one result).
    arities: &'a [usize],
    /// Lambda sub-functions accumulated while compiling this function's body. Drained into the
    /// program's top-level `functions` array after the enclosing function is compiled.
    extra_functions: Vec<Function>,
    /// The table index of the *first* function in `extra_functions` (= `program.functions.len()`
    /// at the time this compiler was created). Lambda indices are `>= base_fn_idx`.
    base_fn_idx: usize,
    /// `n_captures` for each lambda in `extra_functions` (parallel array, same index).
    /// `stack_effect(MakeClosure(idx))` reads `lambda_n_captures[idx - base_fn_idx]`.
    lambda_n_captures: Vec<usize>,
    /// Variant name → its descriptor metadata (construction + pattern dispatch).
    variants: &'a HashMap<String, VariantMeta>,
    /// The shared enum-descriptor table — `stack_effect` reads `MakeEnum`'s payload arity from it.
    enum_descs: &'a [EnumDesc],
    /// Class name → the index of its synthetic constructor function (for `ClassName(args)`).
    classes: &'a HashMap<String, usize>,
    /// `(class, field)` → `(static slot index, field CTy)` (M-mut.7). `ClassName.field` lowers to
    /// `Op::GetStatic(idx)` / `Op::SetStatic(idx)` via the index; the `CTy` lets `ctype` resolve a
    /// static used as an arithmetic operand (`C.total + 1` specializes — without it the VM would
    /// reject what the interpreter accepts, the documented CTy-operand trap).
    statics_index: &'a HashMap<(String, String), (usize, CTy)>,
    /// The shared class-descriptor table — `stack_effect` reads `MakeInstance`'s field count from it.
    class_descs: &'a [ClassDesc],
    /// Field/member name → its index in `BytecodeProgram.names` (for `GetField`/`CallMethod`).
    /// Pre-built from every declared field + method name so member lowering is a lookup, not a mutation.
    names_index: &'a HashMap<String, usize>,
    /// In a method or constructor body, the local slot holding the receiver (`this`): `0` for a
    /// method, the post-promotion instance slot for a constructor. `None` in a free function.
    /// `Expr::This` and a bare field read both load from this slot (decision P4-5/P4-6).
    this_slot: Option<usize>,
    /// Field name → class-aware type of the *current* class (empty outside a method/ctor). Lets a
    /// bare field name (`total`, resolved as `this.total`) work as an arithmetic operand and lets
    /// `expr` lower it to `GetLocal(this) + GetField` when it isn't a local/param/binding. This is
    /// exactly `class_field_ctys[cur_class]`, kept as a direct ref for the bare-field path.
    field_tags: &'a HashMap<String, CTy>,
    /// Program-wide class name → (field name → type) table (M2 Wave 4). `ctype` walks it to resolve
    /// a field read on an *arbitrary* instance (`p.x`, `a.inner.x`), not just `this`.
    class_field_ctys: &'a HashMap<String, HashMap<String, CTy>>,
    /// Program-wide `(class, method) → return type` table (M2 Wave 4). `ctype` reads it to resolve
    /// a method-call result (`c.get() + 1`).
    method_rets: &'a HashMap<(String, String), CTy>,
    /// The class whose body is being compiled (a method or constructor), or `None` in a free
    /// function. `ctype(This)` resolves to `Class(cur_class)`.
    cur_class: Option<String>,
    /// Active `match`-arm bindings (a stack; innermost shadows). Populated while compiling an arm
    /// body, truncated after.
    match_bindings: Vec<MatchBinding>,
    /// When compiling a synthetic constructor body, holds the code indices of the body's `return`
    /// statements (redirected to the ctor epilogue instead of an `Op::Return`). `None` outside a
    /// ctor body. The interpreter discards a ctor body's return and always yields the promoted
    /// instance (`construct`); the epilogue mirrors that exactly (decision P4-4).
    ctor_return_jumps: Option<Vec<usize>>,
    /// Base-relative operand-stack height, tracked so `match` can spill its scrutinee to the
    /// correct slot even mid-expression. Reset to `locals.len()` at each statement boundary and
    /// fixed at `&&`/`||`/`match` control-flow merges; otherwise maintained by `emit`.
    height: usize,
    /// Stack of enclosing loops (M-mut.3). `break`/`continue` emit a placeholder `Jump` recorded in
    /// the innermost frame and patched to the loop's exit / continue target at loop end (the
    /// `ctor_return_jumps` backpatch model). `body_base` is the `locals.len()` both forms pop down
    /// to (dropping body-scope locals while keeping the loop's own locals live). No new `Op` (F5).
    loop_frames: Vec<LoopFrame>,
    /// Stack of enclosing `try` contexts whose `finally` (and any pending `PopHandler`) must run
    /// when control transfers out of them via `return`/`break`/`continue` (M-faults 2b). Innermost
    /// last; pushed while compiling a try *body* or a `catch` body, popped after.
    finally_stack: Vec<TryCtx>,
}

/// One enclosing `try`/`catch` context for finally-on-transfer codegen (M-faults 2b).
struct TryCtx {
    /// The `finally` block to re-emit before a transfer (cloned from the AST), or `None`.
    finally: Option<Vec<Stmt>>,
    /// Whether a `PopHandler` must precede the finally on a transfer — true inside the try *body*
    /// (the handler is still installed), false inside a `catch` body (the unwind already consumed
    /// the handler).
    has_handler: bool,
    /// `loop_frames.len()` when this context was entered — lets `break`/`continue` run only the
    /// finally blocks nested inside the target (innermost) loop.
    loop_depth: usize,
}

/// One enclosing loop's break/continue backpatch state (M-mut.3).
struct LoopFrame {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
    body_base: usize,
}

/// Compile a whole program: a pre-pass indexes every top-level function (so calls — including
/// forward references and recursion — resolve to a static index), then each function body is
/// compiled into its own `Chunk`. Parameters occupy slots `0..arity` at the base of the frame
/// window; every function ends with an implicit `Unit` return (P3-7).
pub fn compile(program: &Program) -> Result<BytecodeProgram, Diagnostic> {
    // The compiler tracks no source position yet, so every fault becomes a position-less
    // compile-stage `Diagnostic` (renders `compile error: …`, unchanged from before).
    compile_program(program).map_err(Diagnostic::compile)
}

/// A ctor param is *promoted* to a field iff it carries a visibility modifier (matching
/// `interpreter::construct` / the checker's promotion rule).
/// Extract the literal text of a string-literal expression (one `StrPart::Literal`). The checker
/// guarantees a fault intrinsic's message is such a literal (M-faults 2a); defaults to empty.
fn str_literal(e: Option<&Expr>) -> String {
    if let Some(Expr::Str(parts, _)) = e {
        if let [crate::ast::StrPart::Literal(s)] = &parts[..] {
            return s.clone();
        }
    }
    String::new()
}

fn is_promoted(p: &CtorParam) -> bool {
    p.modifiers.iter().any(|m| {
        matches!(
            m,
            Modifier::Public | Modifier::Private | Modifier::Protected
        )
    })
}

/// Resolve a declared type annotation into the compiler's class-aware `CTy` (M2 Wave 4), derived
/// purely structurally from the AST. The numeric heads map to `Int`/`Float`; the known
/// primitive/container head names collapse to `Other` (their element types are never operands in
/// the M1 surface); any *other* named type is a user-defined class, kept as `Class(name)` so a
/// field/method read through it resolves. An `Optional` is `Other` (no `null` in M1).
/// The catchable type name(s) of a `catch` clause type (M-faults 2b): one name for a class /
/// interface, or one per member for a union `catch (A | B e)`. The checker has rejected any
/// non-`Error` member (`E-CATCH-TYPE`), so other `Type` shapes never reach here.
fn catch_clause_names(ty: &Type) -> Vec<String> {
    match ty {
        Type::Named { name, .. } => vec![name.clone()],
        Type::Union(members, _) => members.iter().flat_map(catch_clause_names).collect(),
        _ => Vec::new(),
    }
}

/// The compile-time operand type of a `catch` binding (M-faults 2b): the class for a single-type
/// clause (so `e.field` specializes), else `Other` for a union (no single class).
fn catch_binding_cty(ty: &Type) -> CTy {
    match ty {
        Type::Named { name, .. } => CTy::Class(name.clone()),
        _ => CTy::Other,
    }
}

fn resolve_cty(ty: &Type) -> CTy {
    match ty {
        Type::Named { name, args, .. } => match name.as_str() {
            "int" => CTy::Int,
            "float" => CTy::Float,
            // Track the element type so `xs[i]` can be an arithmetic operand (M3 S1.1); a bare
            // `List` (no arg) defaults its element to `Other`.
            "List" => CTy::List(Box::new(args.first().map_or(CTy::Other, resolve_cty))),
            // Track key+value types so `m[k]` can be an arithmetic operand (M-RT S3); a bare `Map`
            // (no args) defaults both to `Other`.
            "Map" => CTy::Map(
                Box::new(args.first().map_or(CTy::Other, resolve_cty)),
                Box::new(args.get(1).map_or(CTy::Other, resolve_cty)),
            ),
            // `string` is tracked distinctly so `string + string` lowers to `Op::Concat` (the
            // string slice).
            "string" => CTy::Str,
            "bool" | "void" | "Set" => CTy::Other,
            other => CTy::Class(other.to_string()),
        },
        // An optional carries its inner's `CTy` (not `Other`): once narrowed (if-let, `??`, `?.`,
        // `match`) the value is the inner `T`, so `if (var x = opt) { x + 1 }` specializes. The
        // checker forbids using a bare `T?` as an operand, so tagging the optional local with the
        // inner `CTy` never mis-specializes an un-narrowed access (M3 S2.4).
        Type::Optional { inner, .. } => resolve_cty(inner),
        // `var` carries no annotation; operand inference reads the initializer expression instead.
        Type::Infer(_) => CTy::Other,
        // An erased generic type parameter (M-RT S7): the value is of an unknown concrete type, so
        // it is never a specialized numeric operand — exactly the `Other` case.
        Type::Erased(_) => CTy::Other,
        // A function type — carry its structure for future lambda support; not a numeric operand.
        Type::Function { params, ret, .. } => CTy::Fn {
            params: params.iter().map(resolve_cty).collect(),
            ret: Box::new(resolve_cty(ret)),
        },
        // A union value is not a specialized arithmetic operand (M-RT S4); after `instanceof`/type-
        // pattern narrowing the *narrowed local* carries the concrete `CTy`, not the union local.
        Type::Union(..) => CTy::Other,
        // An intersection value is likewise not a specialized arithmetic operand (M-RT S5); member
        // access dispatches through the concrete instance with no specialization.
        Type::Intersection(..) => CTy::Other,
    }
}

// impl/free cohesion split (M-Decomp W4.1): program driver + stmt/expr/match clusters.
mod expr;
mod matches;
mod program;
mod stmt;
use program::*;

impl<'a> Compiler<'a> {
    /// A fresh compiler for one function body, sharing the program-level tables (function/variant/
    /// class indices, descriptor tables, name pool). Locals/chunk/height start empty; the caller
    /// seeds params and (for constructors) toggles `ctor_return_jumps`.
    #[allow(clippy::too_many_arguments)]
    fn new(
        fns: &'a HashMap<String, FnMeta>,
        arities: &'a [usize],
        variants: &'a HashMap<String, VariantMeta>,
        enum_descs: &'a [EnumDesc],
        classes: &'a HashMap<String, usize>,
        statics_index: &'a HashMap<(String, String), (usize, CTy)>,
        class_descs: &'a [ClassDesc],
        names_index: &'a HashMap<String, usize>,
        field_tags: &'a HashMap<String, CTy>,
        class_field_ctys: &'a HashMap<String, HashMap<String, CTy>>,
        method_rets: &'a HashMap<(String, String), CTy>,
        base_fn_idx: usize,
    ) -> Self {
        Compiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
            fns,
            arities,
            extra_functions: Vec::new(),
            base_fn_idx,
            lambda_n_captures: Vec::new(),
            variants,
            enum_descs,
            classes,
            statics_index,
            class_descs,
            names_index,
            this_slot: None,
            field_tags,
            class_field_ctys,
            method_rets,
            cur_class: None,
            match_bindings: Vec::new(),
            height: 0,
            ctor_return_jumps: None,
            loop_frames: Vec::new(),
            finally_stack: Vec::new(),
        }
    }

    fn emit(&mut self, op: Op, line: u32) {
        // Maintain the operand-stack height (saturating: control flow after a `Return`/`MatchFail`
        // is dead code whose height is never read). Branch merges reset `height` explicitly.
        let eff = self.stack_effect(&op);
        self.height = self.height.saturating_add_signed(eff);
        self.chunk.emit(op, line);
    }

    /// Net operand-stack delta of one op (`pushes - pops`). Only consumed by `match` (to spill its
    /// scrutinee to the right slot); kept exhaustive so a new op can't silently skew the height.
    fn stack_effect(&self, op: &Op) -> isize {
        match op {
            Op::Const(_) | Op::GetLocal(_) => 1,
            Op::AddI | Op::SubI | Op::MulI | Op::DivI | Op::RemI => -1,
            Op::AddF | Op::SubF | Op::MulF | Op::DivF | Op::RemF => -1,
            // Bitwise binaries pop two, push one (primitives P2).
            Op::BitAnd | Op::BitOr | Op::BitXor | Op::Shl | Op::Shr => -1,
            Op::Eq | Op::Ne | Op::Lt | Op::Gt | Op::Le | Op::Ge => -1,
            Op::Pop | Op::SetLocal(_) | Op::JumpIfFalse(_) | Op::Index | Op::MakeRange(_) => -1,
            // SetIndex pops (container, index, value) and pushes the new container: net -2.
            Op::SetIndex => -2,
            // BitNot is unary (pop one, push one) like Neg/Not.
            Op::Neg | Op::Not | Op::BitNot | Op::Len | Op::Jump(_) => 0,
            Op::MatchTag(_) | Op::GetEnumField(_) => 0, // pop one, push one
            Op::Concat(n) | Op::MakeList(n) => 1 - *n as isize,
            Op::MakeMap(n) => 1 - 2 * *n as isize, // pops 2n (key+value pairs), pushes the map
            // Pops `argc` args, pushes the native's return value (the old `Print` + `Const(Unit)`
            // pair collapses into one op, net delta unchanged).
            Op::CallNative(_, argc) => 1 - *argc as isize,
            Op::Call(idx) => 1 - self.arities[*idx] as isize,
            // Pops `argc` args, dispatches to one overload, pushes its single return value.
            Op::CallOverload(_, argc) => 1 - *argc as isize,
            Op::MakeEnum(idx) => 1 - self.enum_descs[*idx].arity as isize,
            Op::MakeInstance(idx) => 1 - self.class_descs[*idx].fields.len() as isize,
            Op::GetField(_) => 0,   // pop instance, push field value
            Op::SetField(_) => -2,  // pop instance + value, push nothing (statement)
            Op::GetStatic(_) => 1,  // push the static's value
            Op::SetStatic(_) => -1, // pop the value into the static slot
            Op::IsInstance(_) => 0, // pop value, push bool
            // Pops the receiver + `argc` args, pushes one result.
            Op::CallMethod(_, argc) => -(*argc as isize),
            // Terminal (end/redirect the frame): height afterward is dead code, never read.
            Op::Return | Op::Fault(_) => 0,
            // MakeClosure(idx): pops `n_captures` capture values, pushes one `Value::Closure`.
            // Lambdas compiled by THIS compiler occupy [base, base+lambda_n_captures.len()) in the
            // trailing lambda block. Any other index is a named-function reference (never a closure
            // → 0 captures), including a forward-referenced one (its index is below `base`).
            Op::MakeClosure(idx) => {
                let lo = self.base_fn_idx;
                let n = if *idx >= lo && *idx < lo + self.lambda_n_captures.len() {
                    self.lambda_n_captures[idx - lo]
                } else {
                    0 // named function ref — no captures
                };
                1 - n as isize
            }
            // CallValue(argc): pops `argc` args + 1 closure, pushes 1 result → 1 - argc.
            Op::CallValue(argc) => 1 - *argc as isize,
            // M-faults 2b: `Throw` pops the exception value; the handler ops are pure bookkeeping.
            // The catch landing pad's pushed value (+1) is modeled by setting `self.height` directly
            // at the landing pad (like a `match` scrutinee), not via a stack effect here.
            Op::Throw => -1,
            Op::PushHandler(_) | Op::PopHandler => 0,
        }
    }

    fn emit_const(&mut self, v: Value, line: u32) {
        let k = self.chunk.add_const(v);
        self.emit(Op::Const(k), line);
    }

    fn here(&self) -> usize {
        self.chunk.code.len()
    }

    /// Emit a jump placeholder (target 0); returns its code index for `patch_jump`.
    fn emit_jump(&mut self, op: Op, line: u32) -> usize {
        let idx = self.here();
        self.emit(op, line);
        idx
    }

    /// Patch a previously-emitted forward jump to point at the current code position.
    fn patch_jump(&mut self, idx: usize) {
        let target = self.here();
        self.patch_jump_to(idx, target);
    }

    /// Patch a previously-emitted jump to an explicit absolute target — used for `continue`
    /// back-edges (a known earlier position) where the target is not `here()` (M-mut.3).
    fn patch_jump_to(&mut self, idx: usize, target: usize) {
        self.chunk.code[idx] = match self.chunk.code[idx] {
            Op::Jump(_) => Op::Jump(target),
            Op::JumpIfFalse(_) => Op::JumpIfFalse(target),
            // A `try`'s handler target is patched to its catch landing pad (M-faults 2b).
            Op::PushHandler(_) => Op::PushHandler(target),
            ref other => unreachable!("patch_jump on {other:?}"),
        };
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self, line: u32) {
        self.scope_depth -= 1;
        while matches!(self.locals.last(), Some(l) if l.depth > self.scope_depth) {
            self.emit(Op::Pop, line);
            self.locals.pop();
        }
    }

    fn add_local(&mut self, name: &str, ty: CTy) -> usize {
        self.locals.push(Local {
            name: name.to_string(),
            ty,
            depth: self.scope_depth,
        });
        self.locals.len() - 1
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        self.locals.iter().rposition(|l| l.name == name)
    }

    /// Infer whether an arithmetic operand is int- or float-typed, to pick the specialized op
    /// (decision P2-6). Only reached for operands of `+ - * / %`, which the checker guarantees are
    /// numeric. The numeric projection of `ctype` (M2 Wave 4): `ctype` resolves the operand's full
    /// class-aware type and `as_num` narrows it. The error wording matches the pre-Wave-4 paths (a
    /// checker-unreachable surface — no test depends on it — kept faithful regardless).
    fn num_ty(&self, e: &Expr) -> Result<NumTy, String> {
        let cty = self.ctype(e)?;
        Self::as_num(&cty).ok_or_else(|| match e {
            Expr::Ident(name, _) => format!("`{name}` is not numeric"),
            Expr::Call { callee, .. } => match &**callee {
                Expr::Ident(name, _) => format!("`{name}` does not return a numeric type"),
                _ => format!("cannot infer numeric type of {e:?}"),
            },
            _ => format!("cannot infer numeric type of {e:?}"),
        })
    }

    /// Resolve an expression's class-aware type (M2 Wave 4), mirroring `expr`'s resolution order so
    /// a field read / method result / nested member / class-typed payload each resolve once,
    /// recursively. Generalizes the old per-arm `num_ty`: an `Ident` resolves through a `match`-arm
    /// binding, then a local, then a bare field of `this`; `This` is the current class; `Member`
    /// walks the object's class to the field's type; a `Call` resolves to a function/constructor or
    /// method return type. Anything it can name but isn't numeric/class collapses to `Other`; only a
    /// genuinely unresolvable operand errors (the same surface that errored pre-Wave-4).
    fn ctype(&self, e: &Expr) -> Result<CTy, String> {
        match e {
            Expr::Int(..) => Ok(CTy::Int),
            Expr::Float(..) => Ok(CTy::Float),
            // A string literal (incl. an interpolated one — both are `Expr::Str`) is `CTy::Str` so a
            // `"a" + s` concat lowers to `Op::Concat`; `bool`/`bytes` literals are non-operands.
            Expr::Str(..) => Ok(CTy::Str),
            Expr::Bool(..) | Expr::Bytes(..) => Ok(CTy::Other),
            // A list literal's element type comes from its first element (empty → `Other`), so an
            // index into it (`[1, 2, 3][0] + 1`) resolves as an operand (M3 S1.1).
            Expr::List(elems, _) => Ok(CTy::List(Box::new(
                elems
                    .first()
                    .and_then(|el| self.ctype(el).ok())
                    .unwrap_or(CTy::Other),
            ))),
            // `xs[i]` resolves to the list's element type (so `xs[0] + 1` specializes); a non-list
            // receiver collapses to `Other` (checker-unreachable as an arithmetic operand).
            Expr::Index { object, .. } => match self.ctype(object)? {
                CTy::List(elem) => Ok(*elem),
                CTy::Map(_, val) => Ok(*val), // `m[k]` resolves to the value type (M-RT S3)
                _ => Ok(CTy::Other),
            },
            // A map literal's key/value types come from its first pair (≥1, parser-guaranteed), so a
            // `var m = ["a" => 1]; m["a"] + 1` specializes the arithmetic (M-RT S3).
            Expr::Map(pairs, _) => {
                let (k0, v0) = &pairs[0];
                Ok(CTy::Map(
                    Box::new(self.ctype(k0).unwrap_or(CTy::Other)),
                    Box::new(self.ctype(v0).unwrap_or(CTy::Other)),
                ))
            }
            Expr::Ident(name, _) => {
                if let Some(b) = self.match_bindings.iter().rev().find(|b| b.name == *name) {
                    Ok(b.ty.clone())
                } else if let Some(s) = self.resolve_local(name) {
                    Ok(self.locals[s].ty.clone())
                } else if let Some(t) = self.field_tags.get(name) {
                    Ok(t.clone())
                } else if let Some(meta) = self.fns.get(name) {
                    // A bare named-function reference in value position (e.g. `var f = dbl`) is a
                    // function value, so `f(x)` dispatches through `CallValue` like a lambda local.
                    Ok(CTy::Fn {
                        params: meta.params.clone(),
                        ret: Box::new(meta.ret.clone()),
                    })
                } else {
                    Err(format!("undefined variable `{name}`"))
                }
            }
            Expr::This(_) => match &self.cur_class {
                Some(c) => Ok(CTy::Class(c.clone())),
                None => Err("`this` used outside a method".into()),
            },
            Expr::Member { object, name, .. } => {
                // Static read `ClassName.field` resolves to the static's declared `CTy` (M-mut.7) —
                // checked first, since `ctype(object)` would reject the bare class name.
                if let Some(cty) = self.static_cty(object, name) {
                    return Ok(cty);
                }
                let obj_cty = self.ctype(object);
                // A property hook read `o.name` (M-mut.7b): its operand type is the `<name>$get`
                // method's return type. Resolved before the field path so `o.fahrenheit + 1.0`
                // specializes — without it the VM would reject what the interpreter accepts (the
                // documented CTy-operand trap).
                if let Ok(CTy::Class(cls)) = &obj_cty {
                    if let Some(cty) = self.method_rets.get(&(cls.clone(), format!("{name}$get"))) {
                        return Ok(cty.clone());
                    }
                }
                match obj_cty? {
                    CTy::Class(cls) => self
                        .class_field_ctys
                        .get(&cls)
                        .and_then(|fs| fs.get(name))
                        .cloned()
                        .ok_or_else(|| format!("no field `{name}` on `{cls}`")),
                    _ => Err(format!("cannot infer type of field `{name}`")),
                }
            }
            Expr::Call { callee, .. } => match &**callee {
                Expr::Ident(name, _) => {
                    if let Some(meta) = self.fns.get(name) {
                        Ok(meta.ret.clone())
                    } else if self.classes.contains_key(name) {
                        Ok(CTy::Class(name.clone())) // a constructor returns its instance
                    } else if self.variants.contains_key(name) {
                        Ok(CTy::Other) // an enum value: not numeric, not a class we track fields of
                    } else if let Some(slot) = self.resolve_local(name) {
                        // A function-value local (lambda): the call result is the lambda's ret type.
                        match &self.locals[slot].ty {
                            CTy::Fn { ret, .. } => Ok(*ret.clone()),
                            _ => Err(format!("cannot infer numeric type of {e:?}")),
                        }
                    } else {
                        Err(format!("cannot infer numeric type of {e:?}"))
                    }
                }
                // Method call: the return type is keyed on the receiver's runtime class.
                Expr::Member { object, name, .. } => match self.ctype(object)? {
                    CTy::Class(cls) => self
                        .method_rets
                        .get(&(cls.clone(), name.clone()))
                        .cloned()
                        .ok_or_else(|| format!("no method `{name}` on `{cls}`")),
                    _ => Err(format!("cannot infer numeric type of {e:?}")),
                },
                _ => Err(format!("cannot infer numeric type of {e:?}")),
            },
            Expr::Unary { expr, .. } => self.ctype(expr),
            Expr::Binary { lhs, .. } => self.ctype(lhs),
            // `value instanceof C` is a `bool` — never an arithmetic operand, but a `var b = …`
            // initializer reads `ctype`, so resolve it to `Other` rather than erroring.
            Expr::InstanceOf { .. } => Ok(CTy::Other),
            // `inner!` unwraps `T?` to `T`; its operand type is the inner's (so `o! + 1` specializes
            // — `resolve_cty(Optional)` already yields the inner `CTy`). M3 S2.5.
            Expr::Force { inner, .. } => self.ctype(inner),
            // `expr?` unwraps a `Result<T, E>` to its `Ok` payload — generally an erased/unknown
            // operand type, so it is not a specialized arithmetic operand (M-faults 2a).
            Expr::Propagate { .. } => Ok(CTy::Other),
            // `obj with { … }` yields a fresh instance of `obj`'s class — same compile-type as `obj`.
            Expr::CloneWith { object, .. } => self.ctype(object),
            // A `match` value's type is its arms' shared type (checker-guaranteed); infer it from
            // the first arm's body so `var x = match … { … }` specializes like an explicit local.
            Expr::Match { arms, .. } => match arms.first() {
                Some(arm) => self.ctype(&arm.body),
                None => Ok(CTy::Other),
            },
            // A range materializes to `List<int>`, so its compile-type is `List(Int)` — carrying the
            // element type lets `(0..n)[i] + 1` (or a range bound to a `var`, then indexed) specialize.
            Expr::Range { .. } => Ok(CTy::List(Box::new(CTy::Int))),
            // Both `if` branches share a type (checker-guaranteed); infer it from the then-branch so
            // `var x = if (c) { 1 } else { 2 }` specializes arithmetic on `x` (like `Match`).
            Expr::If { then_expr, .. } => self.ctype(then_expr),
            // A lambda's compile-time type reflects its declared params and return type so that
            // a `var f = fn(int x) => x + 1` local later resolves calls on `f` to `CallValue`.
            Expr::Lambda { params, ret, .. } => Ok(CTy::Fn {
                params: params.iter().map(|p| resolve_cty(&p.ty)).collect(),
                ret: Box::new(ret.as_ref().map_or(CTy::Other, resolve_cty)),
            }),
            other => Err(format!("cannot infer numeric type of {other:?}")),
        }
    }

    /// Numeric refinement of a `CTy` — the bridge from "what type the operand is" to "which
    /// specialized arithmetic op." `None` for non-numeric types (a defensive path: the checker
    /// already guarantees arithmetic operands are numeric).
    fn as_num(ty: &CTy) -> Option<NumTy> {
        match ty {
            CTy::Int => Some(NumTy::Int),
            CTy::Float => Some(NumTy::Float),
            CTy::Str
            | CTy::Class(_)
            | CTy::Other
            | CTy::List(_)
            | CTy::Map(..)
            | CTy::Fn { .. } => None,
        }
    }

    /// Resolve a field/member name to its index in the program's `names` pool (for `GetField`). The
    /// pool is pre-built from every declared field name, so a checker-valid read always resolves;
    /// an unknown name would be a compiler bug.
    fn field_name_index(&self, name: &str) -> Result<usize, String> {
        self.names_index
            .get(name)
            .copied()
            .ok_or_else(|| format!("unknown field `{name}`"))
    }

    /// Resolve a `ClassName.field` static-field access to its program-level static slot (M-mut.7).
    /// Returns `Some(idx)` only when `object` is a class *name* (not shadowed by a local) and `field`
    /// is one of its `static` fields — i.e. exactly the static-access shape the checker accepts.
    /// `None` ⇒ fall through to instance-field handling.
    fn static_slot(&self, object: &Expr, field: &str) -> Option<usize> {
        if let Expr::Ident(name, _) = object {
            if self.resolve_local(name).is_none() && self.classes.contains_key(name) {
                return self
                    .statics_index
                    .get(&(name.clone(), field.to_string()))
                    .map(|&(idx, _)| idx);
            }
        }
        None
    }

    /// The `CTy` of a `ClassName.field` static access, or `None` if it is not a static (M-mut.7).
    /// Lets `ctype` treat a static as an arithmetic operand (`C.total + 1` specializes — without it
    /// the VM rejects what the interpreter accepts, the documented CTy-operand trap).
    fn static_cty(&self, object: &Expr, field: &str) -> Option<CTy> {
        if let Expr::Ident(name, _) = object {
            if self.resolve_local(name).is_none() && self.classes.contains_key(name) {
                return self
                    .statics_index
                    .get(&(name.clone(), field.to_string()))
                    .map(|(_, cty)| cty.clone());
            }
        }
        None
    }

    /// The synthetic method name `<name>$get` if `object.name` is a readable property hook
    /// (M-mut.7b) — i.e. `object`'s compile-type is a class with a registered `<name>$get` method.
    /// `None` ⇒ `object.name` is a stored field (or not a hook), handled by `GetField`.
    fn hook_get_method(&self, object: &Expr, name: &str) -> Option<String> {
        if let Ok(CTy::Class(cls)) = self.ctype(object) {
            let m = format!("{name}$get");
            if self.method_rets.contains_key(&(cls, m.clone())) {
                return Some(m);
            }
        }
        None
    }

    /// The synthetic method name `<name>$set` if `object.name` is a writable property hook
    /// (M-mut.7b). `None` ⇒ a stored field, handled by `SetField`.
    fn hook_set_method(&self, object: &Expr, name: &str) -> Option<String> {
        if let Ok(CTy::Class(cls)) = self.ctype(object) {
            let m = format!("{name}$set");
            if self.method_rets.contains_key(&(cls, m.clone())) {
                return Some(m);
            }
        }
        None
    }

    /// Resolve a `match`-arm binding by name (innermost shadows). Returns the `$match` slot and the
    /// payload path to re-extract, cloned so the caller can emit without holding a borrow on `self`.
    fn resolve_binding(&self, name: &str) -> Option<(usize, Vec<PathSeg>)> {
        self.match_bindings
            .iter()
            .rev()
            .find(|b| b.name == name)
            .map(|b| (b.match_slot, b.path.clone()))
    }

    /// Emit the per-step field loads of a binding `path` (the value to descend from is already on
    /// the stack). Each step is an enum-payload index or a named instance-field read.
    fn emit_path(&mut self, path: &[PathSeg], line: u32) {
        for seg in path {
            match seg {
                PathSeg::Enum(i) => self.emit(Op::GetEnumField(*i), line),
                PathSeg::Field(idx) => self.emit(Op::GetField(*idx), line),
            }
        }
    }

    /// Push the sub-value of the `$match` scrutinee (slot `m_slot`) reached by `path`.
    fn emit_load_path(&mut self, m_slot: usize, path: &[PathSeg], line: u32) {
        self.emit(Op::GetLocal(m_slot), line);
        self.emit_path(path, line);
    }
}

#[cfg(test)]
mod tests;
