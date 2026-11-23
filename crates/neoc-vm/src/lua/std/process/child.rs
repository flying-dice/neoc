//! A live process: its three standard streams and the tasks that serve them.
//!
//! tokio's process, pipes and waits are futures, so nothing here needs a thread of its own (as `vnd:portable_pty`'s
//! blocking terminal does). Each process has up to four tasks on the runtime, and nothing the script or the engine
//! does ever waits for one of them:
//!
//! - a **reader** for each piped output stream fills a bounded buffer ([`Output`]) and stops at the end of the
//!   stream (every process that held it is gone), when reading fails, or when the process is closed;
//! - the **writer** takes one piece of input at a time ([`Job`]) and says how the write went; it lets go of the
//!   pipe, which is the end of the process's input, when it is told to (`close_stdin`), when the process has ended
//!   and when the process is closed;
//! - the **child's** task waits for the process, which reaps it, and says how it ended. A kill is made by whoever
//!   asks for it, at once, under the lock the reaping takes ([`Owned`]): it can never reach another process that was
//!   given the id.
//!
//! **The process's group.** On Unix the process is started as the leader of a process group of its own, and a kill
//! is sent to the whole group: a program started through a wrapper (`sh -c`, `npx`) would otherwise live on, and keep
//! the pipes open, after the wrapper was killed. The kill reaches the process itself in any case; of what it
//! started, it misses only a process that moved itself to another group. On Windows only the process itself is
//! terminated.
//!
//! The group is not the foreground group of the engine's terminal. A Ctrl-C typed there reaches the engine and not
//! the process, so a script that should end on one ends its processes itself (`std:workers`' shutdown signal). And a
//! process that reads an inherited standard input which is that terminal is stopped by the terminal (`SIGTTIN`), as
//! every background job is: an inherited input is for an engine whose own input is a pipe or a file.
//!
//! **Closing** ([`Process::close`]: `close()`, the handle being collected, the engine shutting down) never blocks. It
//! kills the process and closes the buffers, which ends every `read` and stops the readers and the writer where they
//! wait. The child's task ends once the process is reaped. Should the runtime itself go first, the task is dropped,
//! and it kills the process as it goes (`KillWithTask`). A program that ends by
//! `std::process::exit` drops nothing: it calls [`kill_all`] first ([`crate::exit`] does).

use std::future::Future;
use std::process::{ExitStatus, Stdio};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, watch};

use super::error::{Kind, ProcessError};
use super::options::{Command, Stream};
use crate::lua::byte_stream::{Job, Output, READ_CHUNK, WriteFailed, submit};
use crate::lua::child_exit::{self, Ended, WaitFailed};
use crate::lua::command_options::existing_dir;

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
    /// Set to tell the writer to let go of the process's input.
    end_input: watch::Sender<bool>,
    /// `None` when the stream was not `"piped"`.
    stdout: Option<Arc<Output>>,
    stderr: Option<Arc<Output>>,
    child: Arc<Owned>,
    exit: watch::Receiver<Ended<Exit>>,
    id: Option<u32>,
    /// Set by [`Process::close`].
    closed: watch::Sender<bool>,
}

impl Process {
    /// Start `command` and the tasks that serve it. It forks (or creates a process on Windows), so call it on the
    /// blocking pool; it must run on the runtime, where the tasks are spawned.
    pub(super) fn spawn(command: &Command) -> Result<Self, ProcessError> {
        let mut child = builder(command)?
            .spawn()
            .map_err(|e| ProcessError::new(Kind::Spawn, e.to_string()))?;
        let id = child.id();
        let (closed, _) = watch::channel(false);
        let (end_input, input_ended) = watch::channel(false);
        let (exit_tx, exit) = watch::channel(None);

        let input = match child.stdin.take() {
            Some(stdin) => {
                let (input, jobs) = mpsc::channel(1);
                let stop = stopped(closed.subscribe(), exit.clone());
                tokio::spawn(write_input(stdin, jobs, input_ended, stop));
                Input::Open(input)
            }
            None => Input::NotPiped,
        };
        let stdout = child.stdout.take().map(read_into_buffer);
        let stderr = child.stderr.take().map(read_into_buffer);
        let child = Owned::new(child);
        tokio::spawn(watch_child(Arc::clone(&child), exit_tx));

        Ok(Self {
            input: Mutex::new(input),
            end_input,
            stdout,
            stderr,
            child,
            exit,
            id,
            closed,
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
        *self.closed.borrow()
    }

    /// The process's id while it runs and is open: afterwards the number is the operating system's to give to
    /// another process.
    pub(super) fn id(&self) -> Option<u32> {
        if self.is_closed() || self.exit.borrow().is_some() {
            return None;
        }
        self.id
    }

    /// Kill the process. Nothing waits here: `wait` says when it is gone.
    pub(super) fn kill(&self) {
        self.child.kill();
    }

    /// Hand `data` to the writer and wait until it is written, or until the input or the process is closed.
    pub(super) fn write(
        &self,
        data: Vec<u8>,
    ) -> impl Future<Output = Result<(), ProcessError>> + Send + use<> {
        let input = match &*self.input.lock() {
            Input::NotPiped => Err(ProcessError::not_piped("stdin")),
            // the writer lets go of the pipe when the process ends: refused here, so that no write races that
            Input::Open(input) if self.exit.borrow().is_none() => Ok(Some(input.clone())),
            Input::Open(_) | Input::Closed => Ok(None),
        };
        let closed = self.closed.subscribe();
        async move {
            submit(input?, data, closed)
                .await
                .map_err(|failed| match failed {
                    WriteFailed::Closed => ProcessError::closed("stdin"),
                    WriteFailed::Io(message) => ProcessError::new(Kind::Io, message),
                })
        }
    }

    /// Let go of the process's input once the write being made is made: the process reads the end of its input.
    /// Writes that wait behind that one are refused. Closing again does nothing.
    pub(super) fn close_stdin(&self) -> Result<(), ProcessError> {
        let mut input = self.input.lock();
        if matches!(*input, Input::NotPiped) {
            return Err(ProcessError::not_piped("stdin"));
        }
        *input = Input::Closed;
        self.end_input.send_replace(true);
        Ok(())
    }

    /// Wait for the process to end.
    pub(super) fn wait(&self) -> impl Future<Output = Result<Exit, ProcessError>> + Send + use<> {
        let ended = child_exit::ended(self.exit.clone());
        async move {
            ended.await.map_err(|failed| match failed {
                WaitFailed::Io(message) => ProcessError::new(Kind::Io, message),
                WaitFailed::Lost => ProcessError::new(
                    Kind::Other,
                    "the task that watched the process stopped without saying how it ended",
                ),
            })
        }
    }

    /// Kill the process and let go of its streams. Never blocks; closing again does nothing. See the module docs.
    pub(super) fn close(&self) {
        if self.closed.send_replace(true) {
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
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.close();
    }
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
        // `Owned` kills the process and its group when it is dropped; this has tokio reap it afterwards
        .kill_on_drop(true);
    // the leader of a group of its own, so that a kill reaches what it started (see the module docs)
    #[cfg(unix)]
    builder.process_group(0);
    Ok(builder)
}

fn stdio(stream: Stream) -> Stdio {
    match stream {
        Stream::Piped => Stdio::piped(),
        Stream::Null => Stdio::null(),
        Stream::Inherit => Stdio::inherit(),
    }
}

/// Resolves when the process is closed or has ended: when its writer has nothing left to do.
async fn stopped(mut closed: watch::Receiver<bool>, mut exit: watch::Receiver<Ended<Exit>>) {
    // an error is the sender gone, which is the handle or the child's task gone: a stop too
    tokio::select! {
        _ = closed.wait_for(|closed| *closed) => {}
        _ = exit.wait_for(Option::is_some) => {}
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

/// The writer's task: one piece of input after the other, until it is told to end the input (`close_stdin`) or
/// `stop` resolves. Dropping `stdin` is the end of the process's input; the jobs still queued are dropped with the
/// queue, which refuses their writes.
async fn write_input(
    mut stdin: ChildStdin,
    mut jobs: mpsc::Receiver<Job>,
    mut input_ended: watch::Receiver<bool>,
    stop: impl Future<Output = ()>,
) {
    let write = async {
        loop {
            let job = tokio::select! {
                // the end first: a write that waited behind the one being made is refused, not made
                biased;
                _ = input_ended.wait_for(|ended| *ended) => break,
                job = jobs.recv() => job,
            };
            let Some(job) = job else { break };
            let written = async {
                stdin.write_all(&job.data).await?;
                stdin.flush().await
            }
            .await;
            job.finish(written);
        }
    };
    tokio::select! {
        () = write => {}
        () = stop => {}
    }
}

/// The operating system's process, shared by the task that waits for it and by whoever kills it.
///
/// A kill must never reach a process that was reaped: its id is then the operating system's to give away, to a
/// process that is not ours. So the wait that reaps (in [`Owned::ended`]) and every kill ([`Owned::kill`]) take the
/// same lock, and a kill looks first: tokio has an id for the process only until it has reaped it. That makes the
/// kill synchronous, which is what an engine that is about to exit needs ([`kill_all`]): no task has to run for it.
struct Owned {
    child: Mutex<Child>,
}

/// Every process that may still run, for [`kill_all`]. Weakly: a process is let go of with its handle and its task.
static LIVE: Mutex<Vec<Weak<Owned>>> = Mutex::new(Vec::new());

impl Owned {
    fn new(child: Child) -> Arc<Self> {
        let owned = Arc::new(Self {
            child: Mutex::new(child),
        });
        let mut live = LIVE.lock();
        live.retain(|other| other.strong_count() > 0);
        live.push(Arc::downgrade(&owned));
        owned
    }

    /// Kill the process if it has not been reaped, and on Unix its group. A group that is gone, or a process that
    /// has just ended, is no error to report: the wait says how it ended.
    fn kill(&self) {
        let mut child = self.child.lock();
        let Some(id) = child.id() else {
            return;
        };
        #[cfg(unix)]
        if let Some(group) = i32::try_from(id)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        #[cfg(not(unix))]
        let _ = id;
        let _ = child.start_kill();
    }

    /// Wait for the process to end. This is the wait that reaps it, so none is left as a zombie.
    async fn ended(&self) -> std::io::Result<ExitStatus> {
        std::future::poll_fn(|cx| {
            // the lock is held for the poll only: a kill is never made while the process is being reaped
            let mut child = self.child.lock();
            let wait = std::pin::pin!(child.wait());
            wait.poll(cx)
        })
        .await
    }
}

impl Drop for Owned {
    /// The handle and the task are both gone (the runtime was dropped with the task in it): the process and its
    /// group are killed here. tokio's own `kill_on_drop` would reach the process alone.
    fn drop(&mut self) {
        self.kill();
    }
}

/// Kill every process any script of this program started and that still runs, at once and without waiting. For a
/// program that is about to end by `std::process::exit`, which runs no destructor and no task: without this the
/// processes would live on. See [`crate::exit`].
pub fn kill_all() {
    let live: Vec<Arc<Owned>> = LIVE.lock().iter().filter_map(Weak::upgrade).collect();
    for owned in live {
        owned.kill();
    }
}

/// Kills the process when the child's task is dropped before the process ended: the runtime went, and the engine,
/// which may live on, still holds the handle. Nothing would wait for the process any more, so it is reaped here: a
/// process that was sent the kill it cannot refuse is gone at once, and the wait for it is short and bounded.
struct KillWithTask(Arc<Owned>);

impl Drop for KillWithTask {
    fn drop(&mut self) {
        self.0.kill();
        for _ in 0..200 {
            // `Ok(None)` is a process that still runs; an error is one there is nothing left to wait for
            if !matches!(self.0.child.lock().try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// The child's task: wait for the process to end and say how it ended.
async fn watch_child(child: Arc<Owned>, exit: watch::Sender<Ended<Exit>>) {
    let child = KillWithTask(child);
    let status = child.0.ended().await;
    exit.send_replace(Some(
        status
            .map(Exit::from)
            .map_err(|e| format!("the process could not be waited for: {e}")),
    ));
}
