//! Writes the tree as Luau declarations, the text luau-lsp reads.
//!
//! A section is its marker line and one `export type` per alias. An alias of a table type with properties is
//! written a property to a line, each under its doc (`--- ...`); every other type is written on one line. A
//! reserved word of Luau is a quoted key as a property (`["end"]`, still read as `value["end"]`) and gets a trailing
//! underscore as a parameter.

use std::fmt::Write as _;

use crate::{Decl, Func, Param, Prop, Ret, Section, Ty};

/// Reserved words of Luau: not usable as a parameter name or a bare property name. (`goto` is not one: Luau
/// does not reserve it.)
pub const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// The marker line of a section, the aliases, then the globals.
#[must_use]
pub fn section(section: &Section) -> String {
    let mut out = format!("-- ===== {} =====\n\n", section.name);
    for decl in &section.aliases {
        alias(&mut out, decl);
    }
    for decl in &section.globals {
        doc(&mut out, "", &decl.doc);
        let _ = writeln!(out, "declare {}: {}\n", decl.name, ty(&decl.ty));
    }
    out
}

/// `--- <line>` for every line of a doc, a bare `---` for an empty one.
fn doc(out: &mut String, indent: &str, text: &str) {
    for line in text.lines() {
        if line.is_empty() {
            let _ = writeln!(out, "{indent}---");
        } else {
            let _ = writeln!(out, "{indent}--- {line}");
        }
    }
}

fn alias(out: &mut String, decl: &Decl) {
    doc(out, "", &decl.doc);
    match &decl.ty {
        Ty::Table { props, index: None } if !props.is_empty() => {
            let _ = writeln!(out, "export type {} = {{", decl.name);
            for prop in props {
                doc(out, "\t", &prop.doc);
                let _ = writeln!(out, "\t{}: {},", prop_name(&prop.name), ty(&prop.ty));
            }
            out.push_str("}\n\n");
        }
        other => {
            let _ = writeln!(out, "export type {} = {}\n", decl.name, ty(other));
        }
    }
}

fn prop_name(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("[\"{name}\"]")
    } else {
        name.to_owned()
    }
}

fn param_name(name: &str) -> String {
    if KEYWORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

/// A type on one line.
#[must_use]
pub fn ty(ty: &Ty) -> String {
    match ty {
        Ty::Name(name) | Ty::Str(name) => name.clone(),
        Ty::Bool(value) => value.to_string(),
        Ty::Optional(inner) => format!("{}?", operand(inner, true)),
        Ty::Union(parts) => joined(parts, " | ", false),
        Ty::Inter(parts) => joined(parts, " & ", true),
        Ty::Array(item) => format!("{{ {} }}", self::ty(item)),
        Ty::Table { props, index } => table(props, index.as_deref()),
        Ty::Func(func) => function(func),
    }
}

fn joined(parts: &[Ty], with: &str, group_unions: bool) -> String {
    parts
        .iter()
        .map(|part| operand(part, group_unions))
        .collect::<Vec<_>>()
        .join(with)
}

/// `ty` where one type is read: a function is parenthesised (what follows it would belong to its return), and
/// so is a union or an intersection where it would join what is next to it.
fn operand(inner: &Ty, group_unions: bool) -> String {
    let text = ty(inner);
    match inner {
        Ty::Func(_) => format!("({text})"),
        Ty::Union(_) | Ty::Inter(_) if group_unions => format!("({text})"),
        _ => text,
    }
}

fn table(props: &[Prop], index: Option<&(Ty, Ty)>) -> String {
    let mut fields: Vec<String> = props
        .iter()
        .map(|prop| format!("{}: {}", prop_name(&prop.name), ty(&prop.ty)))
        .collect();
    if let Some((key, value)) = index {
        fields.push(format!("[{}]: {}", ty(key), ty(value)));
    }
    if fields.is_empty() {
        "{}".to_owned()
    } else {
        format!("{{ {} }}", fields.join(", "))
    }
}

fn function(func: &Func) -> String {
    let mut params: Vec<String> = func.params.iter().map(param).collect();
    if let Some(rest) = &func.rest {
        params.push(format!("...{}", operand(rest, true)));
    }
    let generics = if func.generics.is_empty() {
        String::new()
    } else {
        format!("<{}>", func.generics.join(", "))
    };
    format!(
        "{generics}({}) -> {}",
        params.join(", "),
        returns(&func.ret)
    )
}

fn param(param: &Param) -> String {
    match &param.name {
        Some(name) => format!("{}: {}", param_name(name), ty(&param.ty)),
        None => ty(&param.ty),
    }
}

/// What follows `->`: `()`, one type, `(A, B)`, `...T`. One value that is a function is parenthesised.
#[must_use]
pub fn returns(returned: &Ret) -> String {
    match (returned.values.as_slice(), &returned.rest) {
        ([], None) => "()".to_owned(),
        ([one], None) => operand(one, false),
        ([], Some(rest)) => format!("...{}", operand(rest, true)),
        (values, rest) => {
            let mut items: Vec<String> = values.iter().map(|value| operand(value, false)).collect();
            if let Some(rest) = rest {
                items.push(format!("...{}", operand(rest, true)));
            }
            format!("({})", items.join(", "))
        }
    }
}
