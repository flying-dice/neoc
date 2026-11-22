//! Pseudo-terminals — `vnd:portable_pty` (backed by the `portable-pty` crate, the terminal layer of WezTerm).
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
//! one call does both and returns one `Pty`: the near end of the terminal (`read`, `write`, `resize`) and the process
//! on it (`process_id`, `wait`, `kill`).
//!
//! **Errors.** A call returns `(value, nil)` or `(nil, "portable_pty.<function>: <kind>: <message>")`; the kinds are
//! stable (`error`, `portable_pty.error_kind`). An argument of the wrong type is raised, as everywhere.
//!
//! **Waiting.** `spawn_command`, `read`, `write` and `wait` wait without holding the VM: other tasks run meanwhile.
//! The crate's own reads, writes and waits block, so each terminal has three threads that do them (`terminal`).
//!
//! **Limits.** A read returns at most [`MAX_READ`] bytes and a write takes at most [`MAX_WRITE`]; at most
//! [`terminal::OUTPUT_BUFFER`] bytes of output wait for the script, and a process that writes more waits until they are read.
//! A size is 1 to 32767 rows and columns. Nothing limits how many terminals a script opens.
//!
//! **What starting a command means.** The process runs as the user the engine runs as, with that user's
//! environment: it can do what that user can. Nothing here narrows that, as nothing narrows `std:fs`: a script that
//! is given this module is trusted with the machine.
//!
//! **Lifetime.** A terminal belongs to the script that opened it. `close()`, the handle being collected and the
//! engine shutting down all do the same: the process is ended (hung up, then killed) and the terminal is let go of.

use std::sync::Arc;

use mlua::{Lua, Table, UserData, UserDataMethods, Value};

use crate::lua::facade::Module;
use crate::lua::lifecycle::{Life, Role, async_function, async_method, hold, idle};
use crate::lua::sandbox::{from_err, ok};

mod error;
mod options;
mod terminal;

#[cfg(test)]
mod tests;

use error::{Kind, PtyError};
use terminal::{Taken, Terminal};

/// The most bytes one `read` returns, and how many it returns when it is not told: a size that fits in one message
/// of most transports once it is encoded.
const MAX_READ: usize = 64 * 1024;

/// The most bytes one `write` takes: far more than a paste, and little enough to copy without a thought.
const MAX_WRITE: usize = 1024 * 1024;

const SPAWN_COMMAND: &str = "portable_pty.spawn_command";
const READ: &str = "portable_pty.read";
const WRITE: &str = "portable_pty.write";
const RESIZE: &str = "portable_pty.resize";
const WAIT: &str = "portable_pty.wait";

const WAITS: &str = "It waits like `task.wait`: other tasks run while it does.";

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `vnd:portable_pty` surface through the facade. `spawn_command` and `error_kind` are plain
/// functions; the `Pty` type keeps its own `impl UserData` and is recorded with
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "vnd:portable_pty")?;

    m.set(
        "spawn_command",
        async_function(lua, Role::Wait, spawn_command)?,
    )?;
    m.record_fn(
        "spawn_command",
        &[(
            "opts",
            "{ program: string?, args: { string }?, cwd: string?, env: { [string]: string }?, rows: number?, cols: number? }?",
        )],
        "Result<Pty, string>",
        &format!(
            "Open a pseudo-terminal of `rows` by `cols` (default 24 by 80; each 1 to 32767) and start a command on it: a `Pty`. `program` is a path, or a name looked up in `PATH`, and `args` its arguments (at most 1024). Without `program` it is the user's default shell, started as a login shell: `$SHELL`, else the one of the password database, else `/bin/sh`; on Windows `%ComSpec%`. `cwd` is the directory it starts in (default: the user's home directory, not the engine's working directory). `env` is added to the environment the engine's process has, replacing a variable of the same name; nothing is taken away, and `TERM` is whatever that environment says unless `env` sets it (`xterm-256color` for a terminal that draws like xterm). The process runs as the engine's user and can do what that user can: a script given this module is trusted with the machine. Errors are `portable_pty.<function>: <kind>: <message>` (see `error_kind`): `invalid_input` for an option that is unknown or of the wrong shape, `spawn` when the command could not be started (no such program, not executable, `cwd` is not a directory, or the system refused the terminal or the process). The terminal is closed, and the process ended, by `close()`, when the handle is collected, and when the engine shuts down. {WAITS}"
        ),
    );

    m.func(
        "error_kind",
        &[("err", "string?")],
        "The stable kind inside an error string returned by a `vnd:portable_pty` function, or `nil` when the string is not one of theirs (or is `nil`, so `portable_pty.error_kind(err)` takes the `err` of any call). Such a string is `portable_pty.<function>: <kind>: <message>`. Kinds: `invalid_input` (an argument or option was refused), `spawn` (the command could not be started), `closed` (the terminal was closed), `io` (reading from the terminal or writing to it failed), `other`. The message is the crate's or the operating system's own; it is for a log and may change.",
        |_, err: Option<mlua::LuaString>| {
            Ok(err.and_then(|err| error::kind_of(&err.to_string_lossy()).map(str::to_owned)))
        },
    )?;

    m.userdata("Pty", |p| {
        p.method(
            "read",
            &[("max", "number?")],
            "Result<string, string>",
            &format!(
                "Wait for output and return it: the bytes the process wrote, at most `max` of them (a whole number from 1 to 65536, the default), as soon as there are any. The bytes are what a terminal receives: text in the process's encoding with its escape sequences, and a read may end in the middle of a character. `(nil, nil)` is the end of the output: every process that had the terminal open is gone, or the terminal was closed. `(nil, err)` is a failure (`io`), and the read after it is the end; a `max` out of range is `invalid_input`. At most 256 KiB of output wait to be read: a process that writes more waits in its write until the script reads. A read that is abandoned takes nothing. {WAITS}"
            ),
        )
        .method(
            "write",
            &[("data", "string")],
            "Result<bool, string>",
            &format!(
                "Send bytes to the process as its input, as if typed: `true` once the terminal has taken them all. A line ends with `\\r` or `\\n` as the program on the terminal expects (a shell takes either). At most 1 MiB a call (`invalid_input` past that). Writes are made one after the other, and a process that does not read its input makes the write wait. `closed` after `close()`, also for a write that was waiting; `io` when the terminal refuses the bytes (every process on it is gone). {WAITS}"
            ),
        )
        .method(
            "resize",
            &[("rows", "number"), ("cols", "number")],
            "Result<bool, string>",
            "Tell the terminal its new size, each a whole number from 1 to 32767 (`invalid_input` otherwise): `true`. The process in the foreground is told by the operating system (`SIGWINCH` on Unix). `closed` after `close()`.",
        )
        .method(
            "process_id",
            &[],
            "number?",
            "The id of the process `spawn_command` started, while it runs. `nil` once it has ended or the terminal is closed: the number is then the operating system's to give to another process.",
        )
        .method(
            "wait",
            &[],
            "Result<{ exit_code: number, signal: string?, success: bool }, string>",
            &format!(
                "Wait for the process to end and return how it ended: `exit_code` (0 to 255 on Unix, and 1 when a signal ended it), `signal` (the operating system's words for that signal, which differ between systems; `nil` when it ended by itself, and always on Windows) and `success` (it ended by itself with code 0). It may be called again, and after `close()`: the answer is the same. The output can outlive the process (a child of it still writes): read to the end for all of it. `io` when the operating system does not say how the process ended. {WAITS}"
            ),
        )
        .method(
            "kill",
            &[],
            "()",
            "End the process: on Unix it is sent a hangup (`SIGHUP`, what a terminal that goes away sends) and, if it still runs a quarter of a second later, the kill it cannot refuse (`SIGKILL`); on Windows it is terminated. It returns at once: `wait()` says when the process is gone. A process that has ended is left alone. The process's own children are not signalled here: the operating system hangs up those in the terminal's foreground when the process ends.",
        )
        .method(
            "close",
            &[],
            "()",
            "End the process as `kill()` does and let go of the terminal: output not yet read is dropped, every `read`, waiting or later, returns `(nil, nil)`, and `write` and `resize` fail with `closed`. `wait()` still says how the process ended. Closing again does nothing. A terminal is also closed when its handle is collected, and when the engine shuts down.",
        );
    });

    Ok(m)
}

/// The handle `spawn_command` returns.
pub struct Pty(Arc<Terminal>);

async fn spawn_command(lua: Lua, opts: Option<Value>) -> mlua::Result<(Value, Value)> {
    // read before the first await: a script table can run script code when it is read
    let command = options::command(opts.as_ref(), SPAWN_COMMAND)?;
    drop(opts);
    let command = match command {
        Ok(command) => command,
        Err(e) => return from_err(&lua, SPAWN_COMMAND, e),
    };
    // Do nothing on the first poll (see `std:fs`'s `blocking`): where Luau refuses the yield, the call is dropped
    // after that poll, and a process started now would run although the script was told the call failed.
    tokio::task::yield_now().await;
    // A call that is dropped while this runs drops the terminal when it arrives, which ends the process.
    let spawned = tokio::task::spawn_blocking(move || Terminal::spawn(command))
        .await
        .unwrap_or_else(|e| {
            Err(PtyError::new(
                Kind::Other,
                format!("starting the command failed: {e}"),
            ))
        });
    match spawned {
        Ok(terminal) => {
            let terminal = Arc::new(terminal);
            // weakly: a script that opens and drops terminals in a loop leaves nothing registered. Closed at once
            // when the engine is already shut down.
            Life::of(&lua).on_shutdown_with(&terminal, |terminal| terminal.close());
            ok(&lua, Pty(terminal))
        }
        Err(e) => from_err(&lua, SPAWN_COMMAND, e),
    }
}

/// `read`'s `max`: a whole number of bytes from 1 to [`MAX_READ`].
fn read_size(max: Option<f64>) -> Result<usize, PtyError> {
    match max {
        None => Ok(MAX_READ),
        // checked: a whole number that fits
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(max) if max.fract() == 0.0 && max >= 1.0 && max <= MAX_READ as f64 => Ok(max as usize),
        Some(_) => Err(PtyError::invalid_input(format!(
            "`max` must be a whole number from 1 to {MAX_READ}"
        ))),
    }
}

impl UserData for Pty {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // `Role::Mixed`, not `Role::Wait` (as `std:fs`'s `Watcher:receive`): a `Wait` body would take the bytes when
        // its wait ends and only then queue for the turn, and a call dropped there would lose them. The wait for
        // output is an `idle` wait, and the bytes are taken with the turn held, in the poll that returns them.
        async_method(
            methods,
            "read",
            Role::Mixed,
            |lua, this, max: Option<f64>| {
                let output = this.0.output();
                let max = read_size(max);
                async move {
                    let max = match max {
                        Ok(max) => max,
                        Err(e) => {
                            hold().await;
                            return from_err(&lua, READ, e);
                        }
                    };
                    loop {
                        idle(output.readable()).await?;
                        match output.take(max) {
                            Taken::Bytes(bytes) => return ok(&lua, lua.create_string(&bytes)?),
                            Taken::Nothing => {}
                            Taken::End => return Ok((Value::Nil, Value::Nil)),
                            Taken::Failed(e) => return from_err(&lua, READ, e),
                        }
                    }
                }
            },
        );

        async_method(
            methods,
            "write",
            Role::Wait,
            |lua, this, data: mlua::LuaString| {
                let terminal = Arc::clone(&this.0);
                let data = {
                    let bytes = data.as_bytes();
                    if bytes.len() > MAX_WRITE {
                        Err(PtyError::invalid_input(format!(
                            "one write takes at most {MAX_WRITE} bytes"
                        )))
                    } else {
                        Ok(bytes.to_vec())
                    }
                };
                async move {
                    let written = match data {
                        Err(e) => Err(e),
                        Ok(_) if terminal.is_closed() => Err(PtyError::closed()),
                        Ok(data) if data.is_empty() => Ok(()),
                        Ok(data) => {
                            // nothing on the first poll, as in `spawn_command`: a call that cannot suspend must
                            // not have typed anything
                            tokio::task::yield_now().await;
                            terminal.write(data).await
                        }
                    };
                    match written {
                        Ok(()) => ok(&lua, true),
                        Err(e) => from_err(&lua, WRITE, e),
                    }
                }
            },
        );

        methods.add_method("resize", |lua, this, (rows, cols): (f64, f64)| {
            let resized =
                options::size(rows, cols).and_then(|(rows, cols)| this.0.resize(rows, cols));
            match resized {
                Ok(()) => ok(lua, true),
                Err(e) => from_err(lua, RESIZE, e),
            }
        });

        methods.add_method("process_id", |_, this, (): ()| Ok(this.0.process_id()));

        async_method(methods, "wait", Role::Wait, |lua, this, (): ()| {
            let ended = this.0.wait();
            async move {
                match ended.await {
                    Ok(exit) => {
                        let status = lua.create_table_with_capacity(0, 3)?;
                        status.raw_set("exit_code", exit.exit_code)?;
                        status.raw_set("signal", exit.signal)?;
                        status.raw_set("success", exit.success)?;
                        ok(&lua, status)
                    }
                    Err(e) => from_err(&lua, WAIT, e),
                }
            }
        });

        methods.add_method("kill", |_, this, (): ()| {
            this.0.kill();
            Ok(())
        });

        methods.add_method("close", |_, this, (): ()| {
            this.0.close();
            Ok(())
        });
    }
}
