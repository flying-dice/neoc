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
//! - the **child's** task owns the process: it sees it end, and it is the only one that kills it, so a kill can never
//!   reach another process that was given its id.
//!
//! **The process's group.** On Unix the process is started as the leader of a process group of its own, and a kill
//! is sent to the whole group: a program started through a wrapper (`sh -c`, `npx`) would otherwise live on, and keep
//! the pipes open, after the wrapper was killed. A process that moved itself to another group is not reached. The
//! group is no longer the terminal's foreground group, so a Ctrl-C typed at the engine's terminal reaches the
//! engine and not the process: the engine ends it. On Windows only the process itself is terminated.
//!
//! **Closing** ([`Process::close`]: `close()`, the handle being collected, the engine shutting down) never blocks. It
//! tells the child's task to kill the process and closes the buffers, which ends every `read` and stops the readers
//! and the writer where they wait. The child's task ends once the process is reaped. Should the runtime itself go
//! first, the task is dropped with the process in it, and tokio kills a process that is dropped (`kill_on_drop`):
//! that last resort reaches the process alone, not its group.

use std::future::Future;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{mpsc, watch};

use super::error::{Kind, ProcessError};
use super::options::{Command, Stream};
use crate::lua::byte_stream::{Job, Output, READ_CHUNK, WriteFailed, submit};
use crate::lua::command_options::existing_dir;

/// How the child's task says the process ended: `None` while it runs.
type Ended = Option<Result<Exit, String>>;

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
    /// Set to tell the child's task to kill the process.
    kill: watch::Sender<bool>,
    exit: watch::Receiver<Ended>,
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
        let (kill, killed) = watch::channel(false);
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
        tokio::spawn(watch_child(child, killed, exit_tx));

        Ok(Self {
            input: Mutex::new(input),
            end_input,
            stdout,
            stderr,
            kill,
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

    /// Ask for the process to be killed. Nothing waits here: `wait` says when it is gone.
    pub(super) fn kill(&self) {
        // a task that is gone has seen the process end
        self.kill.send_replace(true);
    }

    /// Hand `data` to the writer and wait until it is written, or until the input or the process is closed.
    pub(super) fn write(
        &self,
        data: Vec<u8>,
    ) -> impl Future<Output = Result<(), ProcessError>> + Send + use<> {
        let input = match &*self.input.lock() {
            Input::Open(input) => Ok(Some(input.clone())),
            Input::Closed => Ok(None),
            Input::NotPiped => Err(ProcessError::not_piped("stdin")),
        };
        let closed = self.closed.subscribe();
        async move {
            let input = input?;
            if data.is_empty() && input.is_some() {
                return Ok(());
            }
            submit(input, data, closed)
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
async fn stopped(mut closed: watch::Receiver<bool>, mut exit: watch::Receiver<Ended>) {
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
            // its call was dropped before anything of it was written
            if job.is_abandoned() {
                continue;
            }
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

/// Kill the process `child`, which has not been reaped, and on Unix its group.
fn kill(child: &mut Child) {
    // The id is there until the process is reaped, and only the child's task reaps it: the number is still this
    // process's, and so its group's. A group that is gone is no error to report.
    #[cfg(unix)]
    if let Some(group) = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    }
    // a process that has just ended refuses the kill, and the wait says how it ended
    let _ = child.start_kill();
}

/// The child's task: wait for the process to end, kill it when told to (or when the handle is gone), and say how
/// it ended. The wait reaps the process, so none is left as a zombie.
async fn watch_child(
    mut child: Child,
    mut killed: watch::Receiver<bool>,
    exit: watch::Sender<Ended>,
) {
    let ended = tokio::select! {
        // the end first: a process that has ended is reaped, and there is nothing to kill
        biased;
        status = child.wait() => Some(status),
        // an error is the handle dropped without a close: a kill too
        _ = killed.wait_for(|killed| *killed) => None,
    };
    let status = match ended {
        Some(status) => status,
        None => {
            kill(&mut child);
            child.wait().await
        }
    };
    exit.send_replace(Some(
        status
            .map(Exit::from)
            .map_err(|e| format!("the process could not be waited for: {e}")),
    ));
}
