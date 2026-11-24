//! What Docker sends over time (a container's logs, the daemon's events, the progress of a pull), as an
//! `mlua_ir::Stream`.
//!
//! A stream is a task that reads Docker's response and a bounded queue between it and the script. The task is
//! registered with the engine's [`Life`], so it stops when the script is unloaded or shut down; it also stops when
//! the script closes the stream or drops it. The queue is bounded, so a script that reads slowly slows the reading
//! down instead of filling memory: Docker's own buffers take up the rest.
//!
//! The request is sent by the task, not by the call that made the stream: `logs` of a container that does not exist
//! returns a stream, and its first `next()` returns the error.

use std::future::Future;

use futures::future::BoxFuture;
use futures::{FutureExt, Stream as FutureStream, StreamExt};
use mlua_ir::Stream;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use super::error::DockerError;
use neoc_vm_core::lifecycle::Life;

/// How many items wait between Docker and the script before the reading task waits too.
const QUEUE: usize = 64;

type Queue<T> = mpsc::Receiver<Result<T, DockerError>>;
type Feed<T> = mpsc::Sender<Result<T, DockerError>>;

/// The script's end of a stream: the queue, and the task that fills it, which ends with this.
struct Reader<T> {
    queue: Queue<T>,
    task: AbortHandle,
}

impl<T> Drop for Reader<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A stream that is not read yet, or is.
enum State<T> {
    /// Made where there was no async runtime to read on: the reading starts with the first `next()`, which has one.
    Waiting(Life, Box<dyn FnOnce(Feed<T>) -> BoxFuture<'static, ()> + Send>),
    Reading(Reader<T>),
}

fn start<T: Send + 'static>(
    life: &Life,
    read: Box<dyn FnOnce(Feed<T>) -> BoxFuture<'static, ()> + Send>,
) -> Reader<T> {
    let (feed, queue) = mpsc::channel(QUEUE);
    Reader { queue, task: life.spawn(read(feed)).abort_handle() }
}

/// Read `source` to its end, or until nobody takes what it yields.
async fn feed<T, S>(source: impl Future<Output = Result<S, DockerError>>, feed: Feed<T>)
where
    S: FutureStream<Item = Result<T, DockerError>>,
{
    let stream = match source.await {
        Ok(stream) => stream,
        Err(error) => {
            let _ = feed.send(Err(error)).await;
            return;
        }
    };
    let mut stream = std::pin::pin!(stream);
    while let Some(item) = stream.next().await {
        // an error ends the stream: Docker sends nothing useful after one
        let failed = item.is_err();
        if feed.send(item).await.is_err() || failed {
            return;
        }
    }
}

/// A stream of what `source` yields (a future, so that what it needs first, a connection, is awaited by the reading
/// task), read on behalf of the script `life` belongs to. The reading starts now.
pub(super) fn reading<T, S>(
    life: &Life,
    source: impl Future<Output = Result<S, DockerError>> + Send + 'static,
) -> Stream<T, DockerError>
where
    T: Send + 'static,
    S: FutureStream<Item = Result<T, DockerError>> + Send + 'static,
{
    let read: Box<dyn FnOnce(Feed<T>) -> BoxFuture<'static, ()> + Send> =
        Box::new(move |queue| feed(source, queue).boxed());
    let state = if tokio::runtime::Handle::try_current().is_ok() {
        State::Reading(start(life, read))
    } else {
        State::Waiting(life.clone(), read)
    };
    Stream::new(futures::stream::unfold(state, |state| async move {
        let mut reader = match state {
            State::Waiting(life, read) => start(&life, read),
            State::Reading(reader) => reader,
        };
        // the end is the task's: Docker sent everything, an error was the last item, or the script was shut down
        let item = reader.queue.recv().await?;
        Some((item, State::Reading(reader)))
    }))
}
