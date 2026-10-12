//! The value-namespace names a module binds.
//!
//! A type's runtime table is a Lua local named like the type. Any value binding
//! spelled the same (`let`, a parameter, a pattern, a closure parameter, a
//! module-level `const` / `fn`) would capture it, so the emitter binds such a
//! type's table to a fresh local instead (`lang/DYNAMIC.md` §3.7). This module
//! finds those value names. The result over-approximates: a name bound anywhere
//! in the module counts, whatever its scope.

use std::collections::HashSet;

use neoc_ast as ast;

/// Every name `module` binds in the value namespace, at any depth.
pub(crate) fn value_binders(module: &ast::Module) -> HashSet<String> {
    let mut out = HashSet::new();
    for item in &module.items {
        item_binders(item, &mut out);
    }
    out
}

fn item_binders(item: &ast::Item, out: &mut HashSet<String>) {
    match &item.kind {
        ast::ItemKind::Fn(f) => {
            out.insert(f.sig.name.clone());
            fn_binders(&f.sig, Some(&f.body), out);
        }
        ast::ItemKind::Const(c) => {
            out.insert(c.name.clone());
            expr_binders(&c.value, out);
        }
        ast::ItemKind::Impl(b) => {
            for m in &b.items {
                fn_binders(&m.sig, Some(&m.body), out);
            }
        }
        ast::ItemKind::Trait(t) => {
            for ti in &t.items {
                fn_binders(&ti.sig, ti.default.as_ref(), out);
            }
        }
        ast::ItemKind::Use(_)
        | ast::ItemKind::TypeAlias(_)
        | ast::ItemKind::Struct(_)
        | ast::ItemKind::Enum(_)
        | ast::ItemKind::Extern(_) => {}
    }
}

fn fn_binders(sig: &ast::FnSig, body: Option<&ast::Block>, out: &mut HashSet<String>) {
    for p in &sig.params {
        if let ast::Param::Named { name, .. } = p {
            out.insert(name.clone());
        }
    }
    if let Some(body) = body {
        block_binders(body, out);
    }
}

fn block_binders(block: &ast::Block, out: &mut HashSet<String>) {
    for stmt in &block.stmts {
        match &stmt.kind {
            ast::StmtKind::Let(l) => {
                pattern_binders(&l.pattern, out);
                if let Some(v) = &l.value {
                    expr_binders(v, out);
                }
            }
            ast::StmtKind::Item(item) => item_binders(item, out),
            ast::StmtKind::Expr(e) => expr_binders(e, out),
        }
    }
    if let Some(v) = &block.value {
        expr_binders(v, out);
    }
}

fn pattern_binders(p: &ast::Pattern, out: &mut HashSet<String>) {
    match p {
        ast::Pattern::Binding { name, .. } => {
            out.insert(name.clone());
        }
        ast::Pattern::Struct { fields, .. } => {
            for f in fields {
                match &f.pattern {
                    Some(inner) => pattern_binders(inner, out),
                    None => {
                        out.insert(f.name.clone());
                    }
                }
            }
        }
        ast::Pattern::Tuple { elems, .. } | ast::Pattern::Or(elems) => {
            for e in elems {
                pattern_binders(e, out);
            }
        }
        ast::Pattern::Wildcard | ast::Pattern::Literal(_) | ast::Pattern::Path(_) => {}
    }
}

fn expr_binders(expr: &ast::Expr, out: &mut HashSet<String>) {
    use ast::ExprKind::{
        Assign, Binary, Block, Bool, Break, Call, Closure, Continue, Field, Float, For, If, Index,
        Int, List, Loop, Match, MethodCall, Path, Range, Return, Str, StructLit, Try, Unary, Unit,
        While,
    };
    match &expr.kind {
        Binary { lhs, rhs, .. } => {
            expr_binders(lhs, out);
            expr_binders(rhs, out);
        }
        Unary { operand, .. } | Try(operand) => expr_binders(operand, out),
        Range { start, end, .. } => {
            expr_binders(start, out);
            expr_binders(end, out);
        }
        Call { callee, args } => {
            expr_binders(callee, out);
            for a in args {
                expr_binders(a, out);
            }
        }
        MethodCall { receiver, args, .. } => {
            expr_binders(receiver, out);
            for a in args {
                expr_binders(a, out);
            }
        }
        Field { base, .. } => expr_binders(base, out),
        Index { base, index } => {
            expr_binders(base, out);
            expr_binders(index, out);
        }
        StructLit { fields, base, .. } => {
            for v in fields.iter().filter_map(|f| f.value.as_ref()) {
                expr_binders(v, out);
            }
            if let Some(b) = base {
                expr_binders(b, out);
            }
        }
        List(items) => {
            for x in items {
                expr_binders(x, out);
            }
        }
        Assign { target, value, .. } => {
            expr_binders(target, out);
            expr_binders(value, out);
        }
        If { cond, then, else_ } => {
            expr_binders(cond, out);
            block_binders(then, out);
            if let Some(e) = else_ {
                expr_binders(e, out);
            }
        }
        Match { scrutinee, arms } => {
            expr_binders(scrutinee, out);
            for arm in arms {
                pattern_binders(&arm.pattern, out);
                if let Some(g) = &arm.guard {
                    expr_binders(g, out);
                }
                expr_binders(&arm.body, out);
            }
        }
        While { cond, body } => {
            expr_binders(cond, out);
            block_binders(body, out);
        }
        For {
            pattern,
            iter,
            body,
        } => {
            pattern_binders(pattern, out);
            expr_binders(iter, out);
            block_binders(body, out);
        }
        Loop { body } => block_binders(body, out),
        Closure { params, body } => {
            out.extend(params.iter().map(|p| p.name.clone()));
            expr_binders(body, out);
        }
        Block(b) => block_binders(b, out),
        Return(e) => {
            if let Some(e) = e {
                expr_binders(e, out);
            }
        }
        Path(_) | Int(_) | Float(_) | Str(_) | Bool(_) | Unit | Break | Continue => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binders(src: &str) -> HashSet<String> {
        let parsed = neoc_parser::parse(src);
        assert!(parsed.ok(), "{:?}", parsed.diagnostics);
        value_binders(&parsed.module)
    }

    fn has(set: &HashSet<String>, names: &[&str]) {
        for n in names {
            assert!(set.contains(*n), "missing `{n}` in {set:?}");
        }
    }

    // `lang/DYNAMIC.md` §3.7: every value-binding form is found.
    #[test]
    fn finds_every_binding_form() {
        let set = binders(
            "const C: number = 1;\n\
             fn top(p: number) -> number {\n\
                 let l = 1;\n\
                 let f = |c: number| c;\n\
                 fn inner(ip: number) -> number { ip }\n\
                 for i in 0..3 { let in_loop = i; }\n\
                 match Some(1) { Some(m) => m, None => 0 }\n\
             }\n\
             struct S { a: number }\n\
             impl S { fn meth(self, mp: number) -> number { mp } }\n\
             trait T { fn d(tp: number) -> number { tp } }",
        );
        has(
            &set,
            &[
                "C", "top", "p", "l", "f", "c", "inner", "ip", "i", "in_loop", "m", "mp", "tp",
            ],
        );
    }

    #[test]
    fn struct_pattern_shorthand_and_nested_bind() {
        let set = binders(
            "struct S { a: number, b: number }\n\
             fn g(s: S) -> number { let S { a, b: bb } = s; a + bb }",
        );
        has(&set, &["a", "bb", "g", "s"]);
        assert!(!set.contains("b"), "{set:?}");
    }

    #[test]
    fn type_and_field_names_are_not_binders() {
        let set = binders("struct Hue { r: number }\nfn f() -> Hue { Hue { r: 1 } }");
        assert!(!set.contains("Hue") && !set.contains("r"), "{set:?}");
    }
}
