//! Server-driven UI: the component schema, the expression language, sources and client state, the view
//! protocol, a validator and a keyed diff. Apps describe views (in Luau); the managed client renders them with
//! the kit and sends events back. `docs/architecture/ui-system.md` is the spec.
//!
//! This is the **single source** of the type pipeline: `agentos-codegen` reads these types and tables and emits
//! the client's TypeScript, the app authors' Luau definitions and constructors, and the component reference.
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
//! | [`diff`] | `diff(old, new)` keyed reconciliation, and `apply` |
//! | [`version`] | `proto` and `kit` versions |
//! | [`legacy`] | the spike's first tree, until the host moves to the schema |

pub mod components;
pub mod diff;
pub mod expr;
pub mod legacy;
pub mod node;
pub mod protocol;
pub mod source;
pub mod spec;
pub mod validate;
pub mod version;

pub use diff::{apply, diff, ApplyError};
pub use expr::{Expr, Func, Namespace, Ref};
pub use legacy::{Direction, ListItem, UiEvent, UiNode, UiPatch};
pub use node::{Action, Handler, Node};
pub use protocol::Op;
pub use source::{ClientDecl, SourceDecl, ViewDecls};
pub use validate::{validate, validate_with, Context, Severity, Violation};
pub use version::{KitVersion, KIT_VERSION, PROTO_VERSION};
