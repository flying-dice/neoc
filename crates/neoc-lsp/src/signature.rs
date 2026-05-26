//! Rendering neoc declaration signatures from the `neoc-types` symbol table.
//!
//! Shared by the IDE features that describe an item by name rather than by
//! cursor position — hover at a call site, completion-item resolve, and (later)
//! signature help. Output is neoc-flavoured: `fn fib(number) -> number`,
//! `struct Point { x: number }`, never the host language. Pure functions over
//! the symbol table, free of I/O.

use neoc_types::{
    EnumInfo, StructFields, StructInfo, SymbolTable, TraitInfo, Ty, TypeDef, VariantFields,
};

use crate::cst;

/// The neoc signature string for `name`, drawn from both namespaces.
///
/// Prefers a type-namespace definition (struct/enum/trait/alias) and falls
/// back to a value-namespace entry (fn/const/constructor).
#[must_use]
pub(crate) fn signature(table: &SymbolTable, name: &str) -> Option<String> {
    if let Some(def) = table.types.get(name) {
        return Some(type_signature(name, def));
    }
    if let Some(ty) = table.values.get(name) {
        return Some(value_signature(name, ty));
    }
    None
}

/// The neoc signature for a top-level `name` and its `///` doc comment, for
/// callers that resolve by name rather than cursor position (completion
/// resolve). The signature is the bare type form (`fn inc(number) -> number`),
/// the same rendering hover shows at a call site. `None` when the name is not a
/// known top-level item.
#[must_use]
pub(crate) fn item_signature_and_doc(source: &str, name: &str) -> Option<(String, Option<String>)> {
    let table = neoc_types::collect(&neoc_parser::parse(source).module);
    let sig = signature(&table, name)?;
    let tree = cst::parse(source);
    let doc = cst::find_decl(&tree, source, name)
        .and_then(|decl| cst::leading_doc_comment(decl.node, source));
    Some((sig, doc))
}

fn type_signature(name: &str, def: &TypeDef) -> String {
    match def {
        TypeDef::Struct(s) => struct_signature(name, s),
        TypeDef::Enum(e) => enum_signature(name, e),
        TypeDef::Trait(t) => trait_signature(name, t),
        TypeDef::Alias { generics, value } => {
            format!("type {name}{} = {value}", generic_list(generics))
        }
        TypeDef::Opaque => format!("ext type {name}"),
    }
}

fn struct_signature(name: &str, s: &StructInfo) -> String {
    let head = format!("struct {name}{}", generic_list(&s.generics));
    match &s.fields {
        StructFields::Unit => format!("{head};"),
        StructFields::Tuple(ty) => format!("{head}({ty});"),
        StructFields::Named(fields) => {
            let body = fields
                .iter()
                .map(|(fname, ty)| format!("{fname}: {ty}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{head} {{ {body} }}")
        }
    }
}

fn enum_signature(name: &str, e: &EnumInfo) -> String {
    let head = format!("enum {name}{}", generic_list(&e.generics));
    let variants = e
        .variants
        .iter()
        .map(|v| match &v.fields {
            VariantFields::Unit => v.name.clone(),
            VariantFields::Tuple(tys) => {
                let inner = tys
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}({inner})", v.name)
            }
            VariantFields::Named(fields) => {
                let inner = fields
                    .iter()
                    .map(|(fname, ty)| format!("{fname}: {ty}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{} {{ {inner} }}", v.name)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{head} {{ {variants} }}")
}

fn trait_signature(name: &str, t: &TraitInfo) -> String {
    let head = format!("trait {name}{}", generic_list(&t.generics));
    let methods = t
        .methods
        .iter()
        .map(|m| {
            let params = m
                .params
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let recv = if m.has_receiver { "self" } else { "" };
            let sep = if m.has_receiver && !params.is_empty() {
                ", "
            } else {
                ""
            };
            format!("  fn {}({recv}{sep}{params}) -> {};", m.name, m.ret)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{head} {{\n{methods}\n}}")
}

/// Renders a value-namespace entry. A function type becomes `fn name(...) ->
/// ret`; anything else becomes `name: ty` (consts, bindings).
fn value_signature(name: &str, ty: &Ty) -> String {
    match ty {
        Ty::Fn(params, ret) => {
            let params = params
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("fn {name}({params}) -> {ret}")
        }
        other => format!("{name}: {other}"),
    }
}

fn generic_list(generics: &[String]) -> String {
    if generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", generics.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_renders_as_its_type() {
        let src = "fn inc(n: number) -> number { n }\n";
        let (sig, doc) = item_signature_and_doc(src, "inc").expect("signature");
        assert_eq!(sig, "fn inc(number) -> number");
        assert_eq!(doc, None);
    }

    #[test]
    fn struct_renders_fields_and_carries_doc() {
        let src = "/// A point.\nstruct Point { x: number, y: number }\n";
        let (sig, doc) = item_signature_and_doc(src, "Point").expect("signature");
        assert_eq!(sig, "struct Point { x: number, y: number }");
        assert_eq!(doc.as_deref(), Some("A point."));
    }

    #[test]
    fn unknown_name_has_no_signature() {
        assert_eq!(item_signature_and_doc("fn main() {}\n", "nope"), None);
    }
}
