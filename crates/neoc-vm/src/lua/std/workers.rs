//! Process-level coordination — `std:workers`.
//!
//! Exposes `workers.on_shutdown()` — an observable signal fired on Ctrl-C
//! so long-running primitives (e.g. `(require("vnd:hyper")).serve`'s
//! `server:wait()`) can exit cleanly. Legacy alias `workers.shutdown_signal()`
//! still works.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use mlua::{Lua, Table, UserData, UserDataMethods};
use tokio::sync::Notify;

use crate::lua::facade::Module;
use crate::lua::lifecycle::{Role, async_method};

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:workers` surface through the facade.
///
/// Both functions are set verbatim with [`crate::lua::facade::Module::set`]:
/// the hand-written registration carried **no** `#[instrument]` span, so
/// routing through the facade's `func` would add one. Each returns the
/// process-global `ShutdownSignal` userdata, whose `is_fired` / `wait` methods
/// keep their own `impl UserData` and are recorded here via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:workers")?;

    m.set("on_shutdown", lua.create_function(shutdown_signal_fn)?)?;
    m.record_fn(
        "on_shutdown",
        &[],
        "ShutdownSignal",
        "Return the process-global `ShutdownSignal`. Fires on Ctrl-C (SIGINT).",
    );

    // Backwards compat alias — deprecated, prefer on_shutdown().
    m.set("shutdown_signal", lua.create_function(shutdown_signal_fn)?)?;
    m.record_fn(
        "shutdown_signal",
        &[],
        "ShutdownSignal",
        "Deprecated alias for `on_shutdown()`. Prefer `on_shutdown()` in new code.",
    );

    m.userdata("ShutdownSignal", |s| {
        s.method(
            "is_fired",
            &[],
            "bool",
            "Return `true` if the signal has already fired. Non-blocking.",
        )
        .method(
            "wait",
            &[],
            "()",
            "Yield the calling coroutine until the signal fires; returns immediately if already fired.",
        );
    });

    Ok(m)
}

// ---------------------------------------------------------------------------
// Shutdown signal — process-global single instance
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ShutdownSignal {
    inner: Arc<ShutdownInner>,
}

struct ShutdownInner {
    notify: Notify,
    fired: AtomicBool,
}

impl ShutdownSignal {
    pub fn current() -> Self {
        static SIG: OnceLock<ShutdownSignal> = OnceLock::new();
        SIG.get_or_init(|| Self {
            inner: Arc::new(ShutdownInner {
                notify: Notify::new(),
                fired: AtomicBool::new(false),
            }),
        })
        .clone()
    }

    pub fn fire(&self) {
        if !self.inner.fired.swap(true, Ordering::SeqCst) {
            self.inner.notify.notify_waiters();
        }
    }

    pub fn is_fired(&self) -> bool {
        self.inner.fired.load(Ordering::SeqCst)
    }

    pub async fn wait(&self) {
        let notified = self.inner.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if self.is_fired() {
            return;
        }
        notified.await;
    }
}

impl UserData for ShutdownSignal {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("is_fired", |_, this, _: ()| Ok(this.is_fired()));
        async_method(methods, "wait", Role::Wait, |_, this, (): ()| {
            let signal = this.clone();
            async move {
                signal.wait().await;
                Ok(())
            }
        });
    }
}

fn shutdown_signal_fn(_: &Lua, _: ()) -> mlua::Result<ShutdownSignal> {
    Ok(ShutdownSignal::current())
}

#[cfg(test)]
mod tests {
    use crate::lua;

    #[test]
    fn on_shutdown_alias() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let e1 = lua::sandbox::make_engine().expect("engine 1");

            let ok: bool = e1
                .load(
                    r#"
                    local workers = require("std:workers")
                    local a = workers.on_shutdown()
                    local b = workers.shutdown_signal()
                    return type(a.is_fired) == "function"
                       and type(b.is_fired) == "function"
                    "#,
                )
                .eval_async()
                .await
                .expect("on_shutdown alias");
            assert!(ok);
        });
    }
}
