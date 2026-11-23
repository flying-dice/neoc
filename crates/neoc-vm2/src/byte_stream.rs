//! The byte streams between a script and a process it started: what `std:process` has, and what a terminal
//! (neoc-vm's `vnd:portable_pty`) will share with it.
//!
//! - [`Output`] holds what the process wrote and the script has not read yet, up to [`OUTPUT_BUFFER`] bytes. Its
//!   reader, a task, waits for room ([`Output::room`], then [`Output::push`]), so a script that reads slowly slows
//!   the process down instead of filling memory.
//! - [`read`] is the body of a method that reads from one: it waits without the VM and takes the bytes with the
//!   turn held, so a call that is dropped loses none.
//! - [`Job`] is one piece of input on its way to a writer, and [`submit`] hands one over and waits for it.
//!
//! What fails here is said in words ([`Read::Failed`], [`WriteFailed`], a `String`): each module makes its own
//! typed error of them.
//!
//! Ported from neoc-vm's `byte_stream.rs`. Left behind: the reader on a thread (`Output::push_blocking`), which
//! only the terminal has; it comes with that module.

use std::collections::VecDeque;

use parking_lot::Mutex;
use tokio::sync::{Notify, mpsc, oneshot, watch};

use crate::lifecycle::idle;

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

/// A read's `max`: from 1 to [`MAX_READ`] bytes, and [`MAX_READ`] when it is not said.
pub(crate) fn read_size(max: Option<u32>) -> Result<usize, String> {
    match max.map(usize::try_from) {
        None => Ok(MAX_READ),
        Some(Ok(max)) if (1..=MAX_READ).contains(&max) => Ok(max),
        Some(_) => Err(format!("`max` must be a whole number from 1 to {MAX_READ}")),
    }
}

/// The bytes of one write: at most [`MAX_WRITE`] of them.
pub(crate) fn write_data(data: Vec<u8>) -> Result<Vec<u8>, String> {
    if data.len() > MAX_WRITE {
        Err(format!("one write takes at most {MAX_WRITE} bytes"))
    } else {
        Ok(data)
    }
}

/// What is there for a read.
enum Taken {
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
    /// Where the reader waits for room, or for the close.
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
            room: Notify::new(),
        }
    }

    /// Tell the reader that there is room or that the stream is closed.
    fn wake_reader(&self) {
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

    /// For the reader: wait for room for one more chunk. `false` once the stream is closed: the reader
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

    /// For the reader: wait for the close.
    pub(crate) async fn closed(&self) {
        self.settled(&self.room, |state| state.closed.then_some(()))
            .await;
    }

    /// For the reader: add what it read (after [`Output::room`]). Dropped when the stream was closed
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
    async fn readable(&self) {
        self.settled(&self.readable, |state| {
            (state.closed || state.ended.is_some() || !state.bytes.is_empty()).then_some(())
        })
        .await;
    }

    /// At most `max` bytes of what is there.
    fn take(&self, max: usize) -> Taken {
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

/// What a read returns.
pub(crate) enum Read {
    Bytes(Vec<u8>),
    /// The end of the output, or a closed stream.
    End,
    /// Reading failed, in the operating system's words. Said once; the read after it is the end.
    Failed(String),
}

/// The body of a method that reads from an [`Output`]: at most `max` bytes, as soon as there are any.
///
/// The method is a `Role::Mixed` one, not a `Role::Wait` (as `std:fs`'s `Watcher:receive`): a `Wait` body would
/// take the bytes when its wait ends and only then queue for the turn, and a call dropped there would lose them.
/// The wait for output is an `idle` wait, and the bytes are taken with the turn held, in the poll that returns them.
///
/// # Errors
/// The call cannot wait where it was made.
pub(crate) async fn read(output: &Output, max: usize) -> mlua::Result<Read> {
    loop {
        idle(output.readable()).await?;
        match output.take(max) {
            Taken::Bytes(bytes) => return Ok(Read::Bytes(bytes)),
            Taken::Nothing => {}
            Taken::End => return Ok(Read::End),
            Taken::Failed(message) => return Ok(Read::Failed(message)),
        }
    }
}

/// One piece of input for a writer, and where it says how the write went.
pub(crate) struct Job {
    pub(crate) data: Vec<u8>,
    done: oneshot::Sender<Result<(), String>>,
}

impl Job {
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
/// (dropping the queue is one way a stream's input is ended). A job that is queued is the writer's: a call that
/// is dropped, or whose task is cancelled, does not take it back.
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
    use std::sync::Arc;
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
        output.push(b"abc");
        output.finish(Err("the stream broke".to_owned()));
        assert!(matches!(output.take(2), Taken::Bytes(bytes) if bytes == b"ab"));
        assert!(matches!(output.take(2), Taken::Bytes(bytes) if bytes == b"c"));
        assert!(matches!(output.take(2), Taken::Failed(message) if message == "the stream broke"));
        assert!(matches!(output.take(2), Taken::End));
    }

    /// A read waits for output and takes no more than it was told; the end is said every time it is asked for.
    #[tokio::test]
    async fn a_read_waits_for_the_output_and_takes_what_it_was_told() {
        let output = Arc::new(Output::new());
        let reading = {
            let output = Arc::clone(&output);
            tokio::spawn(async move { read(&output, 2).await })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!reading.is_finished(), "nothing was written yet");
        output.push(b"abc");
        assert!(matches!(reading.await, Ok(Ok(Read::Bytes(bytes))) if bytes == b"ab"));
        assert!(matches!(read(&output, 2).await, Ok(Read::Bytes(bytes)) if bytes == b"c"));
        output.finish(Ok(()));
        assert!(matches!(read(&output, 2).await, Ok(Read::End)));
        assert!(matches!(read(&output, 2).await, Ok(Read::End)));
    }

    #[test]
    fn a_read_takes_a_whole_number_of_bytes_up_to_the_limit() {
        assert_eq!(read_size(None), Ok(MAX_READ));
        assert_eq!(read_size(Some(1)), Ok(1));
        assert_eq!(read_size(Some(65_536)), Ok(MAX_READ));
        for bad in [0, 65_537, u32::MAX] {
            assert!(read_size(Some(bad)).is_err(), "{bad} was accepted");
        }
        assert_eq!(write_data(vec![0; MAX_WRITE]).map(|data| data.len()), Ok(MAX_WRITE));
        assert!(write_data(vec![0; MAX_WRITE + 1]).is_err());
    }

    #[tokio::test]
    async fn a_write_lets_go_of_the_queue_once_the_writer_has_it_and_a_close_ends_one_that_waits() {
        let (input, mut jobs) = mpsc::channel(1);
        let (closed_tx, closed) = watch::channel(false);

        // the writer has the job and has not finished it: the queue is closed all the same once its owner drops it
        let writing = tokio::spawn(submit(Some(input.clone()), b"one".to_vec(), closed.clone()));
        let job = jobs.recv().await.expect("the job");
        drop(input);
        assert!(matches!(
            tokio::time::timeout(Duration::from_millis(50), jobs.recv()).await,
            Ok(None)
        ));
        job.finish(Ok(()));
        assert!(matches!(writing.await, Ok(Ok(()))));

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
