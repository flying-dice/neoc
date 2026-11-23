//! `Stream`: what Docker sends over time (a container's logs, the daemon's events, the progress of a pull).
//!
//! A stream is a task that reads Docker's response and a bounded queue between it and the script. The task is
//! registered with the engine's [`Life`], so it stops when the script is unloaded or shut down; it also stops when
//! the script closes the stream or drops it. The queue is bounded, so a script that reads slowly slows the reading
//! down instead of filling memory: Docker's own buffers take up the rest.
//!
//! The request is sent by the task, not by the call that made the stream: `logs` of a container that does not exist
//! returns a stream, and its first `next()` returns the error.

use std::future::Future;
use std::sync::Arc;

use bytes::Bytes;
use futures::{Stream as FutureStream, StreamExt};
use mlua::serde::ser::Options as SerializeOptions;
use mlua::{Lua, LuaSerdeExt, UserData, UserDataMethods, Value};
use serde_json::Value as Json;
use tokio::sync::{Mutex, mpsc};
use tokio::task::AbortHandle;

use super::error::{BollardError, Kind};
use crate::lua::lifecycle::{Life, Role, async_method};
use crate::lua::sandbox::{from_err, ok};

/// How many items wait between Docker and the script before the reading task waits too.
const QUEUE: usize = 64;

/// One thing a stream yields.
pub(super) enum Item {
    /// One of Docker's JSON messages (an event, a line of pull progress), as a table.
    Json(Json),
    /// A piece of a container's output: `{ stream = "stdout" | "stderr" | "stdin" | "console", data = <bytes> }`.
    Output { stream: &'static str, data: Bytes },
}

impl Item {
    pub(super) fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        match self {
            Item::Json(json) => to_lua(lua, &json),
            Item::Output { stream, data } => {
                let piece = lua.create_table()?;
                piece.set("stream", stream)?;
                piece.set("data", lua.create_string(&data)?)?;
                Ok(Value::Table(piece))
            }
        }
    }
}

/// One of Docker's JSON values as a plain Lua value: an object or array is a table, `null` is `nil`.
pub(super) fn to_lua(lua: &Lua, json: &Json) -> mlua::Result<Value> {
    let nil_for_null = SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false);
    lua.to_value_with(json, nil_for_null)
}

pub struct Stream {
    inner: Arc<Inner>,
}

struct Inner {
    /// What `next` and `close` say in their errors: `bollard.logs`, `bollard.events`, ...
    op: &'static str,
    queue: Mutex<mpsc::Receiver<Result<Item, BollardError>>>,
    reader: AbortHandle,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl Stream {
    /// Start reading `source` (a future, so that what it needs first, a connection, is awaited by the reading task)
    /// on behalf of the script `life` belongs to.
    ///
    /// # Errors
    /// There is no async runtime to read on (the engine is not driven by one).
    pub(super) fn start<S>(
        life: &Life,
        op: &'static str,
        source: impl Future<Output = Result<S, BollardError>> + Send + 'static,
    ) -> Result<Self, BollardError>
    where
        S: FutureStream<Item = Result<Item, BollardError>> + Send + 'static,
    {
        if tokio::runtime::Handle::try_current().is_err() {
            return Err(BollardError::new(
                Kind::Other,
                "a stream needs the async runtime the engine runs on",
            ));
        }
        let (send, queue) = mpsc::channel(QUEUE);
        let reader = life.spawn(async move {
            let stream = match source.await {
                Ok(stream) => stream,
                Err(e) => {
                    let _ = send.send(Err(e)).await;
                    return;
                }
            };
            let mut stream = std::pin::pin!(stream);
            while let Some(item) = stream.next().await {
                // an error ends the stream: Docker sends nothing useful after one
                let failed = item.is_err();
                if send.send(item).await.is_err() || failed {
                    return;
                }
            }
        });
        Ok(Self {
            inner: Arc::new(Inner {
                op,
                queue: Mutex::new(queue),
                reader: reader.abort_handle(),
            }),
        })
    }
}

#[neoc_macros::lua_userdata]
impl UserData for Stream {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        /// Wait for the next item. `(nil, nil)` is the end: Docker sent everything, or the stream was closed. `(nil, err)` is a failure, and ends the stream too. It waits like `task.wait`: other tasks run while it does.
        #[lua("Result<LuaValue, string>")]
        async_method(methods, "next", Role::Wait, |lua, this, (): ()| {
            let inner = Arc::clone(&this.inner);
            async move {
                // one reader at a time: a second `next` from another task waits for the first
                let item = inner.queue.lock().await.recv().await;
                match item {
                    Some(Ok(item)) => ok(&lua, item.into_lua(&lua)?),
                    Some(Err(e)) => from_err(&lua, inner.op, e),
                    None => Ok((Value::Nil, Value::Nil)),
                }
            }
        });
        /// Stop reading and drop the connection to Docker. Items already read may still be returned by `next`; then it is the end. A stream is also closed when it is collected, and when the script that made it is unloaded or shut down.
        #[lua("()")]
        methods.add_method("close", |_, this, (): ()| {
            this.inner.reader.abort();
            Ok(())
        });
    }
}
