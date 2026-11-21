//! The Luau declarations for app authors: one definitions file, `types/generated/vos.d.luau`, for luau-lsp (the
//! editor, and `bun run lint:luau`). It is never loaded by the host.
//!
//! The file holds, in this order:
//!
//! 1. the declarations of the `std:`, `lib:` and `vnd:` modules an app is offered, taken from neoc's own generated
//!    file (`types/neoc.d.luau`) with the modules in [`vos_domain::NOT_FOR_APPS`] left out ([`neoc`]);
//! 2. vos's types, named `vos_<Name>` (a definitions file has one flat namespace, and neoc names its types
//!    `<ns>_<module>_<Type>` for the same reason): the shared core, the component enums and objects, `ClientDecl`
//!    (from the JSON Schema of the Rust type), the `app` API ([`defs`]), every component's props and the `ui` module
//!    ([`ui`]);
//! 3. the globals: `app`, and the ones an app's environment does not have ([`vos_domain::GLOBALS_NOT_FOR_APPS`])
//!    typed as unusable;
//! 4. the ONE `declare require`. Two `declare require` declarations do not merge (the last one wins), so the
//!    overloads of neoc's modules and vos's are written together here.
//!
//! A module name has a colon, which is not portable in a file name, so nothing here is a file per module.

pub mod defs;
pub mod neoc;
mod schema;
mod ui;

use vos_domain::ui::spec::{EnumDef, ObjectDef, PropSpec, PropType};

use crate::walk::contains_source;

/// The prefix of every type vos declares.
pub(crate) const PREFIX: &str = "vos_";

/// A field name in a table type: bare when it is an identifier, `["..."]` otherwise (Luau's keywords are Lua's).
pub(crate) use crate::lua::key;

/// The declared name of a vos type.
pub(crate) fn named(name: &str) -> String {
    format!("{PREFIX}{name}")
}

/// A string literal type union: `"a" | "b"`.
pub(crate) fn literals(values: &[&str]) -> String {
    values.iter().map(|v| format!("\"{v}\"")).collect::<Vec<_>>().join(" | ")
}

pub(crate) fn enum_type(def: &EnumDef) -> String {
    if def.name.is_empty() {
        literals(def.values)
    } else {
        named(def.name)
    }
}

/// `T?` for a field or parameter, parenthesised when `T` is a union or a function type.
pub(crate) fn optional(ty: &str) -> String {
    if ty.contains('|') || ty.contains("->") {
        format!("({ty})?")
    } else {
        format!("{ty}?")
    }
}

/// The type of a value of `ty`, without the `Expr` wrapper.
pub(crate) fn value_type(ty: &PropType) -> String {
    match ty {
        PropType::Str => "string".to_owned(),
        PropType::Number | PropType::Int | PropType::Spacing => "number".to_owned(),
        PropType::Bool => "boolean".to_owned(),
        PropType::Json => "any".to_owned(),
        PropType::Source => named("Source"),
        PropType::Enum(def) => enum_type(def),
        PropType::Insets => format!("number | {}", named("Insets")),
        PropType::Frame => format!("\"fill\" | {}", named("Frame")),
        PropType::List(inner) => format!("{{ {} }}", value_type(inner)),
        PropType::Object(def) if def.name.is_empty() => object_type(def),
        PropType::Object(def) => named(def.name),
        PropType::Union(options) => options.iter().map(value_type).collect::<Vec<_>>().join(" | "),
    }
}

/// The type of a prop as an author writes it: a binding is a ref, a source handle is a source, the rest may be
/// an expression.
pub(crate) fn field_type(prop: &PropSpec) -> String {
    let value = value_type(&prop.ty);
    if prop.binding.is_some() {
        format!("{}<{}>", named("Ref"), if matches!(prop.ty, PropType::Json) { "any" } else { &value })
    } else if contains_source(&prop.ty) {
        // a source id, or a literal list for unions such as `Picker.options`
        value
    } else {
        format!("{}<{value}>", named("Expr"))
    }
}

/// `{ name: T, other: U? }` for an object type.
pub(crate) fn object_type(def: &ObjectDef) -> String {
    let mut fields: Vec<String> = def.fields.iter().map(field_decl).collect();
    if def.open {
        fields.push("[string]: any".to_owned());
    }
    format!("{{ {} }}", fields.join(", "))
}

fn field_decl(field: &PropSpec) -> String {
    let ty = field_type(field);
    let ty = if field.required { ty } else { optional(&ty) };
    format!("{}: {ty}", key(field.name))
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
    use vos_domain::ui::{
        components::{lookup, PALETTE},
        spec::BindingKind,
    };

    #[test]
    fn optional_parenthesises_unions_and_functions() {
        assert_eq!(optional("string"), "string?");
        assert_eq!(optional("\"a\" | \"b\""), "(\"a\" | \"b\")?");
        assert_eq!(optional("() -> ()"), "(() -> ())?");
    }

    #[test]
    fn value_types() {
        use vos_domain::ui::spec::PropType as T;
        assert_eq!(value_type(&T::Str), "string");
        assert_eq!(value_type(&T::Spacing), "number");
        assert_eq!(value_type(&T::Json), "any");
        assert_eq!(value_type(&T::Insets), "number | vos_Insets");
        assert_eq!(value_type(&T::List(&T::Str)), "{ string }");
        assert_eq!(value_type(&T::Enum(&PALETTE)), "vos_PaletteToken");
        assert_eq!(value_type(&T::Union(&[T::Number, T::Str])), "number | string");
    }

    #[test]
    fn a_binding_is_a_ref_and_a_plain_prop_may_be_an_expression() {
        let toggle = lookup("Toggle").unwrap();
        let on = toggle.props.iter().find(|p| p.name == "on").unwrap();
        assert_eq!(on.binding, Some(BindingKind::Scalar));
        assert_eq!(field_type(on), "vos_Ref<boolean>");
        let title = toggle.props.iter().find(|p| p.name == "title").unwrap();
        assert_eq!(field_type(title), "vos_Expr<string>");
    }

    #[test]
    fn event_payloads_are_plain_values() {
        let field = lookup("TextField").unwrap().events.iter().find(|e| e.name == "submit").unwrap();
        assert_eq!(payload_type(field.payload), "{ value: string }");
        assert_eq!(payload_type(&[]), "{}");
    }
}
