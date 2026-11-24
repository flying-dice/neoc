//! What a script on this adapter can reach, as data: the schema mlua-ir builds from the Rust of every module this
//! crate links (neoc-vm2's `std:`, `lib:` and `vnd:` modules, and the `app` API, `vos:app`). The declarations for
//! app authors (`types/generated/vos.d.luau`, `vos.d.ts`) are written from it by `vos-codegen`, which adds the
//! host's policy: which of the modules an app is offered.

use crate::api;
use mlua_ir::ir::Ir;
use mlua_ir::{Registry, Shape};

/// The module the `app` API is: `require("vos:app")`, and the global `app`.
pub const APP_MODULE: &str = api::MODULE;

/// The global the `app` API also is.
pub const APP_GLOBAL: &str = "app";

/// The type of what an app's entry returns (`vos_app_Handlers`): the one type of the contract that no function of
/// the module mentions, because the host reads it instead of a script passing it.
#[must_use]
pub fn handlers_type() -> String {
    Registry::qualify(Some(APP_MODULE), "Handlers")
}

/// The type a handler and a tool's `run` are called with (`vos_app_Context`).
#[must_use]
pub fn context_type() -> String {
    Registry::qualify(Some(APP_MODULE), "Context")
}

/// The types the schema names that no module declares, because the languages have them: `never`, what a call that
/// never returns (`ctx:fail`) is declared to give.
pub const LANGUAGE_TYPES: &[&str] = &[api::context::NEVER];

/// Every module and type, as the IR the generators read.
#[must_use]
pub fn schema() -> Ir {
    let mut ir = neoc_vm2::declarations::schema();
    // the entry's contract: reached by no function, so the registry of the modules does not have it
    let mut contract = Registry::new();
    api::Handlers::shape(&mut contract);
    for def in contract.into_types() {
        if ir.type_def(&def.name).is_none() {
            ir.types.push(def);
        }
    }
    ir.types.sort_by(|a, b| a.name.cmp(&b.name));
    ir
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua_ir::ir::{ModuleKind, Ty, TypeBody};

    #[test]
    fn the_schema_has_the_app_api_and_the_library() {
        let ir = schema();
        let app = ir.modules.iter().find(|m| m.name == APP_MODULE).expect("vos:app");
        assert_eq!(app.kind, ModuleKind::Require);
        let mut members: Vec<&str> = app
            .constants
            .iter()
            .map(|c| c.name.as_str())
            .chain(app.objects.iter().map(|o| o.name.as_str()))
            .chain(app.functions.iter().map(|f| f.name.as_str()))
            .collect();
        members.sort_unstable();
        assert_eq!(
            members,
            ["agents", "db", "emit", "files", "id", "kv", "log", "notify", "openWindow", "t", "tool"]
        );
        assert!(
            ir.modules.iter().any(|m| m.name == "std:fs") && ir.modules.iter().any(|m| m.name == "lib:json")
        );
    }

    #[test]
    fn the_entrys_contract_is_declared_with_the_types_it_names() {
        let ir = schema();
        let handlers = ir.type_def(&handlers_type()).expect("Handlers");
        let TypeBody::Alias { ty: Ty::Map { value, .. } } = &handlers.body else { panic!("{handlers:?}") };
        let Ty::Function { params, .. } = &**value else { panic!("{value:?}") };
        assert_eq!(params[0].name, "ctx");
        assert_eq!(params[0].ty.name(), Some(context_type().as_str()));
        assert!(matches!(ir.type_def(&context_type()).expect("Context").body, TypeBody::Object { .. }));
    }

    /// A failed file call is a typed error with the kinds of the domain, not a string to parse.
    #[test]
    fn a_file_call_returns_a_typed_error() {
        let ir = schema();
        let error = ir.type_def("vos_app_FileError").expect("FileError");
        let TypeBody::Error { variants, .. } = &error.body else { panic!("{error:?}") };
        let kinds: Vec<&str> = variants.iter().map(|v| v.tag_value.as_str()).collect();
        let domain: Vec<&str> = vos_domain::FileErrorKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(kinds, domain);
    }
}
