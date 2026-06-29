//! Runtime values for both backends. The M1 heap is **immutable + acyclic**: no reassignment, no
//! post-construction field mutation, and a constructor's args are fully evaluated before the
//! instance exists (EV-1). So compound objects are *shared* via `Rc`, not deep-cloned (M2 P5a):
//! cloning a `Value` (the `Op::GetLocal` hot path + every interpreter var-read) is a refcount bump,
//! and `Drop` reclaims correctly — no cycle can leak, so no tracing collector is needed (that is
//! deferred to M3, when mutation could create cycles). See `docs/specs/2026-06-16-m2-p5-object-model-design.md`.

use crate::green::sched::{ChanId, TaskId};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasherDefault, Hasher};
use std::rc::Rc;

/// A hand-rolled **FNV-1a** hasher for instance field maps (M-perf). Field keys are short identifiers
/// (`"v"`, `"x"`, `"price"`), where std's DoS-resistant SipHash is overkill: FNV-1a is a handful of
/// XOR/multiply per byte with no keying overhead, measurably faster for short keys on the object hot
/// path. Field-map keys come only from a program's own source (never attacker-controlled network
/// input), so SipHash's collision-DoS resistance buys nothing here. Std-only, safe, deterministic.
pub struct FnvHasher(u64);

/// Seeded with the 64-bit FNV offset basis, so a fresh hasher (one per key via `BuildHasherDefault`)
/// starts correct and `write` is a pure XOR/multiply loop (no in-band re-seed).
impl Default for FnvHasher {
    fn default() -> Self {
        FnvHasher(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        // FNV-1a per byte: XOR, then multiply by the FNV prime (wrapping). Same constants as
        // `bundle::cross::fnv1a_64`.
        let mut h = self.0;
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self.0 = h;
    }
}

/// A class's instance-field **slot layout** (M-perf slot-indexed fields, S1b): the bidirectional
/// name↔slot map shared by every instance of one class. Built once per class from
/// [`crate::ast::class_field_layout`] (so the two backends agree) and held behind `Rc` on each
/// [`Instance`], so construction *and* access both resolve `name → slot` against the instance's own
/// **runtime** layout — making slot order irrelevant to correctness (the multiple-inheritance
/// base-offset hazard never arises; slots are always runtime-resolved, never statically baked).
#[derive(Debug, Default)]
pub struct ClassLayout {
    /// field name → slot index, keyed by [`FnvHasher`]. The S2 inline cache fast-paths past this
    /// lookup on a monomorphic site; this map is the slow path / first miss.
    slots: HashMap<String, usize, BuildHasherDefault<FnvHasher>>,
    /// slot index → field name, in the deterministic sorted order from `class_field_layout`. Drives
    /// eq/reflect iteration so two instances of one class compare slot-aligned.
    names: Vec<String>,
}

impl ClassLayout {
    /// Build a layout from an **ordered, deduplicated** field-name list (as `class_field_layout`
    /// produces — sorted). The slot of a name is its index in `names`.
    pub fn new(names: Vec<String>) -> Rc<Self> {
        let slots = names
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, n)| (n, i))
            .collect();
        Rc::new(ClassLayout { slots, names })
    }

    /// Build a layout for a **native carrier** (`Regex`, …) from the field names it populates. The
    /// names are sorted and deduped for self-consistency: two carriers of the same kind get an
    /// identical layout, matching what `class_field_layout` produces for the same field set — so
    /// eq/reflect parity holds.
    pub fn from_sorted_names(names: &[&str]) -> Rc<Self> {
        let mut v: Vec<String> = names.iter().map(|s| (*s).to_string()).collect();
        v.sort();
        v.dedup();
        Self::new(v)
    }

    /// The slot index of `name`, or `None` when the name is not a declared storage field of the class.
    #[inline]
    pub fn slot(&self, name: &str) -> Option<usize> {
        self.slots.get(name).copied()
    }

    /// The field names in slot order (slot `i` is `names()[i]`).
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The number of slots (declared storage fields).
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// Whether the class has no storage fields.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    /// An exact fixed-point **`decimal`** (M-NUM S1): value = `unscaled × 10^(-scale)`. `19.99d` is
    /// `{ unscaled: 1999, scale: 2 }`. A distinct primitive from `float` (no implicit coercion — the
    /// whole point is keeping float out of money math). Arithmetic (`+ - *`) is single-sourced in the
    /// `decimal_*` kernels below; any i128 overflow is a clean [`FAULT_DECIMAL_OVERFLOW`] fault,
    /// byte-identical across both backends and the emitted BCMath PHP (the helper bounds-checks the
    /// result against i128 range and faults the same way). Rendering is [`fmt_decimal`].
    Decimal {
        unscaled: i128,
        scale: u8,
    },
    Bool(bool),
    Str(String),
    /// Raw octet sequence (`bytes`). Shared (like `List`) — cloning is a refcount bump. Distinct from
    /// `Str` (which is UTF-8); converts only via the `core.bytes` natives (M6 W0).
    Bytes(Rc<Vec<u8>>),
    Unit,
    /// `null` — the sole inhabitant of an absent optional (`T?`). A non-optional `T` never holds it
    /// (the checker's non-null discipline); PHP-native, erases to PHP `null` (M3 S2).
    Null,
    /// Shared (M2 P5a): cloning a list value is a refcount bump, not a deep element copy.
    List(Rc<Vec<Value>>),
    /// An **insertion-ordered** key→value map (M-RT S3). The order is part of the value: PHP arrays
    /// preserve insertion order, so a `Vec` of pairs (not a `HashMap`) is what keeps a future
    /// `keys()`/iteration byte-identical with the PHP target (risk R1). Shared via `Rc` like `List`
    /// (cloning is a refcount bump). Built and indexed only through the `build_map`/`map_index`
    /// kernels below, so both backends agree on dedup and lookup semantics.
    Map(Rc<Vec<(HKey, Value)>>),
    /// An **insertion-ordered** set of hashable keys (M-RT S7b). Like `Map`, the order is part of the
    /// value (not a `HashSet`): PHP arrays preserve insertion order, so a `Vec` of keys keeps a future
    /// `Set` iteration / `array_values` byte-identical with the PHP target (risk R1). Shared via `Rc`
    /// like `List`/`Map` (cloning is a refcount bump). Built only through the `build_set` kernel below,
    /// so both backends dedup identically.
    Set(Rc<Vec<HKey>>),
    Instance(Rc<Instance>),
    Enum(Rc<EnumVal>),
    /// A first-class function value: either a tree-walking closure (interpreter),
    /// a bare named-function reference, or a VM bytecode closure (Task 4).
    Closure(Rc<ClosureData>),
    /// A typed FIFO channel (M6 W4 green threads) — a **shared-mutable handle** like [`Instance`]:
    /// cloning a `Value::Channel` shares the *same* buffer, so a `send` through one binding is visible
    /// to a `recv` through another (the whole point of a channel). Carries its scheduler [`ChanId`]
    /// (allocated at `Channel.create()`) so a blocking `recv` can register on the right channel's
    /// wait-list; the `Rc<RefCell<VecDeque>>` is the shared buffer of erased `Value`s. The static
    /// element type is the compile-time-only `Channel<T>` annotation. **Opaque** to the
    /// arithmetic/compare/display kernels (the checker forbids using a channel as an operand), so the
    /// single-sourced `value.rs` kernels are untouched. Never transpiled — green threads have no PHP
    /// target (`E-CONCURRENCY-NO-PHP`); a `spawn`/channel program is quarantined from the PHP oracle.
    Channel(ChanId, Rc<RefCell<VecDeque<Value>>>),
    /// A green-task handle (M6 W4): just its scheduler [`TaskId`]. The task's result lives in the
    /// shared `Coop.results` map (keyed by this id), populated when the task completes — eagerly at
    /// `spawn` in the synchronous-degenerate path, or when the task's coroutine finishes in the
    /// cooperative path. `join` reads it by id. Opaque to the kernels; never transpiled.
    Task(TaskId),
}

/// The data of a first-class function value (M3 S3, Task 3).
///
/// - `Tree`: an expression-body lambda captured from the tree-walking interpreter.
/// - `Named`: a bare named-function reference (the name is resolved at call time).
/// - `Byte`: a bytecode closure constructed by the VM in Task 4; constructing it in the
///   interpreter is a bug — any such path panics with `unreachable!`.
#[derive(Debug, Clone)]
pub enum ClosureData {
    Tree {
        params: Vec<crate::ast::Param>,
        ret: Option<crate::ast::Type>,
        body: crate::ast::LambdaBody,
        env: Vec<(String, Value)>,
        /// The captured receiver when the lambda references `this` (Phase 1 closures slice), else
        /// `None`. It is the same `Rc` instance handle the enclosing method holds, so a field
        /// mutation through it is visible to the closure ("live" capture). Set at closure creation;
        /// restored as `self.this` while the body runs.
        this_capture: Option<Value>,
    },
    Named(String),
    /// Bytecode closure — constructed by the VM (Task 4). The interpreter never constructs
    /// this variant; encountering it at runtime is a bug (`unreachable!`).
    Byte {
        func: usize,
        captures: Vec<Value>,
    },
}

/// A class instance — a **shared-mutable handle** (M-mut.6). The `class` is immutable (set once at
/// construction); only `fields` mutates, so it alone is interior-mutable (`RefCell`). Held in
/// `Rc<Instance>`, so cloning a `Value::Instance` shares the *same* cell: a field write through one
/// binding (`o.f = e`) is visible through every other binding — PHP/Java object semantics (F2).
/// Field reads clone the value out and drop the borrow immediately; writes take a `borrow_mut` only
/// after the value is fully evaluated, so a borrow is never held across a re-entrant `eval`/`run`.
#[derive(Debug, Clone)]
pub struct Instance {
    pub class: String,
    /// The shared `name → slot` layout for `class` (M-perf S1b). Every instance of one class shares the
    /// same `Rc`, so field access resolves a slot against the receiver's own runtime layout.
    pub layout: Rc<ClassLayout>,
    /// Slot-indexed field storage, `len() == layout.len()`. The `None` sentinel = an unset field
    /// (preserves the old "read of an unpopulated field faults" semantics — an absent HashMap key
    /// before S1b). Interior-mutable for shared-handle field writes (`o.f = e`), same as before.
    pub fields: RefCell<Vec<Option<Value>>>,
}

impl Instance {
    /// Allocate an instance of `class` with every slot unset (`None`).
    pub fn new(class: String, layout: Rc<ClassLayout>) -> Self {
        let n = layout.len();
        Instance {
            class,
            layout,
            fields: RefCell::new(vec![None; n]),
        }
    }

    /// Read field `name`, cloning the value out (and dropping the borrow before returning, preserving
    /// handle semantics). `None` if the name is not in the layout *or* the slot is unset — the caller
    /// turns that into the same runtime fault as a pre-S1b absent key.
    pub fn get_field(&self, name: &str) -> Option<Value> {
        self.layout
            .slot(name)
            .and_then(|i| self.fields.borrow()[i].clone())
    }

    /// Write field `name`. Returns `false` when `name` is not a declared storage slot — checker-
    /// unreachable for a valid program (the layout is a superset of declared fields), so callers may
    /// ignore the result; surfacing it keeps the write total/panic-free (EV-7).
    pub fn set_field(&self, name: &str, v: Value) -> bool {
        match self.layout.slot(name) {
            Some(i) => {
                self.fields.borrow_mut()[i] = Some(v);
                true
            }
            None => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnumVal {
    pub ty: String,
    pub variant: String,
    pub payload: Vec<Value>,
}

/// Hashable key subset for `Map`/`Set` (`Value` can't derive `Hash`/`Eq`: it
/// holds `f64`). Unused by the M1 sample but required by the value-type signatures.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HKey {
    Int(i64),
    Bool(bool),
    Str(String),
}

impl HKey {
    /// Project a runtime `Value` onto the hashable key subset, or `None` if it isn't a valid map key
    /// (`float`, list, instance, …). The checker forbids non-`{int,bool,string}` key *types*
    /// (`E-MAP-KEY`) and types the index of `m[k]` against the map's key type, so a `None` here is
    /// checker-unreachable — the callers turn it into a clean fault rather than a panic (EV-7).
    pub fn from_value(v: &Value) -> Option<HKey> {
        match v {
            Value::Int(n) => Some(HKey::Int(*n)),
            Value::Bool(b) => Some(HKey::Bool(*b)),
            Value::Str(s) => Some(HKey::Str(s.clone())),
            _ => None,
        }
    }

    /// Inverse of [`HKey::from_value`] — used when a key flows back out as a `Value` (a future
    /// `keys()` native). Total: every `HKey` variant maps to exactly one `Value`.
    pub fn to_value(&self) -> Value {
        match self {
            HKey::Int(n) => Value::Int(*n),
            HKey::Bool(b) => Value::Bool(*b),
            HKey::Str(s) => Value::Str(s.clone()),
        }
    }
}

/// Build an **insertion-ordered** map from evaluated `(key, value)` pairs, matching PHP literal
/// semantics: a duplicate key keeps its **first position** but takes the **last value**
/// (`["a" => 1, "a" => 2]` ⇒ `["a" => 2]`, position of the first `"a"`). Single-sourced so the
/// interpreter (`Expr::Map`) and the VM (`Op::MakeMap`) dedup identically — `run ≡ runvm` (and a
/// non-`HKey` key, checker-unreachable, faults cleanly rather than panicking, EV-7).
pub fn build_map(pairs: Vec<(Value, Value)>) -> Result<Vec<(HKey, Value)>, String> {
    let mut out: Vec<(HKey, Value)> = Vec::with_capacity(pairs.len());
    for (k, v) in pairs {
        let key =
            HKey::from_value(&k).ok_or_else(|| format!("invalid map key: {}", k.type_name()))?;
        if let Some(slot) = out.iter_mut().find(|(ek, _)| *ek == key) {
            slot.1 = v; // existing key: keep first position, take last value (PHP semantics)
        } else {
            out.push((key, v));
        }
    }
    Ok(out)
}

/// Build an **insertion-ordered, deduplicated** set from evaluated element values, keeping each
/// element's **first occurrence** and discarding later duplicates (`Set.of([1, 2, 1]) ⇒ {1, 2}`,
/// in that order) — the same first-seen-order discipline as [`build_map`]'s keys. Single-sourced so
/// the interpreter and the VM dedup identically (`run ≡ runvm`); a non-`HKey` element
/// (checker-unreachable, the checker constrains a `Set<T>` element to the hashable subset) faults
/// cleanly rather than panicking (EV-7).
pub fn build_set(elems: Vec<Value>) -> Result<Vec<HKey>, String> {
    let mut out: Vec<HKey> = Vec::with_capacity(elems.len());
    for e in elems {
        let key = HKey::from_value(&e)
            .ok_or_else(|| format!("invalid set element: {}", e.type_name()))?;
        if !out.contains(&key) {
            out.push(key);
        }
    }
    Ok(out)
}

/// Look a key up in an insertion-ordered map. A missing key is a clean fault (`"map key not found"`),
/// byte-identical across both backends — the differential harness excludes fault cases, and the
/// present-key path is byte-identical to PHP `$m[$k]`. A non-`HKey` index is checker-unreachable
/// (`m[k]` types `k` against the map's key type) but handled defensively (EV-7).
pub fn map_index(map: &[(HKey, Value)], index: &Value) -> Result<Value, String> {
    let key =
        HKey::from_value(index).ok_or_else(|| format!("invalid map key: {}", index.type_name()))?;
    map.iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.clone())
        .ok_or_else(|| "map key not found".to_string())
}

/// Set `list[idx] = v` in place with bounds-checking (M-mut.5). The caller owns the copy-on-write
/// (`Rc::make_mut` before calling), so this mutates a uniquely-owned `Vec`. An out-of-range index
/// faults identically to a read (`"list index out of range"`, `FaultKind::IndexOob`) — note this
/// diverges from PHP, which would *extend* the array; examples only set in-bounds (KNOWN_ISSUES).
pub fn list_set(list: &mut [Value], idx: i64, v: Value) -> Result<(), String> {
    let i = usize::try_from(idx)
        .ok()
        .filter(|i| *i < list.len())
        .ok_or_else(|| "list index out of range".to_string())?;
    list[i] = v;
    Ok(())
}

/// Set `map[key] = v` (M-mut.5): update in place if `key` is present, else append — insertion-ordered
/// like PHP `$m[$k] = $v`, preserving the `Rc<Vec<(HKey, Value)>>` order invariant (R1). The caller
/// owns the COW. A non-`HKey` key is checker-unreachable (EV-7).
pub fn map_set(map: &mut Vec<(HKey, Value)>, key: &Value, v: Value) -> Result<(), String> {
    let k = HKey::from_value(key).ok_or_else(|| format!("invalid map key: {}", key.type_name()))?;
    if let Some(slot) = map.iter_mut().find(|(ek, _)| *ek == k) {
        slot.1 = v;
    } else {
        map.push((k, v));
    }
    Ok(())
}

impl Value {
    /// Short name for diagnostics. Composite types fold to a constant so the
    /// return can stay `&'static str`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Decimal { .. } => "decimal",
            Value::Bool(_) => "bool",
            Value::Str(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::Unit => "unit",
            Value::Null => "null",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
            Value::Instance(_) => "instance",
            Value::Enum(_) => "enum",
            Value::Closure(_) => "function",
            Value::Channel(..) => "channel",
            Value::Task(_) => "task",
        }
    }

    /// Render a *primitive* value for interpolation / `println`. `None` for a
    /// composite value (the caller turns that into a runtime `Diagnostic`). Floats use
    /// Rust `{}` formatting (EV-6): `12.0` -> `"12"`.
    pub fn as_display(&self) -> Option<String> {
        match self {
            Value::Int(n) => Some(n.to_string()),
            Value::Float(x) => Some(format!("{x}")),
            // A decimal renders with exactly `scale` fractional digits (BCMath pads), never `-0`.
            // The same `fmt_decimal` both backends use, and the emitted PHP string already carries
            // this form (a BCMath result string `(string)`s identically) — so interpolating a decimal
            // is byte-identical across run/runvm/PHP (M-NUM S1).
            Value::Decimal { unscaled, scale } => Some(fmt_decimal(*unscaled, *scale)),
            Value::Bool(b) => Some(b.to_string()),
            Value::Str(s) => Some(s.clone()),
            Value::Unit => Some("unit".to_string()),
            // Functions cannot be displayed (the checker forbids interpolating a function
            // value; this arm is only reached through the fallback `_ => None` path — EV-7).
            Value::Closure(_) => None,
            _ => None,
        }
    }

    /// Structural value equality for `==` / `!=`. Cycle-safe (F4): instances became shared-mutable
    /// handles in M-mut.6, so `a.next = b; b.next = a` can form a reference cycle. An unguarded
    /// recursion on such a cycle would overflow the native stack — and at *different* depths per
    /// backend, breaking `agree_err`. The `visited` pair set short-circuits a re-encountered
    /// `(a, b)` pair to `true` (co-inductive bisimulation, the standard correct cyclic equality), so
    /// `==` always terminates deterministically. PHP `==` is likewise cycle-protected.
    pub fn eq_val(&self, other: &Value) -> bool {
        self.eq_val_rec(other, &mut Vec::new())
    }

    /// Recursive worker for [`eq_val`]. `visited` records instance-pointer pairs currently being
    /// compared; only the `Instance` arm consults/extends it (lists/maps/sets/enums are acyclic value
    /// types — a cycle can only thread through an instance handle). Not popping memoizes equal pairs
    /// too, which is sound: a *false* pair short-circuits the whole comparison (every `&&`/`.all()`
    /// propagates the `false` up), so a stale-`true` memo for a false pair is never observed.
    #[allow(clippy::float_cmp)] // intentional: language-level float equality
    fn eq_val_rec(
        &self,
        other: &Value,
        visited: &mut Vec<(*const Instance, *const Instance)>,
    ) -> bool {
        use Value::*;
        match (self, other) {
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            // Decimal equality is **numeric, scale-insensitive** (`1.50d == 1.5d`): align both to the
            // max scale and compare unscaled. Mixed `decimal`/`int` widens the int to scale 0 (so
            // `2d == 2` matches `decimal == int` operator typing). An alignment overflow can't occur
            // here — it would only change two unequal values, never make equal values compare wrong:
            // when alignment overflows, the magnitudes differ by ≥10^Δ at i128 scale, so they are not
            // equal, and `decimal_cmp` returns `None` ⇒ `false`, which is correct.
            (Decimal { .. } | Int(_), Decimal { .. } | Int(_))
                if matches!((self, other), (Decimal { .. }, _) | (_, Decimal { .. })) =>
            {
                decimal_cmp(self, other) == Some(Ordering::Equal)
            }
            (Bool(a), Bool(b)) => a == b,
            (Str(a), Str(b)) => a == b,
            (Bytes(a), Bytes(b)) => a == b,
            (Unit, Unit) => true,
            (List(a), List(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b.iter())
                        .all(|(x, y)| x.eq_val_rec(y, visited))
            }
            // Maps compare **order-independently** (insertion order is part of iteration, not of
            // identity): same key set with `eq_val` values. This matches PHP associative `==`.
            (Map(a), Map(b)) => {
                a.len() == b.len()
                    && a.iter().all(|(k, v)| {
                        b.iter()
                            .find(|(bk, _)| bk == k)
                            .is_some_and(|(_, bv)| v.eq_val_rec(bv, visited))
                    })
            }
            // Sets compare **order-independently** (insertion order is iteration, not identity):
            // same cardinality and same membership. Both are deduped by `build_set`, so a one-way
            // containment check at equal length suffices.
            (Set(a), Set(b)) => a.len() == b.len() && a.iter().all(|k| b.contains(k)),
            (Enum(a), Enum(b)) => {
                a.ty == b.ty
                    && a.variant == b.variant
                    && a.payload.len() == b.payload.len()
                    && a.payload
                        .iter()
                        .zip(&b.payload)
                        .all(|(x, y)| x.eq_val_rec(y, visited))
            }
            (Instance(a), Instance(b)) => {
                let pair = (Rc::as_ptr(a), Rc::as_ptr(b));
                if visited.contains(&pair) {
                    return true; // already comparing this pair (a cycle) → assume equal
                }
                visited.push(pair);
                if a.class != b.class {
                    return false;
                }
                // Same class ⇒ same shared `layout` ⇒ identical slot order, so compare slot-aligned.
                // An unset slot (`None`) is equal only to another unset slot — byte-identical to the
                // pre-S1b key-set comparison (a `None` slot was an absent HashMap key there).
                let fa = a.fields.borrow();
                let fb = b.fields.borrow();
                fa.len() == fb.len()
                    && fa.iter().zip(fb.iter()).all(|(x, y)| match (x, y) {
                        (Some(xv), Some(yv)) => xv.eq_val_rec(yv, visited),
                        (None, None) => true,
                        _ => false,
                    })
            }
            (Null, Null) => true,
            // Functions are not comparable — the checker forbids `==`/`!=` on function
            // types; this arm is a defensive fallback (EV-7, well-typed programs never reach it).
            (Closure(_), _) | (_, Closure(_)) => false,
            _ => false,
        }
    }
}

// --- Arithmetic & comparison kernels (single-sourced; both backends call these) ---
//
// The `Op::Neg` parity bug (M2 P3.5 Wave 0) was possible because integer arithmetic lived in two
// hand-kept-identical copies, one per backend. These kernels are the *one* implementation both the
// tree-walker (`interpreter::arith`/`eval_unary`/`compare`) and the VM (`vm.rs` arith arms +
// `compare`) dispatch into, so the two can no longer drift. They return the bare fault *body*
// (`String`); each backend wraps it in its own error type. Floats can't fault (NaN/inf are valid
// `f64`); only integer overflow and integer division/modulo by zero are faults. The op→bool / op→fn
// projection stays in each backend — their op enums (`BinaryOp` vs `Op`) differ, so only the
// arithmetic and the fault strings are shared, not the dispatch.

/// Evaluate a compile-time **literal-constant** expression to a `Value` — used to seed `static`
/// field storage once at program load (M-mut.7). Both backends call this (F3), so the interpreter's
/// `statics` map and the VM's `static_inits` table hold identical values. Returns `None` for anything
/// that is not a literal; the checker rejects a non-literal static initializer (`E-STATIC-INIT-CONST`),
/// so a `None` is checker-unreachable at load. Scalars + `null` + `bytes` only this slice — richer
/// constant expressions (arithmetic, collection literals) are deferred.
pub fn const_literal(e: &crate::ast::Expr) -> Option<Value> {
    use crate::ast::{Expr, StrPart};
    match e {
        Expr::Int(n, _) => Some(Value::Int(*n)),
        Expr::Float(f, _) => Some(Value::Float(*f)),
        Expr::Decimal {
            unscaled, scale, ..
        } => Some(Value::Decimal {
            unscaled: *unscaled,
            scale: *scale,
        }),
        Expr::Bool(b, _) => Some(Value::Bool(*b)),
        Expr::Null(_) => Some(Value::Null),
        // A plain string literal is a single `Literal` part; any interpolation makes it non-const.
        Expr::Str(parts, _) => match parts.as_slice() {
            [] => Some(Value::Str(String::new())),
            [StrPart::Literal(s)] => Some(Value::Str(s.clone())),
            _ => None,
        },
        Expr::Bytes(b, _) => Some(Value::Bytes(Rc::new(b.clone()))),
        _ => None,
    }
}

/// Canonical fault body for integer `x / 0`. Single-sourced so `run` ≡ `runvm` in the fault path.
pub const FAULT_DIV_ZERO: &str = "division by zero";
/// Canonical fault body for integer `x % 0`.
pub const FAULT_MOD_ZERO: &str = "modulo by zero";
/// Canonical fault body for any integer op whose result leaves `i64` range
/// (`MAX + 1`, `MIN - 1`, `MIN * -1`, `MIN / -1`, `MIN % -1`, `-MIN`).
pub const FAULT_INT_OVERFLOW: &str = "integer overflow";
/// Canonical fault body for a bitwise shift by a negative count (PHP throws `ArithmeticError`).
pub const FAULT_NEGATIVE_SHIFT: &str = "bit shift by negative number";
/// Canonical fault body for a `decimal` `+ - *` (or scale-alignment) whose exact result leaves
/// `i128` range (M-NUM S1). Byte-identical across both Rust backends AND the emitted BCMath PHP (the
/// `__phorj_dec_*` helper bounds-checks its result against i128 range and throws the same body).
pub const FAULT_DECIMAL_OVERFLOW: &str = "decimal overflow";
/// Canonical fault body for `Decimal.div` with a zero divisor (M-NUM S2). Distinct from the integer
/// `FAULT_DIV_ZERO` body so the message is decimal-specific, but it still *contains* the substring
/// `"division by zero"`, so the differential harness classifies it as `FaultKind::DivZero` (run≡runvm
/// parity); the emitted PHP `__phorj_dec_div` helper throws the same body.
pub const FAULT_DECIMAL_DIV_ZERO: &str = "decimal division by zero";
/// Canonical fault body for a negative `scale` argument to `Decimal.div`/`Decimal.round` (M-NUM S2).
/// A scale is the count of fractional digits, so it must be `>= 0`; the PHP helpers throw the same.
pub const FAULT_DECIMAL_SCALE: &str = "decimal scale out of range";
/// Canonical fault body for a bare `decimal % 0` (the exact-remainder operator, 2026-06-27). Contains
/// `"modulo by zero"` so the differential harness classifies it as the same `FaultKind` as int `%0`.
pub const FAULT_DECIMAL_MOD_ZERO: &str = "decimal modulo by zero";
/// Canonical fault body for a bare `decimal / decimal` whose quotient does not terminate
/// (2026-06-27 exact-or-fault `/`): the fraction in lowest terms has a denominator with a prime
/// factor other than 2 or 5 (e.g. `1d / 3d`). Use `Decimal.div(a, b, scale, mode)` for a rounded
/// quotient instead. The emitted PHP `__phorj_dec_div_exact` throws the same body.
pub const FAULT_DECIMAL_NONTERMINATING: &str = "decimal division is not exact";
/// Canonical fault body for `int ** int` with a negative exponent. A negative exponent yields a
/// fractional result, which cannot be the typed `int` the `**` operator promises — so it faults
/// rather than silently widening to `float` (PHP's `2 ** -1 == 0.5`). Use `float**float` for that.
pub const FAULT_NEGATIVE_EXPONENT: &str = "negative exponent";

/// Checked integer addition; overflow is a clean fault, never a panic (EV-7).
pub fn int_add(a: i64, b: i64) -> Result<i64, String> {
    a.checked_add(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer subtraction.
pub fn int_sub(a: i64, b: i64) -> Result<i64, String> {
    a.checked_sub(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer multiplication.
pub fn int_mul(a: i64, b: i64) -> Result<i64, String> {
    a.checked_mul(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer division. `b == 0` is `FAULT_DIV_ZERO`; `i64::MIN / -1` overflows.
pub fn int_div(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        return Err(FAULT_DIV_ZERO.to_string());
    }
    a.checked_div(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer remainder. `b == 0` is `FAULT_MOD_ZERO`; `i64::MIN % -1` overflows.
pub fn int_rem(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        return Err(FAULT_MOD_ZERO.to_string());
    }
    a.checked_rem(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Checked integer negation. `-i64::MIN` overflows (the exact Wave 0 P0 case).
pub fn int_neg(n: i64) -> Result<i64, String> {
    n.checked_neg()
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// `Math.intdiv(a, b)` (M-NUM S3): integer division truncating toward zero (PHP `intdiv`). `b == 0`
/// is [`FAULT_DIV_ZERO`]; `i64::MIN / -1` overflows ([`FAULT_INT_OVERFLOW`]) — both clean faults, never
/// a panic (EV-7). Distinct from [`int_div`] only in name/intent (both truncate toward zero); kept
/// separate so the `intdiv` native and the `/`-on-int operator can diverge later without coupling.
pub fn int_intdiv(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        return Err(FAULT_DIV_ZERO.to_string());
    }
    a.checked_div(b)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// `Convert.toInt(float)` (M-NUM S3): truncate a float toward zero to an `int`, or `None` on NaN,
/// ±∞, or a value outside the i64 range. The range guard uses the float bounds that **both** Rust and
/// PHP agree on at the i64 edge: `2^63` (9223372036854775808.0) is exactly f64-representable, but
/// `i64::MAX` (2^63 − 1) is not — so the in-range window is `[-2^63, 2^63)` (upper **exclusive**;
/// `i64::MIN` is exactly `-2^63`). A value `v` with `LOWER <= v.trunc() < UPPER` casts losslessly via
/// `as i64`; anything else (incl. NaN/±∞, which fail the comparisons) returns `None`. This avoids
/// PHP's surprising `(int)NAN == 0`. Single-sourced so `run`/`runvm` agree; mirrored by the PHP
/// `__phorj_float_to_int` helper (which uses the same `9.2233720368547758E18` literal).
pub fn float_to_int(v: f64) -> Option<i64> {
    const UPPER: f64 = 9_223_372_036_854_775_808.0; // 2^63 — exclusive upper bound
    const LOWER: f64 = -UPPER; // i64::MIN as f64 (exact)
    let t = v.trunc();
    if v.is_finite() && (LOWER..UPPER).contains(&t) {
        Some(t as i64)
    } else {
        None
    }
}

/// `float as int` (M4 as-matrix) — **exact-or-null**: `Some(i)` only when `v` is integral and in
/// i64 range (`3.0 → 3`, `3.9 → None`, NaN/±∞ → None). Unlike [`float_to_int`] (truncate, used by
/// `Convert.toInt`), this never drops a fraction silently — the `as` operator's "no silent loss"
/// rule. `v.fract() == 0.0` is false for NaN/∞, so the finite+range guard in [`float_to_int`] runs
/// only for a genuinely integral value. Mirrored by the PHP `__phorj_float_to_int_exact` helper.
pub fn float_to_int_exact(v: f64) -> Option<i64> {
    if v.fract() == 0.0 {
        float_to_int(v)
    } else {
        None
    }
}

/// `Math.numberFormat(value, decimals)` (M-NUM S4): a non-locale `number_format` — `value` rounded
/// half-away-from-zero to `decimals` places, grouped with `,` every three integer digits and a `.`
/// decimal point. **Digit-string rounding** (2026-06-27): it rounds the *shortest-round-trip decimal
/// string* of `value` (`format!("{value}")`, which the PHP `__phorj_float` helper reproduces
/// byte-for-byte) digit-by-digit with carry — NOT `(value * 10^d).round()`. That removes the previous
/// `.5`-boundary divergence (Rust `f64::round` had no pre-rounding, PHP `round` did): both legs now
/// round the *intended* decimal identically (`numberFormat(0.285, 2) == "0.29"` on all three backends).
/// A negative `decimals` is clamped to `0`. A non-finite `value` is outside the format domain and
/// falls back to its plain display.
pub fn number_format(value: f64, decimals: usize) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    let s = format!("{value}");
    let neg = s.starts_with('-');
    let s = s.strip_prefix('-').unwrap_or(&s);
    let (int_str, frac_str) = match s.split_once('.') {
        Some((i, f)) => (i, f),
        None => (s, ""),
    };
    let mut int_digits: Vec<u8> = int_str.bytes().collect();
    let mut frac_digits: Vec<u8> = frac_str.bytes().collect();
    // Round half-away-from-zero: round up iff the first dropped fractional digit is >= '5'.
    let round_up = frac_digits.get(decimals).is_some_and(|&d| d >= b'5');
    frac_digits.truncate(decimals);
    while frac_digits.len() < decimals {
        frac_digits.push(b'0');
    }
    if round_up {
        let mut carry = 1u8;
        for d in frac_digits.iter_mut().rev() {
            if carry == 0 {
                break;
            }
            let v = *d - b'0' + carry;
            *d = b'0' + v % 10;
            carry = v / 10;
        }
        for d in int_digits.iter_mut().rev() {
            if carry == 0 {
                break;
            }
            let v = *d - b'0' + carry;
            *d = b'0' + v % 10;
            carry = v / 10;
        }
        if carry > 0 {
            int_digits.insert(0, b'0' + carry);
        }
    }
    // Strip leading zeros from the integer part (keep at least one digit).
    while int_digits.len() > 1 && int_digits[0] == b'0' {
        int_digits.remove(0);
    }
    // A result that is entirely zero never carries a sign (no `-0`).
    let all_zero = int_digits.iter().all(|&d| d == b'0') && frac_digits.iter().all(|&d| d == b'0');
    let mut out = String::new();
    if neg && !all_zero {
        out.push('-');
    }
    let n = int_digits.len();
    for (i, b) in int_digits.iter().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    if decimals > 0 {
        out.push('.');
        for b in &frac_digits {
            out.push(*b as char);
        }
    }
    out
}

/// Bitwise AND / OR / XOR on `int` (never fault — total over `i64`). PHP-identical.
pub fn int_bitand(a: i64, b: i64) -> i64 {
    a & b
}
pub fn int_bitor(a: i64, b: i64) -> i64 {
    a | b
}
pub fn int_bitxor(a: i64, b: i64) -> i64 {
    a ^ b
}
/// Bitwise NOT — `~n == -n - 1`, total over `i64`.
pub fn int_bitnot(n: i64) -> i64 {
    !n
}
/// Shift-left, PHP semantics: a negative count faults (`ArithmeticError`); a count ≥ 64 yields 0;
/// otherwise the low 64 bits of the shifted value (`wrapping_shl` would mask the count, so the ≥ 64
/// case is handled explicitly — `1 << 64` is 0, not 1).
pub fn int_shl(a: i64, n: i64) -> Result<i64, String> {
    if n < 0 {
        return Err(FAULT_NEGATIVE_SHIFT.to_string());
    }
    if n >= 64 {
        return Ok(0);
    }
    Ok(a.wrapping_shl(n as u32))
}
/// Shift-right (arithmetic, sign-preserving — PHP semantics): a negative count faults; a count ≥ 64
/// fills with the sign bit (`8 >> 64 == 0`, `-8 >> 64 == -1`); otherwise an arithmetic right shift.
pub fn int_shr(a: i64, n: i64) -> Result<i64, String> {
    if n < 0 {
        return Err(FAULT_NEGATIVE_SHIFT.to_string());
    }
    let n = if n >= 64 { 63 } else { n as u32 };
    Ok(a >> n)
}

/// Float addition. Floats never fault — NaN/inf are valid `f64`.
pub fn float_add(a: f64, b: f64) -> f64 {
    a + b
}
/// Float subtraction.
pub fn float_sub(a: f64, b: f64) -> f64 {
    a - b
}
/// Float multiplication.
pub fn float_mul(a: f64, b: f64) -> f64 {
    a * b
}
/// Float division. A **zero divisor faults** (`FAULT_DIV_ZERO`) — matching int `/0` and PHP 8's
/// `DivisionByZeroError` on `$a / 0.0` — rather than yielding IEEE `inf`/`NaN` (the "any division by
/// zero throws" rule). `-0.0` counts as zero (`-0.0 == 0.0`). A finite-overflow-to-`inf` (huge `a`,
/// tiny non-zero `b`) is *not* a zero division and stays `inf`.
pub fn float_div(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        return Err(FAULT_DIV_ZERO.to_string());
    }
    Ok(a / b)
}
/// Float remainder. A **zero divisor faults** (`FAULT_MOD_ZERO`), like int `%0` (PHP `fmod` would
/// return `NAN`; the emitted PHP routes through `__phorj_rem`, which throws to agree).
pub fn float_rem(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        return Err(FAULT_MOD_ZERO.to_string());
    }
    Ok(a % b)
}

// --- Decimal (fixed-point) kernels (M-NUM S1; single-sourced — both backends + the example oracle
// agree, and the BCMath PHP helper mirrors them). value = `unscaled × 10^(-scale)`. ---

/// Project a `Value` operand of a decimal op onto `(unscaled, scale)`: a `Decimal` verbatim, an `Int`
/// widened to scale 0 (the `decimal op int ⇒ decimal` rule). `None` for anything else — checker-
/// unreachable (the checker guarantees decimal operands are `decimal`/`int`), handled defensively.
fn dec_parts(v: &Value) -> Option<(i128, u8)> {
    match v {
        Value::Decimal { unscaled, scale } => Some((*unscaled, *scale)),
        Value::Int(n) => Some((i128::from(*n), 0)),
        _ => None,
    }
}

/// Multiply `unscaled` by `10^exp`, checked (an alignment that leaves `i128` range faults). Used to
/// align two decimals to a common scale before add/sub/compare.
fn dec_scale_up(unscaled: i128, exp: u8) -> Option<i128> {
    let factor = 10i128.checked_pow(u32::from(exp))?;
    unscaled.checked_mul(factor)
}

/// Align `(a, sa)` and `(b, sb)` to the common scale `max(sa, sb)`, returning the two scaled unscaled
/// values plus that scale. `None` on an alignment overflow (i128 range) — the caller turns it into a
/// clean [`FAULT_DECIMAL_OVERFLOW`] fault. Shared by add/sub and comparison so every path aligns
/// identically.
fn dec_align(a: i128, sa: u8, b: i128, sb: u8) -> Option<(i128, i128, u8)> {
    let scale = sa.max(sb);
    let au = dec_scale_up(a, scale - sa)?;
    let bu = dec_scale_up(b, scale - sb)?;
    Some((au, bu, scale))
}

/// Exact decimal addition (M-NUM S1): result scale = `max(scales)`; align then `checked_add`. Any
/// i128 overflow (incl. the alignment) ⇒ [`FAULT_DECIMAL_OVERFLOW`]. Accepts mixed `(Decimal, Int)`
/// (the int widens to scale 0). Mirrors `bcadd($a, $b, max)`.
pub fn decimal_add(a: &Value, b: &Value) -> Result<Value, String> {
    let (au, sa) = dec_parts(a).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let (bu, sb) = dec_parts(b).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let (x, y, scale) =
        dec_align(au, sa, bu, sb).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let unscaled = x
        .checked_add(y)
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    Ok(Value::Decimal { unscaled, scale })
}

/// Exact decimal subtraction (M-NUM S1): result scale = `max(scales)`; align then `checked_sub`.
/// Mirrors `bcsub($a, $b, max)`. Same overflow + mixed-operand rules as [`decimal_add`].
pub fn decimal_sub(a: &Value, b: &Value) -> Result<Value, String> {
    let (au, sa) = dec_parts(a).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let (bu, sb) = dec_parts(b).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let (x, y, scale) =
        dec_align(au, sa, bu, sb).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let unscaled = x
        .checked_sub(y)
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    Ok(Value::Decimal { unscaled, scale })
}

/// Exact decimal multiplication (M-NUM S1): result scale = `sa + sb` (no truncation), unscaled =
/// `a.unscaled checked_mul b.unscaled`. Mirrors `bcmul($a, $b, sa + sb)`. Same overflow + mixed-
/// operand rules as [`decimal_add`]. The scale sum can't itself overflow `u8` for realistic inputs
/// (two scale-127 decimals would need ~10^254 magnitude — far past i128 — and overflow the mul long
/// before the scale add); a `u8` scale-add overflow is treated as an overflow fault, defensively.
pub fn decimal_mul(a: &Value, b: &Value) -> Result<Value, String> {
    let (au, sa) = dec_parts(a).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let (bu, sb) = dec_parts(b).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let scale = sa
        .checked_add(sb)
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let unscaled = au
        .checked_mul(bu)
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    Ok(Value::Decimal { unscaled, scale })
}

/// Exact decimal remainder (bare `%`, 2026-06-27): align to `max(scales)`, then `x % y` — exact and
/// representable at that scale (no rounding, unlike `/`). A zero divisor faults
/// ([`FAULT_DECIMAL_MOD_ZERO`]); the sign follows the dividend (Rust `%` / PHP `bcmod`). Accepts a
/// mixed `(Decimal, Int)` (the int widens to scale 0). Mirrors `bcmod($a, $b, max)`.
pub fn decimal_rem(a: &Value, b: &Value) -> Result<Value, String> {
    let (au, sa) = dec_parts(a).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let (bu, sb) = dec_parts(b).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let (x, y, scale) =
        dec_align(au, sa, bu, sb).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    if y == 0 {
        return Err(FAULT_DECIMAL_MOD_ZERO.to_string());
    }
    let unscaled = x
        .checked_rem(y)
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    Ok(Value::Decimal { unscaled, scale })
}

/// Euclidean gcd of two non-zero `u128` magnitudes (used by exact decimal division to reduce the
/// quotient fraction to lowest terms before testing termination).
fn u128_gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Bare exact-or-fault decimal division (`/`, 2026-06-27). The value `(a/b)` is returned **exactly** in
/// its minimal-scale form when the quotient terminates (`10d/4d → 2.5`, `1d/8d → 0.125`); a
/// **non-terminating** quotient (`1d/3d`) faults [`FAULT_DECIMAL_NONTERMINATING`], a zero divisor
/// faults [`FAULT_DECIMAL_DIV_ZERO`], and an exact result past `i128` range / scale 255 faults
/// [`FAULT_DECIMAL_OVERFLOW`]. (Use `Decimal.div(a, b, scale, mode)` for a *rounded* quotient.)
/// Algorithm: reduce `a/b` to lowest terms `p/q·10^(sb-sa)`; if `q` (after stripping factors of 2 and
/// 5) is not 1 the decimal repeats → fault; otherwise the exact unscaled is `p·2^(m-i)·5^(m-j)` at
/// scale derived from `max(i,j)` and the `10^(sb-sa)` factor, then trailing zeros are stripped to the
/// canonical minimal form. The emitted PHP `__phorj_dec_div_exact` (bcdiv + exactness check + strip)
/// mirrors this byte-for-byte.
pub fn decimal_div_exact(a: &Value, b: &Value) -> Result<Value, String> {
    let ovf = || FAULT_DECIMAL_OVERFLOW.to_string();
    let (au, sa) = dec_parts(a).ok_or_else(ovf)?;
    let (bu, sb) = dec_parts(b).ok_or_else(ovf)?;
    if bu == 0 {
        return Err(FAULT_DECIMAL_DIV_ZERO.to_string());
    }
    if au == 0 {
        return Ok(Value::Decimal {
            unscaled: 0,
            scale: 0,
        });
    }
    let neg = (au < 0) ^ (bu < 0);
    let mut p = au.unsigned_abs();
    let mut q = bu.unsigned_abs();
    let g = u128_gcd(p, q);
    p /= g;
    q /= g;
    // Strip factors of 2 and 5 from the reduced denominator; anything left ⇒ non-terminating.
    let mut i = 0u32;
    while q % 2 == 0 {
        q /= 2;
        i += 1;
    }
    let mut j = 0u32;
    while q % 5 == 0 {
        q /= 5;
        j += 1;
    }
    if q != 1 {
        return Err(FAULT_DECIMAL_NONTERMINATING.to_string());
    }
    let m = i.max(j);
    let mul2 = 2u128.checked_pow(m - i).ok_or_else(ovf)?;
    let mul5 = 5u128.checked_pow(m - j).ok_or_else(ovf)?;
    let mut mag = p
        .checked_mul(mul2)
        .ok_or_else(ovf)?
        .checked_mul(mul5)
        .ok_or_else(ovf)?;
    // value = mag · 10^(delta - m), delta = sb - sa. exp <= 0 ⇒ that magnitude at scale -exp; exp > 0
    // ⇒ scale 0 after multiplying in the extra factor.
    let delta = i64::from(sb) - i64::from(sa);
    let exp = delta - i64::from(m);
    let scale = if exp <= 0 {
        let s = -exp;
        if s > i64::from(u8::MAX) {
            return Err(FAULT_DECIMAL_OVERFLOW.to_string());
        }
        s as u8
    } else {
        let factor = 10u128
            .checked_pow(u32::try_from(exp).map_err(|_| ovf())?)
            .ok_or_else(ovf)?;
        mag = mag.checked_mul(factor).ok_or_else(ovf)?;
        0
    };
    if mag > i128::MAX as u128 {
        return Err(FAULT_DECIMAL_OVERFLOW.to_string());
    }
    let mut unscaled = if neg { -(mag as i128) } else { mag as i128 };
    // Canonical minimal form: strip trailing zeros (division has no inherent scale, unlike `* + -`),
    // matching the PHP helper's rtrim — so `2.50d / 1d` is `2.5`, not `2.50`.
    let mut scale = scale;
    while scale > 0 && unscaled % 10 == 0 {
        unscaled /= 10;
        scale -= 1;
    }
    Ok(Value::Decimal { unscaled, scale })
}

/// Decimal negation (unary `-`): negate `unscaled` (checked — `i128::MIN` would overflow). The scale
/// is preserved; rendering never produces `-0` (see [`fmt_decimal`]).
pub fn decimal_neg(unscaled: i128, scale: u8) -> Result<Value, String> {
    let unscaled = unscaled
        .checked_neg()
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    Ok(Value::Decimal { unscaled, scale })
}

/// The seven rounding modes a `Decimal.div`/`Decimal.round` accepts (M-NUM S2). The injected
/// `RoundingMode` enum's variant *names* map onto these — the natives read `Value::Enum.variant` and
/// project it here via [`RoundMode::from_variant`], so the rounding kernel is variant-name-agnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundMode {
    /// Ties away from zero (`2.5 -> 3`, `-2.5 -> -3`).
    HalfUp,
    /// Ties toward zero (`2.5 -> 2`, `-2.5 -> -2`).
    HalfDown,
    /// Ties to the nearest even digit — banker's rounding (`2.5 -> 2`, `3.5 -> 4`).
    HalfEven,
    /// Always away from zero (`2.1 -> 3`, `-2.1 -> -3`).
    Up,
    /// Always toward zero — truncate (`2.9 -> 2`, `-2.9 -> -2`); equals a raw `bcdiv`.
    Down,
    /// Always toward `+∞` (`2.1 -> 3`, `-2.9 -> -2`).
    Ceiling,
    /// Always toward `-∞` (`2.9 -> 2`, `-2.1 -> -3`).
    Floor,
}

impl RoundMode {
    /// Map a `RoundingMode` enum variant name to a [`RoundMode`], or `None` for an unknown variant
    /// (checker-unreachable — the injected enum has exactly these seven variants and the native's
    /// signature requires a `RoundingMode`; handled defensively rather than panicking, EV-7).
    pub fn from_variant(variant: &str) -> Option<RoundMode> {
        Some(match variant {
            "HalfUp" => RoundMode::HalfUp,
            "HalfDown" => RoundMode::HalfDown,
            "HalfEven" => RoundMode::HalfEven,
            "Up" => RoundMode::Up,
            "Down" => RoundMode::Down,
            "Ceiling" => RoundMode::Ceiling,
            "Floor" => RoundMode::Floor,
            _ => return None,
        })
    }
}

/// Round the exact rational `n / d` to an integer under `mode` (M-NUM S2) — the single-sourced
/// rounding primitive both backends call and the PHP `__phorj_dec_div`/`_round` helpers replicate
/// step-for-step. The caller guarantees `d != 0` (a zero divisor is the `FAULT_DECIMAL_DIV_ZERO`
/// fault, checked before this). Any `checked_*` overflow ⇒ [`FAULT_DECIMAL_OVERFLOW`].
///
/// The half-decision compares `|rem|` against `d - |rem|` (both non-negative, so no `2*rem`
/// doubling that could overflow `i128`). i128 MIN edges are handled via `unsigned_abs`/`checked_neg`
/// — never a bare `-x` or `x.abs()`.
pub fn round_div(n: i128, d: i128, mode: RoundMode) -> Result<i128, String> {
    // 1. Normalise the divisor's sign so `d > 0`; the quotient sign is unchanged. `d == 0` is the
    //    caller's responsibility (div-by-zero fault).
    let (n, d) = if d < 0 {
        (
            n.checked_neg().ok_or(FAULT_DECIMAL_OVERFLOW)?,
            d.checked_neg().ok_or(FAULT_DECIMAL_OVERFLOW)?,
        )
    } else {
        (n, d)
    };
    // 2. Truncating quotient + dividend-signed remainder (matches BCMath `bcdiv`/`bcmod`).
    let q = n / d; // d != 0 here, and d > 0 so no MIN/-1 overflow
    let rem = n % d;
    if rem == 0 {
        return Ok(q); // exact
    }
    // `s` = sign of the dividend (and of the exact quotient): the direction "away from zero".
    let s: i128 = if n > 0 { 1 } else { -1 };
    // Magnitudes for the half-comparison. `d > 0`, so `d` is its own magnitude; `|rem| <= d - 1 < d`,
    // so `d - abs_rem` is `>= 1 > 0` and never underflows.
    let abs_rem =
        i128::try_from(rem.unsigned_abs()).map_err(|_| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let complement = d - abs_rem; // safe: 0 < abs_rem < d
    let bump = |q: i128, by: i128| q.checked_add(by).ok_or(FAULT_DECIMAL_OVERFLOW.to_string());
    let result = match mode {
        RoundMode::Down => q,
        RoundMode::Up => bump(q, s)?,
        RoundMode::Ceiling => {
            if n > 0 {
                bump(q, 1)?
            } else {
                q
            }
        }
        RoundMode::Floor => {
            if n < 0 {
                bump(q, -1)?
            } else {
                q
            }
        }
        RoundMode::HalfUp => {
            if abs_rem >= complement {
                bump(q, s)?
            } else {
                q
            }
        }
        RoundMode::HalfDown => {
            if abs_rem > complement {
                bump(q, s)?
            } else {
                q
            }
        }
        RoundMode::HalfEven => match abs_rem.cmp(&complement) {
            Ordering::Greater => bump(q, s)?,
            Ordering::Less => q,
            // Exact tie: round to even — bump only if `q` is currently odd.
            Ordering::Equal => {
                if q % 2 != 0 {
                    bump(q, s)?
                } else {
                    q
                }
            }
        },
    };
    Ok(result)
}

/// `Decimal.div(a, b, scale, mode)` (M-NUM S2): the exact rational `a / b`, rounded to `scale`
/// fractional digits under `mode`. Computes `N = a.unscaled * 10^(b.scale + scale)` and
/// `D = b.unscaled * 10^a.scale` (both checked), then `round_div(N, D, mode)` at `scale`.
/// `b == 0` ⇒ [`FAULT_DECIMAL_DIV_ZERO`]; `scale < 0` ⇒ [`FAULT_DECIMAL_SCALE`]; any i128 overflow ⇒
/// [`FAULT_DECIMAL_OVERFLOW`]. Mirrored by the PHP `__phorj_dec_div` helper.
pub fn decimal_div(a: &Value, b: &Value, scale: i64, mode: RoundMode) -> Result<Value, String> {
    let (au, sa) = dec_parts(a).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let (bu, sb) = dec_parts(b).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let out_scale = scale_u8(scale)?;
    if bu == 0 {
        return Err(FAULT_DECIMAL_DIV_ZERO.to_string());
    }
    // N = au * 10^(sb + out_scale); D = bu * 10^sa. Both exponents are non-negative `u8` sums.
    let n_exp = u32::from(sb)
        .checked_add(u32::from(out_scale))
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let n = pow10_mul(au, n_exp)?;
    let d = pow10_mul(bu, u32::from(sa))?;
    let unscaled = round_div(n, d, mode)?;
    Ok(Value::Decimal {
        unscaled,
        scale: out_scale,
    })
}

/// `Decimal.round(d, scale, mode)` (M-NUM S2): re-scale `d` to exactly `scale` fractional digits.
/// Scaling up (`scale >= d.scale`) is exact (`unscaled * 10^Δ`, checked, no rounding); scaling down
/// rounds via `round_div(unscaled, 10^Δ, mode)`. `scale < 0` ⇒ [`FAULT_DECIMAL_SCALE`]; overflow ⇒
/// [`FAULT_DECIMAL_OVERFLOW`]. Mirrored by the PHP `__phorj_dec_round` helper.
pub fn decimal_round(d: &Value, scale: i64, mode: RoundMode) -> Result<Value, String> {
    let (du, sd) = dec_parts(d).ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())?;
    let out_scale = scale_u8(scale)?;
    let unscaled = if out_scale >= sd {
        // Exact up-scale: multiply by 10^(out_scale - sd).
        pow10_mul(du, u32::from(out_scale - sd))?
    } else {
        // Down-scale: divide by 10^(sd - out_scale) with rounding.
        let divisor = pow10(u32::from(sd - out_scale))?;
        round_div(du, divisor, mode)?
    };
    Ok(Value::Decimal {
        unscaled,
        scale: out_scale,
    })
}

/// `Convert.decimalToInt(decimal)` (M-NUM S3): the decimal's integer part, truncated toward zero
/// (drop the fraction), or `None` if that integer part is outside the i64 range. Computed exactly on
/// the i128 carrier — `unscaled / 10^scale` truncates toward zero (i128 `/` rounds toward zero, like
/// PHP `intdiv`/`bcdiv`), then `i64::try_from` range-checks. No string parsing, no BCMath. Single-
/// sourced; mirrored by the PHP `__phorj_dec_to_int` helper (which splits the carrier string before
/// the dot). A non-decimal value is checker-unreachable (handled defensively as `None`).
pub fn decimal_to_int(d: &Value) -> Option<i64> {
    let (unscaled, scale) = match d {
        Value::Decimal { unscaled, scale } => (*unscaled, *scale),
        _ => return None,
    };
    // 10^scale fits i128 for any realistic scale; an absurd scale (>38) overflows pow → None.
    let divisor = 10i128.checked_pow(u32::from(scale))?;
    let int_part = unscaled / divisor; // i128 `/` truncates toward zero
    i64::try_from(int_part).ok()
}

/// `decimal as int` (M4 as-matrix) — **exact-or-null**: `Some(i)` only when the decimal has a zero
/// fractional part and the integer is in i64 range (`3.00d → 3`, `3.50d → None`). Unlike
/// [`decimal_to_int`] (truncate, used by `Convert.decimalToInt`), it never drops a fraction silently
/// — the `as` "no silent loss" rule. Mirrored by the PHP `__phorj_dec_to_int_exact` helper.
pub fn decimal_to_int_exact(d: &Value) -> Option<i64> {
    let (unscaled, scale) = match d {
        Value::Decimal { unscaled, scale } => (*unscaled, *scale),
        _ => return None,
    };
    let divisor = 10i128.checked_pow(u32::from(scale))?;
    if unscaled % divisor != 0 {
        return None; // non-integral → null (exact-or-null)
    }
    i64::try_from(unscaled / divisor).ok()
}

/// `10^exp` as a checked `i128`, [`FAULT_DECIMAL_OVERFLOW`] on overflow (M-NUM S2 helper).
fn pow10(exp: u32) -> Result<i128, String> {
    10i128
        .checked_pow(exp)
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())
}

/// `value * 10^exp` as a checked `i128`, [`FAULT_DECIMAL_OVERFLOW`] on overflow (M-NUM S2 helper).
fn pow10_mul(value: i128, exp: u32) -> Result<i128, String> {
    value
        .checked_mul(pow10(exp)?)
        .ok_or_else(|| FAULT_DECIMAL_OVERFLOW.to_string())
}

/// Validate + narrow a `scale` argument (an `int`, so `i64`) to the `u8` a `Value::Decimal` stores.
/// A negative scale is [`FAULT_DECIMAL_SCALE`]; a scale past `u8::MAX` (255, far beyond any realistic
/// money use) is also [`FAULT_DECIMAL_SCALE`] (M-NUM S2). The PHP helpers throw on `scale < 0` too.
fn scale_u8(scale: i64) -> Result<u8, String> {
    u8::try_from(scale).map_err(|_| FAULT_DECIMAL_SCALE.to_string())
}

/// Numeric, **scale-insensitive** ordering of two decimal operands (mixed `decimal`/`int` allowed):
/// align to the common scale and compare unscaled. `None` if the operands aren't decimal/int
/// (checker-unreachable) **or** an alignment overflow — in the overflow case the operands differ by
/// ≥10^Δ at i128 scale, so they are necessarily unequal; the caller's `< > <= >=` projection treats a
/// `None` like NaN (`false`), which is sound here because equality is `Some(Equal)` only. (M-NUM S1.)
pub fn decimal_cmp(a: &Value, b: &Value) -> Option<Ordering> {
    let (au, sa) = dec_parts(a)?;
    let (bu, sb) = dec_parts(b)?;
    let (x, y, _) = dec_align(au, sa, bu, sb)?;
    Some(x.cmp(&y))
}

/// Render `(unscaled, scale)` as a decimal string with **exactly `scale`** fractional digits — the
/// BCMath-padding form, single-sourced so both backends agree and the emitted PHP (a BCMath result
/// string) matches. `{1999, 2}` → `"19.99"`, `{1500, 3}` → `"1.500"`, `{100, 0}` → `"100"`,
/// `{15, 4}` → `"0.0015"`. Negative values carry a leading `-`; the value `0` (any scale) **never**
/// renders `-0` (M-NUM S1).
pub fn fmt_decimal(unscaled: i128, scale: u8) -> String {
    let neg = unscaled < 0;
    // Magnitude as a string of digits. `unsigned_abs` handles `i128::MIN` without overflow.
    let digits = unscaled.unsigned_abs().to_string();
    let s = scale as usize;
    let body = if s == 0 {
        digits
    } else if digits.len() > s {
        let dot = digits.len() - s;
        format!("{}.{}", &digits[..dot], &digits[dot..])
    } else {
        // Fewer integer digits than the scale → pad with leading zeros after `0.`.
        format!("0.{}{}", "0".repeat(s - digits.len()), digits)
    };
    // Never render `-0` / `-0.00`: only prefix `-` for a genuinely non-zero magnitude.
    if neg && unscaled != 0 {
        format!("-{body}")
    } else {
        body
    }
}

/// Parse the `decimal` literal grammar at runtime for `Decimal.of(string)` (M-NUM S1) — the SAME
/// grammar the lexer accepts for a `…d` literal, returning `(unscaled, scale)` or `None` on a
/// malformed string or i128 overflow (so `Decimal.of` is `decimal?`). Grammar: optional sign, then
/// digits with an optional single fractional part (`12`, `12.34`, `.5`, `-0.50`); NO exponent, NO
/// underscores (a runtime string is exact, unlike a source literal), NO surrounding whitespace. The
/// scale is the count of fractional digits (trailing zeros preserved). Shared by the interpreter, the
/// VM, and mirrored by the PHP `__phorj_dec_of` PCRE helper.
pub fn decimal_of(s: &str) -> Option<(i128, u8)> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (neg, rest) = match bytes[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    if rest.is_empty() {
        return None;
    }
    let (int_part, frac_part) = match rest.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (rest, None),
    };
    // At least one digit overall; each part must be all ASCII digits (an empty integer part like `.5`
    // is allowed, but a trailing `12.` with an empty fractional part is not — matches the lexer, which
    // requires a digit after the dot to treat it as a fraction).
    if let Some(f) = frac_part {
        if f.is_empty() || !f.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
    }
    if !int_part.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if int_part.is_empty() && frac_part.is_none() {
        return None;
    }
    let frac = frac_part.unwrap_or("");
    let scale = u8::try_from(frac.len()).ok()?;
    let combined = format!("{int_part}{frac}");
    if combined.is_empty() {
        return None;
    }
    let magnitude: i128 = combined.parse().ok()?;
    let unscaled = if neg {
        magnitude.checked_neg()?
    } else {
        magnitude
    };
    Some((unscaled, scale))
}
/// Checked integer power `base ** exp` (Phase 1 operators slice). A negative exponent faults
/// ([`FAULT_NEGATIVE_EXPONENT`] — the result can't be a typed `int`); overflow (incl. an exponent
/// too large to fit `u32`) is a clean [`FAULT_INT_OVERFLOW`], never a panic (EV-7). Single-sourced:
/// both the interpreter's `**` arm and the `Core.Math.ipow` native call this, so `run`/`runvm`
/// compute and fault identically. PHP's `**`/`pow` return `int` for the same non-negative,
/// non-overflowing domain, keeping the transpiled output byte-identical there.
pub fn int_pow(base: i64, exp: i64) -> Result<i64, String> {
    if exp < 0 {
        return Err(FAULT_NEGATIVE_EXPONENT.to_string());
    }
    let e = u32::try_from(exp).map_err(|_| FAULT_INT_OVERFLOW.to_string())?;
    base.checked_pow(e)
        .ok_or_else(|| FAULT_INT_OVERFLOW.to_string())
}
/// Float power `base ** exp` (Phase 1 operators slice). Floats never fault — NaN/inf are valid
/// `f64`. `powf` is C `pow` (matching PHP's `**`/`pow` on floats). Single-sourced with `Core.Math.pow`.
pub fn float_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

/// Maximum number of elements a range literal may materialize before faulting (P1-#9). An unbounded
/// `0..n` would otherwise allocate an arbitrarily large `Vec` and abort the process (OOM, exit 101)
/// instead of producing a clean, byte-identical fault on both backends (EV-7). ~10M × 16 B ≈ 160 MB
/// ceiling — generous for any realistic program, well below uncontrolled OOM. Tunable.
pub const MAX_RANGE_LEN: i64 = 10_000_000;

/// Materialize an integer range exactly as both backends do, with a shared size guard (P1-#9). `hi`
/// is the inclusive upper bound: `end` for `..=`, `end - 1` for `..`. An empty/reversed range
/// (`start > hi`) yields `[]`. A range wider than [`MAX_RANGE_LEN`] faults `"range too large"` rather
/// than OOM-aborting. All arithmetic is checked (EV-7): `end - 1` underflow (exclusive `..i64::MIN`)
/// and `hi - start` overflow both resolve without panicking. Single-sourced so `run`/`runvm` fault
/// identically (the differential harness classifies the body substring as `RangeTooLarge`).
pub fn build_range(start: i64, end: i64, inclusive: bool) -> Result<Vec<Value>, String> {
    let hi = if inclusive {
        end
    } else {
        match end.checked_sub(1) {
            Some(h) => h,
            None => return Ok(Vec::new()), // exclusive `start..i64::MIN` — always empty
        }
    };
    if start > hi {
        return Ok(Vec::new());
    }
    let span = hi.checked_sub(start).ok_or("range too large")?;
    if span >= MAX_RANGE_LEN {
        return Err("range too large".to_string());
    }
    Ok((start..=hi).map(Value::Int).collect())
}

/// Ordering probe for `< > <= >=`. `Ok(None)` is the NaN case (every ordered comparison of NaN is
/// `false`); `Err` is a non-comparable operand pairing. The op→bool projection stays backend-local
/// (the op enums differ); only the ordering and the comparability fault are shared.
pub fn compare_ord(a: &Value, b: &Value) -> Result<Option<Ordering>, String> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x.partial_cmp(y)),
        (Value::Float(x), Value::Float(y)) => Ok(x.partial_cmp(y)),
        // Decimal ordering is numeric + scale-insensitive (mixed `decimal`/`int` allowed): the checker
        // guarantees `< > <= >=` operands match (decimal⊕decimal or decimal⊕int), so this only fires
        // for those. `decimal_cmp` returns `None` only on an (unreachable-for-equal-values) alignment
        // overflow, which projects to `false` like NaN — sound, since equality is `Some(Equal)` only.
        (Value::Decimal { .. }, Value::Decimal { .. } | Value::Int(_))
        | (Value::Int(_), Value::Decimal { .. }) => Ok(decimal_cmp(a, b)),
        _ => Err(format!(
            "cannot compare {} and {}",
            a.type_name(),
            b.type_name()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(unscaled: i128, scale: u8) -> Value {
        Value::Decimal { unscaled, scale }
    }

    #[test]
    fn int_intdiv_truncates_and_faults() {
        assert_eq!(int_intdiv(7, 2), Ok(3));
        assert_eq!(int_intdiv(-7, 2), Ok(-3)); // toward zero
        assert_eq!(int_intdiv(7, -2), Ok(-3));
        assert_eq!(int_intdiv(-7, -2), Ok(3));
        assert_eq!(int_intdiv(6, 3), Ok(2));
        // divisor zero → division by zero fault
        assert_eq!(int_intdiv(5, 0).unwrap_err(), FAULT_DIV_ZERO);
        // i64::MIN / -1 overflows → integer overflow fault
        assert_eq!(int_intdiv(i64::MIN, -1).unwrap_err(), FAULT_INT_OVERFLOW);
    }

    #[test]
    fn float_div_rem_by_zero_fault() {
        // Non-zero divisor: ordinary IEEE result.
        assert_eq!(float_div(7.0, 2.0), Ok(3.5));
        assert_eq!(float_rem(7.5, 2.0), Ok(1.5));
        // Zero divisor faults (no IEEE inf/NaN) — both +0.0 and -0.0.
        assert_eq!(float_div(1.0, 0.0).unwrap_err(), FAULT_DIV_ZERO);
        assert_eq!(float_div(1.0, -0.0).unwrap_err(), FAULT_DIV_ZERO);
        assert_eq!(float_rem(1.0, 0.0).unwrap_err(), FAULT_MOD_ZERO);
        // A finite overflow to inf is NOT a zero division — it stays inf.
        assert!(float_div(1.0e308, 1.0e-308).unwrap().is_infinite());
    }

    #[test]
    fn decimal_rem_is_exact_and_faults_on_zero() {
        let d = |u, s| Value::Decimal {
            unscaled: u,
            scale: s,
        };
        // 10.50 % 3.00 = 1.50 (align to scale 2: 1050 % 300 = 150).
        assert!(matches!(
            decimal_rem(&d(1050, 2), &d(300, 2)),
            Ok(Value::Decimal {
                unscaled: 150,
                scale: 2
            })
        ));
        // Mixed scales align to max: 10.5 % 3 (int) → 1.5 (scale 1).
        assert!(matches!(
            decimal_rem(&d(105, 1), &Value::Int(3)),
            Ok(Value::Decimal {
                unscaled: 15,
                scale: 1
            })
        ));
        // Sign follows the dividend: -10 % 3 = -1.
        assert!(matches!(
            decimal_rem(&d(-10, 0), &d(3, 0)),
            Ok(Value::Decimal {
                unscaled: -1,
                scale: 0
            })
        ));
        // Zero divisor faults.
        assert_eq!(
            decimal_rem(&d(5, 0), &d(0, 0)).unwrap_err(),
            FAULT_DECIMAL_MOD_ZERO
        );
    }

    #[test]
    fn decimal_div_exact_terminates_or_faults() {
        let d = |u, s| Value::Decimal {
            unscaled: u,
            scale: s,
        };
        let dv = |a, b| decimal_div_exact(&a, &b);
        // Terminating quotients, minimal form.
        assert!(matches!(
            dv(d(10, 0), d(4, 0)),
            Ok(Value::Decimal {
                unscaled: 25,
                scale: 1
            })
        )); // 2.5
        assert!(matches!(
            dv(d(1, 0), d(8, 0)),
            Ok(Value::Decimal {
                unscaled: 125,
                scale: 3
            })
        )); // 0.125
        assert!(matches!(
            dv(d(10, 0), d(2, 0)),
            Ok(Value::Decimal {
                unscaled: 5,
                scale: 0
            })
        )); // 5
            // Trailing zeros are stripped to minimal form: 2.50 / 1 = 2.5 (matches the PHP rtrim).
        assert!(matches!(
            dv(d(250, 2), d(1, 0)),
            Ok(Value::Decimal {
                unscaled: 25,
                scale: 1
            })
        ));
        // Negative sign carries.
        assert!(matches!(
            dv(d(-1, 0), d(4, 0)),
            Ok(Value::Decimal {
                unscaled: -25,
                scale: 2
            })
        )); // -0.25
            // Non-terminating ⇒ fault.
        assert_eq!(
            dv(d(1, 0), d(3, 0)).unwrap_err(),
            FAULT_DECIMAL_NONTERMINATING
        );
        assert_eq!(
            dv(d(10, 0), d(6, 0)).unwrap_err(),
            FAULT_DECIMAL_NONTERMINATING
        ); // 5/3
           // Zero divisor ⇒ div-zero fault; 0/x = 0.
        assert_eq!(dv(d(5, 0), d(0, 0)).unwrap_err(), FAULT_DECIMAL_DIV_ZERO);
        assert!(matches!(
            dv(d(0, 0), d(5, 0)),
            Ok(Value::Decimal {
                unscaled: 0,
                scale: 0
            })
        ));
    }

    #[test]
    fn float_to_int_guards_the_edge() {
        assert_eq!(float_to_int(3.9), Some(3)); // truncate toward zero
        assert_eq!(float_to_int(-3.9), Some(-3));
        assert_eq!(float_to_int(0.0), Some(0));
        assert_eq!(float_to_int(42.0), Some(42));
        // special values → None (avoids PHP `(int)NAN == 0`)
        assert_eq!(float_to_int(f64::NAN), None);
        assert_eq!(float_to_int(f64::INFINITY), None);
        assert_eq!(float_to_int(f64::NEG_INFINITY), None);
        // out-of-range huge magnitudes → None
        assert_eq!(float_to_int(1e30), None);
        assert_eq!(float_to_int(-1e30), None);
        // near the i64 edge: `i64::MIN as f64` is exactly `-2^63` (representable, in-range);
        // `i64::MAX as f64` rounds UP to `2^63` (== the exclusive UPPER), so it is OUT — exactly the
        // edge the shared bound is chosen to close (Rust and PHP both reject `2^63`).
        assert_eq!(float_to_int(i64::MIN as f64), Some(i64::MIN));
        assert_eq!(float_to_int(i64::MAX as f64), None); // rounds to 2^63 == UPPER (exclusive)
                                                         // a large but exactly-representable in-range value (2^53) round-trips.
        assert_eq!(
            float_to_int(9_007_199_254_740_992.0),
            Some(9_007_199_254_740_992)
        );
    }

    #[test]
    fn decimal_to_int_truncates_toward_zero() {
        assert_eq!(decimal_to_int(&dec(1999, 2)), Some(19)); // 19.99 → 19
        assert_eq!(decimal_to_int(&dec(-1999, 2)), Some(-19)); // -19.99 → -19 (toward zero)
        assert_eq!(decimal_to_int(&dec(100, 0)), Some(100)); // 100 → 100
        assert_eq!(decimal_to_int(&dec(5, 4)), Some(0)); // 0.0005 → 0
        assert_eq!(decimal_to_int(&dec(-5, 1)), Some(0)); // -0.5 → 0
                                                          // integer part out of i64 range → None
        assert_eq!(decimal_to_int(&dec(i128::from(i64::MAX) + 1, 0)), None);
        assert_eq!(decimal_to_int(&dec(i128::from(i64::MIN) - 1, 0)), None);
    }

    #[test]
    fn fmt_decimal_renders_with_exact_scale() {
        assert_eq!(fmt_decimal(1999, 2), "19.99");
        assert_eq!(fmt_decimal(1500, 3), "1.500");
        assert_eq!(fmt_decimal(100, 0), "100");
        assert_eq!(fmt_decimal(15, 4), "0.0015");
        assert_eq!(fmt_decimal(0, 0), "0");
        assert_eq!(fmt_decimal(0, 2), "0.00");
        assert_eq!(fmt_decimal(-5000, 2), "-50.00");
        assert_eq!(fmt_decimal(-1, 3), "-0.001");
        // never `-0` even though the sign bit could be set (it can't be for 0, but guard anyway).
        assert_eq!(fmt_decimal(0, 4), "0.0000");
    }

    /// Assert a decimal-kernel `Ok` matches an expected `(unscaled, scale)` exactly (not just
    /// numerically — the scale is part of the result), since `Value` has no `PartialEq`.
    fn assert_dec(got: Result<Value, String>, unscaled: i128, scale: u8) {
        match got {
            Ok(Value::Decimal {
                unscaled: u,
                scale: s,
            }) => {
                assert_eq!((u, s), (unscaled, scale), "decimal result mismatch");
            }
            other => panic!("expected Ok(Decimal), got {other:?}"),
        }
    }

    #[test]
    fn decimal_add_sub_use_max_scale() {
        // 1.50 + 2.300 = 3.800 (scale 3); align the lower-scale operand up.
        assert_dec(decimal_add(&dec(150, 2), &dec(2300, 3)), 3800, 3);
        // 1.50 - 1.50 = 0.00 (scale 2, no neg zero in render).
        assert_dec(decimal_sub(&dec(150, 2), &dec(150, 2)), 0, 2);
        // mixed decimal + int: int widens to scale 0 → 19.99 + 1 = 20.99.
        assert_dec(decimal_add(&dec(1999, 2), &Value::Int(1)), 2099, 2);
        assert_dec(decimal_add(&Value::Int(1), &dec(1999, 2)), 2099, 2);
    }

    #[test]
    fn decimal_mul_sums_scales() {
        // 1.11 * 1.11 = 1.2321 (scale 4 = 2 + 2).
        assert_dec(decimal_mul(&dec(111, 2), &dec(111, 2)), 12321, 4);
        // decimal * int: 19.99 * 3 = 59.97 (scale 2, int scale 0).
        assert_dec(decimal_mul(&dec(1999, 2), &Value::Int(3)), 5997, 2);
    }

    fn assert_dec_overflow(got: Result<Value, String>) {
        assert_eq!(got.err().as_deref(), Some(FAULT_DECIMAL_OVERFLOW));
    }

    #[test]
    fn decimal_overflow_is_a_clean_fault() {
        let big = dec(i128::MAX, 0);
        assert_dec_overflow(decimal_add(&big, &Value::Int(1)));
        assert_dec_overflow(decimal_mul(&big, &Value::Int(2)));
        // Alignment overflow: scaling i128::MAX up by 10^1 overflows before the add.
        assert_dec_overflow(decimal_add(&big, &dec(0, 1)));
        // Negation of i128::MIN overflows.
        assert_dec_overflow(decimal_neg(i128::MIN, 0));
    }

    #[test]
    fn round_div_all_seven_modes_on_a_positive_tie() {
        // 5/2 = 2.5 — an exact tie; each mode resolves the half differently.
        use RoundMode::*;
        assert_eq!(round_div(5, 2, Down), Ok(2)); // toward 0
        assert_eq!(round_div(5, 2, Up), Ok(3)); // away from 0
        assert_eq!(round_div(5, 2, Ceiling), Ok(3)); // toward +inf
        assert_eq!(round_div(5, 2, Floor), Ok(2)); // toward -inf
        assert_eq!(round_div(5, 2, HalfUp), Ok(3)); // tie away
        assert_eq!(round_div(5, 2, HalfDown), Ok(2)); // tie toward 0
        assert_eq!(round_div(5, 2, HalfEven), Ok(2)); // tie to even (q=2 even)
    }

    #[test]
    fn round_div_all_seven_modes_on_a_negative_tie() {
        // -5/2 = -2.5 — mirror of the positive tie.
        use RoundMode::*;
        assert_eq!(round_div(-5, 2, Down), Ok(-2)); // toward 0
        assert_eq!(round_div(-5, 2, Up), Ok(-3)); // away from 0
        assert_eq!(round_div(-5, 2, Ceiling), Ok(-2)); // toward +inf
        assert_eq!(round_div(-5, 2, Floor), Ok(-3)); // toward -inf
        assert_eq!(round_div(-5, 2, HalfUp), Ok(-3)); // tie away
        assert_eq!(round_div(-5, 2, HalfDown), Ok(-2)); // tie toward 0
        assert_eq!(round_div(-5, 2, HalfEven), Ok(-2)); // tie to even (q=-2 even)
    }

    #[test]
    fn round_div_half_even_picks_the_odd_quotient_up() {
        // 7/2 = 3.5 — tie, q=3 is odd, so HalfEven rounds to the even 4.
        assert_eq!(round_div(7, 2, RoundMode::HalfEven), Ok(4));
        assert_eq!(round_div(-7, 2, RoundMode::HalfEven), Ok(-4));
    }

    #[test]
    fn round_div_non_tie_and_exact() {
        use RoundMode::*;
        // 7/3 = 2.333… — not a tie; the half rules don't trigger (rem < complement).
        assert_eq!(round_div(7, 3, HalfUp), Ok(2));
        assert_eq!(round_div(7, 3, Up), Ok(3));
        assert_eq!(round_div(7, 3, Down), Ok(2));
        assert_eq!(round_div(7, 3, Ceiling), Ok(3));
        assert_eq!(round_div(7, 3, Floor), Ok(2));
        // 8/3 = 2.666… — past the half, so HalfUp/HalfDown/HalfEven all round up.
        assert_eq!(round_div(8, 3, HalfUp), Ok(3));
        assert_eq!(round_div(8, 3, HalfDown), Ok(3));
        assert_eq!(round_div(8, 3, HalfEven), Ok(3));
        // Exact division: every mode agrees, no rounding.
        for m in [HalfUp, HalfDown, HalfEven, Up, Down, Ceiling, Floor] {
            assert_eq!(round_div(6, 3, m), Ok(2));
            assert_eq!(round_div(-6, 3, m), Ok(-2));
        }
    }

    #[test]
    fn round_div_negative_divisor_normalises() {
        // A negative divisor is normalised so the result matches the equivalent positive-divisor form.
        assert_eq!(
            round_div(5, -2, RoundMode::HalfUp),
            round_div(-5, 2, RoundMode::HalfUp)
        );
        assert_eq!(
            round_div(-5, -2, RoundMode::Up),
            round_div(5, 2, RoundMode::Up)
        );
    }

    #[test]
    fn decimal_div_rounds_to_scale() {
        // 10.00 / 3 = 3.3333… → scale 2 HalfEven → 3.33.
        assert_dec(
            decimal_div(&dec(1000, 2), &Value::Int(3), 2, RoundMode::HalfEven),
            333,
            2,
        );
        // 1 / 8 = 0.125 → scale 2 HalfUp → 0.13 (tie at the third digit).
        assert_dec(
            decimal_div(&Value::Int(1), &Value::Int(8), 2, RoundMode::HalfUp),
            13,
            2,
        );
        // 1 / 8 = 0.125 → scale 2 HalfEven → 0.12 (q=12 even).
        assert_dec(
            decimal_div(&Value::Int(1), &Value::Int(8), 2, RoundMode::HalfEven),
            12,
            2,
        );
    }

    #[test]
    fn decimal_div_by_zero_is_a_clean_fault() {
        assert_eq!(
            decimal_div(&dec(1000, 2), &dec(0, 2), 2, RoundMode::HalfUp)
                .err()
                .as_deref(),
            Some(FAULT_DECIMAL_DIV_ZERO)
        );
        // an int-zero divisor too (the int widens to scale 0).
        assert_eq!(
            decimal_div(&dec(1000, 2), &Value::Int(0), 2, RoundMode::HalfUp)
                .err()
                .as_deref(),
            Some(FAULT_DECIMAL_DIV_ZERO)
        );
    }

    #[test]
    fn decimal_div_negative_scale_is_a_clean_fault() {
        assert_eq!(
            decimal_div(&dec(1000, 2), &Value::Int(3), -1, RoundMode::HalfUp)
                .err()
                .as_deref(),
            Some(FAULT_DECIMAL_SCALE)
        );
    }

    #[test]
    fn decimal_round_up_and_down_scale() {
        // 2.345 → scale 2 HalfUp → 2.35 (tie rounds away).
        assert_dec(decimal_round(&dec(2345, 3), 2, RoundMode::HalfUp), 235, 2);
        // 2.345 → scale 2 HalfEven → 2.34 (q=234 even).
        assert_dec(decimal_round(&dec(2345, 3), 2, RoundMode::HalfEven), 234, 2);
        // up-scale is exact: 2.5 → scale 3 → 2.500 (no rounding).
        assert_dec(decimal_round(&dec(25, 1), 3, RoundMode::Down), 2500, 3);
        // same-scale is identity.
        assert_dec(decimal_round(&dec(1999, 2), 2, RoundMode::HalfUp), 1999, 2);
    }

    #[test]
    fn decimal_round_negative_scale_is_a_clean_fault() {
        assert_eq!(
            decimal_round(&dec(2345, 3), -1, RoundMode::HalfUp)
                .err()
                .as_deref(),
            Some(FAULT_DECIMAL_SCALE)
        );
    }

    #[test]
    fn decimal_div_overflow_is_a_clean_fault() {
        // A huge target scale overflows 10^k before the division.
        assert_eq!(
            decimal_div(&Value::Int(1), &Value::Int(3), 200, RoundMode::HalfUp)
                .err()
                .as_deref(),
            Some(FAULT_DECIMAL_OVERFLOW)
        );
    }

    #[test]
    fn decimal_cmp_and_eq_are_scale_insensitive() {
        // 1.50 == 1.5 numerically (scale-insensitive).
        assert_eq!(
            decimal_cmp(&dec(150, 2), &dec(15, 1)),
            Some(Ordering::Equal)
        );
        assert!(dec(150, 2).eq_val(&dec(15, 1)));
        assert!(!dec(150, 2).eq_val(&dec(151, 2)));
        // mixed decimal/int equality: 2.00d == 2.
        assert!(dec(200, 2).eq_val(&Value::Int(2)));
        assert!(Value::Int(2).eq_val(&dec(200, 2)));
        // ordering
        assert_eq!(decimal_cmp(&dec(149, 2), &dec(15, 1)), Some(Ordering::Less));
        assert_eq!(
            compare_ord(&dec(150, 2), &dec(15, 1)),
            Ok(Some(Ordering::Equal))
        );
        // a decimal never equals a float (no cross-type) — handled by eq_val_rec's `_ => false`.
        assert!(!dec(150, 2).eq_val(&Value::Float(1.5)));
    }

    #[test]
    fn decimal_of_parses_the_literal_grammar() {
        assert_eq!(decimal_of("12.34"), Some((1234, 2)));
        assert_eq!(decimal_of("100"), Some((100, 0)));
        assert_eq!(decimal_of("1.500"), Some((1500, 3))); // trailing zeros set scale
        assert_eq!(decimal_of("-0.50"), Some((-50, 2)));
        assert_eq!(decimal_of(".5"), Some((5, 1)));
        assert_eq!(decimal_of("+3.0"), Some((30, 1)));
        // malformed → None
        assert_eq!(decimal_of(""), None);
        assert_eq!(decimal_of("abc"), None);
        assert_eq!(decimal_of("1.2.3"), None);
        assert_eq!(decimal_of("12."), None); // empty fractional part
        assert_eq!(decimal_of("1e3"), None); // no exponent
        assert_eq!(decimal_of("1_000"), None); // no underscores at runtime
        assert_eq!(decimal_of(" 12"), None); // no surrounding whitespace
                                             // i128 overflow → None
        let too_big = "1".repeat(40);
        assert_eq!(decimal_of(&too_big), None);
    }

    #[test]
    fn decimal_as_display_matches_fmt() {
        assert_eq!(dec(1999, 2).as_display().as_deref(), Some("19.99"));
        assert_eq!(dec(100, 0).as_display().as_deref(), Some("100"));
        assert_eq!(dec(0, 2).as_display().as_deref(), Some("0.00"));
        assert_eq!(dec(150, 2).type_name(), "decimal");
    }

    #[test]
    fn int_pow_normal_negative_and_overflow() {
        // Normal non-negative powers.
        assert_eq!(int_pow(2, 10), Ok(1024));
        assert_eq!(int_pow(5, 0), Ok(1)); // anything ** 0 == 1
        assert_eq!(int_pow(7, 1), Ok(7));
        assert_eq!(int_pow(-2, 3), Ok(-8)); // negative base, odd exponent
                                            // A negative exponent can't be a typed `int` → clean fault (EV-7), never a panic.
        assert_eq!(int_pow(2, -1), Err(FAULT_NEGATIVE_EXPONENT.to_string()));
        // Overflow is a clean fault, both for an overflowing result and a huge exponent.
        assert_eq!(int_pow(2, 63), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_pow(2, i64::MAX), Err(FAULT_INT_OVERFLOW.to_string()));
    }

    #[test]
    fn float_pow_matches_powf() {
        assert_eq!(float_pow(3.0, 2.0), 9.0);
        assert_eq!(float_pow(2.0, 10.0), 1024.0);
    }

    #[test]
    fn build_map_dedups_first_position_last_value() {
        // PHP semantics: a duplicate key keeps its first position but takes the last value.
        let m = build_map(vec![
            (Value::Str("a".into()), Value::Int(1)),
            (Value::Str("b".into()), Value::Int(2)),
            (Value::Str("a".into()), Value::Int(9)),
        ])
        .unwrap();
        // `Value` isn't `PartialEq` (holds `f64`), so compare keys directly + values via `eq_val`.
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].0, HKey::Str("a".into())); // first position kept
        assert!(m[0].1.eq_val(&Value::Int(9))); // last value taken
        assert_eq!(m[1].0, HKey::Str("b".into()));
        assert!(m[1].1.eq_val(&Value::Int(2)));
    }

    #[test]
    fn build_map_rejects_non_hashable_key() {
        let e = build_map(vec![(Value::Float(1.0), Value::Int(1))]).unwrap_err();
        assert!(e.contains("invalid map key"), "{e}");
    }

    #[test]
    fn map_index_found_and_missing() {
        let m = vec![
            (HKey::Str("x".into()), Value::Int(10)),
            (HKey::Int(2), Value::Str("two".into())),
        ];
        assert!(map_index(&m, &Value::Str("x".into()))
            .unwrap()
            .eq_val(&Value::Int(10)));
        assert!(map_index(&m, &Value::Int(2))
            .unwrap()
            .eq_val(&Value::Str("two".into())));
        match map_index(&m, &Value::Str("missing".into())) {
            Err(e) => assert_eq!(e, "map key not found"),
            Ok(_) => panic!("expected missing-key fault"),
        }
    }

    #[test]
    fn hkey_value_round_trip() {
        for v in [Value::Int(7), Value::Bool(true), Value::Str("k".into())] {
            assert!(HKey::from_value(&v).unwrap().to_value().eq_val(&v));
        }
        assert!(HKey::from_value(&Value::Float(1.0)).is_none());
    }

    #[test]
    fn map_eq_is_order_independent() {
        let a = Value::Map(Rc::new(vec![
            (HKey::Str("a".into()), Value::Int(1)),
            (HKey::Str("b".into()), Value::Int(2)),
        ]));
        let b = Value::Map(Rc::new(vec![
            (HKey::Str("b".into()), Value::Int(2)),
            (HKey::Str("a".into()), Value::Int(1)),
        ]));
        let c = Value::Map(Rc::new(vec![(HKey::Str("a".into()), Value::Int(1))]));
        assert!(a.eq_val(&b)); // same entries, different order → equal
        assert!(!a.eq_val(&c)); // different key set → not equal
    }

    #[test]
    fn int_kernels_fault_and_overflow() {
        assert_eq!(int_add(2, 3), Ok(5));
        assert_eq!(int_add(i64::MAX, 1), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_sub(i64::MIN, 1), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_mul(i64::MAX, 2), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_div(7, 2), Ok(3));
        assert_eq!(int_div(1, 0), Err(FAULT_DIV_ZERO.to_string()));
        assert_eq!(int_div(i64::MIN, -1), Err(FAULT_INT_OVERFLOW.to_string()));
        assert_eq!(int_rem(7, 3), Ok(1));
        assert_eq!(int_rem(1, 0), Err(FAULT_MOD_ZERO.to_string()));
        assert_eq!(int_neg(5), Ok(-5));
        assert_eq!(int_neg(i64::MIN), Err(FAULT_INT_OVERFLOW.to_string()));
    }

    #[test]
    fn compare_ord_matches_both_backends() {
        assert_eq!(
            compare_ord(&Value::Int(1), &Value::Int(2)),
            Ok(Some(Ordering::Less))
        );
        assert_eq!(
            compare_ord(&Value::Float(2.0), &Value::Float(2.0)),
            Ok(Some(Ordering::Equal))
        );
        // NaN: comparable type, but no ordering -> Ok(None) (callers project to `false`).
        assert_eq!(
            compare_ord(&Value::Float(f64::NAN), &Value::Float(1.0)),
            Ok(None)
        );
        // Mixed/non-numeric operands are a comparability fault.
        assert!(compare_ord(&Value::Int(1), &Value::Float(1.0)).is_err());
        assert!(compare_ord(&Value::Bool(true), &Value::Bool(false)).is_err());
    }

    #[test]
    fn as_display_renders_primitives() {
        assert_eq!(Value::Int(42).as_display().as_deref(), Some("42"));
        assert_eq!(Value::Float(12.0).as_display().as_deref(), Some("12"));
        assert_eq!(
            Value::Float(12.56636).as_display().as_deref(),
            Some("12.56636")
        );
        assert_eq!(Value::Bool(true).as_display().as_deref(), Some("true"));
        assert_eq!(Value::Str("hi".into()).as_display().as_deref(), Some("hi"));
    }

    #[test]
    fn as_display_is_none_for_composite() {
        let inst = Value::Instance(Rc::new(Instance::new(
            "Greeter".into(),
            ClassLayout::new(vec![]),
        )));
        assert!(inst.as_display().is_none());
    }

    #[test]
    fn eq_val_terminates_on_a_reference_cycle() {
        // M-mut.6 / F4: build `a.next = b; b.next = a` (a 2-node instance cycle) and assert `eq_val`
        // returns instead of overflowing the native stack. Without the `visited` guard this test
        // aborts the process via stack overflow; with it, it terminates deterministically.
        let layout = ClassLayout::new(vec!["next".into()]);
        let a = Rc::new(Instance::new("Node".into(), layout.clone()));
        let b = Rc::new(Instance::new("Node".into(), layout));
        a.set_field("next", Value::Instance(b.clone()));
        b.set_field("next", Value::Instance(a.clone()));
        let va = Value::Instance(a);
        let vb = Value::Instance(b);
        // The two cyclic nodes are structurally bisimilar ⇒ equal; the call must terminate.
        assert!(va.eq_val(&vb));
        assert!(va.eq_val(&va.clone()));
    }

    #[test]
    fn eq_val_matches_by_value() {
        assert!(Value::Int(1).eq_val(&Value::Int(1)));
        assert!(!Value::Int(1).eq_val(&Value::Int(2)));
        assert!(!Value::Int(1).eq_val(&Value::Float(1.0))); // no cross-type eq
        assert!(Value::Null.eq_val(&Value::Null)); // null == null
        assert!(!Value::Null.eq_val(&Value::Int(0))); // null != a non-null value
        let a = Value::Enum(Rc::new(EnumVal {
            ty: "Shape".into(),
            variant: "Circle".into(),
            payload: vec![Value::Float(2.0)],
        }));
        let b = a.clone();
        assert!(a.eq_val(&b));
    }

    #[test]
    fn type_name_is_stable() {
        assert_eq!(Value::Unit.type_name(), "unit");
        assert_eq!(Value::List(Rc::new(vec![])).type_name(), "list");
        assert_eq!(Value::Set(Rc::new(vec![])).type_name(), "set");
    }

    #[test]
    fn build_set_dedups_first_seen() {
        // First occurrence kept, later duplicates dropped, order preserved (M-RT S7b).
        let s = build_set(vec![
            Value::Int(3),
            Value::Int(1),
            Value::Int(3),
            Value::Int(2),
            Value::Int(1),
        ])
        .unwrap();
        assert_eq!(s, vec![HKey::Int(3), HKey::Int(1), HKey::Int(2)]);
        // a non-hashable element faults cleanly, never panics (EV-7).
        assert!(build_set(vec![Value::Float(1.0)]).is_err());
    }

    #[test]
    fn eq_val_sets_are_order_independent() {
        let a = Value::Set(Rc::new(vec![HKey::Int(1), HKey::Int(2), HKey::Int(3)]));
        let b = Value::Set(Rc::new(vec![HKey::Int(3), HKey::Int(1), HKey::Int(2)]));
        let c = Value::Set(Rc::new(vec![HKey::Int(1), HKey::Int(2)]));
        assert!(a.eq_val(&b)); // same membership, different order
        assert!(!a.eq_val(&c)); // different cardinality
    }
}
