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
}

impl UserData for ReaderHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // TODO: clean-code - 0.7 - DRY: repeated `match result { Ok => ok, Err => from_err("io.X", e) }` across all 5 methods + copy — extract a small helper or macro.
        methods.add_method("read", |lua, this, n: usize| {
            let mut buf = vec![0u8; n];
            let result = this.inner.lock().read(&mut buf);
            match result {
                Ok(0) => ok(lua, Value::Nil),
                Ok(k) => ok(lua, lua.create_string(&buf[..k])?),
                Err(e) => from_err(lua, "io.read", e),
            }
        });

        methods.add_method("read_line", |lua, this, _: ()| {
            let mut line = String::new();
            let result = this.inner.lock().read_line(&mut line);
            match result {
                Ok(0) => ok(lua, Value::Nil),
                Ok(_) => {
                    // TODO: clean-code - 0.55 - KISS: two sequential `ends_with`+`pop` blocks to strip CRLF — `trim_end_matches(['\r','\n'])` or `str::strip_suffix` chain is clearer.
                    if line.ends_with('\n') {
                        line.pop();
                    }
                    if line.ends_with('\r') {
                        line.pop();
                    }
                    ok(lua, lua.create_string(line.as_bytes())?)
                }
                Err(e) => from_err(lua, "io.read_line", e),
            }
        });

        methods.add_method("read_all", |lua, this, _: ()| {
            let mut buf = Vec::new();
            let result = this.inner.lock().read_to_end(&mut buf);
            match result {
                Ok(_) => ok(lua, lua.create_string(&buf)?),
                Err(e) => from_err(lua, "io.read_all", e),
            }
        });
    }
}

impl UserData for WriterHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("write", |lua, this, data: mlua::String| {
            let result = this.0.lock().write_all(&data.as_bytes());
            match result {
                Ok(()) => ok(lua, true),
                Err(e) => from_err(lua, "io.write", e),
            }
        });

        methods.add_method("flush", |lua, this, _: ()| {
            let result = this.0.lock().flush();
            match result {
                Ok(()) => ok(lua, true),
                Err(e) => from_err(lua, "io.flush", e),
            }
        });
    }
}

// ---------------------------------------------------------------------------
// module — constructors and multi-handle operations
// ---------------------------------------------------------------------------

pub fn module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("from_string", lua.create_function(from_string)?)?;
    t.set("copy", lua.create_function(copy)?)?;
    Ok(t)
}

#[instrument(name = "io.from_string", target = "luau::lib::io", skip_all)]
fn from_string(_: &Lua, s: mlua::String) -> mlua::Result<ReaderHandle> {
    Ok(ReaderHandle::new(std::io::Cursor::new(
        s.as_bytes().to_vec(),
    )))
}

#[instrument(name = "io.copy", target = "luau::lib::io", skip_all)]
fn copy(
    lua: &Lua,
    (reader_ud, writer_ud): (AnyUserData, AnyUserData),
) -> mlua::Result<(Value, Value)> {
    // TODO: clean-code - 0.6 - DRY: two near-identical `match userdata.borrow() { Ok => h, Err => from_err }` blocks — extract a typed helper `borrow_or_err(lua, ud, "io.copy", "...")`.
    let rh = match reader_ud.borrow::<ReaderHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "io.copy", "first argument is not a reader handle"),
    };
    let wh = match writer_ud.borrow::<WriterHandle>() {
        Ok(h) => h,
        Err(_) => return from_err(lua, "io.copy", "second argument is not a writer handle"),
    };
    let mut r = rh.inner.lock();
    let mut w = wh.0.lock();
    let result = std::io::copy(&mut *r, &mut *w);
    // TODO: clean-code - 0.6 - KISS: explicit `drop` chain on guards/refs that go out of scope at function end — redundant; scope handles release.
    drop(r);
    drop(w);
    drop(rh);
    drop(wh);
    match result {
        Ok(n) => ok(lua, n as i64),
        Err(e) => from_err(lua, "io.copy", e),
    }
}
