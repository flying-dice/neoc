//! A live process: its three standard streams and the tasks that serve them.
//!
//! tokio's process, pipes and waits are futures, so nothing here needs a thread of its own (as `vnd:portable_pty`'s
//! blocking terminal does). Each process has up to four tasks on the runtime, and nothing the script or the engine
//! does ever waits for one of them:
//!
//! - a **reader** for each piped output stream fills a bounded buffer ([`Output`]) and stops at the end of the
//!   stream (every process that held it is gone), when reading fails, or when the process is closed;
//! - the **writer** takes one piece of input at a time ([`Job`]) and says how the write went; when its queue is
//!   dropped it lets go of the pipe, which is the end of the process's input;
//! - the **child's** task owns the process: it sees it end, and it is the only one that kills it, so a kill can never
//!   reach another process that was given its id.
//!
//! **Closing** ([`Process::close`]: `close()`, the handle being collected, the engine shutting down) never blocks. It
//! tells the child's task to kill the process, drops the writer's queue and closes the buffers, which ends every
//! `read` and stops the readers and the writer where they wait. The child's task ends once the process is reaped.
//! Should the runtime itself go first, the task is dropped with the process in it, and tokio kills a process that
//! is dropped (`kill_on_drop`).

use std::collections::VecDeque;
use std::future::Future;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{Notify, mpsc, oneshot, watch};

use super::error::{Kind, ProcessError};
use super::options::{Command, Stream};
use crate::lua::command_options::existing_dir;

/// The most output of one stream held between the process and the script. Past it the reader waits, the pipe fills,
/// and the process waits in its write: a script that reads slowly slows the process down instead of filling memory.
pub(super) const OUTPUT_BUFFER: usize = 256 * 1024;

/// How much a reader asks the operating system for at a time.
const READ_CHUNK: usize = 16 * 1024;

/// How a process ended (`std::process::ExitStatus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Exit {
    /// The code it ended with by itself.
    pub(super) code: Option<i32>,
    /// The number of the signal that ended it (Unix).
    pub(super) signal: Option<i32>,
    pub(super) success: bool,
}

impl From<ExitStatus> for Exit {
    fn from(status: ExitStatus) -> Self {
        #[cfg(unix)]
        let signal = std::os::unix::process::ExitStatusExt::signal(&status);
        #[cfg(not(unix))]
        let signal = None;
        Self {
            code: status.code(),
            signal,
            success: status.success(),
        }
    }
}

/// What `read` finds.
pub(super) enum Taken {
    Bytes(Vec<u8>),
    /// Nothing yet: wait again.
    Nothing,
    /// The end of the stream, or a closed process.
    End,
    Failed(ProcessError),
}

struct OutputState {
    bytes: VecDeque<u8>,
    /// Set once the reader stopped: `Ok` at the end of the stream, `Err` when reading failed (said by one `read`).
    ended: Option<Result<(), String>>,
    closed: bool,
}

/// What the process wrote to one stream and the script has not read yet.
pub(super) struct Output {
    state: Mutex<OutputState>,
    /// Told when there is something for `read`: bytes, the end, or a close.
    readable: Notify,
    /// Told when there is something for the reader: room, or a close.
    writable: Notify,
}

impl Output {
    fn new() -> Self {
        Self {
            state: Mutex::new(OutputState {
                bytes: VecDeque::new(),
                ended: None,
                closed: false,
            }),
            readable: Notify::new(),
            writable: Notify::new(),
        }
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

    /// Wait for room for one more chunk. `false` once the process is closed: the reader stops.
    async fn room(&self) -> bool {
        self.settled(&self.writable, |state| {
            if state.closed {
                Some(false)
            } else {
                (state.bytes.len() < OUTPUT_BUFFER).then_some(true)
            }
        })
        .await
    }

    /// Wait for the close.
    async fn closed(&self) {
        self.settled(&self.writable, |state| state.closed.then_some(()))
            .await;
    }

    /// Add what the reader read (after [`Output::room`]). Dropped when the process was closed meanwhile.
    fn push(&self, chunk: &[u8]) {
        {
            let mut state = self.state.lock();
            if state.closed {
                return;
            }
            state.bytes.extend(chunk);
        }
        self.readable.notify_waiters();
    }

    fn finish(&self, end: Result<(), String>) {
        self.state.lock().ended = Some(end);
        self.readable.notify_waiters();
    }

    fn close(&self) {
        {
            let mut state = self.state.lock();
            state.closed = true;
            state.bytes = VecDeque::new();
        }
        self.readable.notify_waiters();
        self.writable.notify_waiters();
    }

    /// Wait until [`Output::take`] has something to say. It takes nothing, so a wait that is dropped loses nothing.
    pub(super) async fn readable(&self) {
        self.settled(&self.readable, |state| {
            (state.closed || state.ended.is_some() || !state.bytes.is_empty()).then_some(())
        })
        .await;
    }

    /// At most `max` bytes of what is there.
    pub(super) fn take(&self, max: usize) -> Taken {
        let mut state = self.state.lock();
        if state.closed {
            return Taken::End;
        }
        if !state.bytes.is_empty() {
            let n = max.min(state.bytes.len());
            let bytes: Vec<u8> = state.bytes.drain(..n).collect();
            drop(state);
            self.writable.notify_waiters();
            return Taken::Bytes(bytes);
        }
        match &mut state.ended {
            None => Taken::Nothing,
            Some(Ok(())) => Taken::End,
            // said once; then it is the end
            Some(end) => match std::mem::replace(end, Ok(())) {
                Err(message) => Taken::Failed(ProcessError::new(Kind::Io, message)),
                Ok(()) => Taken::End,
            },
        }
    }
}

/// One piece of input for the writer, and where it says how the write went.
struct Job {
    data: Vec<u8>,
    done: oneshot::Sender<Result<(), String>>,
}

/// The process's input, as the script sees it.
enum Input {
    /// The writer's queue: one piece at a time, so a second `write` waits for the first.
    Open(mpsc::Sender<Job>),
    /// `close_stdin()` or `close()` was called.
    Closed,
    /// `stdin` was not `"piped"`.
    NotPiped,
}

/// The process a `Child` handle holds. Dropping it closes it.
pub(super) struct Process {
    input: Mutex<Input>,
    /// `None` when the stream was not `"piped"`.
    stdout: Option<Arc<Output>>,
    stderr: Option<Arc<Output>>,
    /// Set to tell the child's task to kill the process.
    kill: watch::Sender<bool>,
    exit: watch::Receiver<Option<Result<Exit, String>>>,
    id: Option<u32>,
    closed: watch::Sender<bool>,
    closing: AtomicBool,
}

impl Process {
    /// Start `command` and the tasks that serve it. It forks (or creates a process on Windows), so call it on the
    /// blocking pool; it must run on the runtime, where the tasks are spawned.
    pub(super) fn spawn(command: Command) -> Result<Self, ProcessError> {
        let mut child = builder(&command)?
            .spawn()
            .map_err(|e| ProcessError::new(Kind::Spawn, e.to_string()))?;
        let id = child.id();
        let (closed, _) = watch::channel(false);

        let input = match child.stdin.take() {
            Some(stdin) => {
                let (input, jobs) = mpsc::channel(1);
                tokio::spawn(write_input(stdin, jobs, closed.subscribe()));
                Input::Open(input)
            }
            None => Input::NotPiped,
        };
        let stdout = child.stdout.take().map(read_into_buffer);
        let stderr = child.stderr.take().map(read_into_buffer);

        let (kill, killed) = watch::channel(false);
        let (exit_tx, exit) = watch::channel(None);
        tokio::spawn(watch_child(child, killed, exit_tx));

        Ok(Self {
            input: Mutex::new(input),
            stdout,
            stderr,
            kill,
            exit,
            id,
            closed,
            closing: AtomicBool::new(false),
        })
    }

    /// The buffer of the process's standard output, when it is a pipe.
    pub(super) fn stdout(&self) -> Option<Arc<Output>> {
        self.stdout.clone()
    }

    /// The buffer of the process's standard error, when it is a pipe.
    pub(super) fn stderr(&self) -> Option<Arc<Output>> {
        self.stderr.clone()
    }

    fn is_closed(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    /// The process's id while it runs and is open: afterwards the number is the operating system's to give to
    /// another process.
    pub(super) fn id(&self) -> Option<u32> {
        if self.is_closed() || self.exit.borrow().is_some() {
            return None;
        }
        self.id
    }

    /// Ask for the process to be killed. Nothing waits here: `wait` says when it is gone.
    pub(super) fn kill(&self) {
        // a task that is gone has seen the process end
        self.kill.send_replace(true);
    }

    /// Hand `data` to the writer and wait until it is written, or until the process is closed.
    pub(super) fn write(
        &self,
        data: Vec<u8>,
    ) -> impl Future<Output = Result<(), ProcessError>> + Send + use<> {
        let input = match &*self.input.lock() {
            Input::Open(input) => Ok(input.clone()),
            Input::Closed => Err(ProcessError::closed("stdin")),
            Input::NotPiped => Err(ProcessError::not_piped("stdin")),
        };
        let mut closed = self.closed.subscribe();
        async move {
            let input = input?;
            if data.is_empty() {
                return Ok(());
            }
            let (done, written) = oneshot::channel();
            let write = async {
                if input.send(Job { data, done }).await.is_err() {
                    return Err(ProcessError::closed("stdin"));
                }
                match written.await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(message)) => Err(ProcessError::new(Kind::Io, message)),
                    Err(_) => Err(ProcessError::closed("stdin")),
                }
            };
            tokio::select! {
                result = write => result,
                _ = closed.wait_for(|closed| *closed) => Err(ProcessError::closed("stdin")),
            }
        }
    }

    /// Let go of the process's input once what is being written is written: the process reads the end of its
    /// input. Closing again does nothing.
    pub(super) fn close_stdin(&self) -> Result<(), ProcessError> {
        let mut input = self.input.lock();
        if matches!(*input, Input::NotPiped) {
            return Err(ProcessError::not_piped("stdin"));
        }
        *input = Input::Closed;
        Ok(())
    }

    /// Wait for the process to end.
    pub(super) fn wait(&self) -> impl Future<Output = Result<Exit, ProcessError>> + Send + use<> {
        let mut exit = self.exit.clone();
        async move {
            match exit.wait_for(Option::is_some).await {
                Ok(status) => match status.clone() {
                    Some(Ok(exit)) => Ok(exit),
                    Some(Err(message)) => Err(ProcessError::new(Kind::Io, message)),
                    None => Err(lost_child()),
                },
                Err(_) => Err(lost_child()),
            }
        }
    }

    /// Kill the process and let go of its streams. Never blocks; closing again does nothing. See the module docs.
    pub(super) fn close(&self) {
        if self.closing.swap(true, Ordering::SeqCst) {
            return;
        }
        self.kill();
        {
            let mut input = self.input.lock();
            if matches!(*input, Input::Open(_)) {
                *input = Input::Closed;
            }
        }
        for output in [&self.stdout, &self.stderr].into_iter().flatten() {
            output.close();
        }
        self.closed.send_replace(true);
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.close();
    }
}

fn lost_child() -> ProcessError {
    ProcessError::new(
        Kind::Other,
        "the task that watched the process stopped without saying how it ended",
    )
}

/// tokio's command for `command`.
fn builder(command: &Command) -> Result<tokio::process::Command, ProcessError> {
    let mut builder = tokio::process::Command::new(&command.program);
    builder.args(&command.args);
    if let Some(cwd) = &command.cwd {
        existing_dir(cwd).map_err(|message| ProcessError::new(Kind::Spawn, message))?;
        builder.current_dir(cwd);
    }
    if command.clear_env {
        builder.env_clear();
    }
    builder.envs(command.env.iter().map(|(name, value)| (name, value)));
    builder
        .stdin(stdio(command.stdin))
        .stdout(stdio(command.stdout))
        .stderr(stdio(command.stderr))
        // the last resort: the child's task was dropped with the runtime before it could kill the process
        .kill_on_drop(true);
    Ok(builder)
}

fn stdio(stream: Stream) -> Stdio {
    match stream {
        Stream::Piped => Stdio::piped(),
        Stream::Null => Stdio::null(),
        Stream::Inherit => Stdio::inherit(),
    }
}

/// Start the reader of `source` and return the buffer it fills.
fn read_into_buffer(source: impl AsyncRead + Send + Unpin + 'static) -> Arc<Output> {
    let output = Arc::new(Output::new());
    tokio::spawn(read_output(source, Arc::clone(&output)));
    output
}

/// The reader's task: one stream into its buffer until the stream ends or the process is closed.
async fn read_output(mut source: impl AsyncRead + Unpin, output: Arc<Output>) {
    let mut chunk = vec![0; READ_CHUNK];
    let end = loop {
        // room first: what was read is never held outside the buffer
        if !output.room().await {
            return;
        }
        let read = tokio::select! {
            read = source.read(&mut chunk) => read,
            () = output.closed() => return,
        };
        match read {
            Ok(0) => break Ok(()),
            Ok(n) => output.push(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => break Err(e.to_string()),
        }
    };
    output.finish(end);
}

/// The writer's task: one piece of input after the other, until its queue is dropped (`close_stdin`) or the
/// process is closed. Dropping `stdin` is the end of the process's input.
async fn write_input(
    mut stdin: ChildStdin,
    mut jobs: mpsc::Receiver<Job>,
    mut closed: watch::Receiver<bool>,
) {
    let write = async {
        while let Some(job) = jobs.recv().await {
            let written = async {
                stdin.write_all(&job.data).await?;
                stdin.flush().await
            };
            let _ = job.done.send(written.await.map_err(|e| e.to_string()));
        }
    };
    tokio::select! {
        () = write => {}
        _ = closed.wait_for(|closed| *closed) => {}
    }
}

/// The child's task: wait for the process to end, kill it when told to (or when the handle is gone), and say how
/// it ended. The wait reaps the process, so none is left as a zombie.
async fn watch_child(
    mut child: Child,
    mut killed: watch::Receiver<bool>,
    exit: watch::Sender<Option<Result<Exit, String>>>,
) {
    let ended = tokio::select! {
        status = child.wait() => Some(status),
        // an error is the handle dropped without a close: a kill too
        _ = killed.wait_for(|killed| *killed) => None,
    };
    let status = match ended {
        Some(status) => status,
        None => {
            // a process that has just ended refuses the kill, and the wait says how it ended
            let _ = child.start_kill();
            child.wait().await
        }
    };
    exit.send_replace(Some(
        status
            .map(Exit::from)
            .map_err(|e| format!("the process could not be waited for: {e}")),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn the_buffer_holds_a_bounded_amount_and_a_close_frees_the_reader() {
        // a source that never ends: the reader stops by itself once the buffer is full
        let output = read_into_buffer(tokio::io::repeat(7));
        let held = |output: &Output| output.state.lock().bytes.len();
        let settled = || async {
            let mut last = usize::MAX;
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let now = held(&output);
                if now == last {
                    return now;
                }
                last = now;
            }
        };
        let full = settled().await;
        assert!(
            (OUTPUT_BUFFER..OUTPUT_BUFFER + READ_CHUNK).contains(&full),
            "{full} bytes are held"
        );

        // a read makes room, and the reader fills it again
        let Taken::Bytes(bytes) = output.take(100_000) else {
            panic!("there are bytes to take");
        };
        assert_eq!(bytes.len(), 100_000);
        let refilled = settled().await;
        assert!((OUTPUT_BUFFER..OUTPUT_BUFFER + READ_CHUNK).contains(&refilled));

        output.close();
        assert!(matches!(output.take(10), Taken::End));
        // the reader lets go of the buffer: this is the only holder left
        tokio::time::timeout(Duration::from_secs(5), async {
            while Arc::strong_count(&output) > 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the reader stopped");
    }

    #[test]
    fn a_read_failure_is_said_once_after_the_bytes_and_then_it_is_the_end() {
        let output = Output::new();
        assert!(matches!(output.take(10), Taken::Nothing));
        output.push(b"abc");
        output.finish(Err("the pipe broke".to_owned()));
        assert!(matches!(output.take(2), Taken::Bytes(bytes) if bytes == b"ab"));
        assert!(matches!(output.take(2), Taken::Bytes(bytes) if bytes == b"c"));
        let Taken::Failed(e) = output.take(2) else {
            panic!("the failure follows the bytes");
        };
        assert_eq!(e.kind(), Kind::Io);
        assert!(matches!(output.take(2), Taken::End));
    }
}
