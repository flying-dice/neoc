//! The connection under an `Attach`: bollard's `AttachContainerResults`, a stream of the container's output and a
//! writer to its input over the one connection Docker hijacked.
//!
//! The output is read by a task of the engine's [`Life`] into a bounded queue, as a stream's is (see `stream`); the
//! input stays here and is written by the script's calls. Both end with the script: when it is unloaded or shut
//! down the task is aborted and the input dropped, which closes the connection, and so does the script letting go
//! of the `Attach`.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::Stream as FutureStream;
use mlua::{Lua, Value};
use mlua_ir::ir::{Returns, Ty};
use mlua_ir::{IntoLuaRet, IntoReturn, Raise, Registry, Shape};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

use super::bollard::LogPiece;
use super::error::DockerError;
use super::stream::{self, Reader};
use neoc_vm_core::lifecycle::{self, Life, Role};

/// The container's input, as bollard gives it.
type Input = Pin<Box<dyn AsyncWrite + Send>>;

/// An attached container, as the script's handles hold it.
pub(super) struct Attached {
    output: Mutex<Reader<LogPiece>>,
    /// `None` once it was closed.
    input: Mutex<Option<Input>>,
    /// The script stopped: an input that is being written is dropped when the write ends.
    stopped: AtomicBool,
}

fn closed(reason: &str) -> DockerError {
    DockerError::Closed { reason: reason.to_owned() }
}

impl Attached {
    pub(super) fn new(
        life: &Life,
        output: impl FutureStream<Item = Result<LogPiece, DockerError>> + Send + 'static,
        input: Input,
    ) -> Arc<Self> {
        let output = stream::start(life, move |queue| stream::pump(output, queue));
        let attached = Arc::new(Self {
            output: Mutex::new(output),
            input: Mutex::new(Some(input)),
            stopped: AtomicBool::new(false),
        });
        // the output's task is the registry's already; the input is dropped here
        life.on_shutdown_with(&attached, |attached| attached.stop());
        attached
    }

    pub(super) async fn next(&self) -> Result<Option<LogPiece>, DockerError> {
        self.output.lock().await.next().await.transpose()
    }

    pub(super) async fn write(&self, data: &[u8]) -> Result<(), DockerError> {
        let mut input = self.input.lock().await;
        let Some(writer) = input.as_mut() else {
            return Err(closed("the container's input is closed"));
        };
        let written = async {
            writer.write_all(data).await?;
            writer.flush().await
        }
        .await;
        if let Err(e) = written {
            *input = None;
            return Err(DockerError::Closed {
                reason: format!("the container's input could not be written: {e}"),
            });
        }
        if self.stopped.load(Ordering::SeqCst) {
            *input = None;
        }
        Ok(())
    }

    pub(super) async fn close_input(&self) -> Result<(), DockerError> {
        let Some(mut writer) = self.input.lock().await.take() else {
            return Ok(());
        };
        writer.shutdown().await.map_err(|e| DockerError::Closed {
            reason: format!("the container's input could not be closed: {e}"),
        })
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Ok(mut input) = self.input.try_lock() {
            *input = None;
        }
    }
}

/// What `Attach:items()` returns: a function that gives the next piece of output at each call, `nil` at the end,
/// and raises an error, for `for piece in attach:items() do`.
pub struct Items(pub(super) Arc<Attached>);

impl Shape for Items {
    fn shape(reg: &mut Registry) -> Ty {
        Ty::Function {
            params: Vec::new(),
            returns: Box::new(Returns::One { ty: Ty::optional(LogPiece::shape(reg)) }),
        }
    }
}

impl IntoLuaRet for Items {
    fn into_lua_ret(self, lua: &Lua, op: &'static str) -> mlua::Result<Value> {
        let attached = self.0;
        // a read waits for the container, not for the VM, as a stream's `items()` does
        let next = lifecycle::async_function(lua, Role::Wait, move |lua: Lua, ()| {
            let attached = attached.clone();
            // hold the engine weakly across the wait: a pending read must not keep a dropped engine alive
            let weak = lua.weak();
            drop(lua);
            async move {
                let piece = attached.next().await;
                let lua = weak.try_upgrade().ok_or_else(|| mlua::Error::runtime("the engine was dropped"))?;
                match piece {
                    Ok(Some(piece)) => piece.into_lua_ret(&lua, op),
                    Ok(None) => Ok(Value::Nil),
                    Err(error) => {
                        let text = error.to_string();
                        match Err::<(), _>(Raise(error)).into_return(&lua, op) {
                            Err(raised) => Err(raised),
                            Ok(_) => Err(mlua::Error::runtime(text)),
                        }
                    }
                }
            }
        })?;
        Ok(Value::Function(next))
    }
}
