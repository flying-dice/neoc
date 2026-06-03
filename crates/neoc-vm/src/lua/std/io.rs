//! Core I/O primitives — `lib.io` (mirrors `std::io`).
//!
//! `ReaderHandle` and `WriterHandle` are the canonical trait-object wrappers
//! (ADR 0003). Methods mirror Rust's Read/Write/BufRead traits.
//!
//! ```lua
//! local reader = lib.io.from_string("hello")
//! local data, err = reader:read_all()
//!
//! local writer, err = lib.fs.open({ path = "out.bin", mode = "w" })
//! local ok, err = writer:write("hello")
//! writer:flush()
//!
//! local n, err = lib.io.copy(reader, writer)
//! ```

use std::io::{BufRead, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mlua::{AnyUserData, Lua, Table, UserData, UserDataMethods, Value};
use parking_lot::Mutex;
use tracing::instrument;

use crate::lua::facade::Module;
use crate::lua::sandbox::{from_err, ok};

// ---------------------------------------------------------------------------
// Handle types
// ---------------------------------------------------------------------------

pub struct ReaderHandle {
    pub inner: Arc<Mutex<Box<dyn BufRead + Send>>>,
    pub borrowed: Arc<AtomicBool>,
}

pub struct WriterHandle(pub Arc<Mutex<Box<dyn Write + Send>>>);

/// RAII guard returned by `ReaderHandle::try_borrow_exclusive`. Clears the
/// borrow flag on drop so subsequent streaming consumers (e.g. another
/// `csv.reader` call) can acquire the handle.
pub struct ExclusiveBorrow {
    flag: Arc<AtomicBool>,
}

impl Drop for ExclusiveBorrow {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

impl ReaderHandle {
    pub fn new<R: BufRead + Send + 'static>(r: R) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Box::new(r))),
            borrowed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Atomically claim exclusive access for a streaming consumer. Returns
    /// `None` if already borrowed. Callers hold the returned guard for the
    /// lifetime of the consumer; the flag clears on drop.
    pub fn try_borrow_exclusive(&self) -> Option<ExclusiveBorrow> {
        self.borrowed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| ExclusiveBorrow {
                flag: self.borrowed.clone(),
            })
    }
}

impl WriterHandle {
    pub fn new<W: Write + Send + 'static>(w: W) -> Self {
        Self(Arc::new(Mutex::new(Box::new(w))))
    }

    /// Write all of `bytes` to the underlying sink, holding the lock for the
    /// call. Lets callers persist without reaching into the inner handle.
    pub fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.0.lock().write_all(bytes)
    }
}

/// Map an `io::Result` to the `(value, nil)` / `(nil, err)` convention shared by
/// every reader/writer method: `on_ok` builds the success value, errors are
/// stringified under `op`.
fn io_result<T>(
    lua: &Lua,
    op: &str,
    result: std::io::Result<T>,
    on_ok: impl FnOnce(&Lua, T) -> mlua::Result<(Value, Value)>,
) -> mlua::Result<(Value, Value)> {
    match result {
        Ok(v) => on_ok(lua, v),
        Err(e) => from_err(lua, op, e),
    }
}

impl UserData for ReaderHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("read", |lua, this, n: usize| {
            let mut buf = vec![0u8; n];
            let result = this.inner.lock().read(&mut buf);
            io_result(lua, "io.read", result, |lua, k| match k {
                0 => ok(lua, Value::Nil),
                k => ok(lua, lua.create_string(&buf[..k])?),
            })
        });

        methods.add_method("read_line", |lua, this, _: ()| {
            let mut line = String::new();
            let result = this.inner.lock().read_line(&mut line);
            io_result(lua, "io.read_line", result, |lua, n| {
                if n == 0 {
                    return ok(lua, Value::Nil);
                }
                // Strip a trailing newline, then an optional preceding CR.
                let trimmed = line.strip_suffix('\n').unwrap_or(&line);
                let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
                ok(lua, lua.create_string(trimmed.as_bytes())?)
            })
        });

        methods.add_method("read_all", |lua, this, _: ()| {
            let mut buf = Vec::new();
            let result = this.inner.lock().read_to_end(&mut buf);
            io_result(lua, "io.read_all", result, |lua, _| {
                ok(lua, lua.create_string(&buf)?)
            })
        });
    }
}

impl UserData for WriterHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("write", |lua, this, data: mlua::String| {
            let result = this.0.lock().write_all(&data.as_bytes());
            io_result(lua, "io.write", result, |lua, ()| ok(lua, true))
        });

        methods.add_method("flush", |lua, this, _: ()| {
            let result = this.0.lock().flush();
            io_result(lua, "io.flush", result, |lua, ()| ok(lua, true))
        });
    }
}

// ---------------------------------------------------------------------------
// module — constructors and multi-handle operations
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    Ok(declare(lua)?.into_table())
}

/// Declares the `std:io` surface through the facade.
///
/// Both module functions are set verbatim with
/// [`crate::lua::facade::Module::set`]: each already carries its own
/// `#[instrument(name = "io.<fn>", target = "luau::lib::io")]` attribute, so
/// routing through the facade would add a second span. `from_string` returns a
/// `ReaderHandle` userdata and `copy` borrows two handles and bakes its own
/// argument-type error messages, so neither fits a regular facade return type.
/// The `ReaderHandle` / `WriterHandle` methods keep their own `impl UserData`
/// and are recorded here for the header via
/// [`crate::lua::facade::Module::userdata`].
pub(crate) fn declare(lua: &Lua) -> mlua::Result<Module<'_>> {
    let mut m = Module::new(lua, "std:io")?;

    m.set("from_string", lua.create_function(from_string)?)?;
    m.record_fn(
        "from_string",
        &[("s", "string")],
        "ReaderHandle",
        "Wrap a Lua string in a `ReaderHandle` so it can be passed to any reader consumer.",
    );

    m.set("copy", lua.create_function(copy)?)?;
    m.record_fn(
        "copy",
        &[("reader", "ReaderHandle"), ("writer", "WriterHandle")],
        "Result<number, string>",
        "Copy all remaining bytes from `reader` into `writer`, returning the total bytes copied.",
    );

    m.userdata("ReaderHandle", |r| {
        r.method(
            "read",
            &[("n", "number")],
            "Result<string, string>",
            "Read up to `n` bytes; returns `(nil, nil)` at EOF.",
        )
        .method(
            "read_line",
            &[],
            "Result<string, string>",
            "Read one line, stripping the trailing newline; returns `(nil, nil)` at EOF.",
        )
        .method(
            "read_all",
            &[],
            "Result<string, string>",
            "Read the entire remaining stream into a single string.",
        );
    });

    m.userdata("WriterHandle", |w| {
        w.method(
            "write",
            &[("data", "string")],
            "Result<bool, string>",
            "Write all bytes of `data` to the stream.",
        )
        .method(
            "flush",
            &[],
            "Result<bool, string>",
            "Flush any buffered bytes to the underlying sink.",
        );
    });

    Ok(m)
}

#[instrument(name = "io.from_string", target = "luau::lib::io", skip_all)]
fn from_string(_: &Lua, s: mlua::String) -> mlua::Result<ReaderHandle> {
    Ok(ReaderHandle::new(std::io::Cursor::new(
        s.as_bytes().to_vec(),
    )))
}

/// Borrow `ud` as handle type `T`, or short-circuit with `io.copy`'s
/// `(nil, err)` tuple describing the bad argument — the caller returns the
/// `Err` payload verbatim.
fn borrow_or_err<T: 'static>(
    lua: &Lua,
    ud: &AnyUserData,
    msg: &str,
) -> Result<mlua::UserDataRef<T>, mlua::Result<(Value, Value)>> {
    ud.borrow::<T>().map_err(|_| from_err(lua, "io.copy", msg))
}

#[instrument(name = "io.copy", target = "luau::lib::io", skip_all)]
fn copy(
    lua: &Lua,
    (reader_ud, writer_ud): (AnyUserData, AnyUserData),
) -> mlua::Result<(Value, Value)> {
    let rh = match borrow_or_err::<ReaderHandle>(
        lua,
        &reader_ud,
        "first argument is not a reader handle",
    ) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let wh = match borrow_or_err::<WriterHandle>(
        lua,
        &writer_ud,
        "second argument is not a writer handle",
    ) {
        Ok(h) => h,
        Err(e) => return e,
    };
    let mut r = rh.inner.lock();
    let mut w = wh.0.lock();
    let result = std::io::copy(&mut *r, &mut *w);
    io_result(lua, "io.copy", result, |lua, n| ok(lua, n as i64))
}
