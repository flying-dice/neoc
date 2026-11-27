//! What a script on this adapter can reach, as data: the schema mlua-ir builds from the Rust of every module this
//! crate links (neoc-vm's `std:`, `lib:` and `vnd:` modules, and the `app` API, `vos:app`), and of the framework
//! it carries as Luau (`vos:docker`, declared in [`crate::docker`]). The declarations for
//! app authors (`types/generated/vos.d.luau`, `vos.d.ts`) are written from it by `vos-codegen`, which adds the
//! host's policy: which of the modules an app is offered.

use crate::{api, docker};
use mlua_ir::ir::Ir;
use mlua_ir::Registry;

/// The module the `app` API is: `require("vos:app")`, and the global `app`.
pub const APP_MODULE: &str = api::MODULE;

/// The module the container framework is: `require("vos:docker")`.
pub const DOCKER_MODULE: &str = docker::MODULE;

/// The global the `app` API also is.
pub const APP_GLOBAL: &str = "app";

/// The module the RPC bus is: `require("vos:rpc")`, and the global `RPC`.
pub const RPC_MODULE: &str = api::RPC_MODULE;

/// The global the RPC bus also is.
pub const RPC_GLOBAL: &str = "RPC";

/// The type a handler is called with (`vos_app_Context`).
#[must_use]
pub fn context_type() -> String {
    Registry::qualify(Some(APP_MODULE), "Context")
}

/// Every module and type, as the IR the generators read.
#[must_use]
pub fn schema() -> Ir {
    let mut ir = neoc_vm::declarations::schema();
    docker::declare(&mut ir);
    ir.types.sort_by(|a, b| a.name.cmp(&b.name));
    ir
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua_ir::ir::{ModuleKind, TypeBody};

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
        assert_eq!(members, ["agents", "db", "files", "id", "kv", "log", "notify", "open_window", "tools"]);
        assert!(
            ir.modules.iter().any(|m| m.name == "std:fs") && ir.modules.iter().any(|m| m.name == "lib:json")
        );
    }

    #[test]
    fn the_rpc_bus_is_its_own_module_in_snake_case() {
        let ir = schema();
        let rpc = ir.modules.iter().find(|m| m.name == RPC_MODULE).expect("vos:rpc");
        assert_eq!(rpc.kind, ModuleKind::Require);
        let mut names: Vec<&str> = rpc.functions.iter().map(|f| f.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["on_notification", "on_request", "send_notification", "send_request"]);
    }

    #[test]
    fn the_context_is_declared_with_its_type() {
        let ir = schema();
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
