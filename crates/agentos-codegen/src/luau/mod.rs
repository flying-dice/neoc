//! Luau output for app authors:
//!
//! * `types/generated/ui.luau`: the `ui.*` constructor module (`require("@agentos/ui")`), with every prop type;
//! * `types/generated/agentos.d.luau`: the definitions file (`declare app: AppApi`) for `luau-lsp`;
//! * `types/generated/app.luau`: a one-line type shim so `require("@agentos/app")` resolves for the type checker
//!   (the host provides the real module natively).
//!
//! Definitions files cannot export types to scripts, so `ui.luau` is self-contained: it declares the shared core
//! types with `export type`, and `agentos.d.luau` carries an identical copy (both are emitted from [`CORE`]).

mod defs;
mod ui;

pub use defs::{app_shim, definitions};
pub use ui::{module, ACTION_BUILDERS};

use agentos_domain::ui::spec::{EnumDef, ObjectDef, PropSpec, PropType};

use crate::walk::contains_source;

/// The types the host and the constructors share. `{E}` is `export ` in `ui.luau` and empty in the definitions.
pub(crate) const CORE: &str = r#"{E}type Ref<T> = { ["$ref"]: string, __phantom: T? }
{E}type Fn = { ["$fn"]: string, args: { any } }
{E}type Expr<T> = T | Ref<T> | Fn
{E}type Source = { ["$source"]: string }
{E}type RowRefs = { [string]: Ref<any> }
{E}type Actor = { name: string, kind: ("human" | "agent") }
{E}type EventArg<P> = { key: any, row: any, value: any, payload: P, actor: Actor }
{E}type Action = { ["do"]: string, [string]: any }
{E}type Handler<P> = ((e: EventArg<P>) -> ()) | Action | { Action | ((e: EventArg<P>) -> ()) }
{E}type Node = {
	type: string,
	key: string?,
	props: { [string]: any }?,
	children: { Node }?,
	slots: { [string]: { Node } }?,
	on: { [string]: any }?,
}
{E}type Child = Node | { Node }
"#;

/// The core types with `{E}` expanded.
pub(crate) fn core(export: bool) -> String {
    CORE.replace("{E}", if export { "export " } else { "" })
}

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

/// A string literal type union: `("a" | "b")`.
pub(crate) fn literals(values: &[&str]) -> String {
    format!("({})", values.iter().map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join(" | "))
}

pub(crate) fn enum_type(def: &EnumDef) -> String {
    if def.name.is_empty() {
        literals(def.values)
    } else {
        def.name.to_owned()
    }
}

/// `T?`, parenthesised when `T` is a union.
pub(crate) fn optional(ty: &str) -> String {
    if ty.contains('|') {
        format!("({ty})?")
    } else {
        format!("{ty}?")
    }
}

/// The Luau type of a value of `ty`, without the `Expr` wrapper.
pub(crate) fn value_type(ty: &PropType) -> String {
    match ty {
        PropType::Str => "string".to_owned(),
        PropType::Number | PropType::Int | PropType::Spacing => "number".to_owned(),
        PropType::Bool => "boolean".to_owned(),
        PropType::Json => "any".to_owned(),
        PropType::Source => "Source".to_owned(),
        PropType::Enum(def) => enum_type(def),
        PropType::Insets => "number | Insets".to_owned(),
        PropType::Frame => "\"fill\" | Frame".to_owned(),
        PropType::List(inner) => format!("{{ {} }}", value_type(inner)),
        PropType::Object(def) if def.name.is_empty() => object_type(def),
        PropType::Object(def) => def.name.to_owned(),
        PropType::Union(options) => options.iter().map(value_type).collect::<Vec<_>>().join(" | "),
    }
}

/// The type of a prop as an author writes it: a binding is a ref, a source handle is a source, the rest may be
/// an expression.
pub(crate) fn field_type(prop: &PropSpec) -> String {
    let value = value_type(&prop.ty);
    if prop.binding.is_some() {
        format!("Ref<{}>", if matches!(prop.ty, PropType::Json) { "any".to_owned() } else { paren(&value) })
    } else if contains_source(&prop.ty) {
        // a source id, or a literal list for unions such as `Picker.options`
        value
    } else {
        format!("Expr<{}>", paren(&value))
    }
}

fn paren(ty: &str) -> String {
    if ty.contains('|') && !ty.starts_with('(') {
        format!("({ty})")
    } else {
        ty.to_owned()
    }
}

/// `{ name: T, other: U? }` for an object type or an event payload.
pub(crate) fn object_type(def: &ObjectDef) -> String {
    let mut fields: Vec<String> = def.fields.iter().map(field_decl).collect();
    if def.open {
        fields.push("[string]: any".to_owned());
    }
    format!("{{ {} }}", fields.join(", "))
}

fn field_decl(field: &PropSpec) -> String {
    let ty = field_type(field);
    format!("{}: {}", key(field.name), if field.required { ty } else { optional(&ty) })
}

/// `{ value: string }` for an event payload (what a handler receives: plain values), `{}` when it carries nothing.
pub(crate) fn payload_type(fields: &[PropSpec]) -> String {
    if fields.is_empty() {
        return "{}".to_owned();
    }
    let declared: Vec<String> = fields
        .iter()
        .map(|f| {
            let ty = value_type(&f.ty);
            format!("{}: {}", key(f.name), if f.required { ty } else { optional(&ty) })
        })
        .collect();
    format!("{{ {} }}", declared.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentos_domain::ui::{
        components::{lookup, PALETTE},
        spec::BindingKind,
    };

    #[test]
    fn keys_are_quoted_when_they_are_not_identifiers_or_are_reserved() {
        assert_eq!(key("title"), "title");
        assert_eq!(key("on_tap"), "on_tap");
        assert_eq!(key("type"), "type", "`type` is a contextual keyword and fine as a field");
        assert_eq!(key("do"), "[\"do\"]");
        assert_eq!(key("end"), "[\"end\"]");
        assert_eq!(key("$ref"), "[\"$ref\"]");
        assert_eq!(key("2x"), "[\"2x\"]");
    }

    #[test]
    fn optional_parenthesises_unions() {
        assert_eq!(optional("string"), "string?");
        assert_eq!(optional("\"a\" | \"b\""), "(\"a\" | \"b\")?");
    }

    #[test]
    fn value_types() {
        use agentos_domain::ui::spec::PropType as T;
        assert_eq!(value_type(&T::Str), "string");
        assert_eq!(value_type(&T::Spacing), "number");
        assert_eq!(value_type(&T::Json), "any");
        assert_eq!(value_type(&T::Insets), "number | Insets");
        assert_eq!(value_type(&T::List(&T::Str)), "{ string }");
        assert_eq!(value_type(&T::Enum(&PALETTE)), "PaletteToken");
        assert_eq!(value_type(&T::Union(&[T::Number, T::Str])), "number | string");
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

    #[test]
    fn the_core_types_expand_the_export_keyword() {
        assert!(core(true).contains("export type Ref<T>"));
        assert!(core(false).starts_with("type Ref<T>"));
        assert!(!core(false).contains("{E}"));
    }
}
