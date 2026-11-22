//! The Luau declarations for app authors: one definitions file, `types/generated/vos.d.luau`, for luau-lsp (the
//! editor, `bun run lint:luau`, and the build of every app's backend). It is never loaded by the host.
//!
//! The file holds, in this order:
//!
//! 1. the declarations of the `std:`, `lib:` and `vnd:` modules an app is offered, taken from neoc's own generated
//!    file (`types/neoc.d.luau`) with the modules in [`vos_domain::NOT_FOR_APPS`] left out ([`neoc`]);
//! 2. vos's types, named `vos_<Name>` (a definitions file has one flat namespace, and neoc names its types
//!    `<ns>_<module>_<Type>` for the same reason): the `app` API ([`defs`]);
//! 3. the globals: `app`, and the ones an app's environment does not have ([`vos_domain::GLOBALS_NOT_FOR_APPS`])
//!    typed as unusable;
//! 4. the ONE `declare require`. Two `declare require` declarations do not merge (the last one wins), so the
//!    overloads of neoc's modules and vos's are written together here.
//!
//! A module name has a colon, which is not portable in a file name, so nothing here is a file per module.

pub mod defs;
pub mod neoc;

/// The prefix of every type vos declares.
pub(crate) const PREFIX: &str = "vos_";

/// The declared name of a vos type.
pub(crate) fn named(name: &str) -> String {
    format!("{PREFIX}{name}")
}
