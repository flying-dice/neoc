//! How the modules written with mlua-ir take part in neoc's runtime: one span per call, and every async body
//! under the lifecycle's rules ([`crate::lifecycle`]).
//!
//! mlua-ir tells the host each call; a function or method says its role with `#[host("Mixed")]` (the
//! [`Role`] names: `Vm`, `Wait`, `Timer`, `Mixed`) and defaults to `Wait`.

use std::any::Any;

use mlua_ir::{Body, Call, Host};
use tracing::Instrument as _;

use crate::lifecycle::{Role, leaf};
use crate::span::dyn_span;

struct Neoc;

/// Installs the host, once per process (the engine factory does it).
pub fn install() {
    mlua_ir::set_host(Neoc);
}

/// The call's span: named `<module stem>.<function>`, at target `lua::lib::<module stem>`.
fn span(call: &Call) -> tracing::Span {
    let module = call.module.unwrap_or("globals");
    let stem = module.split_once(':').map_or(module, |(_, stem)| stem);
    dyn_span(&format!("lua::lib::{stem}"), &format!("{stem}.{}", call.name))
}

fn role(hint: Option<&str>) -> Role {
    match hint {
        None | Some("Wait") => Role::Wait,
        Some("Vm") => Role::Vm,
        Some("Timer") => Role::Timer,
        Some("Mixed") => Role::Mixed,
        #[allow(clippy::panic)] // a typo in a role is a bug in the module, found by its first test
        Some(other) => panic!("`#[host({other:?})]` is not a role: Vm, Wait, Timer or Mixed"),
    }
}

impl Host for Neoc {
    fn enter(&self, call: &Call) -> Option<Box<dyn Any>> {
        Some(Box::new(span(call).entered()))
    }

    fn wrap(&self, call: &Call, body: Body) -> Body {
        Box::pin(leaf(role(call.hint), body.instrument(span(call))))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[should_panic(expected = "is not a role")]
    fn a_role_that_does_not_exist_is_a_bug_found_at_the_first_call() {
        let _ = super::role(Some("Quick"));
    }
}
