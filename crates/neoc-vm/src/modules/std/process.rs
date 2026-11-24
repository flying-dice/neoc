//! Child processes: `std:process` (mirrors `std::process`: a command, the child it starts, its three standard
//! streams and its exit status).
//!
//! ```lua
//! local process = require("std:process")
//!
//! -- a long-lived helper that speaks a protocol over its standard streams
//! local agent = assert(process.spawn({ program = "agent", args = { "--stdio" }, env = { NO_COLOR = "1" } }))
//! assert(agent:write('{"jsonrpc":"2.0","id":1,"method":"initialize"}\n'))
//! local reply = assert(agent:read())
//! agent:close_stdin() -- the end of its input: a well-behaved helper ends by itself
//! local status = assert(agent:wait())
//! print(status.code, status.success)
//! agent:close()
//! ```
//!
//! **One handle.** `std::process` has a `Command` that is built up and a `Child` with a field for each stream; here
//! `spawn` takes the command as one table and returns one `Child`: the process (`id`, `wait`, `kill`) and the near
//! ends of its streams (`write` and `close_stdin` for its input, `read` and `read_stderr` for its output).
//!
//! **Streams.** Each of `stdin`, `stdout` and `stderr` is `"piped"` (the default: a pipe to the script), `"null"`
//! (nothing) or `"inherit"` (the engine's own stream). Bytes pass as they are: nothing is decoded, echoed or
//! joined, and a read returns what has arrived, which may end in the middle of a line. They are not `std:io`'s
//! `Reader` and `Writer`: those block the script while they read and write, and a pipe is waited for.
//!
//! **Errors.** A call returns `(value, nil)` or `(nil, err)`, where `err` is a
//! [`ProcessError`](process::ProcessError): its `kind` is stable, and what the operating system said is its
//! `reason`. An argument of the wrong type is raised, as everywhere.
//!
//! **Waiting.** `spawn`, `read`, `read_stderr`, `write` and `wait` wait without holding the VM: other tasks run
//! meanwhile, so one script holds many processes at once.
//!
//! **Limits.** A read returns at most 64 KiB (`byte_stream::MAX_READ`) and a write takes at most
//! 1 MiB (`byte_stream::MAX_WRITE`); at most 256 KiB (`byte_stream::OUTPUT_BUFFER`) of each output
//! stream wait for the script, and a process that writes more waits until they are read. So a piped stream nobody
//! reads stops the process once it is full: read it, or do not pipe it. Nothing limits how many processes a script
//! starts.
//!
//! **What starting a process means.** The process runs as the user the engine runs as: it can do what that user
//! can. Nothing here narrows that, as nothing narrows `std:fs`: a script that is given this module is trusted with
//! the machine.
//!
//! **Lifetime.** A process belongs to the script that started it. `close()`, the handle being collected and the
//! engine shutting down all do the same: the process is killed and its streams are let go of. On Unix the kill
//! reaches the process's whole group, so what it started goes with it (`child`'s docs say how).
//!
//! The API's rules:
//! - An error is a typed [`ProcessError`](process::ProcessError) (`err.kind`, and `err.cause` of a `spawn` error,
//!   `err.stream` of a `not_piped` one, `err.what` of a `closed` one).
//! - A command of the wrong shape (no table, an unknown key, a value of the wrong type) is raised with its place, as
//!   every wrong argument is. What the shape does not say (an empty program, a NUL byte, too many arguments) is an
//!   `invalid_input` error.
//! - `read` and `read_stderr` take a whole number for `max`: a fraction, a negative number or NaN is raised; 0 and more
//!   than 65536 are `invalid_input`.
//! - `program` and `cwd` are paths: on Unix they need not be UTF-8.

mod child;
pub use child::kill_all;

#[cfg(test)]
mod tests;

#[mlua_ir::module("std:process")]
pub mod process {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{Bytes, LuaError, Object, Shape};
    use serde::{Deserialize, Serialize};

    use super::child::Process;
    use crate::byte_stream::{self, Output, Read};
    use crate::command_options::{self, Refused};
    use crate::lifecycle::{Life, hold};
    use crate::modules::std::osstr::OsPath;

    /// What one of a process's three standard streams is (`std::process::Stdio`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Stdio {
        /// A pipe between the process and the script.
        #[default]
        Piped,
        /// Nothing: the process reads the end of its input at once, and what it writes is thrown away.
        Null,
        /// The engine's own stream.
        Inherit,
    }

    /// One of a process's three standard streams.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Stream {
        /// Its standard input.
        Stdin,
        /// Its standard output.
        Stdout,
        /// Its standard error.
        Stderr,
    }

    impl std::fmt::Display for Stream {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Stream::Stdin => "stdin",
                Stream::Stdout => "stdout",
                Stream::Stderr => "stderr",
            })
        }
    }

    /// What was closed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum Closed {
        /// The process's input: by `close_stdin()` or `close()`, or because the process has ended.
        Stdin,
        /// The engine: the script was shut down, and it starts nothing more.
        Engine,
    }

    impl std::fmt::Display for Closed {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                Closed::Stdin => "stdin",
                Closed::Engine => "the engine",
            })
        }
    }

    /// Why a program could not be started.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum SpawnCause {
        /// There is no such program: not at the path, or not in `PATH`.
        NotFound,
        /// The program is there and may not be run.
        PermissionDenied,
        /// `cwd` does not exist or is not a directory.
        NoDirectory,
        /// Anything else the operating system refused the process for.
        Other,
    }

    /// Why a call of this module failed. The kind is stable; `reason` is the operating system's wording, for a
    /// log, and may change.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum ProcessError {
        /// An option or an argument was refused before anything was started or sent.
        #[error("{reason}")]
        InvalidInput {
            /// What was refused, and why.
            reason: String,
        },
        /// The program could not be started.
        #[error("{reason}")]
        Spawn {
            /// What stood in the way.
            cause: SpawnCause,
            /// What the operating system said.
            reason: String,
        },
        /// The stream is not a pipe: the command was started with `"null"` or `"inherit"` for it.
        #[error("{stream} is not a pipe")]
        NotPiped {
            /// The stream that was read or written.
            stream: Stream,
        },
        /// The process's input or the engine is closed.
        #[error("{what} is closed")]
        Closed {
            /// What is closed.
            what: Closed,
        },
        /// Reading from a stream or writing to it failed, or the operating system did not say how the process
        /// ended.
        #[error("{reason}")]
        Io {
            /// What the operating system said.
            reason: String,
        },
        /// Anything else, reported and never hidden.
        #[error("{reason}")]
        Other {
            /// What went wrong.
            reason: String,
        },
    }

    impl From<Refused> for ProcessError {
        fn from(refused: Refused) -> Self {
            ProcessError::InvalidInput { reason: refused.to_string() }
        }
    }

    /// A command: the program to start and what it is started with.
    #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Shape)]
    pub struct Command {
        /// The program: a path, or a name looked up in `PATH`.
        pub program: OsPath,
        /// Its arguments, at most 1024. No shell reads them, so nothing in them is expanded or split.
        pub args: Option<Vec<String>>,
        /// The directory it starts in; the engine's working directory when not given.
        pub cwd: Option<OsPath>,
        /// Variables added to the environment the engine's process has, replacing one of the same name. At most
        /// 1024.
        pub env: Option<BTreeMap<String, String>>,
        /// Start from an empty environment: the process has only what `env` gives it.
        pub clear_env: Option<bool>,
        /// Its standard input; `"piped"` when not given.
        pub stdin: Option<Stdio>,
        /// Its standard output; `"piped"` when not given.
        pub stdout: Option<Stdio>,
        /// Its standard error; `"piped"` when not given.
        pub stderr: Option<Stdio>,
    }

    impl Command {
        /// Refuse what the operating system would not take, or should not be asked to.
        pub(super) fn check(&self) -> Result<(), ProcessError> {
            command_options::program("program", self.program.as_ref())?;
            if let Some(args) = &self.args {
                command_options::args("args", args)?;
            }
            if let Some(cwd) = &self.cwd {
                command_options::dir("cwd", cwd.as_ref())?;
            }
            if let Some(env) = &self.env {
                command_options::env("env", env)?;
            }
            Ok(())
        }
    }

    /// How a process ended (`std::process::ExitStatus`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Shape)]
    pub struct ExitStatus {
        /// The code it ended with by itself; none when a signal ended it.
        pub code: Option<i32>,
        /// The number of the signal that ended it; none when it ended by itself, and always on Windows.
        pub signal: Option<i32>,
        /// It ended by itself with code 0.
        pub success: bool,
    }

    /// A process a script started, and the near ends of its standard streams. Copies of the handle are the same
    /// process; it is killed when the last one is collected.
    #[derive(Clone, Object)]
    pub struct Child {
        process: Arc<Process>,
    }

    /// The body of `read` and `read_stderr`. A `Role::Mixed` body: see [`byte_stream::read`].
    async fn read_from(
        stream: Stream,
        output: Option<Arc<Output>>,
        max: Option<u32>,
    ) -> mlua::Result<Result<Option<Bytes>, ProcessError>> {
        let wanted = byte_stream::read_size(max)
            .map_err(|reason| ProcessError::InvalidInput { reason })
            .and_then(|max| output.map(|output| (output, max)).ok_or(ProcessError::NotPiped { stream }));
        let (output, max) = match wanted {
            Ok(wanted) => wanted,
            Err(e) => {
                hold().await;
                return Ok(Err(e));
            }
        };
        Ok(match byte_stream::read(&output, max).await? {
            Read::Bytes(bytes) => Ok(Some(Bytes(bytes))),
            Read::End => Ok(None),
            Read::Failed(reason) => Err(ProcessError::Io { reason }),
        })
    }

    #[mlua_ir::object]
    impl Child {
        /// Send bytes to the process's standard input: `true` once the pipe has taken them all. Nothing is added:
        /// a protocol of lines needs its `\n`. At most 1 MiB a call (`invalid_input` past that). Writes are made
        /// one after the other, and a process that does not read its input makes the write wait.
        ///
        /// `not_piped` when `stdin` is not a pipe; `closed` after `close_stdin()`, after `close()` and once the
        /// process has ended, also for a write that was waiting; `io` when the pipe refuses the bytes (the process
        /// closed its input). A write cannot be taken back: cancelling the task that waits in one does not stop
        /// it, and it may still be made.
        pub async fn write(&self, data: Bytes) -> Result<(), ProcessError> {
            let data =
                byte_stream::write_data(data.0).map_err(|reason| ProcessError::InvalidInput { reason })?;
            let write = self.process.write(data);
            // nothing on the first poll, as in `spawn`: a call that cannot suspend must not have written anything
            tokio::task::yield_now().await;
            write.await
        }

        /// Close the process's standard input: `true`. The process then reads the end of its input, which is how a
        /// program that reads until then is asked to finish. A write the pipe is taking is finished first; writes
        /// that wait behind it fail with `closed`. Closing again does nothing. `not_piped` when `stdin` is not a
        /// pipe.
        pub fn close_stdin(&self) -> Result<(), ProcessError> {
            self.process.close_stdin()
        }

        /// Wait for the process's standard output and return it: the bytes the process wrote, at most `max` of
        /// them (from 1 to 65536, the default), as soon as there are any. The bytes are the process's own, and a
        /// read may end in the middle of a line or of a character.
        ///
        /// `nil` without an error is the end of the output: every process that had the pipe open is gone or closed
        /// it, or `close()` was called. An error is a failure: `not_piped` when `stdout` is not a pipe, `io` when
        /// reading failed (the read after it is the end), `invalid_input` for a `max` out of range. At most 256 KiB
        /// of output wait to be read: a process that writes more waits in its write until the script reads. A read
        /// that is abandoned takes nothing.
        ///
        /// ```luau
        /// local function output(child)
        ///     local seen = ""
        ///     while true do
        ///         local bytes, err = child:read()
        ///         assert(err == nil, tostring(err))
        ///         if bytes == nil then
        ///             return seen
        ///         end
        ///         seen ..= bytes
        ///     end
        /// end
        /// ```
        #[host("Mixed")]
        pub async fn read(&self, max: Option<u32>) -> mlua::Result<Result<Option<Bytes>, ProcessError>> {
            read_from(Stream::Stdout, self.process.stdout(), max).await
        }

        /// `read` for the process's standard error, which has a buffer of its own.
        #[host("Mixed")]
        pub async fn read_stderr(
            &self,
            max: Option<u32>,
        ) -> mlua::Result<Result<Option<Bytes>, ProcessError>> {
            read_from(Stream::Stderr, self.process.stderr(), max).await
        }

        /// The operating system's id of the process, while it runs. `nil` once it has ended or `close()` was
        /// called: the number is then the operating system's to give to another process.
        pub fn id(&self) -> Option<u32> {
            self.process.id()
        }

        /// Wait for the process to end and return how it ended. It does not close the process's input: a process
        /// that reads to the end of its input needs `close_stdin()` first. It may be called again, and after
        /// `close()`: the answer is the same. The output can outlive the process (a child of it still writes):
        /// read to the end for all of it. `io` when the operating system does not say how the process ended.
        pub async fn wait(&self) -> Result<ExitStatus, ProcessError> {
            self.process.wait().await
        }

        /// Kill the process: on Unix it and every process in its group (those it started, unless they moved to a
        /// group of their own) are sent the kill they cannot refuse (`SIGKILL`); on Windows the process alone is
        /// terminated. It returns at once: `wait()` says when the process is gone. Once `wait()` has said so, a
        /// kill does nothing, and what the process left behind runs on. To ask a process to finish instead, close
        /// its input (`close_stdin`) or tell it in its own protocol.
        pub fn kill(&self) {
            self.process.kill();
        }

        /// Kill the process as `kill()` does and let go of its streams: output not yet read is dropped, every
        /// `read` and `read_stderr`, waiting or later, returns `nil`, and `write` fails with `closed`. `wait()`
        /// still says how the process ended. Closing again does nothing. A process is also closed when its handle
        /// is collected, and when the engine shuts down.
        pub fn close(&self) {
            self.process.close();
        }
    }

    /// Start a program: a `Child`. No shell reads the command, so nothing in it is expanded or split.
    ///
    /// A piped output that nobody reads stops the process once 256 KiB of it wait: read it, or do not pipe it.
    /// The process runs as the engine's user and can do what that user can: a script given this module is trusted
    /// with the machine.
    ///
    /// Errors: `invalid_input` for a command the operating system would not take (an empty program, a NUL byte,
    /// more than 1024 arguments or variables), `spawn` when the program could not be started (`cause` says whether
    /// there is no such program, it may not be run, or `cwd` is not a directory), `closed` when the engine has
    /// shut down.
    ///
    /// The process is killed, and its streams let go of, by `close()`, when the handle is collected, and when the
    /// engine shuts down. On Unix it is started in a process group of its own, which a kill reaches as a whole.
    /// That group is not the foreground of the engine's terminal: a Ctrl-C typed there does not reach the process
    /// (a script that should end on one kills its processes itself), and a process that reads an inherited `stdin`
    /// which is that terminal is stopped by it.
    ///
    /// ```luau
    /// local child, err = process.spawn({ program = "neoc-no-such-program", args = { "--version" } })
    /// assert(child == nil and err ~= nil and err.kind == "spawn" and err.cause == "not_found")
    /// ```
    pub async fn spawn(lua: Lua, command: Command) -> Result<Child, ProcessError> {
        const ENGINE_CLOSED: ProcessError = ProcessError::Closed { what: Closed::Engine };
        command.check()?;
        let life = Life::of(&lua);
        if life.is_closed() {
            return Err(ENGINE_CLOSED);
        }
        // Do nothing on the first poll (see `std:fs`'s `blocking`): where Luau refuses the yield, the call is
        // dropped after that poll, and a process started now would run although the script was told the call
        // failed.
        tokio::task::yield_now().await;
        // The process is registered where it is started: a call that is dropped while this runs drops the process
        // when it arrives, which kills it, and one whose task is cancelled (its future kept, never polled again)
        // leaves a process the engine's shutdown still closes. Weakly: a script that starts and drops processes in
        // a loop leaves nothing registered. Closed at once when the engine is already shut down.
        let registry = life.clone();
        let process = tokio::task::spawn_blocking(move || {
            let process = Arc::new(Process::spawn(&command)?);
            registry.on_shutdown_with(&process, |process| process.close());
            Ok(process)
        })
        .await
        .unwrap_or_else(|e| {
            Err(ProcessError::Other { reason: format!("starting the program failed: {e}") })
        })?;
        if life.is_closed() {
            // the engine shut down while the program was being started: it is closed already
            return Err(ENGINE_CLOSED);
        }
        Ok(Child { process })
    }
}
