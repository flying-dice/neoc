//! Child processes — `std:process` (mirrors `std::process`: a command, the child it starts, its three standard
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
//! joined, and a read returns what has arrived, which may end in the middle of a line.
//!
//! **Errors.** A call returns `(value, nil)` or `(nil, "process.<function>: <kind>: <message>")`; the kinds are
//! stable (`error`, `process.error_kind`). An argument of the wrong type is raised, as everywhere.
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

use std::sync::Arc;

use mlua::{Lua, Table, UserData, UserDataMethods, Value};

use crate::lua::byte_stream::{self, Output, WAITS};
use crate::lua::facade::Module;
use crate::lua::lifecycle::{Life, Role, async_function, async_method};
use crate::lua::sandbox::{from_err, ok};

mod child;
mod error;
mod options;

#[cfg(test)]
mod tests;

use child::Process;
use error::{Kind, ProcessError};

const SPAWN: &str = "process.spawn";
const READ: &str = "process.read";
const READ_STDERR: &str = "process.read_stderr";
const WRITE: &str = "process.write";
const CLOSE_STDIN: &str = "process.close_stdin";
const WAIT: &str = "process.wait";

const STREAM: &str = r#"("piped" | "null" | "inherit")?"#;

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:process` surface through the facade. `spawn` and `error_kind` are plain functions; the
/// `Child` type keeps its own `impl UserData` and is recorded with [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:process")?;

    m.set("spawn", async_function(lua, Role::Wait, spawn)?)?;
    m.record_fn(
        "spawn",
        &[(
            "opts",
            &format!(
                "{{ program: string, args: {{ string }}?, cwd: string?, env: {{ [string]: string }}?, clear_env: boolean?, stdin: {STREAM}, stdout: {STREAM}, stderr: {STREAM} }}"
            ),
        )],
        "Result<Child, string>",
        &format!(
            "Start a program: a `Child`. `program` is a path, or a name looked up in `PATH`, and `args` its arguments (at most 1024); no shell reads them, so nothing in them is expanded or split. `cwd` is the directory it starts in (default: the engine's working directory). `env` is added to the environment the engine's process has, replacing a variable of the same name; with `clear_env` the process starts from an empty environment and has only what `env` gives it. Each of `stdin`, `stdout` and `stderr` is `\"piped\"` (the default: a pipe between the process and this handle), `\"null\"` (nothing: the process reads the end of its input, and what it writes is thrown away) or `\"inherit\"` (the engine's own stream). A piped output that nobody reads stops the process once 256 KiB of it wait: read it, or do not pipe it. The process runs as the engine's user and can do what that user can: a script given this module is trusted with the machine. Errors are `process.<function>: <kind>: <message>` (see `error_kind`): `invalid_input` for an option that is missing, unknown or of the wrong shape, `spawn` when the program could not be started (no such program, not executable, `cwd` is not a directory, or the system refused the process), `closed` when the engine has shut down. The process is killed, and its streams let go of, by `close()`, when the handle is collected, and when the engine shuts down. On Unix it is started in a process group of its own, which a kill reaches as a whole. That group is not the foreground of the engine's terminal: a Ctrl-C typed there does not reach the process (a script that should end on one kills its processes itself), and a process that reads an inherited `stdin` which is that terminal is stopped by it. {WAITS}"
        ),
    );

    m.func(
        "error_kind",
        &[("err", "string?")],
        "The stable kind inside an error string returned by a `std:process` function, or `nil` when the string is not one of theirs (or is `nil`, so `process.error_kind(err)` takes the `err` of any call). Such a string is `process.<function>: <kind>: <message>`. Kinds: `invalid_input` (an argument or option was refused), `spawn` (the program could not be started), `not_piped` (the stream was not started as a pipe), `closed` (the stream, the process or the engine was closed), `io` (reading from a stream or writing to it failed, or the system did not say how the process ended), `other`. The message is the operating system's own; it is for a log and may change.",
        |_, err: Option<mlua::LuaString>| {
            Ok(err.and_then(|err| error::kind_of(&err.to_string_lossy()).map(str::to_owned)))
        },
    )?;

    m.userdata("Child", |c| {
        c.method(
            "write",
            &[("data", "string")],
            "Result<bool, string>",
            &format!(
                "Send bytes to the process's standard input: `true` once the pipe has taken them all. Nothing is added: a protocol of lines needs its `\\n`. At most 1 MiB a call (`invalid_input` past that). Writes are made one after the other, and a process that does not read its input makes the write wait. `not_piped` when `stdin` is not a pipe; `closed` after `close_stdin()`, after `close()` and once the process has ended, also for a write that was waiting; `io` when the pipe refuses the bytes (the process closed its input). A write cannot be taken back: cancelling the task that waits in one does not stop it, and it may still be made. {WAITS}"
            ),
        )
        .method(
            "close_stdin",
            &[],
            "Result<bool, string>",
            "Close the process's standard input: `true`. The process then reads the end of its input, which is how a program that reads until then is asked to finish. A write the pipe is taking is finished first; writes that wait behind it fail with `closed`. Closing again does nothing. `not_piped` when `stdin` is not a pipe.",
        )
        .method(
            "read",
            &[("max", "number?")],
            "Result<string, string>",
            &format!(
                "Wait for the process's standard output and return it: the bytes the process wrote, at most `max` of them (a whole number from 1 to 65536, the default), as soon as there are any. The bytes are the process's own, and a read may end in the middle of a line or of a character. `(nil, nil)` is the end of the output: every process that had the pipe open is gone or closed it, or `close()` was called. `(nil, err)` is a failure: `not_piped` when `stdout` is not a pipe, `io` when reading failed (the read after it is the end), `invalid_input` for a `max` out of range. At most 256 KiB of output wait to be read: a process that writes more waits in its write until the script reads. A read that is abandoned takes nothing. {WAITS}"
            ),
        )
        .method(
            "read_stderr",
            &[("max", "number?")],
            "Result<string, string>",
            &format!(
                "`read` for the process's standard error, which has a buffer of its own. {WAITS}"
            ),
        )
        .method(
            "id",
            &[],
            "number?",
            "The operating system's id of the process, while it runs. `nil` once it has ended or `close()` was called: the number is then the operating system's to give to another process.",
        )
        .method(
            "wait",
            &[],
            "Result<{ code: number?, signal: number?, success: bool }, string>",
            &format!(
                "Wait for the process to end and return how it ended: `code` (the exit code when it ended by itself, else `nil`), `signal` (the number of the signal that ended it; `nil` when it ended by itself, and always on Windows) and `success` (it ended by itself with code 0). It does not close the process's input: a process that reads to the end of its input needs `close_stdin()` first. It may be called again, and after `close()`: the answer is the same. The output can outlive the process (a child of it still writes): read to the end for all of it. `io` when the operating system does not say how the process ended. {WAITS}"
            ),
        )
        .method(
            "kill",
            &[],
            "()",
            "Kill the process: on Unix it and every process in its group (those it started, unless they moved to a group of their own) are sent the kill they cannot refuse (`SIGKILL`); on Windows the process alone is terminated. It returns at once: `wait()` says when the process is gone. A process that has ended is left alone, and so is what it left behind. To ask a process to finish instead, close its input (`close_stdin`) or tell it in its own protocol.",
        )
        .method(
            "close",
            &[],
            "()",
            "Kill the process as `kill()` does and let go of its streams: output not yet read is dropped, every `read` and `read_stderr`, waiting or later, returns `(nil, nil)`, and `write` fails with `closed`. `wait()` still says how the process ended. Closing again does nothing. A process is also closed when its handle is collected, and when the engine shuts down.",
        );
    });

    Ok(m)
}

/// The handle `spawn` returns.
pub struct Child(Arc<Process>);

async fn spawn(lua: Lua, opts: Option<Value>) -> mlua::Result<(Value, Value)> {
    // read before the first await: a script table can run script code when it is read
    let command = options::command(opts.as_ref(), SPAWN)?;
    drop(opts);
    let command = match command {
        Ok(command) => command,
        Err(e) => return from_err(&lua, SPAWN, e),
    };
    let life = Life::of(&lua);
    if life.is_closed() {
        return from_err(&lua, SPAWN, ProcessError::closed("the engine"));
    }
    // Do nothing on the first poll (see `std:fs`'s `blocking`): where Luau refuses the yield, the call is dropped
    // after that poll, and a process started now would run although the script was told the call failed.
    tokio::task::yield_now().await;
    // The process is registered where it is started: a call that is dropped while this runs drops the process when
    // it arrives, which kills it, and one whose task is cancelled (its future kept, never polled again) leaves a
    // process the engine's shutdown still closes. Weakly: a script that starts and drops processes in a loop leaves
    // nothing registered. Closed at once when the engine is already shut down.
    let registry = life.clone();
    let spawned = tokio::task::spawn_blocking(move || {
        let process = Arc::new(Process::spawn(&command)?);
        registry.on_shutdown_with(&process, |process| process.close());
        Ok(process)
    })
    .await
    .unwrap_or_else(|e| {
        Err(ProcessError::new(
            Kind::Other,
            format!("starting the program failed: {e}"),
        ))
    });
    match spawned {
        Ok(process) => {
            if life.is_closed() {
                // the engine shut down while the program was being started: it is closed already
                return from_err(&lua, SPAWN, ProcessError::closed("the engine"));
            }
            ok(&lua, Child(process))
        }
        Err(e) => from_err(&lua, SPAWN, e),
    }
}

/// One of the process's two output streams, as a method reads it.
struct Source {
    method: &'static str,
    op: &'static str,
    stream: &'static str,
    buffer: fn(&Process) -> Option<Arc<Output>>,
}

const SOURCES: [Source; 2] = [
    Source {
        method: "read",
        op: READ,
        stream: "stdout",
        buffer: Process::stdout,
    },
    Source {
        method: "read_stderr",
        op: READ_STDERR,
        stream: "stderr",
        buffer: Process::stderr,
    },
];

/// The method that reads `source` (`Role::Mixed`: see [`byte_stream::read`]).
fn add_read<M: UserDataMethods<Child>>(methods: &mut M, source: &'static Source) {
    async_method(
        methods,
        source.method,
        Role::Mixed,
        move |lua, this, max: Option<f64>| {
            let wanted = byte_stream::read_size(max)
                .map_err(ProcessError::invalid_input)
                .and_then(|max| match (source.buffer)(&this.0) {
                    Some(output) => Ok((output, max)),
                    None => Err(ProcessError::not_piped(source.stream)),
                });
            byte_stream::read(lua, source.op, wanted, |message| {
                ProcessError::new(Kind::Io, message)
            })
        },
    );
}

impl UserData for Child {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        async_method(
            methods,
            "write",
            Role::Wait,
            |lua, this, data: mlua::LuaString| {
                let data = byte_stream::write_data(&data).map_err(ProcessError::invalid_input);
                let write = data.map(|data| this.0.write(data));
                async move {
                    let written = match write {
                        Err(e) => Err(e),
                        Ok(write) => {
                            // nothing on the first poll, as in `spawn`: a call that cannot suspend must not have
                            // written anything
                            tokio::task::yield_now().await;
                            write.await
                        }
                    };
                    match written {
                        Ok(()) => ok(&lua, true),
                        Err(e) => from_err(&lua, WRITE, e),
                    }
                }
            },
        );

        methods.add_method("close_stdin", |lua, this, (): ()| {
            match this.0.close_stdin() {
                Ok(()) => ok(lua, true),
                Err(e) => from_err(lua, CLOSE_STDIN, e),
            }
        });

        for source in &SOURCES {
            add_read(methods, source);
        }

        methods.add_method("id", |_, this, (): ()| Ok(this.0.id()));

        async_method(methods, "wait", Role::Wait, |lua, this, (): ()| {
            let ended = this.0.wait();
            async move {
                match ended.await {
                    Ok(exit) => {
                        let status = lua.create_table_with_capacity(0, 3)?;
                        status.raw_set("code", exit.code)?;
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
