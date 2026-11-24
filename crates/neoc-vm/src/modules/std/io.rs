//! Core I/O primitives: `std:io` (mirrors `std::io`).
//!
//! `Reader` and `Writer` are the handles every byte source and sink is given to a script as (a string here, a file
//! from `std:fs`), and what the reader and writer functions of the format modules take (`csv.reader`,
//! `toml.decode`, `serde_json.to_writer`, ...). Their methods mirror Rust's `Read`, `BufRead` and `Write`.
//!
//! ```lua
//! local io = require("std:io")
//! local reader = io.from_string("one\ntwo\n")
//! local line = reader:read_line()   -- "one"
//! local rest = reader:read_all()    -- "two\n"
//! ```
//!
//! The API's rules:
//! - The types are `Reader` and `Writer`.
//! - A failure is a typed `IoError` whose kind is the operating system's (`not_found`, `broken_pipe`, ...).
//! - `read_line` gives the line's bytes whatever they are: a line need not be UTF-8.
//! - `copy` refuses a wrong argument as every function does: raised, with its place.

/// The most one `read(n)` call returns (16 MiB): the contract is "up to `n` bytes", and a Lua-sized `n` must not
/// become a Lua-sized allocation.
const READ_CHUNK_MAX: u64 = 16 << 20;

#[mlua_ir::module("std:io")]
pub mod io {
    use std::io::{BufRead, Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use mlua_ir::{Bytes, LuaError, Object};
    use parking_lot::Mutex;
    use serde::Serialize;

    use super::READ_CHUNK_MAX;

    /// Why reading or writing failed. The kind is the operating system's; `reason` is what it said.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum IoError {
        /// What was asked for does not exist.
        #[error("{reason}")]
        NotFound { reason: String },
        /// The process may not do this.
        #[error("{reason}")]
        PermissionDenied { reason: String },
        /// What was to be made is already there.
        #[error("{reason}")]
        AlreadyExists { reason: String },
        /// An argument is not one the operation takes.
        #[error("{reason}")]
        InvalidInput { reason: String },
        /// The bytes read are not what they should be.
        #[error("{reason}")]
        InvalidData { reason: String },
        /// The operation took too long.
        #[error("{reason}")]
        TimedOut { reason: String },
        /// The operation was interrupted; it can be tried again.
        #[error("{reason}")]
        Interrupted { reason: String },
        /// The source ended before everything expected was read.
        #[error("{reason}")]
        UnexpectedEof { reason: String },
        /// The sink took none of the bytes it was given.
        #[error("{reason}")]
        WriteZero { reason: String },
        /// The other end of the pipe is closed.
        #[error("{reason}")]
        BrokenPipe { reason: String },
        /// There is not enough memory for what was asked.
        #[error("{reason}")]
        OutOfMemory { reason: String },
        /// The platform cannot do this.
        #[error("{reason}")]
        Unsupported { reason: String },
        /// Any other failure.
        #[error("{reason}")]
        Other { reason: String },
    }

    impl From<std::io::Error> for IoError {
        fn from(error: std::io::Error) -> Self {
            use std::io::ErrorKind as Kind;
            let reason = error.to_string();
            match error.kind() {
                Kind::NotFound => IoError::NotFound { reason },
                Kind::PermissionDenied => IoError::PermissionDenied { reason },
                Kind::AlreadyExists => IoError::AlreadyExists { reason },
                Kind::InvalidInput => IoError::InvalidInput { reason },
                Kind::InvalidData => IoError::InvalidData { reason },
                Kind::TimedOut => IoError::TimedOut { reason },
                Kind::Interrupted => IoError::Interrupted { reason },
                Kind::UnexpectedEof => IoError::UnexpectedEof { reason },
                Kind::WriteZero => IoError::WriteZero { reason },
                Kind::BrokenPipe => IoError::BrokenPipe { reason },
                Kind::OutOfMemory => IoError::OutOfMemory { reason },
                Kind::Unsupported => IoError::Unsupported { reason },
                _ => IoError::Other { reason },
            }
        }
    }

    /// A source of bytes, read front to back. Copies of the handle read the same source.
    #[derive(Clone, Object)]
    pub struct Reader {
        source: Arc<Mutex<Box<dyn BufRead + Send>>>,
        claimed: Arc<AtomicBool>,
    }

    /// A [`Reader`] claimed by one streaming consumer (a CSV reader, say); dropping it lets the next one claim.
    pub struct Claim {
        claimed: Arc<AtomicBool>,
    }

    impl Drop for Claim {
        fn drop(&mut self) {
            self.claimed.store(false, Ordering::Release);
        }
    }

    /// A [`Reader`]'s source as a `Read` that takes the lock for each read, for a consumer that outlives the call
    /// it was made in.
    pub struct Locking {
        source: Arc<Mutex<Box<dyn BufRead + Send>>>,
    }

    impl Read for Locking {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.source.lock().read(buf)
        }
    }

    impl Reader {
        /// A handle on `source`, for the module that opens it (`std:fs` for a file).
        pub fn of<R: BufRead + Send + 'static>(source: R) -> Self {
            Self { source: Arc::new(Mutex::new(Box::new(source))), claimed: Arc::new(AtomicBool::new(false)) }
        }

        /// Claims the reader for one streaming consumer, which keeps the claim for as long as it reads; `None`
        /// when another consumer holds it.
        pub fn claim(&self) -> Option<Claim> {
            self.claimed
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .ok()
                .map(|_| Claim { claimed: self.claimed.clone() })
        }

        /// The source as a `Read` that locks for each read.
        pub fn locking(&self) -> Locking {
            Locking { source: self.source.clone() }
        }

        /// Everything left in the source.
        pub fn remaining(&self) -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            self.source.lock().read_to_end(&mut bytes)?;
            Ok(bytes)
        }
    }

    #[mlua_ir::object]
    impl Reader {
        /// Up to `n` bytes (fewer when the source has fewer at hand, and never more than 16 MiB at once), or `nil`
        /// at the end.
        ///
        /// ```luau
        /// local reader = io.from_string("hello world")
        /// assert(reader:read(5) == "hello" and reader:read(100) == " world" and reader:read(1) == nil)
        /// ```
        pub fn read(&self, n: u64) -> Result<Option<Bytes>, IoError> {
            let len = usize::try_from(n.min(READ_CHUNK_MAX)).unwrap_or(usize::MAX);
            let mut buf = Vec::new();
            buf.try_reserve_exact(len)
                .map_err(|e| IoError::OutOfMemory { reason: format!("cannot allocate {len} bytes: {e}") })?;
            buf.resize(len, 0);
            let read = self.source.lock().read(&mut buf)?;
            buf.truncate(read);
            Ok((read > 0).then_some(Bytes(buf)))
        }

        /// The next line without its line ending (`\n` or `\r\n`), or `nil` at the end.
        ///
        /// ```luau
        /// local reader = io.from_string("one\r\ntwo")
        /// assert(reader:read_line() == "one" and reader:read_line() == "two" and reader:read_line() == nil)
        /// ```
        pub fn read_line(&self) -> Result<Option<Bytes>, IoError> {
            let mut line = Vec::new();
            if self.source.lock().read_until(b'\n', &mut line)? == 0 {
                return Ok(None);
            }
            if line.last() == Some(&b'\n') {
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
            }
            Ok(Some(Bytes(line)))
        }

        /// Everything left in the source, as one string.
        pub fn read_all(&self) -> Result<Bytes, IoError> {
            Ok(Bytes(self.remaining()?))
        }
    }

    /// A sink for bytes. Copies of the handle write to the same sink.
    #[derive(Clone, Object)]
    pub struct Writer {
        sink: Arc<Mutex<Box<dyn Write + Send>>>,
    }

    impl Writer {
        /// A handle on `sink`, for the module that opens it (`std:fs` for a file).
        pub fn of<W: Write + Send + 'static>(sink: W) -> Self {
            Self { sink: Arc::new(Mutex::new(Box::new(sink))) }
        }

        /// Writes all of `bytes`, holding the lock for the call.
        pub fn write_all(&self, bytes: &[u8]) -> std::io::Result<()> {
            self.sink.lock().write_all(bytes)
        }
    }

    #[mlua_ir::object]
    impl Writer {
        /// Write all of `data`.
        pub fn write(&self, data: Bytes) -> Result<(), IoError> {
            Ok(self.write_all(&data)?)
        }

        /// Hand any buffered bytes to the underlying sink.
        pub fn flush(&self) -> Result<(), IoError> {
            Ok(self.sink.lock().flush()?)
        }
    }

    /// A reader over the bytes of `data`, for anything that takes a reader.
    ///
    /// ```luau
    /// assert(io.from_string("hello"):read_all() == "hello")
    /// ```
    pub fn from_string(data: Bytes) -> Reader {
        Reader::of(std::io::Cursor::new(data.0))
    }

    /// Copy everything left in `reader` to `writer`; how many bytes that was.
    pub fn copy(reader: Reader, writer: Writer) -> Result<u64, IoError> {
        let mut source = reader.source.lock();
        let mut sink = writer.sink.lock();
        Ok(std::io::copy(&mut *source, &mut *sink)?)
    }
}

/// What the tests of the modules that take a reader or a writer share.
#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::io::Writer;

    /// A sink a test reads back.
    #[derive(Clone, Default)]
    pub(crate) struct Sink {
        pub(crate) bytes: Arc<Mutex<Vec<u8>>>,
        pub(crate) flushes: Arc<Mutex<u32>>,
    }

    impl std::io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.lock().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            *self.flushes.lock() += 1;
            Ok(())
        }
    }

    impl Sink {
        /// What was written, as text.
        pub(crate) fn text(&self) -> String {
            String::from_utf8(self.bytes.lock().clone()).expect("UTF-8")
        }
    }

    /// A source and a sink that always fail.
    pub(crate) struct Broken;

    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the pipe is closed"))
        }
    }

    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "read-only"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("cannot flush"))
        }
    }

    /// An engine with `writer` (over the returned sink), `broken_reader` and `broken_writer` as globals, and the
    /// module `name` as the global `global`.
    pub(crate) fn engine(name: &str, global: &str) -> (mlua::Lua, Sink) {
        use super::io::Reader;
        let lua = neoc_vm_core::make_engine().expect("engine");
        let sink = Sink::default();
        let globals = lua.globals();
        globals.set("writer", lua.create_userdata(Writer::of(sink.clone())).unwrap()).unwrap();
        globals.set("broken_writer", lua.create_userdata(Writer::of(Broken)).unwrap()).unwrap();
        globals
            .set("broken_reader", lua.create_userdata(Reader::of(std::io::BufReader::new(Broken))).unwrap())
            .unwrap();
        let require: mlua::Function = globals.get("require").unwrap();
        globals.set("io", require.call::<mlua::Table>("std:io").unwrap()).unwrap();
        globals.set(global, require.call::<mlua::Table>(name).unwrap()).unwrap();
        (lua, sink)
    }
}

#[cfg(test)]
mod tests {
    use super::io::{IoError, Reader};
    use super::testing::engine;

    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        engine("std:io", "io").0.load(code).eval().unwrap()
    }

    #[test]
    fn read_gives_up_to_n_bytes_then_nil() {
        let ok: bool = eval(
            r#"
            local reader = io.from_string("hello world")
            assert(reader:read(5) == "hello")
            assert(reader:read(1) == " ")
            local rest, err = reader:read(100)
            assert(rest == "world" and err == nil)
            local done, done_err = reader:read(1)
            assert(done == nil and done_err == nil)
            local empty, empty_err = io.from_string(""):read(1)
            return empty == nil and empty_err == nil
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_read_far_past_what_can_be_allocated_returns_what_the_reader_has() {
        let (data, clamped): (String, usize) = eval(
            r#"
            local data = assert(io.from_string("hello"):read(2 ^ 40))
            local big = io.from_string(string.rep("x", (16 * 1024 * 1024) + 5))
            return data, #assert(big:read(2 ^ 40))
            "#,
        );
        assert_eq!(data, "hello");
        assert_eq!(clamped, 16 << 20, "one read gives at most 16 MiB");
    }

    #[test]
    fn read_refuses_a_count_that_is_not_one() {
        let refused: String = eval(
            r#"
            local reader = io.from_string("x")
            assert(not pcall(reader.read, reader, 1.5) and not pcall(reader.read, reader, "1") and not pcall(reader.read, reader))
            local _, err = pcall(reader.read, reader, -1)
            return tostring(err)
            "#,
        );
        assert!(
            refused.contains("std:io.Reader:read: argument #1 'n': expected a whole number from 0"),
            "got: {refused}"
        );
    }

    #[test]
    fn read_line_strips_the_line_ending() {
        let lines: Vec<String> = eval(
            r#"
            local reader = io.from_string("foo\nbar\r\n\nlast")
            local out = {}
            while true do
                local line, err = reader:read_line()
                assert(err == nil)
                if line == nil then break end
                table.insert(out, line)
            end
            assert(reader:read_line() == nil)
            return out
            "#,
        );
        assert_eq!(lines, ["foo", "bar", "", "last"]);
    }

    #[test]
    fn a_line_is_bytes() {
        let ok: bool = eval(
            r#"
            local reader = io.from_string("\255\254\n\0x\r\r\n")
            return reader:read_line() == "\255\254" and reader:read_line() == "\0x\r"
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn read_all_gives_what_is_left() {
        let ok: bool = eval(
            r#"
            local reader = io.from_string("hello world")
            reader:read(6)
            local rest, err = reader:read_all()
            assert(rest == "world" and err == nil)
            assert(reader:read_all() == "" and io.from_string(""):read_all() == "")
            return io.from_string("\0\1\2\254\255"):read_all() == "\0\1\2\254\255"
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_copy_of_the_handle_reads_the_same_source() {
        let (first, second): (String, String) = eval(
            r#"
            local reader = io.from_string("ab")
            local seen
            coroutine.wrap(function() seen = reader:read(1) end)()
            return seen, reader:read(1)
            "#,
        );
        assert_eq!((first.as_str(), second.as_str()), ("a", "b"));
    }

    #[test]
    fn write_and_flush_reach_the_sink() {
        let (lua, sink) = engine("std:io", "io");
        let ok: bool = lua
            .load(
                r#"
                local ok, err = writer:write("hello")
                assert(ok == true and err == nil)
                assert(writer:write("\0\1\2\254\255"))
                local flushed, flush_err = writer:flush()
                return flushed == true and flush_err == nil
                "#,
            )
            .eval()
            .unwrap();
        assert!(ok);
        assert_eq!(*sink.bytes.lock(), b"hello\0\x01\x02\xfe\xff");
        assert_eq!(*sink.flushes.lock(), 1);
    }

    #[test]
    fn copy_moves_what_is_left_and_counts_it() {
        let (lua, sink) = engine("std:io", "io");
        let (n, empty): (u64, u64) = lua
            .load(
                r#"
                local reader = io.from_string("skip copy me")
                reader:read(5)
                local n, err = io.copy(reader, writer)
                assert(err == nil)
                return n, (assert(io.copy(reader, writer)))
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!((n, empty), (7, 0));
        assert_eq!(sink.text(), "copy me");
    }

    #[test]
    fn copy_refuses_what_is_not_a_reader_and_a_writer() {
        let (lua, _) = engine("std:io", "io");
        let (first, second): (String, String) = lua
            .load(
                r#"
                local reader = io.from_string("x")
                local ok1, first = pcall(io.copy, writer, writer)
                local ok2, second = pcall(io.copy, reader, reader)
                assert(not ok1 and not ok2 and not pcall(io.copy, reader) and not pcall(io.copy, "x", writer))
                return tostring(first), tostring(second)
                "#,
            )
            .eval()
            .unwrap();
        assert!(first.contains("std:io.copy: argument #1 'reader': expected a Reader"), "got: {first}");
        assert!(second.contains("std:io.copy: argument #2 'writer': expected a Writer"), "got: {second}");
    }

    #[test]
    fn a_failure_is_a_typed_error() {
        let (lua, _) = engine("std:io", "io");
        let seen: Vec<String> = lua
            .load(
                r#"
                local out = {}
                for _, call in {
                    function() return broken_reader:read(1) end,
                    function() return broken_reader:read_line() end,
                    function() return broken_reader:read_all() end,
                    function() return broken_writer:write("x") end,
                    function() return broken_writer:flush() end,
                    function() return io.copy(broken_reader, writer) end,
                    function() return io.copy(io.from_string("x"), broken_writer) end,
                } do
                    local value, err = call()
                    assert(value == nil and type(err.reason) == "string" and err.message == err.reason)
                    table.insert(out, tostring(err))
                end
                return out
                "#,
            )
            .eval()
            .unwrap();
        assert_eq!(
            seen,
            [
                "std:io.Reader:read: broken_pipe: the pipe is closed",
                "std:io.Reader:read_line: broken_pipe: the pipe is closed",
                "std:io.Reader:read_all: broken_pipe: the pipe is closed",
                "std:io.Writer:write: permission_denied: read-only",
                "std:io.Writer:flush: other: cannot flush",
                "std:io.copy: broken_pipe: the pipe is closed",
                "std:io.copy: permission_denied: read-only",
            ]
        );
    }

    #[test]
    fn an_error_keeps_the_operating_systems_kind() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            IoError::from(Error::new(ErrorKind::NotFound, "gone")),
            IoError::NotFound { reason: "gone".into() }
        );
        assert_eq!(
            IoError::from(Error::new(ErrorKind::UnexpectedEof, "short")),
            IoError::UnexpectedEof { reason: "short".into() }
        );
        assert_eq!(
            IoError::from(Error::new(ErrorKind::AddrInUse, "busy")),
            IoError::Other { reason: "busy".into() }
        );
    }

    #[test]
    fn a_reader_is_claimed_by_one_consumer_at_a_time() {
        use std::io::Read;
        let reader = Reader::of(std::io::Cursor::new(b"abcdef".to_vec()));
        let claim = reader.claim().expect("free");
        assert!(reader.claim().is_none() && reader.clone().claim().is_none(), "copies share the claim");
        drop(claim);
        let _again = reader.claim().expect("free again");

        let mut locking = reader.locking();
        let mut two = [0; 2];
        locking.read_exact(&mut two).unwrap();
        assert_eq!(&two, b"ab");
        assert_eq!(reader.remaining().unwrap(), b"cdef", "the locking view reads the same source");
    }
}
