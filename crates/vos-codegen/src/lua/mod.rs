//! Lua 5.1 output for app authors:
//!
//! * `types/generated/ui.lua`: the `ui.*` constructor module that apps load with `require("@agentos/ui")` and the
//!   host embeds. Plain Lua 5.1, no annotations (see [`module`]);
//! * `types/generated/luals/*`: lua-language-server (`LuaLS`) definition files, `---@meta` `LuaCATS`, for the editor
//!   and `bun run lint:lua` (see [`meta`]). Never loaded by the host.
//!
//! `docs/reference/components.md` is the prose reference for the constructors and their props.

mod meta;
mod ui;

pub use meta::{agentos_definitions, app_module_definitions, ui_module_definitions};
pub use ui::{module, ACTION_BUILDERS};

use std::collections::BTreeMap;

use vos_domain::ui::{
    expr::Func,
    spec::{ComponentSpec, EnumDef, ObjectDef, PropSpec, PropType, SlotSpec},
};

use crate::walk::{contains_source, split_group};

/// The Lua keywords, which cannot be bare table keys.
const RESERVED: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// A table key: bare when it is an identifier, `["..."]` otherwise.
pub(crate) fn key(name: &str) -> String {
    let ident = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && !RESERVED.contains(&name);
    if ident {
        name.to_owned()
    } else {
        format!("[\"{name}\"]")
    }
}

/// The slot kinds of a component, as authors write them.
pub(crate) struct Slots<'a> {
    pub(crate) plain: Vec<&'a SlotSpec>,
    pub(crate) groups: BTreeMap<&'a str, Vec<&'a SlotSpec>>,
    pub(crate) prefixes: Vec<&'a SlotSpec>,
}

pub(crate) fn classify<'a>(slots: impl Iterator<Item = &'a SlotSpec>) -> Slots<'a> {
    let mut out = Slots { plain: Vec::new(), groups: BTreeMap::new(), prefixes: Vec::new() };
    for slot in slots {
        if slot.prefix {
            out.prefixes.push(slot);
        } else if let Some((group, _)) = split_group(slot.name) {
            out.groups.entry(group).or_default().push(slot);
        } else {
            out.plain.push(slot);
        }
    }
    out
}

pub(crate) fn group_name(prefix: &str) -> &str {
    prefix.trim_end_matches(':')
}

/// The Lua name of an expression function: `and`/`or`/`not`/`if` are keywords.
pub(crate) fn helper_name(f: Func) -> String {
    match f {
        Func::And => "all".to_owned(),
        Func::Or => "any".to_owned(),
        Func::Not => "negate".to_owned(),
        Func::If => "when".to_owned(),
        other => other.name().strip_prefix("fmt.").unwrap_or(other.name()).to_owned(),
    }
}

/// The one-line comment above a constructor.
pub(crate) fn describe(c: &ComponentSpec) -> String {
    format!("-- {} (Tier {}, since {})", c.doc, c.tier as u8, c.since)
}

/// A string literal type union: `"a"|"b"`.
pub(crate) fn literals(values: &[&str]) -> String {
    values.iter().map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join("|")
}

pub(crate) fn enum_type(def: &EnumDef) -> String {
    if def.name.is_empty() {
        literals(def.values)
    } else {
        def.name.to_owned()
    }
}

/// `T?` for a field or parameter, parenthesised when `T` is a union.
pub(crate) fn optional(ty: &str) -> String {
    if ty.contains('|') {
        format!("({ty})?")
    } else {
        format!("{ty}?")
    }
}

/// The `LuaCATS` type of a value of `ty`, without the `Expr` wrapper.
pub(crate) fn value_type(ty: &PropType) -> String {
    match ty {
        PropType::Str => "string".to_owned(),
        PropType::Number | PropType::Int | PropType::Spacing => "number".to_owned(),
        PropType::Bool => "boolean".to_owned(),
        PropType::Json => "any".to_owned(),
        PropType::Source => "Source".to_owned(),
        PropType::Enum(def) => enum_type(def),
        PropType::Insets => "number|Insets".to_owned(),
        PropType::Frame => "\"fill\"|Frame".to_owned(),
        PropType::List(inner) => {
            let inner = value_type(inner);
            if inner.contains('|') {
                format!("({inner})[]")
            } else {
                format!("{inner}[]")
            }
        }
        PropType::Object(def) if def.name.is_empty() => object_type(def),
        PropType::Object(def) => def.name.to_owned(),
        PropType::Union(options) => options.iter().map(value_type).collect::<Vec<_>>().join("|"),
    }
}

/// The type of a prop as an author writes it: a binding is a ref, a source handle is a source, the rest may be
/// an expression.
pub(crate) fn field_type(prop: &PropSpec) -> String {
    let value = value_type(&prop.ty);
    if prop.binding.is_some() {
        format!("Ref<{}>", if matches!(prop.ty, PropType::Json) { "any" } else { &value })
    } else if contains_source(&prop.ty) {
        // a source id, or a literal list for unions such as `Picker.options`
        value
    } else {
        format!("Expr<{value}>")
    }
}

/// `{ name: T, other?: U }` for an object type.
pub(crate) fn object_type(def: &ObjectDef) -> String {
    let mut fields: Vec<String> = def.fields.iter().map(field_decl).collect();
    if def.open {
        fields.push("[string]: any".to_owned());
    }
    format!("{{ {} }}", fields.join(", "))
}

fn field_decl(field: &PropSpec) -> String {
    let optional = if field.required { "" } else { "?" };
    format!("{}{optional}: {}", key(field.name), field_type(field))
}

/// `{ value: string }` for an event payload (what a handler receives: plain values), `{}` when it carries nothing.
pub(crate) fn payload_type(fields: &[PropSpec]) -> String {
    if fields.is_empty() {
        return "{}".to_owned();
    }
    let declared: Vec<String> = fields
        .iter()
        .map(|f| format!("{}{}: {}", key(f.name), if f.required { "" } else { "?" }, value_type(&f.ty)))
        .collect();
    format!("{{ {} }}", declared.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vos_domain::ui::{
        components::{lookup, PALETTE},
        spec::BindingKind,
    };

    #[test]
    fn keys_are_quoted_when_they_are_not_identifiers_or_are_reserved() {
        assert_eq!(key("title"), "title");
        assert_eq!(key("on_tap"), "on_tap");
        assert_eq!(key("type"), "type", "`type` is a function name, not a keyword, and fine as a field");
        assert_eq!(key("do"), "[\"do\"]");
        assert_eq!(key("end"), "[\"end\"]");
        assert_eq!(key("$ref"), "[\"$ref\"]");
        assert_eq!(key("2x"), "[\"2x\"]");
    }

    #[test]
    fn optional_parenthesises_unions() {
        assert_eq!(optional("string"), "string?");
        assert_eq!(optional("\"a\"|\"b\""), "(\"a\"|\"b\")?");
    }

    #[test]
    fn value_types() {
        use vos_domain::ui::spec::PropType as T;
        assert_eq!(value_type(&T::Str), "string");
        assert_eq!(value_type(&T::Spacing), "number");
        assert_eq!(value_type(&T::Json), "any");
        assert_eq!(value_type(&T::Insets), "number|Insets");
        assert_eq!(value_type(&T::List(&T::Str)), "string[]");
        assert_eq!(value_type(&T::List(&T::Insets)), "(number|Insets)[]");
        assert_eq!(value_type(&T::Enum(&PALETTE)), "PaletteToken");
        assert_eq!(value_type(&T::Union(&[T::Number, T::Str])), "number|string");
    }

    #[test]
    fn a_binding_is_a_ref_and_a_plain_prop_may_be_an_expression() {
        let toggle = lookup("Toggle").unwrap();
        let on = toggle.props.iter().find(|p| p.name == "on").unwrap();
        assert_eq!(on.binding, Some(BindingKind::Scalar));
        assert_eq!(field_type(on), "Ref<boolean>");
        let title = toggle.props.iter().find(|p| p.name == "title").unwrap();
        assert_eq!(field_type(title), "Expr<string>");
    }

    #[test]
    fn event_payloads_are_plain_values() {
        let field = lookup("TextField").unwrap().events.iter().find(|e| e.name == "submit").unwrap();
        assert_eq!(payload_type(field.payload), "{ value: string }");
        assert_eq!(payload_type(&[]), "{}");
    }
}
