//! CSV reading and writing: `lib:csv`.
//!
//! Backed by the `csv` crate. A text is read as rows (lists of fields: `parse`, `rows`) or as records keyed by
//! its header row (`parse_records`, `records`), all at once or as a stream; it is written from rows or records
//! (`stringify`, `writer`).
//!
//! Ported from neoc-vm's `lib:csv`. What changed on purpose:
//! - What a row is no longer depends on an option: `parse` / `rows` give lists of strings, `parse_records` /
//!   `records` give records keyed by header (they were `parse` / `rows` with `headers = true`), so `types` belongs
//!   to the record functions only.
//! - `rows` / `records` return a stream (`:next()`, `:items()`, `:close()`); `rows` was a bare iterator function.
//! - The header list is a parameter of `stringify` and `writer` (it was a required `headers` option), and
//!   `stringify` takes positional rows as well as records.
//! - Failures are typed: `CsvError` (`unequal_lengths`, `duplicate_header`, `unknown_column`, `cannot_coerce`,
//!   `invalid_utf8`, `malformed`) and `WriteError` (`no_headers`, `wrong_length`, `finished`, `rejected`).
//! - A wrong option (a delimiter of two characters, an unknown column type) or a row that is neither a list nor a
//!   record is refused as an argument. A `types` entry for a column the header does not have is an error (it was
//!   ignored).
//!
//! TODO(std:io): `csv.reader(handle)` (rows from a `ReaderHandle`, with `:headers()`) waits for `std:io`.

/// How much of an offending line an error quotes.
const MAX_QUOTED_BYTES: usize = 80;

/// `text` cut to `max` bytes (on a character boundary), with `…` when something was cut.
fn truncated(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let end = (0..=max).rev().find(|at| text.is_char_boundary(*at)).unwrap_or(0);
    format!("{}…", &text[..end])
}

/// A record's bytes without its terminator.
fn without_terminator(bytes: &[u8]) -> &[u8] {
    // The csv crate's position is just past the first terminator byte. After a CRLF that is the `\n`, so the
    // slice starts with the previous record's `\n`.
    let bytes = bytes.strip_prefix(b"\n").unwrap_or(bytes);
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    bytes.strip_suffix(b"\r").unwrap_or(bytes)
}

fn wide(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[mlua_ir::module("lib:csv")]
pub mod csv {
    use std::collections::{BTreeMap, BTreeSet};
    use std::io::Cursor;
    use std::sync::Arc;

    use ::csv::ByteRecord;
    use mlua_ir::{Bytes, LuaError, Object, OneOf, Options, Shape, Stream, Text};
    use parking_lot::Mutex;
    use serde::{Deserialize, Serialize};

    use super::{MAX_QUOTED_BYTES, truncated, wide, without_terminator};

    /// A one-character symbol of the dialect (a delimiter, a quote): a single ASCII character.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Text)]
    #[text(validate = "single_byte")]
    #[serde(try_from = "String", into = "String")]
    pub struct Symbol(String);

    fn single_byte(text: &str) -> Result<(), String> {
        if text.len() == 1 { Ok(()) } else { Err(format!("`{text}` is not a single ASCII character")) }
    }

    impl TryFrom<String> for Symbol {
        type Error = String;
        fn try_from(text: String) -> Result<Self, String> {
            single_byte(&text).map(|()| Self(text))
        }
    }

    impl From<Symbol> for String {
        fn from(symbol: Symbol) -> Self {
            symbol.0
        }
    }

    impl Symbol {
        fn of(byte: u8) -> Self {
            Self(char::from(byte).to_string())
        }

        fn byte(&self) -> u8 {
            self.0.bytes().next().unwrap_or(b',')
        }
    }

    /// What a typed column's fields become.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
    #[serde(rename_all = "snake_case")]
    pub enum ColumnType {
        /// A number, as Rust reads one (`1`, `-2.5`, `1e3`).
        Number,
        /// `true` / `1` or `false` / `0`, in any case.
        Boolean,
    }

    impl ColumnType {
        fn name(self) -> &'static str {
            match self {
                ColumnType::Number => "number",
                ColumnType::Boolean => "boolean",
            }
        }
    }

    /// Why a text could not be read as CSV.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum CsvError {
        /// A row has more or fewer fields than the first one.
        #[error("line {line}: found {found} fields, expected {expected}: \"{text}\"")]
        UnequalLengths {
            /// The line the row starts on, counting from 1.
            line: u32,
            /// How many fields it has.
            found: u32,
            /// How many the first row has.
            expected: u32,
            /// The row as written, cut to 80 bytes.
            text: String,
        },
        /// Two columns of the header row have the same name.
        #[error("line 1: duplicate header \"{header}\": \"{text}\"")]
        DuplicateHeader {
            /// The repeated name.
            header: String,
            /// The header row as written, cut to 80 bytes.
            text: String,
        },
        /// `types` names a column the header row does not have.
        #[error("`types` names the column \"{column}\", which the header row does not have")]
        UnknownColumn {
            /// The name in `types`.
            column: String,
        },
        /// A field of a typed column is not what the type reads.
        #[error("line {line}: column \"{column}\" cannot coerce \"{value}\" to {}", .expected.name())]
        CannotCoerce {
            /// The line the row starts on, counting from 1.
            line: u32,
            /// The column's header.
            column: String,
            /// The field, cut to 80 bytes.
            value: String,
            /// The column's type.
            expected: ColumnType,
        },
        /// A header, or a field of a typed column, is not UTF-8.
        #[error("line {line}: invalid utf-8: {reason}")]
        InvalidUtf8 {
            /// The line the row starts on, counting from 1.
            line: u32,
            /// Where the bytes stop being UTF-8.
            reason: String,
        },
        /// Anything else the reader refuses.
        #[error("{reason}")]
        Malformed {
            /// What the reader said.
            reason: String,
        },
    }

    /// Why rows could not be written as CSV.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error, LuaError)]
    #[serde(tag = "kind", rename_all = "snake_case")]
    pub enum WriteError {
        /// The header list is empty.
        #[error("the header list must name at least one column")]
        NoHeaders,
        /// A positional row does not have one value per header.
        #[error("row has {found} values, expected {expected} (one per header)")]
        WrongLength {
            /// How many values the row has.
            found: u32,
            /// How many headers there are.
            expected: u32,
        },
        /// The writer was already finished.
        #[error("already finished")]
        Finished,
        /// Anything else the writer refuses.
        #[error("{reason}")]
        Rejected {
            /// What the writer said.
            reason: String,
        },
    }

    /// The dialect `parse` and `rows` read.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    pub struct ReadOptions {
        /// What separates fields.
        pub delimiter: Symbol,
        /// What quotes a field.
        pub quote: Symbol,
        /// What escapes a quote inside a quoted field; the quote itself (doubled) when not given.
        pub escape: Option<Symbol>,
        /// A line that starts with this is skipped; no comments when not given.
        pub comment: Option<Symbol>,
    }

    impl Default for ReadOptions {
        fn default() -> Self {
            Self { delimiter: Symbol::of(b','), quote: Symbol::of(b'"'), escape: None, comment: None }
        }
    }

    /// The dialect `parse_records` and `records` read, and the types of its columns.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    pub struct RecordOptions {
        /// What separates fields.
        pub delimiter: Symbol,
        /// What quotes a field.
        pub quote: Symbol,
        /// What escapes a quote inside a quoted field; the quote itself (doubled) when not given.
        pub escape: Option<Symbol>,
        /// A line that starts with this is skipped; no comments when not given.
        pub comment: Option<Symbol>,
        /// Columns to read as something other than a string, by header. An empty field of a typed column is
        /// left out of its record.
        pub types: BTreeMap<String, ColumnType>,
    }

    impl Default for RecordOptions {
        fn default() -> Self {
            let ReadOptions { delimiter, quote, escape, comment } = ReadOptions::default();
            Self { delimiter, quote, escape, comment, types: BTreeMap::new() }
        }
    }

    /// The dialect `stringify` and `writer` write.
    #[derive(Debug, Clone, Serialize, Deserialize, Options)]
    pub struct WriteOptions {
        /// What separates fields.
        pub delimiter: Symbol,
        /// What quotes a field that needs it.
        pub quote: Symbol,
    }

    impl Default for WriteOptions {
        fn default() -> Self {
            Self { delimiter: Symbol::of(b','), quote: Symbol::of(b'"') }
        }
    }

    /// A field of a record: the text as written, or what its column's type reads it as.
    #[derive(Debug, OneOf)]
    pub enum Field {
        /// A field of a `number` column.
        Number(f64),
        /// A field of a `boolean` column.
        Flag(bool),
        /// A field of an untyped column: its bytes, unquoted.
        Text(Bytes),
    }

    /// A value to write: text as it is, a number the shortest way that reads back, a boolean as `true` / `false`.
    #[derive(Debug, Clone, OneOf)]
    pub enum Cell {
        /// Written as it is (quoted when it needs to be).
        Text(String),
        /// Written the shortest way that reads back (`30`, `1.5`).
        Number(f64),
        /// Written `true` or `false`.
        Flag(bool),
    }

    /// A row to write: one value per header in order, or a record whose values are looked up by header (a missing
    /// one is an empty field, and keys that are not headers are ignored).
    #[derive(Debug, Clone, OneOf)]
    pub enum Row {
        /// One value per header, in order.
        Positional(Vec<Cell>),
        /// Values by header.
        Keyed(BTreeMap<String, Cell>),
    }

    /// A text being read: the `csv` reader over it, and the text itself for quoting a line in an error.
    #[derive(Debug)]
    struct Source {
        reader: ::csv::Reader<Cursor<Vec<u8>>>,
        done: bool,
    }

    impl Source {
        fn new(text: Vec<u8>, has_headers: bool, delimiter: &Symbol, quote: &Symbol, escape: Option<&Symbol>, comment: Option<&Symbol>) -> Self {
            let reader = ::csv::ReaderBuilder::new()
                .has_headers(has_headers)
                .delimiter(delimiter.byte())
                .quote(quote.byte())
                .escape(Some(escape.unwrap_or(quote).byte()))
                .comment(comment.map(Symbol::byte))
                .flexible(false)
                .from_reader(Cursor::new(text));
            Self { reader, done: false }
        }

        /// The source from byte `start` up to where the reader is, as an error quotes it.
        fn quoted(&self, start: u64) -> String {
            let text = self.reader.get_ref().get_ref();
            let end = usize::try_from(self.reader.position().byte()).unwrap_or(usize::MAX).min(text.len());
            let start = usize::try_from(start).unwrap_or(usize::MAX).min(end);
            truncated(&String::from_utf8_lossy(without_terminator(&text[start..end])), MAX_QUOTED_BYTES)
        }

        fn refused(&self, error: &::csv::Error) -> CsvError {
            match error.kind() {
                ::csv::ErrorKind::UnequalLengths { pos, expected_len, len } => CsvError::UnequalLengths {
                    line: pos.as_ref().map_or(0, |p| wide(p.line())),
                    found: wide(*len),
                    expected: wide(*expected_len),
                    text: pos.as_ref().map(|p| self.quoted(p.byte())).unwrap_or_default(),
                },
                ::csv::ErrorKind::Utf8 { pos, err } => {
                    CsvError::InvalidUtf8 { line: pos.as_ref().map_or(0, |p| wide(p.line())), reason: err.to_string() }
                }
                _ => CsvError::Malformed { reason: error.to_string() },
            }
        }

        /// The header row, with no name twice.
        fn headers(&mut self) -> Result<Vec<String>, CsvError> {
            let headers: Vec<String> = match self.reader.headers() {
                Ok(record) => record.iter().map(str::to_owned).collect(),
                Err(error) => return Err(self.refused(&error)),
            };
            let mut seen = BTreeSet::new();
            match headers.iter().find(|header| !seen.insert(header.as_str())) {
                Some(repeated) => Err(CsvError::DuplicateHeader { header: repeated.clone(), text: self.quoted(0) }),
                None => Ok(headers),
            }
        }

        /// The next row and the line it starts on; `None` at the end, and after an error.
        fn next_row(&mut self) -> Option<Result<(ByteRecord, u32), CsvError>> {
            if self.done {
                return None;
            }
            let mut record = ByteRecord::new();
            match self.reader.read_byte_record(&mut record) {
                Ok(true) => {
                    let line = record.position().map_or(0, |p| wide(p.line()));
                    Some(Ok((record, line)))
                }
                Ok(false) => {
                    self.done = true;
                    None
                }
                Err(error) => {
                    self.done = true;
                    Some(Err(self.refused(&error)))
                }
            }
        }
    }

    /// The rows of a text, as lists of fields.
    #[derive(Debug)]
    struct Rows {
        source: Source,
    }

    impl Rows {
        fn new(text: Bytes, opts: &ReadOptions) -> Self {
            let source = Source::new(text.0, false, &opts.delimiter, &opts.quote, opts.escape.as_ref(), opts.comment.as_ref());
            Self { source }
        }
    }

    impl Iterator for Rows {
        type Item = Result<Vec<Bytes>, CsvError>;

        fn next(&mut self) -> Option<Self::Item> {
            let row = self.source.next_row()?;
            Some(row.map(|(record, _)| record.iter().map(|field| Bytes(field.to_vec())).collect()))
        }
    }

    /// The rows of a text after its header row, as records.
    #[derive(Debug)]
    struct Records {
        source: Source,
        /// Read with the first record, so a stream fails where it is read, not where it is made.
        headers: Option<Vec<String>>,
        types: BTreeMap<String, ColumnType>,
    }

    impl Records {
        fn new(text: Bytes, opts: RecordOptions) -> Self {
            let source = Source::new(text.0, true, &opts.delimiter, &opts.quote, opts.escape.as_ref(), opts.comment.as_ref());
            Self { source, headers: None, types: opts.types }
        }

        fn read_headers(&mut self) -> Result<Vec<String>, CsvError> {
            let headers = self.source.headers()?;
            match self.types.keys().find(|column| !headers.contains(column)) {
                Some(column) => Err(CsvError::UnknownColumn { column: column.clone() }),
                None => Ok(headers),
            }
        }

        fn record(&self, headers: &[String], row: &ByteRecord, line: u32) -> Result<BTreeMap<String, Field>, CsvError> {
            let mut record = BTreeMap::new();
            for (header, raw) in headers.iter().zip(row.iter()) {
                let field = match self.types.get(header) {
                    None => Field::Text(Bytes(raw.to_vec())),
                    Some(_) if raw.is_empty() => continue,
                    Some(expected) => coerced(raw, *expected, header, line)?,
                };
                record.insert(header.clone(), field);
            }
            Ok(record)
        }
    }

    fn coerced(raw: &[u8], expected: ColumnType, column: &str, line: u32) -> Result<Field, CsvError> {
        let text = std::str::from_utf8(raw)
            .map_err(|e| CsvError::InvalidUtf8 { line, reason: format!("column \"{column}\": {e}") })?;
        let field = match expected {
            ColumnType::Number => text.parse::<f64>().ok().map(Field::Number),
            ColumnType::Boolean => match text.to_ascii_lowercase().as_str() {
                "true" | "1" => Some(Field::Flag(true)),
                "false" | "0" => Some(Field::Flag(false)),
                _ => None,
            },
        };
        field.ok_or_else(|| CsvError::CannotCoerce {
            line,
            column: column.to_owned(),
            value: truncated(text, MAX_QUOTED_BYTES),
            expected,
        })
    }

    impl Iterator for Records {
        type Item = Result<BTreeMap<String, Field>, CsvError>;

        fn next(&mut self) -> Option<Self::Item> {
            if self.source.done {
                return None;
            }
            let headers = match self.headers.take() {
                Some(headers) => headers,
                None => match self.read_headers() {
                    Ok(headers) => headers,
                    Err(error) => {
                        self.source.done = true;
                        return Some(Err(error));
                    }
                },
            };
            let item = self.source.next_row()?.and_then(|(row, line)| self.record(&headers, &row, line));
            self.source.done |= item.is_err();
            self.headers = Some(headers);
            Some(item)
        }
    }

    /// Every row of `text`, each a list of its fields. Rows must all have as many fields as the first.
    ///
    /// ```luau
    /// local rows = assert(csv.parse("a,b\n1,2"))
    /// assert(rows[1][1] == "a" and rows[2][2] == "2")
    /// local _, err = csv.parse("a,b\n1,2,3")
    /// assert(err ~= nil and err.kind == "unequal_lengths" and err.line == 2)
    /// ```
    pub fn parse(text: Bytes, opts: ReadOptions) -> Result<Vec<Vec<Bytes>>, CsvError> {
        Rows::new(text, &opts).collect()
    }

    /// Every row of `text` after its header row, each a record keyed by header. Fields are strings, except those
    /// of the columns `opts.types` names.
    ///
    /// ```luau
    /// local people = assert(csv.parse_records("name,age\nada,36\ngrace,", { types = { age = "number" } }))
    /// assert(people[1].name == "ada" and people[1].age == 36 and people[2].age == nil)
    /// ```
    pub fn parse_records(text: Bytes, opts: RecordOptions) -> Result<Vec<BTreeMap<String, Field>>, CsvError> {
        Records::new(text, opts).collect()
    }

    /// The rows of `text` one at a time, each a list of its fields. The stream ends at the first error.
    ///
    /// ```luau
    /// local fields = 0
    /// for row in csv.rows("a,b\n1,2"):items() do
    ///     fields += #row
    /// end
    /// assert(fields == 4)
    /// ```
    pub fn rows(text: Bytes, opts: ReadOptions) -> Stream<Vec<Bytes>, CsvError> {
        Stream::new(futures::stream::iter(Rows::new(text, &opts)))
    }

    /// The rows of `text` after its header row one at a time, each a record keyed by header. The stream ends at
    /// the first error.
    ///
    /// ```luau
    /// local stream = csv.records("name,age\nada,36")
    /// local person = assert(stream:next())
    /// assert(person.name == "ada" and person.age == "36")
    /// assert(stream:next() == nil)
    /// ```
    pub fn records(text: Bytes, opts: RecordOptions) -> Stream<BTreeMap<String, Field>, CsvError> {
        Stream::new(futures::stream::iter(Records::new(text, opts)))
    }

    fn cell_text(cell: Cell) -> String {
        match cell {
            Cell::Text(text) => text,
            Cell::Number(number) => number.to_string(),
            Cell::Flag(flag) => flag.to_string(),
        }
    }

    /// The fields of `row`, one per header.
    fn fields(row: Row, headers: &[String]) -> Result<Vec<String>, WriteError> {
        let mut keyed = match row {
            // an empty table is a record with every value missing
            Row::Positional(cells) if cells.is_empty() => BTreeMap::new(),
            Row::Positional(cells) if cells.len() == headers.len() => return Ok(cells.into_iter().map(cell_text).collect()),
            Row::Positional(cells) => {
                return Err(WriteError::WrongLength { found: wide(cells.len() as u64), expected: wide(headers.len() as u64) });
            }
            Row::Keyed(keyed) => keyed,
        };
        Ok(headers.iter().map(|header| keyed.remove(header).map(cell_text).unwrap_or_default()).collect())
    }

    fn rejected(error: impl std::fmt::Display) -> WriteError {
        WriteError::Rejected { reason: error.to_string() }
    }

    /// A CSV writer with its header row written.
    fn started(headers: &[String], opts: &WriteOptions) -> Result<::csv::Writer<Vec<u8>>, WriteError> {
        if headers.is_empty() {
            return Err(WriteError::NoHeaders);
        }
        let mut writer = ::csv::WriterBuilder::new()
            .delimiter(opts.delimiter.byte())
            .quote(opts.quote.byte())
            .flexible(false)
            .from_writer(Vec::new());
        writer.write_record(headers).map_err(rejected)?;
        Ok(writer)
    }

    fn finished(writer: ::csv::Writer<Vec<u8>>) -> Result<Bytes, WriteError> {
        writer.into_inner().map(Bytes).map_err(|e| rejected(e.error()))
    }

    /// `rows` as CSV text under a header row: `headers` names the columns and gives their order.
    ///
    /// ```luau
    /// local text = assert(csv.stringify({ { name = "ada", age = 36 }, { "grace", 45 } }, { "name", "age" }))
    /// assert(text == "name,age\nada,36\ngrace,45\n")
    /// ```
    pub fn stringify(rows: Vec<Row>, headers: Vec<String>, opts: WriteOptions) -> Result<Bytes, WriteError> {
        let mut writer = started(&headers, &opts)?;
        for row in rows {
            writer.write_record(fields(row, &headers)?).map_err(rejected)?;
        }
        finished(writer)
    }

    #[derive(Debug)]
    struct Pending {
        /// `None` once finished.
        writer: Option<::csv::Writer<Vec<u8>>>,
        headers: Vec<String>,
    }

    /// CSV text being written row by row, in memory, until `finish`.
    #[derive(Debug, Clone, Object)]
    pub struct Writer {
        pending: Arc<Mutex<Pending>>,
    }

    #[mlua_ir::object]
    impl Writer {
        /// Append a row: one value per header in order, or a record looked up by header.
        pub fn row(&self, values: Row) -> Result<(), WriteError> {
            let mut pending = self.pending.lock();
            let fields = fields(values, &pending.headers)?;
            let writer = pending.writer.as_mut().ok_or(WriteError::Finished)?;
            writer.write_record(fields).map_err(rejected)
        }

        /// The text written so far. The writer takes no more rows, and a second `finish` is an error.
        pub fn finish(&self) -> Result<Bytes, WriteError> {
            finished(self.pending.lock().writer.take().ok_or(WriteError::Finished)?)
        }
    }

    /// A writer whose text starts with the header row `headers`.
    ///
    /// ```luau
    /// local w = assert(csv.writer({ "test", "status" }))
    /// assert(w:row({ "login", "PASS" }))
    /// assert(w:row({ test = "signup", status = "FAIL" }))
    /// assert(w:finish() == "test,status\nlogin,PASS\nsignup,FAIL\n")
    /// ```
    pub fn writer(headers: Vec<String>, opts: WriteOptions) -> Result<Writer, WriteError> {
        let writer = started(&headers, &opts)?;
        Ok(Writer { pending: Arc::new(Mutex::new(Pending { writer: Some(writer), headers })) })
    }
}

#[cfg(test)]
mod tests {
    /// What `code` returns, run where it may wait (a stream's `next` yields).
    fn eval<T: mlua::FromLuaMulti>(code: &str) -> T {
        try_eval(code).unwrap()
    }

    fn try_eval<T: mlua::FromLuaMulti>(code: &str) -> mlua::Result<T> {
        let lua = crate::make_engine().expect("engine");
        let m: mlua::Table = lua.load(r#"return require("lib:csv")"#).eval().unwrap();
        lua.globals().set("csv", m).unwrap();
        tokio::runtime::Runtime::new().unwrap().block_on(lua.load(code).eval_async())
    }

    /// `(kind, tostring)` of the error `<call>` returns.
    fn refused(call: &str) -> (String, String) {
        eval(&format!("local v, e = {call}; assert(v == nil); return e.kind, tostring(e)"))
    }

    #[test]
    fn parse_rows_no_headers() {
        let result: Vec<Vec<String>> = eval(r#"return (csv.parse("a,b,c\n1,2,3\n4,5,6"))"#);
        assert_eq!(result, vec![vec!["a", "b", "c"], vec!["1", "2", "3"], vec!["4", "5", "6"]]);
    }

    #[test]
    fn parse_with_headers() {
        let (name, age, count): (String, String, i64) = eval(
            r#"
            local rows = csv.parse_records("name,age\nAlice,30\nBob,25")
            return rows[1].name, rows[2].age, #rows
            "#,
        );
        assert_eq!((name.as_str(), age.as_str(), count), ("Alice", "25", 2));
    }

    #[test]
    fn parse_with_types() {
        let (n, t, yes, no): (f64, String, bool, bool) = eval(
            r#"
            local rows = csv.parse_records("duration,passed,skipped\n1.23,TRUE,0", {
                types = { duration = "number", passed = "boolean", skipped = "boolean" },
            })
            return rows[1].duration, type(rows[1].duration), rows[1].passed, rows[1].skipped
            "#,
        );
        assert!((n - 1.23).abs() < 1e-9);
        assert_eq!(t, "number");
        assert!(yes && !no);
    }

    #[test]
    fn types_belong_to_records_and_name_real_columns() {
        let e: String = eval(
            r#"
            local ok, e = pcall(csv.parse, "a,b\n1,2", { types = { a = "number" } })
            assert(not ok)
            assert(not pcall(csv.parse_records, "a\n1", { types = { a = "integer" } }))
            return tostring(e)
            "#,
        );
        assert!(e.contains("lib:csv.parse: argument #2 'opts'"), "got: {e}");
        let (kind, text) = refused(r#"csv.parse_records("a,b\n1,2", { types = { c = "number" } })"#);
        assert_eq!(kind, "unknown_column");
        assert!(text.contains("\"c\""), "got: {text}");
    }

    #[test]
    fn parse_duplicate_headers_errors() {
        let (kind, e) = refused(r#"csv.parse_records("name,age,name\nAlice,30,bad")"#);
        assert_eq!(kind, "duplicate_header");
        assert_eq!(e, r#"lib:csv.parse_records: duplicate_header: line 1: duplicate header "name": "name,age,name""#);
    }

    #[test]
    fn parse_unequal_lengths_errors() {
        let (kind, e) = refused(r#"csv.parse_records("a,b\n1,2\n3")"#);
        assert_eq!(kind, "unequal_lengths");
        assert!(e.contains("line 3: found 1 fields, expected 2"), "got: {e}");
    }

    #[test]
    fn parse_error_preserves_source_quoting() {
        // The first field is quoted in the source though it is one character; the error quotes the source.
        let (_, e) = refused(r#"csv.parse('a,b,c\n"x",y,z,extra\n')"#);
        assert!(e.ends_with(r#": ""x",y,z,extra""#), "expected source-quoted snippet, got: {e}");
    }

    #[test]
    fn parse_error_preserves_escaped_quotes() {
        let (_, e) = refused(r#"csv.parse('a,b\n"he said ""hi""",1,extra')"#);
        assert!(e.contains(r#""he said ""hi""",1,extra"#), "expected escaped quotes preserved, got: {e}");
    }

    // CRLF-terminated input: the snippet is the record's own bytes, with no `\n` carried over from the CRLF before.
    #[test]
    fn parse_error_strips_leading_lf_from_crlf_input() {
        let (_, e) = refused(r#"csv.parse('a,b\r\n"x",y,extra\r\n')"#);
        assert!(e.ends_with(r#": ""x",y,extra""#), "expected snippet with no leading \\n, got: {e}");
    }

    #[test]
    fn parse_error_truncates_long_line() {
        let script = format!(r#"csv.parse('a,b\n{},extra')"#, "x".repeat(120) + ",y");
        let (_, e) = refused(&script);
        assert!(e.ends_with("…\""), "expected trailing ellipsis, got: {e}");
        let cut: String = eval(&format!("local _, e = {script}; return e.text"));
        assert_eq!(cut, format!("{}…", "x".repeat(80)));
    }

    #[test]
    fn parse_empty_field_untyped_is_empty_string() {
        assert_eq!(eval::<String>(r#"local r = csv.parse("a,,c") return r[1][2]"#), "");
        assert_eq!(eval::<String>(r#"local r = csv.parse_records("a,b\n1,") return r[1].b"#), "");
    }

    #[test]
    fn parse_empty_field_typed_is_nil() {
        let is_nil: bool = eval(
            r#"
            local r = csv.parse_records("name,age\nAlice,", { types = { age = "number" } })
            return r[1].age == nil and r[1].name == "Alice"
            "#,
        );
        assert!(is_nil);
    }

    #[test]
    fn parse_type_coercion_failure_errors() {
        let (kind, e) = refused(r#"csv.parse_records("dur\n1\nabc", { types = { dur = "number" } })"#);
        assert_eq!(kind, "cannot_coerce");
        assert_eq!(e, r#"lib:csv.parse_records: cannot_coerce: line 3: column "dur" cannot coerce "abc" to number"#);
        let (kind, e) = refused(r#"csv.parse_records("ok\nmaybe", { types = { ok = "boolean" } })"#);
        assert_eq!(kind, "cannot_coerce");
        assert!(e.ends_with("to boolean"), "got: {e}");
    }

    #[test]
    fn parse_dialects() {
        assert_eq!(eval::<String>(r#"local r = csv.parse_records("a\tb\nx\ty", { delimiter = "\t" }) return r[1].b"#), "y");
        assert_eq!(eval::<String>(r#"local r = csv.parse("'a;b';c", { delimiter = ";", quote = "'" }) return r[1][1]"#), "a;b");
        assert_eq!(eval::<String>(r#"local r = csv.parse('"a\\"b",c', { escape = "\\" }) return r[1][1]"#), "a\"b");
        assert_eq!(eval::<i64>(r##"return #csv.parse("# note\na,b\n# more\n1,2", { comment = "#" })"##), 2);
        assert_eq!(eval::<String>(r#"local r = csv.parse('"say ""hi""",x') return r[1][1]"#), "say \"hi\"");
    }

    #[test]
    fn a_symbol_is_one_character() {
        let e: String = eval(
            r#"
            assert(not pcall(csv.parse, "a,b", { delimiter = "" }))
            assert(not pcall(csv.parse, "a,b", { quote = "é" }))
            assert(not pcall(csv.parse, "a,b", { comment = 1 }))
            assert(not pcall(csv.stringify, {}, { "a" }, { delimiter = "::" }))
            local _, e = pcall(csv.parse, "a,b", { delimiter = ",," })
            return tostring(e)
            "#,
        );
        assert!(e.contains("lib:csv.parse: argument #2 'opts'"), "got: {e}");
        assert!(e.contains("is not a single ASCII character"), "got: {e}");
    }

    #[test]
    fn parse_handles_quoted_field_with_newline() {
        assert_eq!(eval::<String>(r#"local r = csv.parse("a,b\n\"line\n1\",\"x\"") return r[2][1]"#), "line\n1");
    }

    #[test]
    fn fields_are_bytes() {
        let ok: bool = eval(r#"local r = csv.parse("a\255,b") return r[1][1] == "a\255" and #r[1][1] == 2"#);
        assert!(ok, "an untyped field is not required to be UTF-8");
        let (kind, _) = refused(r#"csv.parse_records("a\255,b\n1,2")"#);
        assert_eq!(kind, "invalid_utf8", "a header is a key: it is text");
        let (kind, e) = refused(r#"csv.parse_records("n\n\255", { types = { n = "number" } })"#);
        assert_eq!(kind, "invalid_utf8");
        assert!(e.contains("line 2") && e.contains("column \"n\""), "got: {e}");
    }

    #[test]
    fn empty_text_has_no_rows() {
        let counts: Vec<i64> = eval(r#"return { #csv.parse(""), #csv.parse_records(""), #csv.parse_records("a,b\n") }"#);
        assert_eq!(counts, [0, 0, 0]);
    }

    #[test]
    fn rows_stream_traverses_all_rows() {
        let (rows, records, name): (i64, i64, String) = eval(
            r#"
            local rows, records, name = 0, 0, ""
            for row in csv.rows("a,b\n1,2\n3,4\n5,6"):items() do
                assert(#row == 2)
                rows += 1
            end
            for record in csv.records("a,b\n1,2\n3,4\n5,6", { types = { a = "number" } }):items() do
                assert(type(record.a) == "number" and type(record.b) == "string")
                records += 1
                name ..= record.b
            end
            return rows, records, name
            "#,
        );
        assert_eq!((rows, records, name.as_str()), (4, 3, "246"));
    }

    #[test]
    fn a_stream_ends_and_stays_ended() {
        let ok: bool = eval(
            r#"
            local stream = csv.rows("a\nb")
            local first, second = stream:next(), stream:next()
            local third, err = stream:next()
            local closed = csv.records("a\n1\n2")
            assert(closed:next().a == "1")
            closed:close()
            return first[1] == "a" and second[1] == "b" and third == nil and err == nil and stream:next() == nil
                and closed:next() == nil
            "#,
        );
        assert!(ok);
    }

    #[test]
    fn a_stream_fails_where_it_is_read() {
        let raised = try_eval::<i64>(r#"local n = 0; for _ in csv.records("a,b\n1,2\n3"):items() do n += 1 end; return n"#);
        let message = raised.unwrap_err().to_string();
        assert!(message.contains("lib:csv.records: unequal_lengths: line 3"), "got: {message}");

        let (seen, kind, text, after): (i64, String, String, bool) = eval(
            r#"
            local stream = csv.rows('a,b\n"x",y,extra\nc,d')
            local seen = 0
            while true do
                local row, err = stream:next()
                if err then
                    local more, again = stream:next()
                    return seen, err.kind, err.text, more == nil and again == nil
                end
                seen += 1
            end
            "#,
        );
        assert_eq!((seen, kind.as_str(), text.as_str()), (1, "unequal_lengths", r#""x",y,extra"#));
        assert!(after, "the stream is over after its error");

        // the header row is read with the first record
        let (kind, op): (String, String) = eval(
            r#"
            local stream = csv.records("a,a\n1,2")
            local _, err = stream:next()
            assert(stream:next() == nil)
            return err.kind, err.op
            "#,
        );
        assert_eq!((kind.as_str(), op.as_str()), ("duplicate_header", "lib:csv.records"));
    }

    #[test]
    fn stringify_writes_headers_and_rows() {
        let out: String = eval(
            r#"
            return (csv.stringify({
                { name = "Alice", age = "30" },
                { name = "Bob",   age = "25" },
            }, { "name", "age" }))
            "#,
        );
        assert_eq!(out, "name,age\nAlice,30\nBob,25\n");
    }

    #[test]
    fn stringify_missing_key_emits_empty_and_extra_keys_are_ignored() {
        let out: String = eval(r#"return (csv.stringify({ { name = "Alice", note = "x" }, {} }, { "name", "age" }))"#);
        assert_eq!(out, "name,age\nAlice,\n,\n");
    }

    #[test]
    fn stringify_takes_positional_rows_and_a_dialect() {
        let out: String = eval(r#"return (csv.stringify({ { "a b", "c\td" }, { x = "'q'" } }, { "x", "y" }, { delimiter = "\t", quote = "'" }))"#);
        assert_eq!(out, "x\ty\na b\t'c\td'\n'''q'''\t\n");
        let quoted: String = eval(r#"return (csv.stringify({ { "a,b", 'say "hi"', "two\nlines" } }, { "x", "y", "z" }))"#);
        assert_eq!(quoted, "x,y,z\n\"a,b\",\"say \"\"hi\"\"\",\"two\nlines\"\n");
    }

    #[test]
    fn stringify_requires_headers() {
        let (kind, e) = refused(r#"csv.stringify({}, {})"#);
        assert_eq!(kind, "no_headers");
        assert!(e.starts_with("lib:csv.stringify: no_headers:"), "got: {e}");
        assert_eq!(refused(r#"csv.writer({})"#).0, "no_headers");
        let missing = try_eval::<mlua::Value>(r#"return csv.stringify({})"#).unwrap_err().to_string();
        assert!(missing.contains("argument #2 'headers'"), "got: {missing}");
    }

    #[test]
    fn writer_accepts_array_and_keyed_rows() {
        let out: String = eval(
            r#"
            local w = assert(csv.writer({ "test", "status" }))
            assert(w:row({ "login", "PASS" }) == true)
            assert(w:row({ test = "signup", status = "FAIL" }))
            assert(w:row({ status = "SKIP" }))
            return (w:finish())
            "#,
        );
        assert_eq!(out, "test,status\nlogin,PASS\nsignup,FAIL\n,SKIP\n");
    }

    #[test]
    fn writer_writes_numbers_and_booleans() {
        let out: String = eval(
            r#"
            local w = csv.writer({ "n", "f", "big", "yes", "no" })
            w:row({ 30, 1.5, 1e15, true, false })
            w:row({ n = -7, f = 0.25 })
            return (w:finish())
            "#,
        );
        assert_eq!(out, "n,f,big,yes,no\n30,1.5,1000000000000000,true,false\n-7,0.25,,,\n");
    }

    #[test]
    fn writer_array_length_mismatch_errors() {
        let (kind, e) = refused(r#"csv.writer({ "a", "b", "c" }):row({ "x", "y" })"#);
        assert_eq!(kind, "wrong_length");
        assert!(e.contains("row has 2 values, expected 3"), "got: {e}");
        let (found, expected): (i64, i64) =
            eval(r#"local _, e = csv.stringify({ { 1, 2, 3 } }, { "a" }); return e.found, e.expected"#);
        assert_eq!((found, expected), (3, 1));
    }

    #[test]
    fn a_row_is_a_list_or_a_record_of_plain_values() {
        let e: String = eval(
            r#"
            local w = csv.writer({ "a", "b" })
            assert(not pcall(w.row, w, { "x", b = "y" }), "not both")
            assert(not pcall(w.row, w, { "x", {} }), "a table is not a value")
            assert(not pcall(w.row, w, { [1] = "x", [3] = "z" }), "no holes")
            assert(not pcall(w.row, w, "x,y"))
            local _, e = pcall(w.row, w, { a = print })
            assert(w:finish() == "a,b\n", "a refused row writes nothing")
            return tostring(e)
            "#,
        );
        assert!(e.contains("lib:csv.Writer:row: argument #1 'values'"), "got: {e}");
    }

    #[test]
    fn writer_finishes_once() {
        let (kinds, op): (Vec<String>, String) = eval(
            r#"
            local w = csv.writer({ "a" })
            assert(w:finish() == "a\n")
            local text, twice = w:finish()
            local ok, after = w:row({ "x" })
            assert(text == nil and ok == nil)
            return { twice.kind, after.kind }, after.op
            "#,
        );
        assert_eq!(kinds, ["finished", "finished"]);
        assert_eq!(op, "lib:csv.Writer:row");
    }

    #[test]
    fn round_trip_parse_stringify() {
        let out: String = eval(
            r#"
            local input = "name,city\nAlice,Sheffield\nBob,Leeds\n"
            local records = assert(csv.parse_records(input))
            local rows = assert(csv.parse(input))
            local headers = table.remove(rows, 1)
            assert(csv.stringify(rows, headers) == input)
            return (csv.stringify(records, { "name", "city" }))
            "#,
        );
        assert_eq!(out, "name,city\nAlice,Sheffield\nBob,Leeds\n");
    }
}
