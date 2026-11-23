//! The byte streams between a script and a process it started: what `std:process` and `vnd:portable_pty` share.
//!
//! - [`Output`] holds what the process wrote and the script has not read yet, up to [`OUTPUT_BUFFER`] bytes. Its
//!   reader waits for room, on a thread ([`Output::push_blocking`]) or in a task ([`Output::room`] then
//!   [`Output::push`]), so a script that reads slowly slows the process down instead of filling memory.
//! - [`read`] is the body of a method that reads from one: it waits without the VM and takes the bytes with the
//!   turn held, so a call that is dropped loses none.
//! - [`Job`] is one piece of input on its way to a writer, and [`submit`] hands one over and waits for it.
//!
//! What fails here is said in words ([`Taken::Failed`], [`WriteFailed`], a `String`): each module wraps them in its
//! own error.

use std::collections::VecDeque;
use std::fmt::Display;
use std::sync::Arc;

use mlua::{Lua, Value};
use parking_lot::{Condvar, Mutex};
use tokio::sync::{Notify, mpsc, oneshot, watch};

use crate::lua::lifecycle::{hold, idle};
use crate::lua::sandbox::{from_err, ok};

/// The most bytes one read returns, and how many it returns when it is not told: a size that fits in one message
/// of most transports once it is encoded.
pub(crate) const MAX_READ: usize = 64 * 1024;

/// The most bytes one write takes: far more than a paste or a message, and little enough to copy without a thought.
pub(crate) const MAX_WRITE: usize = 1024 * 1024;

/// The most output held between the process and the script. Past it the reader waits, the operating system's own
/// buffer fills, and the process waits in its write.
pub(crate) const OUTPUT_BUFFER: usize = 256 * 1024;

/// How much a reader asks the operating system for at a time.
pub(crate) const READ_CHUNK: usize = 16 * 1024;

/// What the docs of every method that waits end with.
pub(crate) const WAITS: &str = "It waits like `task.wait`: other tasks run while it does.";

/// A read's `max`: a whole number of bytes from 1 to [`MAX_READ`].
pub(crate) fn read_size(max: Option<f64>) -> Result<usize, String> {
    match max {
        None => Ok(MAX_READ),
        // checked: a whole number that fits
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(max) if max.fract() == 0.0 && max >= 1.0 && max <= MAX_READ as f64 => Ok(max as usize),
        Some(_) => Err(format!("`max` must be a whole number from 1 to {MAX_READ}")),
    }
}

/// The bytes of one write: at most [`MAX_WRITE`] of them.
pub(crate) fn write_data(data: &mlua::LuaString) -> Result<Vec<u8>, String> {
    let bytes = data.as_bytes();
    if bytes.len() > MAX_WRITE {
        Err(format!("one write takes at most {MAX_WRITE} bytes"))
    } else {
        Ok(bytes.to_vec())
    }
}

/// What a read finds.
pub(crate) enum Taken {
    Bytes(Vec<u8>),
    /// Nothing yet: wait again.
    Nothing,
    /// The end of the output, or a closed stream.
    End,
    /// Reading failed, in the operating system's words.
    Failed(String),
}

struct OutputState {
    bytes: VecDeque<u8>,
    /// Set once the reader stopped: `Ok` at the end of the output, `Err` when reading failed (said by one read).
    ended: Option<Result<(), String>>,
    closed: bool,
}

/// The output a process wrote and the script has not read yet.
pub(crate) struct Output {
    state: Mutex<OutputState>,
    /// Told when there is something for a read: bytes, the end, or a close.
    readable: Notify,
    /// Where a reader on a thread waits for room, or for the close.
    room_blocking: Condvar,
    /// Where a reader in a task waits for room, or for the close.
    room: Notify,
}

impl Output {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(OutputState {
                bytes: VecDeque::new(),
                ended: None,
                closed: false,
            }),
            readable: Notify::new(),
            room_blocking: Condvar::new(),
            room: Notify::new(),
        }
    }

    /// Tell the reader, wherever it waits, that there is room or that the stream is closed.
    fn wake_reader(&self) {
        self.room_blocking.notify_all();
        self.room.notify_waiters();
    }

    /// Wait until `ready` says something about the state, listening to `told` for changes.
    async fn settled<T>(&self, told: &Notify, ready: impl Fn(&OutputState) -> Option<T>) -> T {
        loop {
            let changed = told.notified();
            let mut changed = std::pin::pin!(changed);
            // listen before looking: what changes after the look is heard
            changed.as_mut().enable();
            if let Some(found) = ready(&self.state.lock()) {
                return found;
            }
            changed.await;
        }
    }

    /// For a reader on a thread: add what it read, waiting for room first. `false` once the stream is closed: the
    /// reader stops.
    pub(crate) fn push_blocking(&self, chunk: &[u8]) -> bool {
        let mut state = self.state.lock();
        while state.bytes.len() >= OUTPUT_BUFFER && !state.closed {
            self.room_blocking.wait(&mut state);
        }
        if state.closed {
            return false;
        }
        state.bytes.extend(chunk);
        drop(state);
        self.readable.notify_waiters();
        true
    }

    /// For a reader in a task: wait for room for one more chunk. `false` once the stream is closed: the reader
    /// stops.
    pub(crate) async fn room(&self) -> bool {
        self.settled(&self.room, |state| {
            if state.closed {
                Some(false)
            } else {
                (state.bytes.len() < OUTPUT_BUFFER).then_some(true)
            }
        })
        .await
    }

    /// For a reader in a task: wait for the close.
    pub(crate) async fn closed(&self) {
        self.settled(&self.room, |state| state.closed.then_some(()))
            .await;
    }

    /// For a reader in a task: add what it read (after [`Output::room`]). Dropped when the stream was closed
    /// meanwhile.
    pub(crate) fn push(&self, chunk: &[u8]) {
        {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.bytes.extend(chunk);
        }
        self.readable.notify_waiters();
    }

    /// The reader stopped: at the end of the output (`Ok`), or because reading failed.
    pub(crate) fn finish(&self, end: Result<(), String>) {
        self.state.lock().ended = Some(end);
        self.readable.notify_waiters();
    }

    /// Drop what was not read, end every read and free a reader that waits for room.
    pub(crate) fn close(&self) {
        {
            let mut state = self.state.lock();
            state.closed = true;
            state.bytes = VecDeque::new();
        }
        self.readable.notify_waiters();
        self.wake_reader();
    }

    /// Wait until [`Output::take`] has something to say. It takes nothing, so a wait that is dropped loses nothing.
    pub(crate) async fn readable(&self) {
        self.settled(&self.readable, |state| {
            (state.closed || state.ended.is_some() || !state.bytes.is_empty()).then_some(())
        })
        .await;
    }

    /// At most `max` bytes of what is there.
    pub(crate) fn take(&self, max: usize) -> Taken {
        let mut state = self.state.lock();
        if state.closed {
            return Taken::End;
        }
        if !state.bytes.is_empty() {
            let n = max.min(state.bytes.len());
            let bytes: Vec<u8> = state.bytes.drain(..n).collect();
            drop(state);
            self.wake_reader();
            return Taken::Bytes(bytes);
        }
        match &mut state.ended {
            None => Taken::Nothing,
            Some(Ok(())) => Taken::End,
            // said once; then it is the end
            Some(end) => match std::mem::replace(end, Ok(())) {
                Err(message) => Taken::Failed(message),
                Ok(()) => Taken::End,
            },
        }
    }

    #[cfg(test)]
    fn held(&self) -> usize {
        self.state.lock().bytes.len()
    }
}

/// The body of a method that reads from an [`Output`]: `(bytes, nil)`, `(nil, nil)` at the end, or
/// `(nil, "<op>: <error>")`. `wanted` is the buffer and the most bytes to take, or why the call was refused;
/// `failed` makes the module's error of a failed read.
///
/// The method is a `Role::Mixed` one, not a `Role::Wait` (as `std:fs`'s `Watcher:receive`): a `Wait` body would
/// take the bytes when its wait ends and only then queue for the turn, and a call dropped there would lose them.
/// The wait for output is an `idle` wait, and the bytes are taken with the turn held, in the poll that returns them.
pub(crate) async fn read<E: Display>(
    lua: Lua,
    op: &str,
    wanted: Result<(Arc<Output>, usize), E>,
    failed: fn(String) -> E,
) -> mlua::Result<(Value, Value)> {
    let (output, max) = match wanted {
        Ok(wanted) => wanted,
        Err(e) => {
            hold().await;
            return from_err(&lua, op, e);
        }
    };
    loop {
        idle(output.readable()).await?;
        match output.take(max) {
            Taken::Bytes(bytes) => return ok(&lua, lua.create_string(&bytes)?),
            Taken::Nothing => {}
            Taken::End => return Ok((Value::Nil, Value::Nil)),
            Taken::Failed(message) => return from_err(&lua, op, failed(message)),
        }
    }
}

/// One piece of input for a writer, and where it says how the write went.
pub(crate) struct Job {
    pub(crate) data: Vec<u8>,
    done: oneshot::Sender<Result<(), String>>,
}

impl Job {
    /// Nobody waits for this write any more (its call was dropped): a writer that has not started it skips it.
    pub(crate) fn is_abandoned(&self) -> bool {
        self.done.is_closed()
    }

    /// Say how the write went.
    pub(crate) fn finish(self, written: std::io::Result<()>) {
        // the call may be gone
        let _ = self.done.send(written.map_err(|e| e.to_string()));
    }
}

/// Why a write was not made.
pub(crate) enum WriteFailed {
    /// The stream was closed, before the write or while it waited.
    Closed,
    /// The operating system refused the bytes, in its words.
    Io(String),
}

/// Hand `data` to the writer behind `input` and wait until it is written, or until `closed` says the stream is.
/// `None` is a stream that is closed already.
///
/// The queue is let go of as soon as the writer has the job, so a write that waits does not keep the queue open
/// (dropping the queue is how a stream's input is ended). A call that is dropped before its job is started
/// writes nothing ([`Job::is_abandoned`]); one that is dropped later cannot take back what the writer already has.
pub(crate) async fn submit(
    input: Option<mpsc::Sender<Job>>,
    data: Vec<u8>,
    mut closed: watch::Receiver<bool>,
) -> Result<(), WriteFailed> {
    let Some(input) = input else {
        return Err(WriteFailed::Closed);
    };
    let (done, written) = oneshot::channel();
    let write = async {
        let queued = input.send(Job { data, done }).await;
        drop(input);
        if queued.is_err() {
            return Err(WriteFailed::Closed);
        }
        match written.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => Err(WriteFailed::Io(message)),
            Err(_) => Err(WriteFailed::Closed),
        }
    };
    tokio::select! {
        result = write => result,
        _ = closed.wait_for(|closed| *closed) => Err(WriteFailed::Closed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(10);

    /// Wait until the buffer holds at least `least` bytes.
    fn filled(output: &Output, least: usize) {
        let started = Instant::now();
        while output.held() < least {
            assert!(started.elapsed() < TIMEOUT, "the reader did not fill it");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_reader_on_a_thread_fills_a_bounded_buffer_and_a_close_frees_it() {
        let output = Arc::new(Output::new());
        let pushes = Arc::new(AtomicUsize::new(0));
        let reader = {
            let (output, pushes) = (Arc::clone(&output), Arc::clone(&pushes));
            std::thread::spawn(move || {
                let chunk = vec![7; READ_CHUNK];
                while output.push_blocking(&chunk) {
                    pushes.fetch_add(1, Ordering::SeqCst);
                }
            })
        };
        // a full buffer stops the reader: it never holds more than one chunk past the limit
        filled(&output, OUTPUT_BUFFER);
        assert!(output.held() < OUTPUT_BUFFER + READ_CHUNK);
        let full = pushes.load(Ordering::SeqCst);

        // a read makes room, and the reader fills it again
        let Taken::Bytes(bytes) = output.take(100_000) else {
            panic!("there are bytes to take");
        };
        assert_eq!(bytes.len(), 100_000);
        filled(&output, OUTPUT_BUFFER);
        assert!(output.held() < OUTPUT_BUFFER + READ_CHUNK);
        assert!(pushes.load(Ordering::SeqCst) > full);

        output.close();
        reader.join().expect("the reader ended");
        assert!(matches!(output.take(10), Taken::End));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reader_in_a_task_fills_a_bounded_buffer_and_a_close_frees_it() {
        let output = Arc::new(Output::new());
        let reader = {
            let output = Arc::clone(&output);
            tokio::spawn(async move {
                let chunk = vec![7; READ_CHUNK];
                while output.room().await {
                    output.push(&chunk);
                }
            })
        };
        filled(&output, OUTPUT_BUFFER);
        assert!(output.held() < OUTPUT_BUFFER + READ_CHUNK);

        let Taken::Bytes(bytes) = output.take(100_000) else {
            panic!("there are bytes to take");
        };
        assert_eq!(bytes.len(), 100_000);
        filled(&output, OUTPUT_BUFFER);
        assert!(output.held() < OUTPUT_BUFFER + READ_CHUNK);

        // a reader that waits for the close hears it too
        let waiting = {
            let output = Arc::clone(&output);
            tokio::spawn(async move { output.closed().await })
        };
        output.close();
        tokio::time::timeout(TIMEOUT, async {
            reader.await.expect("the reader ended");
            waiting.await.expect("the close was heard");
        })
        .await
        .expect("the close freed the reader");
        assert!(matches!(output.take(10), Taken::End));
    }

    #[test]
    fn a_read_failure_is_said_once_after_the_bytes_and_then_it_is_the_end() {
        let output = Output::new();
        assert!(matches!(output.take(10), Taken::Nothing));
        assert!(output.push_blocking(b"abc"));
        output.finish(Err("the stream broke".to_owned()));
        assert!(matches!(output.take(2), Taken::Bytes(bytes) if bytes == b"ab"));
        assert!(matches!(output.take(2), Taken::Bytes(bytes) if bytes == b"c"));
        assert!(matches!(output.take(2), Taken::Failed(message) if message == "the stream broke"));
        assert!(matches!(output.take(2), Taken::End));
    }

    #[test]
    fn a_read_takes_a_whole_number_of_bytes_up_to_the_limit() {
        assert_eq!(read_size(None), Ok(MAX_READ));
        assert_eq!(read_size(Some(1.0)), Ok(1));
        assert_eq!(read_size(Some(65_536.0)), Ok(MAX_READ));
        for bad in [0.0, -1.0, 1.5, 65_537.0, f64::NAN, f64::INFINITY] {
            assert!(read_size(Some(bad)).is_err(), "{bad} was accepted");
        }
    }

    #[tokio::test]
    async fn a_write_lets_go_of_the_queue_once_the_writer_has_it_and_a_dropped_one_is_abandoned() {
        let (input, mut jobs) = mpsc::channel(1);
        let (closed_tx, closed) = watch::channel(false);

        // the writer has the job and has not finished it: the queue is closed all the same once its owner drops it
        let writing = tokio::spawn(submit(Some(input.clone()), b"one".to_vec(), closed.clone()));
        let job = jobs.recv().await.expect("the job");
        drop(input);
        assert!(!job.is_abandoned());
        assert!(matches!(
            tokio::time::timeout(Duration::from_millis(50), jobs.recv()).await,
            Ok(None)
        ));
        job.finish(Ok(()));
        assert!(matches!(writing.await, Ok(Ok(()))));

        // a call that is dropped leaves a job nobody waits for
        let (input, mut jobs) = mpsc::channel(1);
        let dropped = tokio::spawn(submit(Some(input), b"two".to_vec(), closed.clone()));
        let job = jobs.recv().await.expect("the job");
        dropped.abort();
        let _ = dropped.await;
        assert!(job.is_abandoned());

        // a close ends a write that waits, and no queue is a closed stream
        let (input, mut jobs) = mpsc::channel(1);
        let waiting = tokio::spawn(submit(Some(input), b"three".to_vec(), closed.clone()));
        let _job = jobs.recv().await.expect("the job");
        closed_tx.send_replace(true);
        assert!(matches!(waiting.await, Ok(Err(WriteFailed::Closed))));
        assert!(matches!(
            submit(None, b"four".to_vec(), closed).await,
            Err(WriteFailed::Closed)
        ));
    }
}
