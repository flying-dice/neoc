//! Pseudo-terminals: `vnd:portable_pty` (the `portable-pty` crate, the terminal layer of WezTerm).
//!
//! ```lua
//! local portable_pty = require("vnd:portable_pty")
//!
//! -- the user's default shell, in their home directory
//! local shell = assert(portable_pty.spawn_command({ rows = 24, cols = 80, env = { TERM = "xterm-256color" } }))
//! assert(shell:write("echo hello\n"))
//! while true do
//!     local output, err = shell:read()
//!     if not output then
//!         break -- the end of the output (err is nil) or a failure
//!     end
//!     io.write(output)
//! end
//!
//! -- one command
//! local ls = assert(portable_pty.spawn_command({ program = "ls", args = { "-l" }, cwd = "/tmp" }))
//! local status = assert(ls:wait())
//! print(status.exit_code)
//! ls:close()
//! ```
//!
//! **One handle.** The crate opens a pair (`openpty`) and starts a command on its far end (`spawn_command`); here
//! one call does both and returns one `Pty`: the near end of the terminal (`read`, `write`, `resize`) and the
//! process on it (`process_id`, `wait`, `kill`).
//!
//! **Errors.** A call returns `(value, nil)` or `(nil, err)`, where `err` is a
//! [`PtyError`](portable_pty::PtyError): its `kind` is stable, and what the crate or the operating system said is
//! its `reason`. An argument of the wrong type is raised, as everywhere.
//!
//! **Waiting.** `spawn_command`, `read`, `write` and `wait` wait without holding the VM: other tasks run meanwhile.
//! The crate's own reads, writes and waits block, so each terminal has three threads that do them (`terminal`).
//!
//! **Limits.** A read returns at most 64 KiB (`byte_stream::MAX_READ`) and a write takes at most
//! 1 MiB (`byte_stream::MAX_WRITE`); at most 256 KiB (`byte_stream::OUTPUT_BUFFER`) of output wait
//! for the script, and a process that writes more waits until they are read.
//! A size is 1 to 32767 rows and columns. Nothing limits how many terminals a script opens.
//!
//! **What starting a command means.** The process runs as the user the engine runs as, with that user's
//! environment: it can do what that user can. Nothing here narrows that, as nothing narrows `std:fs`: a script that
//! is given this module is trusted with the machine.
//!
//! **Lifetime.** A terminal belongs to the script that opened it. `close()`, the handle being collected and the
//! engine shutting down all do the same: the process is ended (on Unix hung up, then killed; on Windows terminated)
//! and the terminal is let go of.
//!
//! **Windows.** The terminal is a ConPTY pseudo-console. It does not say when the last process on it is gone, so
//! the output ends when the process `spawn_command` started does: the pseudo-console is let go of then, which
//! ends what else was attached to it, and a `resize` after that is `closed`. What is read is what the
//! pseudo-console draws: the process's text with the console's own escape sequences around it. Input is keys: a
//! line ends with `\r`.
//!
//! The first thing a pseudo-console writes is a question, "where is the cursor?" (`ESC [ 6 n`), and nothing runs
//! on it before the answer is written back (`ESC [ <row> ; <column> R`, so `"\27[1;1R"` for the top left corner).
//! A terminal emulator the bytes are passed to answers by itself. A script that reads the output itself writes the
//! answer: the bytes pass as they are in both directions, and this module puts none of its own among them.
//!
//! Ported from the old VM's `vnd:portable_pty`. What changed on purpose:
//! - An error is a typed [`PtyError`](portable_pty::PtyError) (`err.kind`, `err.reason`); it was the text
//!   `portable_pty.<function>: <kind>: <message>`, read back with `error_kind` and `error_message`, which are gone.
//! - Options of the wrong shape (no table, an unknown key, a value of the wrong type) are raised with their place,
//!   as every wrong argument is; they were an `invalid_input` error. What the shape does not say (an empty program,
//!   a NUL byte, too many arguments, a size of 0 or past 32767, `args` without `program`) still is one.
//! - `read`'s `max` and the rows and columns of `resize` are whole numbers: a fraction, a negative number or NaN is
//!   raised; a `max` of 0 or past 65536 and a size of 0 or past 32767 are still `invalid_input`.
//! - `program` and `cwd` are paths: on Unix they need not be UTF-8.
//! - On Windows the output ends when the process does (see above); the old VM's never ended before `close()`.

mod options;
mod terminal;

#[cfg(test)]
mod tests;

#[mlua_ir::module("vnd:portable_pty")]
pub mod portable_pty {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use mlua::Lua;
    use mlua_ir::{Bytes, LuaError, Object, Options, Shape};
    use serde::{Deserialize, Serialize};

    use super::options;
    use super::terminal::Terminal;
    use crate::byte_stream::{self, Read};
    use crate::command_options::Refused;
    use crate::lifecycle::{Life, hold};
    use crate::modules::std::osstr::OsPath;

    /// Why a call of this module failed. The kind is stable; `reason` is the crate's or the operating system's
    /// wording, for a log, and may change.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum PtyError {
        /// An option or an argument was refused before anything was started or sent.
        #[error("{reason}")]
        InvalidInput {
            /// What was refused, and why.
            reason: String,
        },
        /// The command could not be started: there is no such program, it is not executable, `cwd` is not a
        /// directory, or the operating system refused the terminal or the process.
        #[error("{reason}")]
        Spawn {
            /// What the crate or the operating system said.
            reason: String,
        },
        /// The terminal was closed (by `close`, or with the engine).
        #[error("the terminal is closed")]
        Closed,
        /// Reading from the terminal or writing to it failed, or the operating system did not say how the process
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

    impl From<Refused> for PtyError {
        fn from(refused: Refused) -> Self {
            PtyError::InvalidInput { reason: refused.to_string() }
        }
    }

    /// What `spawn_command` starts, and the size of the terminal it starts it on.
    #[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Options)]
    pub struct Spawn {
        /// The program: a path, or a name looked up in `PATH`. Without it the user's default shell is started,
        /// as a login shell: `$SHELL`, else the one of the password database, else `/bin/sh`; on Windows
        /// `%ComSpec%`.
        pub program: Option<OsPath>,
        /// The program's arguments, at most 1024. They need `program`: the default shell takes none.
        pub args: Option<Vec<String>>,
        /// The directory the command starts in; the user's home directory when not given (not the engine's working
        /// directory).
        pub cwd: Option<OsPath>,
        /// Variables added to the environment the engine's process has, replacing one of the same name; nothing is
        /// taken away. At most 1024. `TERM` is whatever that environment says unless it is set here
        /// (`xterm-256color` for a terminal that draws like xterm).
        pub env: Option<BTreeMap<String, String>>,
        /// The rows of the terminal, from 1 to 32767; 24 when not given.
        pub rows: Option<u16>,
        /// The columns of the terminal, from 1 to 32767; 80 when not given.
        pub cols: Option<u16>,
    }

    /// How a process ended.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Shape)]
    pub struct ExitStatus {
        /// The code it ended with: 0 to 255 on Unix, and 1 when a signal ended it.
        pub exit_code: u32,
        /// The operating system's words for the signal that ended it, which differ between systems; none when it
        /// ended by itself, and always on Windows.
        pub signal: Option<String>,
        /// It ended by itself with code 0.
        pub success: bool,
    }

    /// A terminal and the process on it: what `spawn_command` returns. Copies of the handle are the same terminal;
    /// it is closed when the last one is collected.
    #[derive(Clone, Object)]
    pub struct Pty {
        terminal: Arc<Terminal>,
    }

    #[mlua_ir::object]
    impl Pty {
        /// Wait for output and return it: the bytes the process wrote, at most `max` of them (from 1 to 65536, the
        /// default), as soon as there are any. The bytes are what a terminal receives: text in the process's
        /// encoding with its escape sequences, and a read may end in the middle of a character.
        ///
        /// `nil` without an error is the end of the output: every process that had the terminal open is gone (on
        /// Windows: the process `spawn_command` started is), or the terminal was closed. An error is a failure:
        /// `io` when reading failed (the read after it is the end), `invalid_input` for a `max` out of range. At
        /// most 256 KiB of output wait to be read: a process that writes more waits in its write until the script
        /// reads. A read that is abandoned takes nothing.
        ///
        /// ```luau
        /// local function output(pty)
        ///     local seen = ""
        ///     while true do
        ///         local bytes, err = pty:read()
        ///         assert(err == nil, tostring(err))
        ///         if bytes == nil then
        ///             return seen
        ///         end
        ///         seen ..= bytes
        ///     end
        /// end
        /// ```
        // `Mixed`: see `byte_stream::read`
        #[host("Mixed")]
        pub async fn read(&self, max: Option<u32>) -> mlua::Result<Result<Option<Bytes>, PtyError>> {
            let max = match byte_stream::read_size(max) {
                Ok(max) => max,
                Err(reason) => {
                    hold().await;
                    return Ok(Err(PtyError::InvalidInput { reason }));
                }
            };
            let output = self.terminal.output();
            Ok(match byte_stream::read(&output, max).await? {
                Read::Bytes(bytes) => Ok(Some(Bytes(bytes))),
                Read::End => Ok(None),
                Read::Failed(reason) => Err(PtyError::Io { reason }),
            })
        }

        /// Send bytes to the process as its input, as if typed: `true` once the terminal has taken them all. A
        /// line ends with `\r` or `\n` as the program on the terminal expects (a shell takes either; on Windows it
        /// is `\r`). At most 1 MiB a call (`invalid_input` past that). Writes are made one after the other, and a
        /// process that does not read its input makes the write wait.
        ///
        /// `closed` after `close()`, also for a write that was waiting; `io` when the terminal refuses the bytes
        /// (every process on it is gone).
        pub async fn write(&self, data: Bytes) -> Result<(), PtyError> {
            let data = byte_stream::write_data(data.0).map_err(|reason| PtyError::InvalidInput { reason })?;
            if self.terminal.is_closed() {
                return Err(PtyError::Closed);
            }
            if data.is_empty() {
                return Ok(());
            }
            // nothing on the first poll, as in `spawn_command`: a call that cannot suspend must not have typed
            // anything
            tokio::task::yield_now().await;
            self.terminal.write(data).await
        }

        /// Tell the terminal its new size, each from 1 to 32767 (`invalid_input` otherwise): `true`. The process
        /// in the foreground is told by the operating system (`SIGWINCH` on Unix). `closed` after `close()`, and
        /// on Windows once the process has ended.
        pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
            let (rows, cols) = options::size(rows, cols)?;
            self.terminal.resize(rows, cols)
        }

        /// The id of the process `spawn_command` started, while it runs. `nil` once it has ended or the terminal
        /// is closed: the number is then the operating system's to give to another process.
        pub fn process_id(&self) -> Option<u32> {
            self.terminal.process_id()
        }

        /// Wait for the process to end and return how it ended. It may be called again, and after `close()`: the
        /// answer is the same. On Unix the output can outlive the process (a child of it still writes): read to
        /// the end for all of it. `io` when the operating system does not say how the process ended.
        pub async fn wait(&self) -> Result<ExitStatus, PtyError> {
            self.terminal.wait().await
        }

        /// End the process: on Unix it is sent a hangup (`SIGHUP`, what a terminal that goes away sends) and, if
        /// it still runs a quarter of a second later, the kill it cannot refuse (`SIGKILL`); on Windows it is
        /// terminated. It returns at once: `wait()` says when the process is gone. A process that has ended is
        /// left alone. The process's own children are not signalled here: the operating system hangs up those in
        /// the terminal's foreground when the process ends.
        pub fn kill(&self) {
            self.terminal.kill();
        }

        /// End the process as `kill()` does and let go of the terminal: output not yet read is dropped, every
        /// `read`, waiting or later, returns `nil`, and `write` and `resize` fail with `closed`. `wait()` still
        /// says how the process ended. Closing again does nothing. A terminal is also closed when its handle is
        /// collected, and when the engine shuts down.
        pub fn close(&self) {
            self.terminal.close();
        }
    }

    /// Open a pseudo-terminal and start a command on it: a `Pty`. Without options it is the user's default shell
    /// in their home directory, on a terminal of 24 rows by 80 columns.
    ///
    /// The process runs as the engine's user and can do what that user can: a script given this module is trusted
    /// with the machine.
    ///
    /// Errors: `invalid_input` for a command the operating system would not take (an empty program, a NUL byte,
    /// more than 1024 arguments or variables, `args` without `program`) and for a size of 0 or past 32767; `spawn`
    /// when the command could not be started (no such program, not executable, `cwd` is not a directory, or the
    /// system refused the terminal or the process).
    ///
    /// The terminal is closed, and the process ended, by `close()`, when the handle is collected, and when the
    /// engine shuts down. A terminal opened after the engine has shut down is closed already.
    ///
    /// On Windows nothing runs on the terminal before its first output, the question `"\27[6n"` (where is the
    /// cursor?), is answered: write `"\27[1;1R"`, or pass the output to a terminal emulator, which answers.
    ///
    /// ```luau
    /// local pty, err = portable_pty.spawn_command({ program = "/nowhere/neoc-no-such-program" })
    /// assert(pty == nil and err ~= nil and err.kind == "spawn")
    /// ```
    pub async fn spawn_command(lua: Lua, opts: Spawn) -> Result<Pty, PtyError> {
        let command = options::command(opts)?;
        // Do nothing on the first poll (see `std:fs`'s `blocking`): where Luau refuses the yield, the call is
        // dropped after that poll, and a process started now would run although the script was told the call
        // failed.
        tokio::task::yield_now().await;
        // A call that is dropped while this runs drops the terminal when it arrives, which ends the process.
        let terminal =
            tokio::task::spawn_blocking(move || Terminal::spawn(command)).await.unwrap_or_else(|e| {
                Err(PtyError::Other { reason: format!("starting the command failed: {e}") })
            })?;
        let terminal = Arc::new(terminal);
        // weakly: a script that opens and drops terminals in a loop leaves nothing registered. Closed at once
        // when the engine is already shut down.
        Life::of(&lua).on_shutdown_with(&terminal, |terminal| terminal.close());
        Ok(Pty { terminal })
    }
}
