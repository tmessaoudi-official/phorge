# `core.html` — Typed, Auto-Escaping HTML — Design Spec

> **Status:** 🚧 In progress — **Wave 1 (escape kernel) shipped**; builders (Wave 2) + `html"…"`
> sugar (Wave 3) proposed. See §9.
> **Milestone:** M3 ergonomics follow-up / M6 web companion (HTML is what a `Response` body usually
> carries — this is the authoring layer above `examples/web/handler.phg`).
> **Trigger:** developer question (2026-06-19) — *"in a `.phg` file, if I want to write HTML, how do
> I do it, like in PHP?"* — and the locked answer *"all three layered together"* (typed kernel +
> builders + `html"…"` interpolation sugar, one coherent design).
> **Code state at spec time:** master `04f18b6` (extension-policy spec), tree clean, CI green.
> **Supersedes:** the two sibling backlog items are folded in here — *inline-HTML / template syntax*
> becomes **Wave 3** (`html"…"`); *multi-line strings* becomes a **prerequisite** for Wave 3 (§8).

---

## 1. Problem

PHP's headline feature is that a `.php` file *is* an HTML template — you drop out of `<?php ?>` and
type HTML directly. That ergonomics is also PHP's most infamous footgun: `echo "<h1>$name</h1>"` with
an untrusted `$name` is a stored-XSS hole, and the language does nothing to stop it. Escaping is
opt-in (`htmlspecialchars`), so the *unsafe* path is the *short* path.

Phorge's contract is **Phorge : PHP :: TypeScript : JavaScript** — keep the ergonomics, fix the
footgun at the type level. TypeScript didn't make JS templating safe by adding syntax; safety came
from *types*. So the Phorge answer to "how do I write HTML" is not "a string" — it is **a distinct
type `Html` that you cannot produce from untrusted text except through an escaping boundary.** The
unsafe path stops compiling.

Today Phorge has **no** HTML story: a handler builds its body with `bytes`/`string` concatenation
(`examples/web/handler.phg`), which is exactly PHP's unsafe path with extra ceremony.

## 2. Goals / Non-Goals

**Goals**
- **XSS-safe by construction.** Untrusted `string` cannot reach rendered HTML without passing
  `html.text` (auto-escape) or the explicit, greppable `html.raw` (audited trust). Enforced by the
  *checker*, not by discipline.
- **Three layered authoring levels, one design** (decision: all three together):
  1. **Kernel** — the `Html` type + the single escape boundary.
  2. **Builders** — typed element constructors (`html.el`, `html.div`, …) composing `Html`.
  3. **Sugar** — `html"<h1>{name}</h1>"` interpolation that *feels like writing HTML* and
     auto-escapes every hole. This is the "like PHP" layer.
- **Byte-identical** `run ≡ runvm ≡ php`, gated by `tests/differential.rs` via a shipped example.
- **Tier-1 transpile only** — escaping erases to `htmlspecialchars` (Core/`standard`, always
  compiled, survives `php -n`). Honors the extension policy (`2026-06-19-extension-policy-design.md`).

**Non-Goals**
- **Not** a full templating engine (no loops/conditionals *inside* the literal — you use Phorge's own
  `for`/`if`/lambdas/`|>` to build `List<Html>`, then interpolate). The literal interpolates
  *values*, it is not a second language.
- **Not** HTML *parsing*/sanitizing arbitrary markup (that needs a real HTML5 parser → a tier-3
  module later). `html.raw` is trust, not sanitize.
- **Not** a DOM / client-side story. This emits server-rendered HTML strings.
- **Not** CSS/JS-context escaping in v1 (style="" / on*="" / `<script>` bodies need context-specific
  escaping — see §8 open questions; v1 covers text + attribute-value contexts, the 99% case).

## 3. The kernel — `Html` as an erased newtype

`Html` is a **distinct checker type** (`types::Ty::Html`) that **erases to PHP `string`** —
structurally identical to how `bytes` is a distinct `Ty` erasing to PHP `string` (M6 W0). There is
**no new AST variant**: a type annotation is `ast::Type::Named { name: "Html" }` (the parser already
produces it for any name), and the checker maps `"Html" → Ty::Html` alongside `int`/`string`/`bytes`
— so the surface change is checker-only. At runtime an `Html` value is carried as a `Value::Str` (no new `Value` variant —
the safety lives entirely in the *type*, which the checker erases before the backends run, exactly
like type aliases and `bytes`). This means **zero new `Op`**, zero VM/interpreter divergence surface:
the kernel is pure `native.rs` + checker + transpiler.

The whole safety property reduces to one rule the checker enforces:

> **`string` is not assignable to `Html`, and `Html` is not assignable to `string`.** The only bridges
> are the named natives below.

So `html.div([], [user_input])` is a **type error** (`user_input : string`, builder wants `Html`
children); you must write `html.div([], [html.text(user_input)])`. The footgun does not compile.

### 3.1 Boundary natives (`core.html`)

| Native | Signature | Meaning | PHP emission (tier-1) |
|--------|-----------|---------|-----------------------|
| `html.text` | `(string) -> Html` | **Lift untrusted text in, auto-escaped.** The safe boundary. | `htmlspecialchars({a}, ENT_QUOTES, 'UTF-8')` |
| `html.raw` | `(string) -> Html` | **Audited trust opt-out** — caller asserts the string is already safe markup. Greppable (`grep html.raw`). | `({a})` (identity) |
| `html.render` | `(Html) -> string` | **Exit boundary** — turn finished `Html` into a `string` for output. | `({a})` (identity — `Html` is already a string at runtime) |
| `html.concat` | `(List<Html>) -> Html` | Join a list of `Html` fragments (the builders' primitive). | `implode('', {a})` |

`html.text`/`html.raw`/`html.render` are runtime *identity-or-escape* on a `Value::Str`; `concat`
joins. All four are pure (ignore the output buffer), single-sourced `eval` shared by both backends.

### 3.2 The escaping table — THE byte-identity invariant

`html.text`'s Rust `eval` and its PHP emission **must produce byte-identical output.** This is the
single highest-risk point in the whole feature (a one-character divergence breaks the spine). The
spec pins it exactly:

- **PHP side:** always emit `htmlspecialchars($s, ENT_QUOTES, 'UTF-8')` — flags pinned, never the
  bare default (PHP's default flags have changed across versions; pinning makes the output
  version-stable and `php -n`-safe).
- **Rust side (`eval`):** replicate that *exact* five-character replacement table, in this order:

  | char | replacement |
  |------|-------------|
  | `&`  | `&amp;`  |
  | `<`  | `&lt;`   |
  | `>`  | `&gt;`   |
  | `"`  | `&quot;` |
  | `'`  | `&#039;` |

  `&` **must be replaced first** (otherwise the `&` it inserts gets double-escaped). Inputs are valid
  UTF-8 (Phorge strings are UTF-8), so `htmlspecialchars`' invalid-byte handling never triggers — no
  divergence there (noted in §8). A unit test asserts the Rust table equals `php -n`'s
  `htmlspecialchars($s, ENT_QUOTES, 'UTF-8')` over an adversarial fixture (`& < > " ' <script>` …).

## 4. Builders — composing `Html`

Two kernel constructors cover all of HTML; named helpers are sugar over them.

| Native | Signature | PHP emission |
|--------|-----------|--------------|
| `html.el` | `(string tag, List<Attr>, List<Html>) -> Html` | `'<'.$tag.attrs.'>'.implode('',$children).'</'.$tag.'>'` |
| `html.void_el` | `(string tag, List<Attr>) -> Html` | `'<'.$tag.attrs.'/>'` (br, img, input, hr, meta…) |
| `html.attr` | `(string name, string value) -> Attr` | ` $name="htmlspecialchars($value, ENT_QUOTES, 'UTF-8')"` |
| `html.bool_attr` | `(string name) -> Attr` | ` $name` (disabled, checked, required…) |

`Attr` is a second erased newtype (`Ty::Attr` → PHP `string`), so an attribute value is *also*
auto-escaped and you cannot smuggle a raw string into the attribute position. `tag`/`name` are
author-supplied literals (trusted); only *values* and *children* carry untrusted data, and both have
escaping boundaries.

**Named convenience set (Wave 2)** — thin wrappers, each one `html.el`/`html.void_el` with the tag
baked: `div p span a h1 h2 h3 ul ol li table tr td section header footer nav button label` +
void `br img input hr`. These are *Phorge `package main` functions in the `core.html` module's own
`.phg`? No* — they are native registry entries (consistent with the rest of `core.*`), so they erase
the same way and need no stdlib-in-Phorge bootstrapping.

### 4.1 Worked example (kernel + builders)

```phorge
package main;
import core.html;
import core.console;

fn card(string title, string body) -> Html {
  return html.div(
    [html.attr("class", "card")],
    [ html.el("h2", [], [html.text(title)]),
      html.el("p",  [], [html.text(body)]) ]
  );
}

fn main() {
  var page = card("Tom & \"Jerry\"", "<script>alert(1)</script>");
  console.println(html.render(page));
}
// → <div class="card"><h2>Tom &amp; &quot;Jerry&quot;</h2><p>&lt;script&gt;alert(1)&lt;/script&gt;</p></div>
//   identical on run / runvm / real PHP.
```

## 5. Sugar — `html"…"` interpolation (the "like PHP" layer)

A new **prefixed string literal** `html"…"`, lexed like `b"…"` (a dedicated scanner — `scan_html`,
mirroring `scan_bytes`), then **desugared in the parser** into kernel calls. No new `Op`, no new
runtime: after desugaring the AST contains only `html.raw`/`html.text`/`html.concat` calls, so all
three backends and the byte-identity gate see ordinary native calls.

**Desugaring rule** (`html"…"` with literal chunks `Lᵢ` and holes `{eᵢ}`):

```
html"<h1>{name}</h1>"
⇓  (parser)
html.concat([ html.raw("<h1>"), HOLE(name), html.raw("</h1>") ])
```

- Literal chunks → `html.raw(chunk)` (author-written markup is trusted by definition).
- Each hole `{e}` → **`HOLE(e)`** resolved *by the hole's type*, in the checker:
  - `e : Html` → embedded directly (already safe — lets you nest builders / other `html"…"`).
  - `e : string` → wrapped `html.text(e)` (auto-escaped — the safe default for raw data).
  - `e : int`/`float`/`bool` → `html.text(to_string(e))` (escaped; numbers are safe but go through the
    same path for uniformity).
  - any other type → **compile error** `E-HTML-HOLE` ("cannot interpolate `<T>` into html; render it
    to a string or Html first").

This is the crucial safety point: **the default hole behavior is escape.** To inject trusted markup
you must *visibly* write `{html.raw(x)}`. Unsafe is long; safe is short — the inverse of PHP.

```phorge
var name = user_input();                 // untrusted string
var rows = items |> map(render_row);      // List<Html> (built with builders/html"…")
var page = html"
  <section class=\"profile\">
    <h1>{name}</h1>                        // escaped
    <ul>{html.concat(rows)}</ul>          // Html, embedded
    {html.raw(trusted_footer)}            // explicit, audited
  </section>
";
console.println(html.render(page));
```

`{` / `}` escaping inside the literal follows the same convention chosen for regular interpolation
(`"{...}"`); `\"` escapes a quote (as in the example). Attribute *values* written as literals inside
`html"…"` are author-trusted (part of the markup); to put untrusted data in an attribute you
interpolate a hole *inside the quotes*: `<a href=\"{url}\">` → the `{url}` hole escapes via
`html.text` in attribute context (§8 notes the attribute-vs-text escaping nuance — both are covered
by `htmlspecialchars(…, ENT_QUOTES)`, so v1 uses one escaper for both).

## 6. Why this shape (challenged alternatives)

| Alternative | Why rejected |
|-------------|--------------|
| `Html` = plain `string` (no newtype) | No compile-time safety — collapses to PHP's footgun. The entire value of the feature is the type wall. |
| New `Value::Html` runtime variant | Pointless runtime cost + a new divergence surface across interpreter/VM. The property is static; erase it like `bytes`. Rejected. |
| Sugar-only (`html"…"`, no kernel) | Can't compose programmatically (build a `List<Html>` in a loop, factor a `card()` helper). Templating-in-strings is exactly PHP's dead-end. The kernel is what makes it a *library*. |
| Kernel-only (no sugar) | Verbose for real pages — the developer explicitly asked for the "like PHP" feel. Sugar is the payoff; kernel is the foundation. Ship both, kernel first. |
| Builders as Phorge `.phg` stdlib | Phorge has no stdlib-in-Phorge bootstrap; every `core.*` is native-registry. Stay consistent — native entries erase cleanly and need no loader bootstrap. |

## 7. Implementation waves (kernel first — sugar last)

> Each wave ends green (`cargo test` + `PHORGE_REQUIRE_PHP=1`), clippy + fmt clean, and ships its
> example in the same change (developer rule: examples ship with features).

- **Wave 1 — kernel.** `Type::Html`/`Ty::Html` + checker assignability wall; `core.html` natives
  `text`/`raw`/`render`/`concat` with the pinned escaping table (§3.2) + the Rust↔`htmlspecialchars`
  byte-identity unit test. Example: `examples/guide/html.phg`. **No `Op`, no lexer/parser change.**
- **Wave 2 — builders.** `Ty::Attr`; `el`/`void_el`/`attr`/`bool_attr` + the named convenience set.
  Extend `examples/guide/html.phg` (or `examples/web/`) to render a real page; rewrite
  `examples/web/handler.phg`'s body construction to use `core.html` (dogfood). Still no syntax change.
- **Wave 3 — sugar.** `html"…"` prefixed literal: `scan_html` in the lexer (mirror `scan_bytes`) +
  parser desugar to kernel calls + the typed `HOLE` resolution + `E-HTML-HOLE`. Multi-line spanning
  comes free (§8 — `"…"` already accepts raw newlines). Example: a `html"…"` page byte-identical on
  all three backends.

Risk is strictly increasing across waves and each wave is independently shippable — if Wave 3's lexer
change proves thorny, Waves 1–2 already deliver safe HTML.

## 8. Open questions / deferrals

- **Multi-line strings — already satisfied.** `html"…"` is only ergonomic if it can span lines.
  [Verified: `src/lexer.rs:180` `scan_string` pushes a literal newline via the `Some(other) =>
  bytes.push(other)` arm — there is no newline-terminates-string check, so ordinary `"…"` (and the
  `b"…"` scanner it mirrors) already accept raw newlines and span lines.] `scan_html` inherits this
  for free → **multi-line is not a Wave 3 blocker.** This retires the *multi-line strings* backlog
  item for the basic (raw-newline) case; only *named heredoc delimiters* would be genuinely new, and
  that is optional polish, not a dependency.
- **Context-specific escaping.** v1 uses one escaper (`htmlspecialchars`, ENT_QUOTES) for both text
  and attribute-value contexts — correct for those two. URL context (`href="{url}"` with a
  `javascript:` URL), CSS context, and `<script>` bodies need *different* escaping and are **not
  safe** under v1's single escaper. v1 scope is text + attribute value; a later wave can add
  `html.url_attr`/typed URL values. Documented as a KNOWN_ISSUES limitation, not a silent gap.
- **`html.raw` audit story.** Trust opt-out is greppable by design; consider a `W-HTML-RAW` lint
  (like `W-FORCE-UNWRAP`) once the warning channel is proven worth extending. Deferred.
- **Invalid UTF-8.** `htmlspecialchars` with a malformed string + `ENT_SUBSTITUTE` differs from naive
  Rust replacement — but Phorge `string` is always valid UTF-8, so the case is unreachable. Noted so
  a future `bytes`-to-html bridge re-examines it.
- **`core.list` dependency.** Builders take `List<Html>`; `html.concat` consumes one. List literals
  exist; `map`/`filter` over them (for `items |> map(render_row)`) need `core.list` (deferred for
  `List<T>`-generic natives / S3 lambdas). The kernel works without it (explicit list literals);
  the ergonomic `|> map` example lands when `core.list` does.

## 9. What's in force vs proposed

| Piece | State |
|-------|-------|
| This design | ✅ spec landed |
| `Html` type + escape kernel `text`/`raw`/`render` (Wave 1) | ✅ shipped — byte-identical run/runvm/PHP; `examples/guide/html.phg` |
| `Attr` + builders + named set + `concat` (Wave 2) | 🔲 proposed |
| `html"…"` sugar + `E-HTML-HOLE` (Wave 3) | 🔲 proposed |
| Multi-line string literals | ✅ already supported (`scan_string` accepts raw newlines) |
