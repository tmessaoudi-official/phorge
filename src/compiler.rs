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
    MatchArm, Modifier, Param, Pattern, Program, Stmt, StrPart, Type, UnaryOp,
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
}

/// Per-variant metadata gathered in the pre-pass: its index into the `enum_descs` table (for
/// `MakeEnum`/`MatchTag`) and the class-aware type of each payload field (so a payload binding —
/// including a class-typed one — resolves through `ctype`). Decision P4-2.
struct VariantMeta {
    index: usize,
    field_tags: Vec<CTy>,
}

/// A `match`-arm payload binding: the name, the slot of the hidden `$match` scrutinee local, and
/// the payload-index `path` from the scrutinee to the bound value. Bindings are *re-extracted* at
/// each use (`GetLocal $match` + `GetEnumField` per path step) rather than stored as stack locals,
/// which keeps arm bodies stack-neutral and sidesteps mid-expression slot bookkeeping (P4-7).
struct MatchBinding {
    name: String,
    match_slot: usize,
    path: Vec<usize>,
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

fn compile_program(program: &Program) -> Result<BytecodeProgram, String> {
    let mut order: Vec<&FunctionDecl> = Vec::new();
    let mut fns: HashMap<String, FnMeta> = HashMap::new();
    // Enum pre-pass: one `EnumDesc` per variant of every declared enum, plus the variant-name →
    // metadata map both construction and `match` resolve through (decision P4-2).
    let mut enum_descs: Vec<EnumDesc> = Vec::new();
    let mut variants: HashMap<String, VariantMeta> = HashMap::new();
    let mut class_decls: Vec<&ClassDecl> = Vec::new();
    for it in &program.items {
        match it {
            Item::Function(f) => {
                fns.insert(
                    f.name.clone(),
                    FnMeta {
                        index: order.len(),
                        ret: f.ret.as_ref().map_or(CTy::Other, resolve_cty),
                        params: f.params.iter().map(|p| resolve_cty(&p.ty)).collect(),
                    },
                );
                order.push(f);
            }
            Item::Enum(e) => {
                for v in &e.variants {
                    variants.insert(
                        v.name.clone(),
                        VariantMeta {
                            index: enum_descs.len(),
                            field_tags: v.fields.iter().map(|p| resolve_cty(&p.ty)).collect(),
                        },
                    );
                    enum_descs.push(EnumDesc {
                        ty: e.name.clone(),
                        variant: v.name.clone(),
                        arity: v.fields.len(),
                    });
                }
            }
            Item::Class(c) => class_decls.push(c),
            // Interfaces emit no bytecode; they feed only the `class_implements` table built below.
            Item::Interface(_) => {}
            Item::Import { .. } => {}
            // Aliases are expanded out of the AST before compiling (checker::expand_aliases); this
            // arm only satisfies the exhaustive match.
            Item::TypeAlias { .. } => {}
        }
    }
    let main = fns
        .get("main")
        .map(|m| m.index)
        .ok_or_else(|| "no `main` function".to_string())?;

    // Class pre-pass (decision P4-2/P4-4/P4-6). Function indices are laid out as
    // `[free fns | constructors | methods]` so free-function indices — and `main` — stay put.
    // `class_descs` lists the promoted fields a `MakeInstance` populates (mirroring the
    // interpreter's runtime promotion); the `names` pool interns every readable field name AND
    // every method name (so `obj.field`/`obj.m()` lower to a name-pool index); `class_field_tags`
    // records each class's field types for bare-field (`this.field`) resolution; `methods` is the
    // `(class, method) → fn index` dispatch table `Op::CallMethod` reads at runtime. Explicit
    // `Field` members are named but absent from `class_descs.fields` — like the interpreter they
    // are unpopulated, so reading one faults.
    let nfree = order.len();
    let nclasses = class_decls.len();
    let mut classes: HashMap<String, usize> = HashMap::new();
    let mut class_descs: Vec<ClassDesc> = Vec::new();
    // Program-wide field-type table, keyed by class *name* so `ctype` can resolve a field read on
    // any instance (not just `this`) — `obj.field` looks up `class_field_ctys[class_of(obj)][field]`.
    let mut class_field_ctys: HashMap<String, HashMap<String, CTy>> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let mut names_index: HashMap<String, usize> = HashMap::new();
    let mut intern = |name: &str, names: &mut Vec<String>| {
        if !names_index.contains_key(name) {
            names_index.insert(name.to_string(), names.len());
            names.push(name.to_string());
        }
    };
    for (ci, c) in class_decls.iter().enumerate() {
        classes.insert(c.name.clone(), nfree + ci);
        let (params, _) = ctor_parts(c);
        let mut fields: Vec<String> = Vec::new();
        let mut tags: HashMap<String, CTy> = HashMap::new();
        for p in params {
            if is_promoted(p) {
                fields.push(p.name.clone());
                intern(&p.name, &mut names);
                tags.insert(p.name.clone(), resolve_cty(&p.ty));
            }
        }
        for m in &c.members {
            match m {
                ClassMember::Field { name, ty, .. } => {
                    intern(name, &mut names); // readable, but unpopulated by construction
                    tags.insert(name.clone(), resolve_cty(ty));
                }
                ClassMember::Method(f) => intern(&f.name, &mut names),
                ClassMember::Constructor { .. } => {}
            }
        }
        class_descs.push(ClassDesc {
            class: c.name.clone(),
            fields,
        });
        class_field_ctys.insert(c.name.clone(), tags);
    }
    // `intern`'s unique borrow of `names_index` ends at its last call above (NLL), so the
    // immutable `&names_index` borrows below are free.

    // Methods follow the constructors in the index space; build the dispatch table — and the
    // `(class, method) → return type` table `ctype` reads for a method-call result — in lockstep.
    let mut methods: HashMap<(String, String), usize> = HashMap::new();
    let mut method_rets: HashMap<(String, String), CTy> = HashMap::new();
    let mut methods_to_compile: Vec<(usize, &FunctionDecl)> = Vec::new();
    let mut next_idx = nfree + nclasses;
    for (ci, c) in class_decls.iter().enumerate() {
        for m in &c.members {
            if let ClassMember::Method(f) = m {
                methods.insert((c.name.clone(), f.name.clone()), next_idx);
                method_rets.insert(
                    (c.name.clone(), f.name.clone()),
                    f.ret.as_ref().map_or(CTy::Other, resolve_cty),
                );
                methods_to_compile.push((ci, f));
                next_idx += 1;
            }
        }
    }

    // Arities for *every* function index — free fns, constructors, then methods (`this` + params)
    // — so `stack_effect` can size an `Op::Call` into a constructor (methods dispatch via
    // `CallMethod`, whose arg count is in the op, so their arity entries are for completeness).
    let mut arities: Vec<usize> = order.iter().map(|f| f.params.len()).collect();
    for c in &class_decls {
        arities.push(ctor_parts(c).0.len());
    }
    for (_, f) in &methods_to_compile {
        arities.push(1 + f.params.len());
    }

    // Free functions have no enclosing class, so no `this` and no field scope.
    let empty_fields: HashMap<String, CTy> = HashMap::new();
    let mut functions = Vec::with_capacity(next_idx);
    // Lambdas live in a trailing block *after* all `next_idx` named functions, so every named
    // function keeps its hoist-order index (`Op::Call` targets and the `main` entry stay valid even
    // when an earlier function defines a lambda). Each function's lambdas are numbered from
    // `next_idx + lambdas.len()` and accumulated here; appended to `functions` once all are compiled.
    let mut lambdas: Vec<Function> = Vec::new();
    for f in &order {
        // This function's lambda sub-functions are numbered starting at `next_idx + lambdas.len()`
        // — the start of its slice of the trailing lambda block.
        let base = next_idx + lambdas.len();
        let mut c = Compiler::new(
            &fns,
            &arities,
            &variants,
            &enum_descs,
            &classes,
            &class_descs,
            &names_index,
            &empty_fields,
            &class_field_ctys,
            &method_rets,
            base,
        );
        for p in &f.params {
            c.add_local(&p.name, resolve_cty(&p.ty));
        }
        c.height = c.locals.len(); // params occupy slots `0..arity` (decision P3-1)
        let last_line = f.span.line;
        for s in &f.body {
            c.stmt(s)?;
        }
        c.emit_const(Value::Unit, last_line);
        c.emit(Op::Return, last_line);
        functions.push(Function {
            name: f.name.clone(),
            arity: f.params.len(),
            n_captures: 0, // named free functions are never constructed as closures
            chunk: c.chunk,
        });
        // Drain any lambda sub-functions emitted during this body's compilation.
        lambdas.extend(c.extra_functions);
    }
    for (ci, cd) in class_decls.iter().enumerate() {
        let base = next_idx + lambdas.len();
        let (f, extras) = compile_constructor(
            cd,
            ci,
            &fns,
            &arities,
            &variants,
            &enum_descs,
            &classes,
            &class_descs,
            &names_index,
            &class_field_ctys[&cd.name],
            &class_field_ctys,
            &method_rets,
            base,
        )?;
        functions.push(f);
        lambdas.extend(extras);
    }
    for (ci, f) in &methods_to_compile {
        let class_name = &class_decls[*ci].name;
        let base = next_idx + lambdas.len();
        let (func, extras) = compile_method(
            class_name,
            f,
            &fns,
            &arities,
            &variants,
            &enum_descs,
            &classes,
            &class_descs,
            &names_index,
            &class_field_ctys[class_name],
            &class_field_ctys,
            &method_rets,
            base,
        )?;
        functions.push(func);
        lambdas.extend(extras);
    }

    // Append the trailing lambda block. Named functions occupy `0..next_idx` (hoist order); every
    // lambda follows at the index it was numbered with (`next_idx + its position within lambdas`).
    functions.extend(lambdas);

    Ok(BytecodeProgram {
        functions,
        main,
        enum_descs,
        class_descs,
        names,
        methods,
        // The single shared interface table — same algorithm as the interpreter + checker, so the
        // VM's `Op::IsInstance` against an interface is byte-identical (M-RT S2).
        class_implements: crate::ast::class_implements(program),
    })
}

/// The constructor's `(params, body)`, or `(&[], &[])` for a class with no explicit constructor
/// (which still builds an empty instance — interpreter parity).
fn ctor_parts(c: &ClassDecl) -> (&[CtorParam], &[Stmt]) {
    for m in &c.members {
        if let ClassMember::Constructor { params, body, .. } = m {
            return (params, body);
        }
    }
    (&[], &[])
}

/// A ctor param is *promoted* to a field iff it carries a visibility modifier (matching
/// `interpreter::construct` / the checker's promotion rule).
fn is_promoted(p: &CtorParam) -> bool {
    p.modifiers.iter().any(|m| {
        matches!(
            m,
            Modifier::Public | Modifier::Private | Modifier::Protected
        )
    })
}

/// Compile one class's synthetic constructor `<Class>::new` (decision P4-4). Layout: ctor params
/// occupy slots `0..nparams`; the prologue loads the promoted params and `MakeInstance` builds the
/// instance into slot `nparams`; the body runs for side effects with the instance live; the
/// epilogue loads and returns that instance. The body's own `return`s are redirected to the
/// epilogue (never an `Op::Return`), so — exactly like the interpreter — a ctor body cannot change
/// the result: the promoted instance is always returned.
#[allow(clippy::too_many_arguments)]
fn compile_constructor<'a>(
    c: &ClassDecl,
    desc_idx: usize,
    fns: &'a HashMap<String, FnMeta>,
    arities: &'a [usize],
    variants: &'a HashMap<String, VariantMeta>,
    enum_descs: &'a [EnumDesc],
    classes: &'a HashMap<String, usize>,
    class_descs: &'a [ClassDesc],
    names_index: &'a HashMap<String, usize>,
    field_tags: &'a HashMap<String, CTy>,
    class_field_ctys: &'a HashMap<String, HashMap<String, CTy>>,
    method_rets: &'a HashMap<(String, String), CTy>,
    base_fn_idx: usize,
) -> Result<(Function, Vec<Function>), String> {
    let (params, body) = ctor_parts(c);
    let line = c.span.line;
    let mut comp = Compiler::new(
        fns,
        arities,
        variants,
        enum_descs,
        classes,
        class_descs,
        names_index,
        field_tags,
        class_field_ctys,
        method_rets,
        base_fn_idx,
    );
    comp.cur_class = Some(c.name.clone()); // `this` resolves to this class (ctype)
    for p in params {
        comp.add_local(&p.name, resolve_cty(&p.ty));
    }
    comp.height = comp.locals.len();
    // Prologue: load promoted params in declaration order, then build the instance. `MakeInstance`
    // pops exactly those values (matching `class_descs[desc_idx].fields`), so the order lines up.
    for (slot, p) in params.iter().enumerate() {
        if is_promoted(p) {
            comp.emit(Op::GetLocal(slot), line);
        }
    }
    comp.emit(Op::MakeInstance(desc_idx), line);
    let inst_slot = comp.add_local("$this", CTy::Other);
    comp.this_slot = Some(inst_slot); // a ctor body may reference `this` / bare fields
                                      // Body: returns are redirected to the epilogue (the body cannot change the constructed value).
    comp.ctor_return_jumps = Some(Vec::new());
    for s in body {
        comp.stmt(s)?;
    }
    let jumps = comp.ctor_return_jumps.take().unwrap_or_default();
    // Epilogue: every redirected `return` and the natural fall-through converge here.
    for j in jumps {
        comp.patch_jump(j);
    }
    comp.emit(Op::GetLocal(inst_slot), line);
    comp.emit(Op::Return, line);
    Ok((
        Function {
            name: format!("{}::new", c.name),
            arity: params.len(),
            n_captures: 0, // constructors are never closures
            chunk: comp.chunk,
        },
        comp.extra_functions,
    ))
}

/// Compile one instance method as a function (decision P4-6). Layout: slot 0 is the receiver
/// (`this`), slots `1..=nparams` are the params; the body runs with `this` and the class's field
/// scope live; an implicit `Unit` return terminates it (P3-7). The frame is opened by
/// `Op::CallMethod`, which places the receiver at slot 0.
#[allow(clippy::too_many_arguments)]
fn compile_method<'a>(
    class_name: &str,
    f: &FunctionDecl,
    fns: &'a HashMap<String, FnMeta>,
    arities: &'a [usize],
    variants: &'a HashMap<String, VariantMeta>,
    enum_descs: &'a [EnumDesc],
    classes: &'a HashMap<String, usize>,
    class_descs: &'a [ClassDesc],
    names_index: &'a HashMap<String, usize>,
    field_tags: &'a HashMap<String, CTy>,
    class_field_ctys: &'a HashMap<String, HashMap<String, CTy>>,
    method_rets: &'a HashMap<(String, String), CTy>,
    base_fn_idx: usize,
) -> Result<(Function, Vec<Function>), String> {
    let mut comp = Compiler::new(
        fns,
        arities,
        variants,
        enum_descs,
        classes,
        class_descs,
        names_index,
        field_tags,
        class_field_ctys,
        method_rets,
        base_fn_idx,
    );
    comp.cur_class = Some(class_name.to_string()); // `this` resolves to this class (ctype)
    comp.add_local("$this", CTy::Other); // slot 0 = receiver
    for p in &f.params {
        comp.add_local(&p.name, resolve_cty(&p.ty));
    }
    comp.this_slot = Some(0);
    comp.height = comp.locals.len();
    let last_line = f.span.line;
    for s in &f.body {
        comp.stmt(s)?;
    }
    comp.emit_const(Value::Unit, last_line);
    comp.emit(Op::Return, last_line);
    Ok((
        Function {
            name: format!("{class_name}::{}", f.name),
            arity: 1 + f.params.len(),
            n_captures: 0, // methods are never closures
            chunk: comp.chunk,
        },
        comp.extra_functions,
    ))
}

/// Resolve a declared type annotation into the compiler's class-aware `CTy` (M2 Wave 4), derived
/// purely structurally from the AST. The numeric heads map to `Int`/`Float`; the known
/// primitive/container head names collapse to `Other` (their element types are never operands in
/// the M1 surface); any *other* named type is a user-defined class, kept as `Class(name)` so a
/// field/method read through it resolves. An `Optional` is `Other` (no `null` in M1).
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
            "bool" | "string" | "void" | "Set" => CTy::Other,
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
            Op::Eq | Op::Ne | Op::Lt | Op::Gt | Op::Le | Op::Ge => -1,
            Op::Pop | Op::SetLocal(_) | Op::JumpIfFalse(_) | Op::Index | Op::MakeRange(_) => -1,
            // SetIndex pops (container, index, value) and pushes the new container: net -2.
            Op::SetIndex => -2,
            Op::Neg | Op::Not | Op::Len | Op::Jump(_) => 0,
            Op::MatchTag(_) | Op::GetEnumField(_) => 0, // pop one, push one
            Op::Concat(n) | Op::MakeList(n) => 1 - *n as isize,
            Op::MakeMap(n) => 1 - 2 * *n as isize, // pops 2n (key+value pairs), pushes the map
            // Pops `argc` args, pushes the native's return value (the old `Print` + `Const(Unit)`
            // pair collapses into one op, net delta unchanged).
            Op::CallNative(_, argc) => 1 - *argc as isize,
            Op::Call(idx) => 1 - self.arities[*idx] as isize,
            Op::MakeEnum(idx) => 1 - self.enum_descs[*idx].arity as isize,
            Op::MakeInstance(idx) => 1 - self.class_descs[*idx].fields.len() as isize,
            Op::GetField(_) => 0,   // pop instance, push field value
            Op::SetField(_) => -2,  // pop instance + value, push nothing (statement)
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
            Expr::Bool(..) | Expr::Str(..) | Expr::Bytes(..) => Ok(CTy::Other),
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
            Expr::Member { object, name, .. } => match self.ctype(object)? {
                CTy::Class(cls) => self
                    .class_field_ctys
                    .get(&cls)
                    .and_then(|fs| fs.get(name))
                    .cloned()
                    .ok_or_else(|| format!("no field `{name}` on `{cls}`")),
                _ => Err(format!("cannot infer type of field `{name}`")),
            },
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
            CTy::Class(_) | CTy::Other | CTy::List(_) | CTy::Map(..) | CTy::Fn { .. } => None,
        }
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        // Every statement begins with a clean operand stack (transients == 0), so the live operand
        // height equals the live-locals count. Anchoring here keeps `match`'s scrutinee slot exact
        // regardless of any height drift in preceding dead-code-after-`return`.
        self.height = self.locals.len();
        match s {
            Stmt::VarDecl { ty, name, init, .. } => {
                self.expr(init)?; // value stays on the stack as the new local's slot
                                  // `var` carries no annotation — derive the local's `CTy` from the initializer so
                                  // later arithmetic on it still specializes (AddI/AddF). `ctype` is total over
                                  // checker-valid initializers here; fall back to `Other` defensively so a `var`
                                  // never makes a program the interpreter accepts fail to compile (parity spine).
                let local_ty = match ty {
                    Type::Infer(_) => self.ctype(init).unwrap_or(CTy::Other),
                    _ => resolve_cty(ty),
                };
                self.add_local(name, local_ty);
                Ok(())
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => match target {
                // Local reassignment reuses `Op::SetLocal` — no new Op (M-mut.1). The checker
                // guarantees the target is a `mutable` in-scope local, so the slot always resolves.
                Expr::Ident(name, _) => {
                    let slot = self
                        .resolve_local(name)
                        .ok_or_else(|| format!("unresolved local in assignment: {name}"))?;
                    self.expr(value)?; // push the new value
                    self.emit(Op::SetLocal(slot), span.line); // set-and-pop into the existing slot
                    Ok(())
                }
                // Value-type element set `xs[i] = e` / `m[k] = e` (M-mut.5). The container is a
                // mutable local (checker-enforced); load it, push index + value, `SetIndex` (COW),
                // then store the resulting container back. Nested places (`a[i][j]`, `this.f[i]`)
                // are a later slice — the checker rejects a non-Ident container as `E-ASSIGN-TARGET`.
                Expr::Index { object, index, .. } => {
                    let name = match &**object {
                        Expr::Ident(n, _) => n,
                        _ => unreachable!("checker restricts index-assign to a local container"),
                    };
                    let slot = self
                        .resolve_local(name)
                        .ok_or_else(|| format!("unresolved local in index-assignment: {name}"))?;
                    self.emit(Op::GetLocal(slot), span.line); // [container]
                    self.expr(index)?; // [container, index]
                    self.expr(value)?; // [container, index, value]
                    self.emit(Op::SetIndex, span.line); // [newcontainer]
                    self.emit(Op::SetLocal(slot), span.line); // write back
                    Ok(())
                }
                // Shared-mutable instance field set `o.f = e` / `this.f = e` (M-mut.6). Evaluate the
                // object then the value (interpreter eval order), then `SetField` mutates the shared
                // `Rc<Instance>` cell in place and pops both. The field is checker-guaranteed `mutable`.
                Expr::Member { object, name, .. } => {
                    self.expr(object)?; // [instance]
                    self.expr(value)?; // [instance, value]
                    let idx = self.field_name_index(name)?;
                    self.emit(Op::SetField(idx), span.line); // mutate in place, pop both
                    Ok(())
                }
                _ => unreachable!("checker rejects other assignment targets"),
            },
            Stmt::Expr(e, span) => {
                self.expr(e)?;
                self.emit(Op::Pop, span.line);
                Ok(())
            }
            Stmt::Return { value, span } => {
                // Inside a synthetic constructor body, a `return` does not yield the body's value:
                // the interpreter discards it and always returns the promoted instance
                // (`construct`). So evaluate any operand for its side effects, drop it, and jump to
                // the ctor epilogue (which loads + returns the instance). The checker pins a ctor
                // body's return type to `Unit`, so `value` is `None` or a unit-typed expression.
                if self.ctor_return_jumps.is_some() {
                    if let Some(e) = value {
                        self.expr(e)?;
                        self.emit(Op::Pop, span.line);
                    }
                    let j = self.emit_jump(Op::Jump(0), span.line);
                    self.ctor_return_jumps
                        .as_mut()
                        .expect("ctor_return_jumps is Some")
                        .push(j);
                    return Ok(());
                }
                match value {
                    Some(e) => self.expr(e)?,
                    None => self.emit_const(Value::Unit, span.line),
                }
                self.emit(Op::Return, span.line);
                Ok(())
            }
            Stmt::Block(stmts, span) => {
                self.begin_scope();
                for st in stmts {
                    self.stmt(st)?;
                }
                self.end_scope(span.line);
                Ok(())
            }
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => self.compile_if(
                cond,
                bind.as_deref(),
                then_block,
                else_block.as_deref(),
                span.line,
            ),
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => self.compile_for(name, resolve_cty(ty), iter, body, span.line),
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => self.compile_while(cond, body, *post_cond, span.line),
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                span,
            } => self.compile_cfor(
                init.as_deref(),
                cond.as_ref(),
                step.as_deref(),
                body,
                span.line,
            ),
            Stmt::Break(span) => self.compile_break_continue(true, span.line),
            Stmt::Continue(span) => self.compile_break_continue(false, span.line),
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Int(n, sp) => self.emit_const(Value::Int(*n), sp.line),
            Expr::Float(x, sp) => self.emit_const(Value::Float(*x), sp.line),
            Expr::Bool(b, sp) => self.emit_const(Value::Bool(*b), sp.line),
            Expr::Str(parts, sp) => self.compile_str(parts, sp.line)?,
            Expr::Bytes(b, sp) => {
                self.emit_const(Value::Bytes(std::rc::Rc::new(b.clone())), sp.line)
            }
            Expr::Ident(name, sp) => {
                // Resolution order mirrors the interpreter's `eval_ident`: a `match`-arm binding
                // (re-extracted from `$match` along its payload path; P4-7) shadows a local/param,
                // which shadows a bare field of `this` (a method/ctor body, lowered to
                // `this.field`). An unresolved name is a compiler bug (the checker ran first).
                if let Some((slot, path)) = self.resolve_binding(name) {
                    self.emit(Op::GetLocal(slot), sp.line);
                    for i in path {
                        self.emit(Op::GetEnumField(i), sp.line);
                    }
                } else if let Some(slot) = self.resolve_local(name) {
                    self.emit(Op::GetLocal(slot), sp.line);
                } else if let (Some(this), true) =
                    (self.this_slot, self.field_tags.contains_key(name))
                {
                    let idx = self.field_name_index(name)?;
                    self.emit(Op::GetLocal(this), sp.line);
                    self.emit(Op::GetField(idx), sp.line);
                } else if let Some(idx) = self.fns.get(name).map(|m| m.index) {
                    // Bare named-function reference in value position → a zero-capture closure.
                    // Read the index from the immutable `self.fns` borrow into a local before
                    // calling `self.emit` (which needs `&mut self`).
                    self.emit(Op::MakeClosure(idx), sp.line);
                } else {
                    return Err(format!("undefined variable `{name}`"));
                }
            }
            Expr::List(items, sp) => {
                for it in items {
                    self.expr(it)?;
                }
                self.emit(Op::MakeList(items.len()), sp.line);
            }
            Expr::Map(pairs, sp) => {
                // Push each key then its value (source order); `Op::MakeMap(n)` pops the 2n values and
                // builds the insertion-ordered map via the shared `build_map` kernel (M-RT S3).
                for (k, v) in pairs {
                    self.expr(k)?;
                    self.expr(v)?;
                }
                self.emit(Op::MakeMap(pairs.len()), sp.line);
            }
            Expr::Unary { op, expr, span } => {
                self.expr(expr)?;
                match op {
                    UnaryOp::Neg => self.emit(Op::Neg, span.line),
                    UnaryOp::Not => self.emit(Op::Not, span.line),
                }
            }
            Expr::Binary { op, lhs, rhs, span } => self.compile_binary(*op, lhs, rhs, span.line)?,
            Expr::InstanceOf {
                value,
                type_name,
                span,
            } => {
                // Push the value, then a single `IsInstance` op carrying the class name inline pops
                // it and pushes a `Bool` (M-RT S1). The class name lives in the op (like `Fault`), so
                // no name-pool entry is needed and the runtime predicate matches the interpreter.
                self.expr(value)?;
                self.emit(Op::IsInstance(type_name.clone()), span.line);
            }
            Expr::Call { callee, args, span } => self.compile_call(callee, args, span.line)?,
            Expr::Null(sp) => self.emit_const(Value::Null, sp.line),
            Expr::This(sp) => match self.this_slot {
                // `this` is the receiver local: slot 0 in a method, the instance slot in a ctor.
                Some(slot) => self.emit(Op::GetLocal(slot), sp.line),
                // Checker-unreachable (`this` outside a method/ctor); mirrors the interpreter.
                None => return Err("`this` used outside a method".into()),
            },
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => {
                // Field read: evaluate the object, then look its field up at runtime by name
                // (decision P4-5). Runtime lookup keeps the compiler untyped; the fault on a miss
                // is byte-identical to the interpreter's. `?.` (safe) short-circuits a null receiver.
                let line = span.line;
                if *safe {
                    self.compile_safe_access(object, line, |c| {
                        let idx = c.field_name_index(name)?;
                        c.emit(Op::GetField(idx), line);
                        Ok(())
                    })?;
                } else {
                    self.expr(object)?;
                    let idx = self.field_name_index(name)?;
                    self.emit(Op::GetField(idx), line);
                }
            }
            Expr::Index {
                object,
                index,
                span,
            } => {
                // Push the list, then the index; `Op::Index` pops index-then-list and pushes the
                // bounds-checked element clone (the same op `compile_for` already uses).
                self.expr(object)?;
                self.expr(index)?;
                self.emit(Op::Index, span.line);
            }
            Expr::Force { inner, span } => self.compile_force(inner, span.line)?,
            Expr::CloneWith {
                object,
                fields,
                span,
            } => self.compile_clone_with(object, fields, span.line)?,
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => self.compile_match(scrutinee, arms, span.line)?,
            Expr::Range {
                start,
                end,
                inclusive,
                span,
            } => {
                // Push start, then end; `MakeRange` pops end-then-start and materializes the list.
                self.expr(start)?;
                self.expr(end)?;
                self.emit(Op::MakeRange(*inclusive), span.line);
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => {
                // Lower like `&&`/`||`: branch on the cond, each arm leaves exactly one value, and
                // the merge height is reset so both arms agree on the single result slot.
                self.expr(cond)?;
                let else_j = self.emit_jump(Op::JumpIfFalse(0), span.line); // pops cond
                let h_merge = self.height; // both arms converge to one value above this
                self.expr(then_expr)?;
                let end_j = self.emit_jump(Op::Jump(0), span.line);
                self.patch_jump(else_j);
                self.height = h_merge; // else path starts at the merge height
                self.expr(else_expr)?;
                self.patch_jump(end_j);
            }
            Expr::Lambda {
                params,
                body,
                ret,
                span,
            } => self.compile_lambda(params, body, ret.as_ref(), span.line)?,
            // `html"…"` literals are erased to `html.concat([…])` kernel calls by
            // `checker::resolve_html` before compilation; the compiler never sees one.
            Expr::Html(..) => unreachable!("html literal not resolved before compilation"),
        }
        Ok(())
    }

    fn compile_str(&mut self, parts: &[StrPart], line: u32) -> Result<(), String> {
        // A single literal segment (or empty) is just a string constant.
        if let [StrPart::Literal(s)] = parts {
            self.emit_const(Value::Str(s.clone()), line);
            return Ok(());
        }
        if parts.is_empty() {
            self.emit_const(Value::Str(String::new()), line);
            return Ok(());
        }
        for part in parts {
            match part {
                StrPart::Literal(s) => self.emit_const(Value::Str(s.clone()), line),
                StrPart::Expr(e) => self.expr(e)?,
            }
        }
        self.emit(Op::Concat(parts.len()), line);
        Ok(())
    }

    fn compile_binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        line: u32,
    ) -> Result<(), String> {
        use BinaryOp::*;
        // Short-circuit logical ops desugar to jumps (decision P2-5).
        match op {
            And => {
                self.expr(lhs)?;
                let l_false = self.emit_jump(Op::JumpIfFalse(0), line); // pops lhs
                let h_merge = self.height; // both branches converge to one bool above this
                self.expr(rhs)?;
                let l_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(l_false);
                self.height = h_merge; // false-path: reset before pushing the literal `false`
                self.emit_const(Value::Bool(false), line);
                self.patch_jump(l_end);
                return Ok(());
            }
            Or => {
                self.expr(lhs)?;
                let l_rhs = self.emit_jump(Op::JumpIfFalse(0), line); // pops lhs
                let h_merge = self.height;
                self.emit_const(Value::Bool(true), line);
                let l_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(l_rhs);
                self.height = h_merge; // rhs-path: reset before evaluating rhs
                self.expr(rhs)?;
                self.patch_jump(l_end);
                return Ok(());
            }
            Coalesce => {
                // `a ?? b`: keep `a` unless it is null, without re-evaluating it. Stash `a` in a
                // scratch local (the `$match`-scrutinee trick), test it against `null`; if null,
                // evaluate `b` and overwrite the slot with it. No new `Op` (decision S2-OPS).
                self.expr(lhs)?; // [a] — a lands in the scratch slot
                                 // The scratch slot is `a`'s frame-relative position (top of stack), NOT
                                 // `locals.len()`: live transients (e.g. earlier interpolation segments) may sit
                                 // below it, so `add_local`'s index would be wrong. Mirrors `compile_match`'s
                                 // `m_slot = self.height - 1`. Addressed numerically by Get/SetLocal — no `Local` entry.
                let slot = self.height - 1;
                self.emit(Op::GetLocal(slot), line); // [a, a]
                self.emit_const(Value::Null, line); // [a, a, null]
                self.emit(Op::Eq, line); // [a, bool]
                let keep = self.emit_jump(Op::JumpIfFalse(0), line); // [a]; jump if a != null
                let h_merge = self.height;
                self.expr(rhs)?; // [a, b]
                self.emit(Op::SetLocal(slot), line); // [b] — overwrite the slot with b
                self.patch_jump(keep); // keep-path arrives with [a]; both leave one value at `slot`
                self.height = h_merge;
                return Ok(());
            }
            _ => {}
        }
        // Strict ops: evaluate both, then emit.
        match op {
            Add | Sub | Mul | Div | Rem => {
                let nt = self.num_ty(lhs)?;
                self.expr(lhs)?;
                self.expr(rhs)?;
                let emit = match (op, nt) {
                    (Add, NumTy::Int) => Op::AddI,
                    (Add, NumTy::Float) => Op::AddF,
                    (Sub, NumTy::Int) => Op::SubI,
                    (Sub, NumTy::Float) => Op::SubF,
                    (Mul, NumTy::Int) => Op::MulI,
                    (Mul, NumTy::Float) => Op::MulF,
                    (Div, NumTy::Int) => Op::DivI,
                    (Div, NumTy::Float) => Op::DivF,
                    (Rem, NumTy::Int) => Op::RemI,
                    (Rem, NumTy::Float) => Op::RemF,
                    _ => unreachable!("arithmetic op set"),
                };
                self.emit(emit, line);
            }
            Eq => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::Eq, line);
            }
            NotEq => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::Ne, line);
            }
            Lt | Gt | Le | Ge => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(
                    match op {
                        Lt => Op::Lt,
                        Gt => Op::Gt,
                        Le => Op::Le,
                        Ge => Op::Ge,
                        _ => unreachable!(),
                    },
                    line,
                );
            }
            Pipe => unreachable!("`|>` is lowered to a call in the parser"),
            And | Or | Coalesce => unreachable!("handled above"),
        }
        Ok(())
    }

    fn compile_call(&mut self, callee: &Expr, args: &[Expr], line: u32) -> Result<(), String> {
        if let Expr::Ident(name, _) = callee {
            if let Some(meta) = self.fns.get(name) {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::Call(meta.index), line);
                return Ok(());
            }
            // A local variable with a function type (lambda or named-fn ref): push the closure
            // first (by its local slot), then the args, then dispatch with `CallValue`.
            if let Some(slot) = self.resolve_local(name) {
                if matches!(self.locals[slot].ty, CTy::Fn { .. }) {
                    self.emit(Op::GetLocal(slot), line); // push the closure value
                    for a in args {
                        self.expr(a)?;
                    }
                    self.emit(Op::CallValue(args.len()), line);
                    return Ok(());
                }
            }
            // A match-arm binding with a function type (lambda passed as an argument).
            if let Some((slot, path)) = self.resolve_binding(name) {
                if matches!(
                    self.match_bindings
                        .iter()
                        .rev()
                        .find(|b| b.name == *name)
                        .map(|b| &b.ty),
                    Some(CTy::Fn { .. })
                ) {
                    // Re-extract the closure from its binding path.
                    self.emit(Op::GetLocal(slot), line);
                    for i in path {
                        self.emit(Op::GetEnumField(i), line);
                    }
                    for a in args {
                        self.expr(a)?;
                    }
                    self.emit(Op::CallValue(args.len()), line);
                    return Ok(());
                }
            }
            // An enum variant constructor: `Variant(args)` (or a bare `Variant`, args empty).
            // The checker has already verified arity, so push the payload and tag it (P4-3).
            if let Some(meta) = self.variants.get(name) {
                let idx = meta.index;
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::MakeEnum(idx), line);
                return Ok(());
            }
            // A class constructor: `ClassName(args)` calls the synthetic `<Class>::new`, which
            // promotes its params into fields and returns the instance (decision P4-4).
            if let Some(&ctor_idx) = self.classes.get(name) {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::Call(ctor_idx), line);
                return Ok(());
            }
            // Unreachable for checker-validated programs; mirrors `interpreter::eval_call`'s wording.
            return Err(format!("`{name}` is not a function, variant, or class"));
        }
        // Method call `object.name(args)`: evaluate the receiver, then the args, and dispatch by
        // name at runtime off the receiver's class (decision P4-6).
        if let Expr::Member {
            object, name, safe, ..
        } = callee
        {
            // Namespaced native call: `console.println(x)` — a member call whose head is an imported
            // module qualifier rather than a value (M3 Wave 1). Locals-first: only an identifier that
            // is *not* a bound local/match-binding can be a qualifier, and the checker has already
            // enforced that it was imported and the native exists, so `index_of_by_leaf` is an
            // unambiguous lower (every stdlib leaf is distinct). Lowers to `Op::CallNative`, which
            // pushes the native's result — no separate `Const(Unit)` (the old `Print` path's pair).
            if !*safe {
                if let Expr::Ident(q, _) = &**object {
                    if self.resolve_local(q).is_none() && self.resolve_binding(q).is_none() {
                        if let Some(idx) = crate::native::index_of_by_leaf(q, name) {
                            for a in args {
                                self.expr(a)?;
                            }
                            self.emit(Op::CallNative(idx, args.len()), line);
                            return Ok(());
                        }
                    }
                }
            }
            // `o?.m(args)`: a null receiver short-circuits — the args are NOT evaluated and the
            // method is NOT dispatched (the null-skip lowering jumps over the whole `access`).
            if *safe {
                return self.compile_safe_access(object, line, |c| {
                    for a in args {
                        c.expr(a)?;
                    }
                    let idx = c.field_name_index(name)?;
                    c.emit(Op::CallMethod(idx, args.len()), line);
                    Ok(())
                });
            }
            self.expr(object)?;
            for a in args {
                self.expr(a)?;
            }
            let idx = self.field_name_index(name)?;
            self.emit(Op::CallMethod(idx, args.len()), line);
            return Ok(());
        }
        // Inline lambda call: `(fn(int x) => x+1)(3)` or (after pipe lowering) `3 |> fn(int v) =>
        // v+10`. Compile the lambda expression to push a closure, then push args, then dispatch.
        if let Expr::Lambda {
            params,
            body,
            ret,
            span,
        } = callee
        {
            self.compile_lambda(params, body, ret.as_ref(), span.line)?;
            for a in args {
                self.expr(a)?;
            }
            self.emit(Op::CallValue(args.len()), line);
            return Ok(());
        }
        Err("unsupported call target".into())
    }

    /// Lower a `?.` access (field read or method call): evaluate `object`; if it is `null`, the
    /// whole access short-circuits to `null`; otherwise run `access`, which transforms the receiver
    /// on top of the stack into the member result. No new `Op` (decision S2-OPS): a scratch local
    /// peeks the receiver for the null test (the `$coalesce` trick from `??`), then a
    /// `JumpIfFalse`/`Jump` pair selects the path. Both paths leave exactly one value at the
    /// receiver's slot, so the static height is the receiver's height throughout.
    fn compile_safe_access(
        &mut self,
        object: &Expr,
        line: u32,
        access: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        self.expr(object)?; // [.., recv]
                            // `recv`'s frame-relative slot (top of stack), NOT `locals.len()`: live transients (earlier
                            // interpolation segments, an enclosing `??`'s lhs, …) may sit below it. Mirrors
                            // `compile_match`'s `m_slot = self.height - 1`; addressed numerically, no `Local` entry.
        let slot = self.height - 1;
        self.emit(Op::GetLocal(slot), line); // [.., recv, recv]
        self.emit_const(Value::Null, line); // [.., recv, recv, null]
        self.emit(Op::Eq, line); // [.., recv, bool]
        let do_access = self.emit_jump(Op::JumpIfFalse(0), line); // [.., recv]; recv != null → access
        let to_end = self.emit_jump(Op::Jump(0), line); // recv == null → keep recv (= null), skip access
        self.patch_jump(do_access);
        let h = self.height;
        access(self)?; // [.., recv] -> [.., member]
        self.patch_jump(to_end);
        self.height = h; // both paths converge here with one value at the receiver's slot
        Ok(())
    }

    /// `inner!` checked force-unwrap (M3 S2.5). Evaluate the inner; a non-consuming null-test keeps
    /// the value when present, else raises `Op::Fault(ForceUnwrapNull)` — byte-identical to the
    /// interpreter's `"force-unwrap of null"` fault. No new `Op` (the fault op is the generalized
    /// `MatchFail`). `o! + 1` still specializes because `ctype(Force)` resolves the result's type.
    fn compile_force(&mut self, inner: &Expr, line: u32) -> Result<(), String> {
        self.expr(inner)?; // [opt] — stays as the result when non-null
                           // `opt`'s frame-relative slot (top of stack), NOT `locals.len()`: transients may sit below
                           // it (e.g. `"{a!} {b!}"`). Mirrors `compile_match`. `ctype(Force)` handles operand typing of
                           // the *result*, so the scratch needs no `CTy`. Addressed numerically, no `Local` entry.
        let slot = self.height - 1;
        self.emit(Op::GetLocal(slot), line); // [opt, opt]
        self.emit_const(Value::Null, line); // [opt, opt, null]
        self.emit(Op::Eq, line); // [opt, opt == null]
        let ok = self.emit_jump(Op::JumpIfFalse(0), line); // [opt]; non-null → keep, skip the fault
        self.emit(Op::Fault(FaultMsg::ForceUnwrapNull), line); // null → clean fault (terminal)
        self.patch_jump(ok);
        Ok(())
    }

    /// `obj with { f = e, … }` (M-mut.4a). Reconstruct a fresh instance: evaluate `obj` into a
    /// scratch slot, then push each descriptor field in order — the override expr if named, else a
    /// `GetField` re-read of the source — and `MakeInstance` (which runs **no** constructor body,
    /// Fork 2 = B). Collapse the new instance over the scratch slot so the expression leaves one
    /// value. No new `Op`. The checker proved `obj` is a concrete class and the names are its fields.
    fn compile_clone_with(
        &mut self,
        object: &Expr,
        fields: &[(String, Expr)],
        line: u32,
    ) -> Result<(), String> {
        let class = match self.ctype(object)? {
            CTy::Class(name) => name,
            _ => return Err("`with` requires a class instance".into()),
        };
        let desc_idx = self
            .class_descs
            .iter()
            .position(|d| d.class == class)
            .ok_or_else(|| format!("unknown class `{class}` in `with`"))?;
        let field_names = self.class_descs[desc_idx].fields.clone();
        self.expr(object)?; // [.., src]
        let src_slot = self.height - 1; // numeric scratch (transients may sit below), like `compile_match`
        for fname in &field_names {
            if let Some((_, e)) = fields.iter().find(|(n, _)| n == fname) {
                self.expr(e)?; // [.., override]
            } else {
                self.emit(Op::GetLocal(src_slot), line); // [.., src]
                let idx = self.field_name_index(fname)?;
                self.emit(Op::GetField(idx), line); // [.., src.field]
            }
        }
        self.emit(Op::MakeInstance(desc_idx), line); // pops the fields → [.., src, newInstance]
        self.emit(Op::SetLocal(src_slot), line); // collapse newInstance over the scratch → [.., newInstance]
        Ok(())
    }

    /// Compile a `fn(params) => body` expression-body lambda (M3 S3 Task 4).
    ///
    /// Layout:
    ///   - Compute the lambda's free variables (sorted, deterministic — invariant #8).
    ///   - Filter out names that resolve to top-level functions (not captures).
    ///   - For each capture: emit `GetLocal(slot)` to push it onto the stack.
    ///   - Build a sub-`Function` with layout `[captures.., params..]`.
    ///     * The sub-compiler's locals start with the captures (in free-var order),
    ///       then the declared params — matching the frame layout `CallValue` sets up.
    ///   - Append the sub-`Function` to `self.extra_functions` and record its `n_captures`.
    ///   - Emit `Op::MakeClosure(fn_idx)` which pops the captures and pushes a `Value::Closure`.
    fn compile_lambda(
        &mut self,
        params: &[Param],
        body: &LambdaBody,
        _ret: Option<&Type>,
        line: u32,
    ) -> Result<(), String> {
        // 1. Compute free variables of the lambda body.
        let all_free = free_vars(params, body);
        // 2. Filter to only variables that resolve to a local in the *enclosing* scope
        //    (names that are top-level functions are resolved statically at call time and
        //    don't need to be captured — `compile_call` handles them via `Op::Call`).
        let captures: Vec<(usize, String)> = all_free
            .into_iter()
            .filter_map(|name| {
                // Only capture locals; top-level functions, variants, and classes are not.
                self.resolve_local(&name)
                    .filter(|_| !self.fns.contains_key(&name))
                    .map(|slot| (slot, name))
            })
            .collect();
        let n_captures = captures.len();

        // 3. Build the sub-function's index in the global table.
        //    `base_fn_idx` is the start of this compilation's slice of the trailing lambda block;
        //    each lambda this compiler emits takes the next slot, hence `base + len`.
        let fn_idx = self.base_fn_idx + self.extra_functions.len();

        // 4. Build a sub-compiler for the lambda body.
        //    This lambda occupies global slot `fn_idx`; step 8 appends its nested lambdas
        //    *immediately after* it, so they start at `fn_idx + 1`. The sub-compiler therefore
        //    treats `fn_idx + 1` as the start of its own (nested) lambda slice.
        let sub_base = fn_idx + 1;
        let empty_fields: HashMap<String, CTy> = HashMap::new();
        // A lambda body cannot reference `this` or bare fields (checker enforces E-LAMBDA-THIS),
        // so we create the sub-compiler without field scope or a class context.
        let mut sub = Compiler::new(
            self.fns,
            self.arities,
            self.variants,
            self.enum_descs,
            self.classes,
            self.class_descs,
            self.names_index,
            &empty_fields,
            self.class_field_ctys,
            self.method_rets,
            sub_base,
        );

        // 5. Seed the sub-compiler's locals: captures first, then params.
        //    Frame layout expected by `Op::CallValue`: [caps.., args..]
        for (_, cap_name) in &captures {
            // The capture's type comes from the enclosing scope's local.
            let slot = self
                .resolve_local(cap_name)
                .expect("capture must resolve in enclosing scope");
            let ty = self.locals[slot].ty.clone();
            sub.add_local(cap_name, ty);
        }
        for p in params {
            sub.add_local(&p.name, resolve_cty(&p.ty));
        }
        sub.height = sub.locals.len();

        // 6. Compile the body. Expression-body: evaluate + explicit Return.
        match body {
            LambdaBody::Expr(e) => {
                sub.expr(e)?;
                sub.emit(Op::Return, line);
            }
            LambdaBody::Block(stmts) => {
                for s in stmts {
                    sub.stmt(s)?;
                }
                sub.emit_const(Value::Unit, line);
                sub.emit(Op::Return, line);
            }
        }

        // 7. Collect any nested lambdas compiled inside the sub-compiler.
        let mut nested_extras = sub.extra_functions;

        // 8. Build the sub-function and append it to our own extra_functions.
        let lambda_fn = Function {
            name: format!("<lambda@{line}>"),
            arity: n_captures + params.len(),
            n_captures,
            chunk: sub.chunk,
        };
        self.extra_functions.push(lambda_fn);
        self.lambda_n_captures.push(n_captures);
        // Drain nested extras: their indices follow this lambda in the table.
        self.extra_functions.append(&mut nested_extras);

        // 9. Push capture values onto the stack (enclosing scope), then emit MakeClosure.
        for (slot, _) in &captures {
            self.emit(Op::GetLocal(*slot), line);
        }
        self.emit(Op::MakeClosure(fn_idx), line);
        Ok(())
    }

    fn compile_if(
        &mut self,
        cond: &Expr,
        bind: Option<&str>,
        then_block: &[Stmt],
        else_block: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
        if let Some(name) = bind {
            return self.compile_if_let(name, cond, then_block, else_block, line);
        }
        self.expr(cond)?;
        let else_jump = self.emit_jump(Op::JumpIfFalse(0), line); // pops cond
        self.begin_scope();
        for s in then_block {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let end_jump = self.emit_jump(Op::Jump(0), line);
        self.patch_jump(else_jump);
        if let Some(eb) = else_block {
            self.begin_scope();
            for s in eb {
                self.stmt(s)?;
            }
            self.end_scope(line);
        }
        self.patch_jump(end_jump);
        Ok(())
    }

    /// `if (var name = cond)` (M3 S2.4). The scrutinee value lands in a scoped local that *is* the
    /// binding `name` (its `CTy` is the optional's inner type so `name + 1` still specializes); a
    /// non-consuming null-test (`GetLocal; Const null; Ne`) selects the branch. No new `Op` — the
    /// scrutinee slot persists across both arms and is popped by the enclosing `end_scope`. The
    /// checker forbids referencing `name` in the else block, so leaving it registered is harmless.
    fn compile_if_let(
        &mut self,
        name: &str,
        cond: &Expr,
        then_block: &[Stmt],
        else_block: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        self.expr(cond)?; // [opt] — this slot becomes the binding `name`
        let inner_cty = self.ctype(cond).unwrap_or(CTy::Other);
        let slot = self.add_local(name, inner_cty);
        self.emit(Op::GetLocal(slot), line); // [opt, opt]
        self.emit_const(Value::Null, line); // [opt, opt, null]
        self.emit(Op::Ne, line); // [opt, opt != null]
        let else_jump = self.emit_jump(Op::JumpIfFalse(0), line); // pops bool → [opt]; jump if null
        self.begin_scope();
        for s in then_block {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let end_jump = self.emit_jump(Op::Jump(0), line);
        self.patch_jump(else_jump);
        if let Some(eb) = else_block {
            self.begin_scope();
            for s in eb {
                self.stmt(s)?;
            }
            self.end_scope(line);
        }
        self.patch_jump(end_jump);
        self.end_scope(line); // pops the scrutinee slot (`name`) — both arms converge with [opt] live
        Ok(())
    }

    /// `for (T name in iter)` desugars to a counter loop over an inline list
    /// (decision P2-7). Hidden locals `$for_list` and `$for_idx` bracket `name`.
    fn compile_for(
        &mut self,
        name: &str,
        elem_ty: CTy,
        iter: &Expr,
        body: &[Stmt],
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        self.expr(iter)?; // [list]
        let s_list = self.add_local("$for_list", CTy::Other);
        self.emit_const(Value::Int(0), line); // [list, 0]
        let s_idx = self.add_local("$for_idx", CTy::Int);

        // break/continue pop down to here (just `$for_list`+`$for_idx` live): a `break` lands at the
        // exit (where `end_scope` drops those two), a `continue` at the index-increment — both after
        // the loop variable has been dropped, so the pop count covers the loop var + any body locals.
        let body_base = self.locals.len();
        let loop_start = self.here();
        self.emit(Op::GetLocal(s_idx), line);
        self.emit(Op::GetLocal(s_list), line);
        self.emit(Op::Len, line); // [idx, len]
        self.emit(Op::Lt, line); // [idx < len]
        let exit_jump = self.emit_jump(Op::JumpIfFalse(0), line);

        self.emit(Op::GetLocal(s_list), line);
        self.emit(Op::GetLocal(s_idx), line);
        self.emit(Op::Index, line); // [elem]
        self.add_local(name, elem_ty); // elem becomes the loop variable

        self.loop_frames.push(LoopFrame {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_base,
        });
        self.begin_scope(); // body's own locals get cleaned each iteration
        for s in body {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let frame = self.loop_frames.pop().expect("for loop frame");

        self.emit(Op::Pop, line); // drop the loop variable
        self.locals.pop(); // unregister `name`

        // idx = idx + 1 — also the `continue` target (loop var already dropped above).
        let cont_target = self.here();
        for j in &frame.continue_jumps {
            self.patch_jump_to(*j, cont_target);
        }
        self.emit(Op::GetLocal(s_idx), line);
        self.emit_const(Value::Int(1), line);
        self.emit(Op::AddI, line);
        self.emit(Op::SetLocal(s_idx), line);
        self.emit(Op::Jump(loop_start), line);

        self.patch_jump(exit_jump);
        for j in &frame.break_jumps {
            self.patch_jump(*j);
        }
        self.end_scope(line); // pops $for_idx, $for_list
        Ok(())
    }

    /// `while (cond) { .. }` / `do { .. } while (cond);` (M-mut.3). Lowers to a `JumpIfFalse`-guarded
    /// back-edge — no new loop opcode (F5). A loop frame collects `break`/`continue` jumps; `break`
    /// targets the exit, `continue` the condition re-test (the loop top for `while`, the bottom test
    /// for `do-while`). `body_base` is the locals depth both forms pop down to.
    fn compile_while(
        &mut self,
        cond: &Expr,
        body: &[Stmt],
        post_cond: bool,
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        let body_base = self.locals.len();
        if post_cond {
            // do-while: body first, condition (and the continue target) at the bottom.
            let loop_start = self.here();
            self.loop_frames.push(LoopFrame {
                break_jumps: Vec::new(),
                continue_jumps: Vec::new(),
                body_base,
            });
            self.begin_scope();
            for s in body {
                self.stmt(s)?;
            }
            self.end_scope(line);
            let frame = self.loop_frames.pop().expect("do-while loop frame");

            let cont_target = self.here();
            for j in &frame.continue_jumps {
                self.patch_jump_to(*j, cont_target);
            }
            self.expr(cond)?; // [cond]
            let exit_jump = self.emit_jump(Op::JumpIfFalse(0), line); // false → exit
            self.emit(Op::Jump(loop_start), line); // true → loop again
            self.patch_jump(exit_jump);
            for j in &frame.break_jumps {
                self.patch_jump(*j);
            }
        } else {
            // while: condition (and the continue target) at the top.
            let loop_start = self.here();
            self.expr(cond)?; // [cond]
            let exit_jump = self.emit_jump(Op::JumpIfFalse(0), line); // pops cond
            self.loop_frames.push(LoopFrame {
                break_jumps: Vec::new(),
                continue_jumps: Vec::new(),
                body_base,
            });
            self.begin_scope();
            for s in body {
                self.stmt(s)?;
            }
            self.end_scope(line);
            let frame = self.loop_frames.pop().expect("while loop frame");

            self.emit(Op::Jump(loop_start), line);
            self.patch_jump(exit_jump);
            for j in &frame.break_jumps {
                self.patch_jump(*j);
            }
            for j in &frame.continue_jumps {
                self.patch_jump_to(*j, loop_start);
            }
        }
        self.end_scope(line);
        Ok(())
    }

    /// C-style `for (init; cond; step) { body }` (M-mut.3). `init`'s binding lives in the loop's own
    /// scope; `continue` jumps to `step`, `break` to the exit. Same jump back-edge as `compile_while`.
    fn compile_cfor(
        &mut self,
        init: Option<&Stmt>,
        cond: Option<&Expr>,
        step: Option<&Stmt>,
        body: &[Stmt],
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        if let Some(s) = init {
            self.stmt(s)?;
        }
        // break/continue pop down to here (init's local stays live; the exit's `end_scope` drops it).
        let body_base = self.locals.len();
        let loop_start = self.here();
        let exit_jump = if let Some(c) = cond {
            self.expr(c)?;
            Some(self.emit_jump(Op::JumpIfFalse(0), line))
        } else {
            None // `for (init;;step)` — no condition, loop until `break`
        };
        self.loop_frames.push(LoopFrame {
            break_jumps: Vec::new(),
            continue_jumps: Vec::new(),
            body_base,
        });
        self.begin_scope();
        for s in body {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let frame = self.loop_frames.pop().expect("c-for loop frame");

        // `continue` lands at the step (run the step, then re-test).
        let cont_target = self.here();
        for j in &frame.continue_jumps {
            self.patch_jump_to(*j, cont_target);
        }
        if let Some(s) = step {
            self.stmt(s)?;
        }
        self.emit(Op::Jump(loop_start), line);

        if let Some(e) = exit_jump {
            self.patch_jump(e);
        }
        for j in &frame.break_jumps {
            self.patch_jump(*j);
        }
        self.end_scope(line); // pops init's local
        Ok(())
    }

    /// Emit a `break` (`is_break`) or `continue`: pop every body-scope local back down to the
    /// innermost loop's `body_base`, then a placeholder `Jump` recorded in the loop frame for the
    /// loop to patch (exit for `break`, continue-target for `continue`). No new `Op` (F5). The
    /// checker rejects break/continue outside a loop, so the frame is always present.
    fn compile_break_continue(&mut self, is_break: bool, line: u32) -> Result<(), String> {
        let body_base = self
            .loop_frames
            .last()
            .map(|f| f.body_base)
            .ok_or("`break`/`continue` outside a loop")?;
        for _ in 0..(self.locals.len() - body_base) {
            self.emit(Op::Pop, line);
        }
        let j = self.emit_jump(Op::Jump(0), line);
        let frame = self.loop_frames.last_mut().expect("loop frame present");
        if is_break {
            frame.break_jumps.push(j);
        } else {
            frame.continue_jumps.push(j);
        }
        Ok(())
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

    /// Resolve a `match`-arm binding by name (innermost shadows). Returns the `$match` slot and the
    /// payload path to re-extract, cloned so the caller can emit without holding a borrow on `self`.
    fn resolve_binding(&self, name: &str) -> Option<(usize, Vec<usize>)> {
        self.match_bindings
            .iter()
            .rev()
            .find(|b| b.name == name)
            .map(|b| (b.match_slot, b.path.clone()))
    }

    /// `match scrutinee { pat => body, … }` as an expression (decision P4-7). The scrutinee is
    /// evaluated once and spilled to a hidden `$match` slot; each arm tests its pattern (skipping
    /// to the next arm on mismatch), binds payloads by re-extraction, then leaves its body's single
    /// value on the stack. A matched arm jumps past the rest to a collapse that overwrites the
    /// scrutinee slot with the result — so the whole `match` leaves exactly one value.
    fn compile_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        line: u32,
    ) -> Result<(), String> {
        // Class-aware type of the scrutinee, for a catch-all binding's type (best-effort: an
        // unresolvable scrutinee collapses to `Other`, which `as_num` rejects as an operand anyway).
        // A class-typed scrutinee's catch-all binding keeps its class, so `x.field` resolves (Wave 4).
        let scrut_cty = self.ctype(scrutinee).unwrap_or(CTy::Other);
        self.expr(scrutinee)?;
        let m_slot = self.height - 1; // scrutinee now on top: its base-relative slot
        let mut end_jumps = Vec::new();
        for arm in arms {
            self.height = m_slot + 1; // each arm dispatches with just the scrutinee live
            let mut skips = Vec::new();
            self.emit_pattern_test(&arm.pattern, m_slot, &[], &mut skips, line)?;
            let n_before = self.match_bindings.len();
            self.register_bindings(&arm.pattern, m_slot, &[], scrut_cty.clone())?;
            self.expr(&arm.body)?; // -> [.., scrutinee, result]
            self.match_bindings.truncate(n_before);
            end_jumps.push(self.emit_jump(Op::Jump(0), line));
            for j in skips {
                self.patch_jump(j); // a mismatch lands at the next arm
            }
        }
        self.emit(Op::Fault(FaultMsg::NonExhaustiveMatch), line); // checker-unreachable backstop (EV-7 parity)
        for j in end_jumps {
            self.patch_jump(j); // matched arms converge here: [.., scrutinee, result]
        }
        self.height = m_slot + 2;
        self.emit(Op::SetLocal(m_slot), line); // result overwrites scrutinee slot -> [.., result]
        Ok(())
    }

    /// Emit the test for `pat` against the `$match` sub-value reached by `path`. On a mismatch the
    /// emitted `JumpIfFalse`'s index is recorded in `skips` (the caller patches them to the next
    /// arm). Wildcard and binding patterns always match, so they emit no test.
    fn emit_pattern_test(
        &mut self,
        pat: &Pattern,
        m_slot: usize,
        path: &[usize],
        skips: &mut Vec<usize>,
        line: u32,
    ) -> Result<(), String> {
        match pat {
            Pattern::Wildcard(_) | Pattern::Binding { .. } => {}
            Pattern::Int(n, _) => self.emit_literal_test(m_slot, path, Value::Int(*n), skips, line),
            Pattern::Float(x, _) => {
                self.emit_literal_test(m_slot, path, Value::Float(*x), skips, line);
            }
            Pattern::Str(s, _) => {
                self.emit_literal_test(m_slot, path, Value::Str(s.clone()), skips, line);
            }
            Pattern::Bool(b, _) => {
                self.emit_literal_test(m_slot, path, Value::Bool(*b), skips, line);
            }
            Pattern::Null(_) => {
                // M3 S2.6: the arm matches iff the scrutinee is `null` — a literal `Eq null` test
                // (interpreter parity, `match_pattern`). `eq_val` defines `(Null, Null) => true`.
                self.emit_literal_test(m_slot, path, Value::Null, skips, line);
            }
            Pattern::Variant { name, fields, .. } => {
                let idx = self
                    .variants
                    .get(name)
                    .ok_or_else(|| format!("unknown variant `{name}`"))?
                    .index;
                self.emit_load_path(m_slot, path, line);
                self.emit(Op::MatchTag(idx), line);
                skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
                for (i, fp) in fields.iter().enumerate() {
                    let mut sub = path.to_vec();
                    sub.push(i);
                    self.emit_pattern_test(fp, m_slot, &sub, skips, line)?;
                }
            }
            // M-RT S4 type pattern: the arm matches iff the sub-value is an instance of `type_name`
            // — the SAME runtime test as `instanceof`, reusing `Op::IsInstance` (no new op). Mirrors
            // the Variant flow: load the value, test, skip on a false result.
            Pattern::Type { type_name, .. } => {
                self.emit_load_path(m_slot, path, line);
                self.emit(Op::IsInstance(type_name.clone()), line);
                skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
            }
        }
        Ok(())
    }

    /// Load the `$match` sub-value at `path`, compare it to `lit`, and skip the arm on inequality.
    fn emit_literal_test(
        &mut self,
        m_slot: usize,
        path: &[usize],
        lit: Value,
        skips: &mut Vec<usize>,
        line: u32,
    ) {
        self.emit_load_path(m_slot, path, line);
        self.emit_const(lit, line);
        self.emit(Op::Eq, line);
        skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
    }

    /// Push the sub-value of the `$match` scrutinee (slot `m_slot`) reached by `path`.
    fn emit_load_path(&mut self, m_slot: usize, path: &[usize], line: u32) {
        self.emit(Op::GetLocal(m_slot), line);
        for &i in path {
            self.emit(Op::GetEnumField(i), line);
        }
    }

    /// Register (emitting no code) every binding introduced by `pat`, so the arm body can
    /// re-extract them. `cur_ty` is the class-aware type of the value `pat` matches (for `ctype`) —
    /// a class-typed payload keeps its class, so `binding.field` resolves (Wave 4).
    fn register_bindings(
        &mut self,
        pat: &Pattern,
        m_slot: usize,
        path: &[usize],
        cur_ty: CTy,
    ) -> Result<(), String> {
        match pat {
            Pattern::Binding { name, .. } => self.match_bindings.push(MatchBinding {
                name: name.clone(),
                match_slot: m_slot,
                path: path.to_vec(),
                ty: cur_ty,
            }),
            Pattern::Variant { name, fields, .. } => {
                let field_tags = self
                    .variants
                    .get(name)
                    .ok_or_else(|| format!("unknown variant `{name}`"))?
                    .field_tags
                    .clone();
                for (i, fp) in fields.iter().enumerate() {
                    let mut sub = path.to_vec();
                    sub.push(i);
                    let ty = field_tags.get(i).cloned().unwrap_or(CTy::Other);
                    self.register_bindings(fp, m_slot, &sub, ty)?;
                }
            }
            // M-RT S4 type pattern: bind the matched value (the whole sub-value at `path`) as the
            // narrowed class, so `binding.field` resolves (the Wave-4 class-aware operand path).
            Pattern::Type {
                type_name,
                binding: Some(name),
                ..
            } => self.match_bindings.push(MatchBinding {
                name: name.clone(),
                match_slot: m_slot,
                path: path.to_vec(),
                ty: CTy::Class(type_name.clone()),
            }),
            _ => {} // wildcard / literals / `Type _` bind nothing
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;
    use crate::vm::Vm;

    /// Compile + run a program on the VM, returning captured output. Auto-prepends the reserved
    /// `package main;` (M5 S1, line-preserving) so existing test programs need no per-case edit.
    fn run(src: &str) -> Result<String, String> {
        let src = with_pkg(src);
        let tokens = lex(&src).expect("lex ok");
        let prog = Parser::new(tokens).parse_program().expect("parse ok");
        let program = compile(&prog).map_err(|d| d.to_string())?;
        Vm::new(&program).run().map_err(|d| d.to_string())
    }

    fn with_pkg(src: &str) -> String {
        if src.trim_start().starts_with("package ") {
            src.to_string()
        } else {
            format!("package main; {src}")
        }
    }

    fn out(src: &str) -> String {
        run(src).expect("run ok")
    }

    #[test]
    fn prints_a_literal_string() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("hi"); }"#),
            "hi\n"
        );
    }

    #[test]
    fn integer_arithmetic_in_interpolation() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{1 + 2 * 3}"); }"#),
            "7\n"
        );
    }

    #[test]
    fn float_arithmetic_formats_like_interpreter() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{3.0 * 4.0}"); }"#),
            "12\n"
        );
    }

    #[test]
    fn comparison_and_short_circuit() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{1 < 2 && 3 >= 3}"); }"#),
            "true\n"
        );
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{1 > 2 || false}"); }"#),
            "false\n"
        );
    }

    #[test]
    fn unary_negation_and_not() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { Console.println("{-5}"); Console.println("{!true}"); }"#),
            "-5\nfalse\n"
        );
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let e = run(r#"import Core.Console;
function main() { Console.println("{1 / 0}"); }"#)
        .unwrap_err();
        assert!(e.contains("division by zero"), "{e}");
    }

    #[test]
    fn missing_main_is_compile_error() {
        let e = run(r#"function other() {}"#).unwrap_err();
        assert!(e.contains("main"), "{e}");
    }

    #[test]
    fn user_function_call_runs() {
        let src = r#"import Core.Console;
function inc(int n) -> int { return n + 1; } function main() { Console.println("{inc(4)}"); }"#;
        assert_eq!(out(src), "5\n");
    }

    #[test]
    fn recursion_runs() {
        let src = r#"import Core.Console;
function fib(int n) -> int {
            if (n < 2) { return n; }
            return fib(n - 1) + fib(n - 2);
        } function main() { Console.println("{fib(10)}"); }"#;
        assert_eq!(out(src), "55\n");
    }

    #[test]
    fn undefined_call_target_rejected() {
        // A name that is neither a function, `println`, a variant, nor a declared class is rejected
        // with the interpreter's wording (checker-unreachable; defensive compiler path).
        let src = r#"import Core.Console;
function main() { Console.println("{Circle(2.0)}"); }"#;
        let e = run(src).unwrap_err();
        assert!(e.contains("not a function, variant, or class"), "{e}");
    }

    #[test]
    fn class_construction_and_field_read() {
        let src = r#"import Core.Console;
class Point { constructor(public int x, public int y) {} }
            function main() { Point p = Point(3, 4); Console.println("{p.x},{p.y}"); }"#;
        assert_eq!(out(src), "3,4\n");
    }

    #[test]
    fn constructor_body_runs_for_side_effects() {
        // The promoted instance is the result; the body's `println` is a side effect.
        let src = r#"import Core.Console;
class Greeter { constructor(public string name) { Console.println("made {name}"); } }
            function main() { Greeter g = Greeter("Ada"); Console.println("hi {g.name}"); }"#;
        assert_eq!(out(src), "made Ada\nhi Ada\n");
    }

    #[test]
    fn constructor_early_return_still_yields_instance() {
        // A bare `return;` exits the body early but the promoted instance is still returned.
        let src = r#"import Core.Console;
class C { constructor(public int x) { if (x > 0) { return; } Console.println("np"); } }
            function main() { C a = C(5); Console.println("{a.x}"); C b = C(0); Console.println("{b.x}"); }"#;
        assert_eq!(out(src), "5\nnp\n0\n");
    }

    #[test]
    fn method_reads_bare_field_and_dispatches() {
        // `total` in the method body resolves to `this.total`; `c.add(23)` dispatches on the class.
        let src = r#"import Core.Console;
class Counter { constructor(private int total) {} function add(int n) -> int { return total + n; } }
            function main() { Counter c = Counter(100); Console.println("{c.add(23)}"); }"#;
        assert_eq!(out(src), "123\n");
    }

    #[test]
    fn method_calls_method_via_this() {
        let src = r#"import Core.Console;
class C { constructor(public int x) {}
                function dbl() -> int { return this.x + this.x; }
                function quad() -> int { int d = this.dbl(); return d + d; } }
            function main() { C c = C(5); Console.println("{c.quad()}"); }"#;
        assert_eq!(out(src), "20\n");
    }

    #[test]
    fn method_recursion_through_this() {
        let src = r#"import Core.Console;
class F { constructor(public int base) {}
                function fact(int n) -> int { if (n <= 1) { return 1; } return n * this.fact(n - 1); } }
            function main() { F f = F(0); Console.println("{f.fact(5)}"); }"#;
        assert_eq!(out(src), "120\n");
    }

    #[test]
    fn var_decl_and_use() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { int x = 10; Console.println("{x + 5}"); }"#),
            "15\n"
        );
    }

    #[test]
    fn multiple_locals_resolve_to_distinct_slots() {
        let src = r#"import Core.Console;
function main() { int a = 1; int b = 2; Console.println("{a + b}"); }"#;
        assert_eq!(out(src), "3\n");
    }

    #[test]
    fn float_local_uses_float_arithmetic() {
        let src = r#"import Core.Console;
function main() { float r = 2.0; Console.println("{r * r}"); }"#;
        assert_eq!(out(src), "4\n");
    }

    #[test]
    fn if_else_picks_branch() {
        let src = r#"import Core.Console;
function main() { if (1 < 2) { Console.println("yes"); } else { Console.println("no"); } }"#;
        assert_eq!(out(src), "yes\n");
    }

    #[test]
    fn if_without_else() {
        let src = r#"import Core.Console;
function main() { if (1 > 2) { Console.println("never"); } Console.println("after"); }"#;
        assert_eq!(out(src), "after\n");
    }

    #[test]
    fn for_loop_over_list() {
        let src = r#"import Core.Console;
function main() { List<int> xs = [1, 2, 3]; for (int x in xs) { Console.println("{x}"); } }"#;
        assert_eq!(out(src), "1\n2\n3\n");
    }

    #[test]
    fn indexing_reads_element() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { List<int> xs = [7, 8, 9]; Console.println("{xs[1]}"); }"#),
            "8\n"
        );
    }

    #[test]
    fn indexing_out_of_range_faults() {
        let e = run(r#"import Core.Console;
function main() { List<int> xs = [1]; Console.println("{xs[3]}"); }"#)
        .unwrap_err();
        assert!(e.contains("list index out of range"), "{e}");
    }

    #[test]
    fn for_loop_body_locals_do_not_leak() {
        // A body-local must be cleaned each iteration (stack stays balanced).
        let src = r#"import Core.Console;
function main() {
            List<int> xs = [1, 2];
            for (int x in xs) { int y = x + 10; Console.println("{y}"); }
            Console.println("done");
        }"#;
        assert_eq!(out(src), "11\n12\ndone\n");
    }

    #[test]
    fn ranges_iterate_on_vm() {
        assert_eq!(
            out(r#"import Core.Console;
function main() { for (int i in 0..3) { Console.println("{i}"); } }"#),
            "0\n1\n2\n"
        );
        assert_eq!(
            out(r#"import Core.Console;
function main() { for (int i in 2..=4) { Console.println("{i}"); } }"#),
            "2\n3\n4\n"
        );
    }

    #[test]
    fn expression_if_on_vm() {
        // value-position if, then arithmetic on the result (height-merge + ctype specialization)
        assert_eq!(
            out(r#"import Core.Console;
function main() { var x = if (true) { 10 } else { 20 }; Console.println("{x + x}"); }"#),
            "20\n"
        );
        assert_eq!(
            out(r#"import Core.Console;
function main() { var x = if (false) { 10 } else { 20 }; Console.println("{x + 1}"); }"#),
            "21\n"
        );
    }

    #[test]
    fn enum_construct_and_match_binds_payload() {
        let src = r#"import Core.Console;
enum Grade { Pass(int s), Fail(int s), }
            function d(Grade g) -> string { return match g { Pass(s) => "P{s}", Fail(s) => "F{s}", }; }
            function main() { Console.println(d(Pass(9))); Console.println(d(Fail(3))); }"#;
        assert_eq!(out(src), "P9\nF3\n");
    }

    #[test]
    fn match_literal_arms_and_catch_all_binding() {
        let src = r#"import Core.Console;
function f(int n) -> string { return match n { 0 => "z", 1 => "o", x => "m{x}", }; }
            function main() { Console.println(f(0)); Console.println(f(1)); Console.println(f(9)); }"#;
        assert_eq!(out(src), "z\no\nm9\n");
    }

    #[test]
    fn match_as_binary_operand_tracks_scrutinee_slot() {
        // The lhs `1` is live on the operand stack when the `match` rhs compiles, so the scrutinee
        // must spill to a transient-aware slot (not `locals.len()`).
        let src = r#"import Core.Console;
function g(int n) -> int { return 1 + match n { 0 => 10, _ => 20 }; }
            function main() { Console.println("{g(0)}"); Console.println("{g(5)}"); }"#;
        assert_eq!(out(src), "11\n21\n");
    }

    #[test]
    fn nested_match_reextracts_outer_binding() {
        // Inner `match` compiles while the outer scrutinee occupies slot `locals.len()`; its own
        // scrutinee must land one slot higher (height tracking), and the inner arm re-extracts the
        // outer binding `b` from the outer scrutinee.
        let src = r#"import Core.Console;
enum Pair { P(int a, int b), }
            function f(Pair p) -> string {
                return match p { P(a, b) => match a { 0 => "z b={b}", _ => "a={a} b={b}", }, };
            }
            function main() { Console.println(f(P(0, 9))); Console.println(f(P(5, 2))); }"#;
        assert_eq!(out(src), "z b=9\na=5 b=2\n");
    }
}
