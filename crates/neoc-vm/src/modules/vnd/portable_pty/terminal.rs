//! A live terminal: the pseudo-terminal, the process on it, and the three threads that do its blocking work.
//!
//! The crate's reader, writer and child are blocking: a read returns when the process writes, a write when the
//! process has room for it, a wait when the process ends. None of them can be interrupted from outside. So each has
//! a thread of its own, and nothing the script or the engine does ever waits for one of those threads:
//!
//! - the **reader** fills a bounded buffer ([`Output`]) and stops when the terminal's other end is closed (every
//!   process that held it is gone), when reading fails, or when the terminal is closed;
//! - the **writer** takes one piece of input at a time ([`Job`]) and says how the write went;
//! - the **child's** thread owns the process: it sees it end, and it is the only one that signals it.
//!
//! Not the runtime's blocking pool (`spawn_blocking`, which `std:fs` uses for work that ends by itself): a read lasts
//! as long as the process is silent, a pool thread would be held all that time, and a runtime waits for its pool when
//! it is dropped, so one silent shell would keep the program from ending.
//!
//! **Closing** ([`Terminal::close`]: `close()`, the handle being collected, the engine shutting down) never blocks. It
//! tells the child's thread to end the process, drops the writer's queue and the terminal's own descriptor, and
//! closes the buffer, which ends every `read` and frees a reader that was waiting for room. The threads then end by
//! themselves: the child's once the process is gone (a quarter of a second later at most, the crate's grace between
//! the hangup and the kill), the reader and the writer once
//! the terminal's other end is closed, which the process's end does. A process that outlives its parent and keeps the
//! terminal open (one started with `nohup`, say) keeps the reader in its read until it ends too: that costs a parked
//! thread and a descriptor, and holds neither the engine nor the runtime.
//!
//! **Windows.** A pseudo-console keeps its output pipe open for as long as it exists, whoever is still attached to
//! it: the reader would never see the end. So the child's thread lets go of the pseudo-console when the process has
//! ended ([`release`]), which flushes what it still had to draw and ends the output; and a reader whose buffer was
//! closed reads on to the end of the pipe and throws it away, because the pseudo-console's host cannot go while
//! output it wrote is unread.

use std::future::Future;
use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::mpsc::{self as std_mpsc, RecvTimeoutError};
use std::time::Duration;

use ::portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use parking_lot::Mutex;
use tokio::sync::{mpsc, watch};

use super::options::Command;
use super::portable_pty::{ExitStatus as Exit, PtyError};
use crate::byte_stream::{Job, Output, READ_CHUNK, WriteFailed, submit};
use crate::child_exit::{self, Ended, WaitFailed};
use crate::command_options::existing_dir;

/// How long the child's thread sleeps between two looks at a process that gave no sign of ending. The end of the
/// output wakes it at once (see [`Signal::Look`]), so this is only how late `wait()` learns of a process that ended
/// while another one still holds its terminal (and, on Windows, of every process's end: there the output ends
/// after the process, not before).
const POLL: Duration = Duration::from_millis(100);

/// The first sleep after a sign that the process is ending; it doubles up to [`POLL`].
const POLL_SOON: Duration = Duration::from_millis(1);

/// The stack of each thread: they hold a small buffer and call the operating system.
const THREAD_STACK: usize = 128 * 1024;

/// What the child's thread is told.
enum Signal {
    /// End the process.
    Kill,
    /// The process may have ended (its output did): look now, not at the next [`POLL`].
    Look,
}

/// The terminal's own end: resized here, let go of at close (and, on Windows, when the process has ended).
type Master = Arc<Mutex<Option<Box<dyn MasterPty + Send>>>>;

/// Let go of the terminal's own end. The lock is not held while the operating system closes it.
fn release(master: &Master) {
    let taken = master.lock().take();
    drop(taken);
}

/// The terminal a `Pty` handle holds. Dropping it closes it.
pub(super) struct Terminal {
    output: Arc<Output>,
    master: Master,
    /// The writer's queue: one piece at a time, so a second `write` waits for the first.
    input: Mutex<Option<mpsc::Sender<Job>>>,
    child: Mutex<std_mpsc::Sender<Signal>>,
    exit: watch::Receiver<Ended<Exit>>,
    process_id: Option<u32>,
    closed: watch::Sender<bool>,
}

impl Terminal {
    /// Open a terminal and start `command` on it. Blocking (it forks, or starts a console host on Windows): call
    /// it on the blocking pool.
    pub(super) fn spawn(command: Command) -> Result<Self, PtyError> {
        let size = PtySize { rows: command.rows, cols: command.cols, pixel_width: 0, pixel_height: 0 };
        let builder = builder(command)?;
        let pair = native_pty_system().openpty(size).map_err(not_started)?;
        let child = pair.slave.spawn_command(builder).map_err(not_started)?;
        // the process has its own copy of the terminal's other end: with ours gone, its end is the end of the output
        drop(pair.slave);
        let process_id = child.process_id();

        // The reader and the writer first: on Windows the child's thread lets go of the terminal when the process
        // has ended, which may be at once.
        let (signals, signalled) = std_mpsc::channel();
        let (closed, _) = watch::channel(false);
        let attached = attach(pair.master.as_ref(), &signals, closed.subscribe());
        let master: Master = Arc::new(Mutex::new(Some(pair.master)));

        // from here on the child's thread owns the process, and ends it if the rest was not set up
        let (exit_tx, exit) = watch::channel(None);
        let ended = Arc::clone(&master);
        thread("neoc-pty-child", move || {
            watch_child(child, &signalled, &exit_tx);
            if cfg!(windows) {
                release(&ended);
            }
        })?;
        let (output, input) = match attached {
            Ok(attached) => attached,
            Err(e) => {
                let _ = signals.send(Signal::Kill);
                return Err(e);
            }
        };
        Ok(Self {
            output,
            master,
            input: Mutex::new(Some(input)),
            child: Mutex::new(signals),
            exit,
            process_id,
            closed,
        })
    }

    pub(super) fn output(&self) -> Arc<Output> {
        Arc::clone(&self.output)
    }

    pub(super) fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    /// The process's id while it runs and the terminal is open: afterwards the number is the operating system's to
    /// give to another process.
    pub(super) fn process_id(&self) -> Option<u32> {
        if self.is_closed() || self.exit.borrow().is_some() {
            return None;
        }
        self.process_id
    }

    pub(super) fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let master = self.master.lock();
        let Some(master) = master.as_ref() else {
            return Err(PtyError::Closed);
        };
        master
            .resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| PtyError::Io { reason: format!("{e:#}") })
    }

    /// Ask for the process to be ended. Nothing waits here: `wait` says when it is gone.
    pub(super) fn kill(&self) {
        // a thread that is gone has seen the process end
        let _ = self.child.lock().send(Signal::Kill);
    }

    /// Hand `data` to the writer and wait until it is written, or until the terminal is closed.
    pub(super) fn write(&self, data: Vec<u8>) -> impl Future<Output = Result<(), PtyError>> + Send + use<> {
        let write = submit(self.input.lock().clone(), data, self.closed.subscribe());
        async move {
            write.await.map_err(|failed| match failed {
                WriteFailed::Closed => PtyError::Closed,
                WriteFailed::Io(reason) => PtyError::Io { reason },
            })
        }
    }

    /// Wait for the process to end.
    pub(super) fn wait(&self) -> impl Future<Output = Result<Exit, PtyError>> + Send + use<> {
        let ended = child_exit::ended(self.exit.clone());
        async move {
            ended.await.map_err(|failed| match failed {
                WaitFailed::Io(reason) => PtyError::Io { reason },
                WaitFailed::Lost => PtyError::Other {
                    reason: "the thread that watched the process stopped without saying how it ended"
                        .to_owned(),
                },
            })
        }
    }

    /// End the process and let go of the terminal. Never blocks; closing again does nothing. See the module docs.
    pub(super) fn close(&self) {
        if self.closed.send_replace(true) {
            return;
        }
        self.kill();
        drop(self.input.lock().take());
        // the buffer first: on Windows the reader must be reading on (not waiting for room) for the
        // pseudo-console to go
        self.output.close();
        release(&self.master);
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.close();
    }
}

/// The crate's error (an `anyhow` one, with its causes) as the reason a command was not started.
fn not_started(e: impl std::fmt::Display) -> PtyError {
    PtyError::Spawn { reason: format!("{e:#}") }
}

/// Start the reader and the writer of the terminal `master` is the near end of.
fn attach(
    master: &(dyn MasterPty + Send),
    signals: &std_mpsc::Sender<Signal>,
    closed: watch::Receiver<bool>,
) -> Result<(Arc<Output>, mpsc::Sender<Job>), PtyError> {
    let reader = master.try_clone_reader().map_err(|e| PtyError::Other { reason: format!("{e:#}") })?;
    let writer = master.take_writer().map_err(|e| PtyError::Other { reason: format!("{e:#}") })?;
    let output = Arc::new(Output::new());
    let (input, jobs) = mpsc::channel(1);
    {
        let output = Arc::clone(&output);
        let look = signals.clone();
        thread("neoc-pty-read", move || {
            read_output(reader, &output);
            // the process is ending or has ended: the child's thread looks now
            let _ = look.send(Signal::Look);
        })?;
    }
    thread("neoc-pty-write", move || {
        write_input(writer, jobs, &closed);
    })?;
    Ok((output, input))
}

fn thread(name: &str, body: impl FnOnce() + Send + 'static) -> Result<(), PtyError> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .stack_size(THREAD_STACK)
        .spawn(body)
        .map(drop)
        .map_err(|e| PtyError::Other { reason: format!("the terminal's thread could not be started: {e}") })
}

/// The crate's command for `command`. The directory is checked here: the crate falls back to the home directory
/// without a word when the one it was given is not there.
fn builder(command: Command) -> Result<CommandBuilder, PtyError> {
    let mut builder = match &command.program {
        Some(program) => {
            let mut builder = CommandBuilder::new(program);
            builder.args(&command.args);
            builder
        }
        None => CommandBuilder::new_default_prog(),
    };
    if let Some(cwd) = &command.cwd {
        existing_dir(cwd).map_err(|reason| PtyError::Spawn { reason })?;
        builder.cwd(cwd);
    }
    // On Windows the crate's builder does not start from the engine's environment alone: it reads the machine's
    // and the user's variables from the registry and puts them over it, so a variable the engine was started with
    // (`PATH`, where a bare `program` is looked up; `ComSpec`, the default shell) would be lost to the registry's.
    // The engine's own go back on top: the process has the engine's environment, as on Unix, and what only the
    // registry has stays.
    #[cfg(windows)]
    for (name, value) in std::env::vars_os() {
        builder.env(name, value);
    }
    for (name, value) in &command.env {
        builder.env(name, value);
    }
    Ok(builder)
}

/// The reader's thread: the terminal's output into the buffer until its other end is closed.
fn read_output(mut reader: Box<dyn Read + Send>, output: &Output) {
    let mut chunk = vec![0; READ_CHUNK];
    let end = loop {
        match reader.read(&mut chunk) {
            Ok(0) => break Ok(()),
            Ok(n) => {
                if !output.push_blocking(&chunk[..n]) {
                    if cfg!(windows) {
                        discard(reader, &mut chunk);
                    }
                    return;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            // Windows ends a console's pipe with an error, not with a read of nothing
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => break Ok(()),
            Err(e) => break Err(e.to_string()),
        }
    };
    output.finish(end);
}

/// Read to the end of a closed terminal's output and keep none of it (Windows: see the module docs).
fn discard(mut reader: Box<dyn Read + Send>, chunk: &mut [u8]) {
    loop {
        match reader.read(chunk) {
            Ok(0) => return,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

/// The writer's thread: one piece of input after the other, until the terminal is closed. Dropping the writer says
/// "end of input" to the process (the crate sends the terminal's end-of-file character).
fn write_input(
    mut writer: Box<dyn Write + Send>,
    mut jobs: mpsc::Receiver<Job>,
    closed: &watch::Receiver<bool>,
) {
    while let Some(job) = jobs.blocking_recv() {
        if *closed.borrow() {
            break;
        }
        let written = writer.write_all(&job.data).and_then(|()| writer.flush());
        job.finish(written);
    }
}

/// The child's thread: look at the process until it has ended, end it when told to (or when the terminal is
/// dropped), and say how it ended. The process is signalled only here and only before it is reaped, so a signal can
/// never reach another process that was given its id.
fn watch_child(
    mut child: Box<dyn Child + Send + Sync>,
    signals: &std_mpsc::Receiver<Signal>,
    exit: &watch::Sender<Ended<Exit>>,
) {
    let mut pause = POLL_SOON;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        match signals.recv_timeout(pause) {
            Ok(Signal::Look) => pause = POLL_SOON,
            Err(RecvTimeoutError::Timeout) => pause = (pause * 2).min(POLL),
            // The crate's kill: on Unix a hangup, a grace for the process to end by itself, then the kill that
            // cannot be refused; on Windows the process is terminated. Then the wait that reaps it.
            Ok(Signal::Kill) | Err(RecvTimeoutError::Disconnected) => {
                let _ = child.kill();
                break child.wait();
            }
        }
    };
    exit.send_replace(Some(
        status
            .map(|status| Exit {
                exit_code: status.exit_code(),
                signal: status.signal().map(str::to_owned),
                success: status.success(),
            })
            .map_err(|e| format!("the process could not be waited for: {e}")),
    ));
}
