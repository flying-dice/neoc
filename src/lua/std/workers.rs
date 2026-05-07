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

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("on_shutdown", lua.create_function(shutdown_signal_fn)?)?;
    // Backwards compat alias — deprecated, prefer on_shutdown().
    t.set("shutdown_signal", lua.create_function(shutdown_signal_fn)?)?;
    Ok(t)
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
        methods.add_async_method("wait", |_, this, _: ()| {
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
