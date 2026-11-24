//! The declarations of the modules this crate links, read from the schema mlua-ir builds from their Rust.
//!
//! `declarations::luau()` is the `.d.luau` luau-lsp reads, `declarations::typescript()` the `.d.ts` `TypeScriptToLua`
//! reads; `cargo run -p neoc-vm2 --example declarations -- <dir>` writes both next to the schema.

use mlua_ir::ir::Ir;

/// Every module and type, as the IR the generators read.
#[must_use]
pub fn schema() -> Ir {
    // referencing the modules keeps them linked, so their registrations are in the schema
    let _ = crate::modules::names().count();
    mlua_ir::schema(env!("CARGO_PKG_VERSION"))
}

/// The Luau declarations (`--definitions`).
#[must_use]
pub fn luau() -> String {
    mlua_ir_gen::luau::file(&schema())
}

/// The TypeScript declarations.
#[must_use]
pub fn typescript() -> String {
    mlua_ir_gen::ts::file(&schema())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_declarations_describe_the_pilots_with_their_types() {
        let luau = luau();
        assert!(luau.contains("lib_base64"), "{luau}");
        assert!(luau.contains("std_time_Duration"), "{luau}");
        let ts = typescript();
        assert!(ts.contains("lib:base64") && ts.contains("std:time"), "{ts}");
        // Genuinely unknown data is `any` in Luau (which cannot narrow `unknown`) and `unknown` in TypeScript; so
        // TypeScript shows that nothing else is untyped.
        assert!(!ts.contains(": any") && !ts.contains("<any>") && !ts.contains("any[]"), "nothing is `any`: {ts}");
    }

    #[test]
    fn every_error_is_a_declared_union_of_kinds() {
        let ir = schema();
        let luau = luau();
        for kind in ["invalid_byte", "invalid_padding"] {
            assert!(luau.contains(kind), "{kind} is declared:\n{luau}");
        }
        assert!(ir.type_def("lib_base64_DecodeError").is_some());
    }
}
