//! The async runtime the host runs on, and where its top-level future runs.
//!
//! Apps' Lua runs on whatever thread polls an app's VM, and recursion through a Rust host function that calls back
//! into Lua (a `lib:cache` eviction callback, a `__tostring`, a `string.gsub` callback, a request handler's options
//! table) is stopped by Luau at 200 C levels, an ordinary error, provided the stack holds those levels: that is
//! [`VM_THREAD_STACK`] (8 MiB, from neoc-vm2's `lifecycle.rs`, which has the measurements). So every thread that polls
//! a VM gets it: the workers and the blocking pool (`thread_stack_size` sets both), and the top-level future runs on a
//! worker (`tokio::spawn`), not on the thread that calls `block_on`, whose stack the operating system decides. The host
//! has no dedicated thread that calls into Lua.

use anyhow::Context;
use std::future::Future;
use vos_lua::VM_THREAD_STACK;

/// A multi-thread runtime whose every thread has [`VM_THREAD_STACK`] of stack.
///
/// # Errors
/// The runtime could not be started.
pub fn build() -> anyhow::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(VM_THREAD_STACK)
        .build()
        .context("start the async runtime")
}

/// Run `top` to completion on a worker of `runtime` (never on the calling thread). A panic in it is raised here again.
///
/// # Errors
/// What `top` returned, or the task failed to finish.
pub fn run<F>(runtime: &tokio::runtime::Runtime, top: F) -> anyhow::Result<()>
where
    F: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    runtime.block_on(async move {
        match tokio::spawn(top).await {
            Ok(done) => done,
            Err(e) => match e.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(e) => Err(anyhow::anyhow!("the host task did not finish: {e}")),
            },
        }
    })
}
