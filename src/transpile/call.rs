//! PHP transpiler — call (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl Transpiler {
    pub(super) fn emit_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<String, String> {
        if let Expr::Ident(name, _) = callee {
            // Fault intrinsics (M-faults 2a) → PHP exceptions (a `throw` expression, PHP 8.0+). The
            // fault text is single-sourced on `FaultMsg::message` so it reads identically to the
            // backends (panics aren't runnable examples, so this isn't oracle-compared, but it stays
            // valid, faithful PHP).
            use crate::chunk::FaultMsg;
            match name.as_str() {
                "panic" => {
                    let m = FaultMsg::Panic(lit_arg(args.first())).message();
                    return Ok(format!(
                        "throw new \\RuntimeException(\"{}\")",
                        php_escape(&m)
                    ));
                }
                "todo" => {
                    return Ok(format!(
                        "throw new \\RuntimeException(\"{}\")",
                        php_escape(&FaultMsg::Todo.message())
                    ));
                }
                "unreachable" => {
                    return Ok(format!(
                        "throw new \\LogicException(\"{}\")",
                        php_escape(&FaultMsg::Unreachable.message())
                    ));
                }
                "assert" => {
                    let c = self.emit_expr(&args[0])?;
                    let m = FaultMsg::Assert(lit_arg(args.get(1))).message();
                    return Ok(format!(
                        "({c} ? null : throw new \\RuntimeException(\"{}\"))",
                        php_escape(&m)
                    ));
                }
                _ => {}
            }
            let argv = self.emit_args(args)?;
            // Enum variant or class construction → `new`; mirrors the evaluator's dispatch. A
            // cross-package class name is mangled (FQN); a variant subclass lives in its enum's
            // namespace, so a cross-package variant is constructed fully-qualified too.
            if self.variants.contains(name) {
                return Ok(format!("new {}({argv})", self.variant_ref(name)));
            }
            if self.classes.contains(name) {
                return Ok(format!("new {}({argv})", php_type_ref(name)));
            }
            // A closure stored in a local variable (e.g. a `\Closure` parameter or a `var`-bound
            // lambda) must be called as `$f(…)` — PHP requires the `$` sigil on variable-call sites.
            if self.is_local(name) {
                return Ok(format!("${name}({argv})"));
            }
            // A resolved cross-package call carries a mangled (`\`-bearing) name → emit it
            // fully-qualified (leading `\`). A bare name (same-`Main`-namespace call) stays bare.
            if self.namespaced && name.contains('\\') {
                return Ok(format!("\\{name}({argv})"));
            }
            return Ok(format!("{name}({argv})")); // free function
        }
        if let Expr::Member { .. } = callee {
            return self.emit_member_call(callee, args);
        }
        // A lambda literal OR any general expression that evaluates to a function value — `adder()(x)`
        // (call a returned closure), `fns[i](x)`, `(c ? f : g)(x)`. PHP invokes a callable value with
        // `(<expr>)(args)`. The checker has verified the callee is function-typed; mirrors the VM's
        // `CallValue` path and the interpreter, so all three backends agree.
        let f = self.emit_expr(callee)?;
        let argv = self.emit_args(args)?;
        Ok(format!("({f})({argv})"))
    }

    pub(super) fn emit_args(&mut self, args: &[Expr]) -> Result<String, String> {
        let parts: Result<Vec<_>, _> = args.iter().map(|a| self.emit_expr(a)).collect();
        Ok(parts?.join(", "))
    }

    pub(super) fn emit_member_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
    ) -> Result<String, String> {
        if let Expr::Member {
            object, name, safe, ..
        } = callee
        {
            // Namespaced native call: `console.println(x)` → the native's PHP erasure (M3 Wave 1).
            // Resolved through the import map (the transpiler has no variable scope to tell a
            // qualifier from a value; the checker rejects a local shadowing an imported qualifier,
            // so a same-spelled value receiver is impossible).
            if !*safe {
                if let Expr::Ident(q, _) = &**object {
                    if let Some(idx) = self
                        .imports
                        .get(q)
                        .and_then(|m| crate::native::index_of(m, name))
                    {
                        let argv: Vec<String> = args
                            .iter()
                            .map(|a| self.emit_expr(a))
                            .collect::<Result<_, _>>()?;
                        // `Reflect.kind` emits the gated `__phorge_kind` helper; a native's `php`
                        // closure has no `&mut self` to set the flag, so set it here (the established
                        // gated-helper pattern — see `emit_runtime_helpers`).
                        let nat = &crate::native::registry()[idx];
                        if nat.module == "Core.Reflect" {
                            match nat.name {
                                "kind" => self.uses_reflect_kind = true,
                                "className" => self.uses_reflect_class_name = true,
                                "interfaces" | "parents" | "methods" | "fields" => {
                                    self.uses_reflect_tables = true;
                                }
                                _ => {}
                            }
                        }
                        if nat.module == "Core.Json" {
                            match nat.name {
                                // `stringifyPretty` reuses `__phorge_json_encode` for scalars/empties,
                                // so it gates both the pretty and the compact helper.
                                "stringify" => self.uses_json_encode = true,
                                "stringifyPretty" => {
                                    self.uses_json_pretty = true;
                                    self.uses_json_encode = true;
                                }
                                "parse" => self.uses_json_decode = true,
                                _ => {}
                            }
                        }
                        if nat.module == "Core.Text" && nat.name == "parseInt" {
                            self.uses_text_parse_int = true;
                        }
                        if nat.module == "Core.List" {
                            match nat.name {
                                "sort" => self.uses_list_sort = true,
                                "sortWith" => self.uses_list_sort_with = true,
                                _ => {}
                            }
                        }
                        if nat.module == "Core.Map" {
                            match nat.name {
                                "set" => self.uses_map_set = true,
                                "remove" => self.uses_map_remove = true,
                                _ => {}
                            }
                        }
                        // `Convert.toString` erases to the existing `__phorge_str` helper — gate it.
                        if nat.module == "Core.Convert" && nat.name == "toString" {
                            self.uses_str = true;
                        }
                        let php = (nat.php)(&argv);
                        // Inside a namespace block a bare `strlen(...)` would resolve to
                        // `CurrentNs\strlen`; emit `\strlen(...)` for global-function natives (M5-8).
                        return Ok(if self.namespaced && looks_like_global_call(&php) {
                            format!("\\{php}")
                        } else {
                            php
                        });
                    }
                }
            }
            let o = self.emit_expr(object)?;
            let a = self.emit_args(args)?;
            let arrow = if *safe { "?->" } else { "->" };
            return Ok(format!("{o}{arrow}{name}({a})"));
        }
        Err("transpile error: bad member call".into())
    }
}
