//! Server-driven UI: the component schema, the expression language, sources and client state, the view
//! protocol, a validator and a keyed diff. Apps describe views (in Lua); the managed client renders them with
//! the kit and sends events back. `docs/architecture/ui-system.md` is the spec.
//!
//! This is the **single source** of the type pipeline: `vos-codegen` reads these types and tables and emits
//! the client's TypeScript, the app authors' Lua definitions and constructors, and the component reference.
//!
//! | Module | What it holds |
//! |---|---|
//! | [`node`] | the node shape, handlers, client actions, key paths and test-id derivation |
//! | [`expr`] | prop values: literals, refs and the closed `$fn` set |
//! | [`source`] | source descriptors and client-state declarations |
//! | [`spec`] | the schema vocabulary: props, events, slots, accessibility, tiers |
//! | [`components`] | the catalogue: every component as `const` data |
//! | [`protocol`] | the view protocol messages and the method table |
//! | [`validate`] | `validate(node)`: the tree checker the host runs on every render |
//! | [`bindings`] | the two-way bound source fields of a tree, and how each is reconciled |
//! | [`diff`] | `diff(old, new)` keyed reconciliation, and `apply` |
//! | [`version`] | `proto` and `kit` versions |

pub mod bindings;
pub mod components;
pub mod diff;
pub mod expr;
pub mod node;
pub mod protocol;
pub mod source;
pub mod spec;
pub mod validate;
pub mod version;

pub use diff::{apply, diff, ApplyError};
pub use expr::{Expr, Func, Namespace, Ref};
pub use node::{Action, Handler, Node};
pub use protocol::Op;
pub use source::{ClientDecl, SourceDecl, ViewDecls};
pub use validate::{validate, validate_with, Context, Severity, Violation};
pub use version::{KitVersion, KIT_VERSION, PROTO_VERSION};
