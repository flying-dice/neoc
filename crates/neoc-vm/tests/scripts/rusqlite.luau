--!strict
-- vnd:rusqlite as a script sees it: its answers (rusqlite.json), then what it asserts.
local rusqlite = require("vnd:rusqlite")
local out = {}

local opened, open_err = rusqlite.open(":memory:")
out.opened = (opened :: any) ~= nil and open_err == nil
local db = assert(opened)

-- rows affected
out.created = db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT, r REAL, b BLOB, n TEXT)")
out.inserted = db:execute("INSERT INTO t (v, r, b, n) VALUES ('a', 3.25, X'0001FEFF', NULL)")
db:execute("INSERT INTO t (v, r) VALUES ('b', 2), ('a', -0.5)")
out.updated = db:execute("UPDATE t SET v = 'x' WHERE v = 'a'")
out.deleted_none = db:execute("DELETE FROM t WHERE id = 99")

-- rows, in order, column by column
local queried, query_err = db:query("SELECT id, v, r FROM t ORDER BY id")
out.query_ok = query_err == nil
local rows = assert(queried)
out.rows = {}
for i, row in rows do
  out.rows[i] = { row.id, row.v, row.r }
end
out.empty = #assert(db:query("SELECT * FROM t WHERE id > 99"))

-- the type of each value
local row = assert(
  db:query_one("SELECT id, v, r, b, n, typeof(b) AS tb, 1.5 AS real, 7 AS whole, 'txt' AS text FROM t WHERE id = 1")
)
out.types = { type(row.id), type(row.v), type(row.r), type(row.b), type(row.n), row.tb } :: { any }
local blob = row.b :: string
out.blob = { #blob, string.byte(blob, 1, -1) }
out.literals = { row.real, row.whole, row.text }
out.null_is_absent = row.n == nil

-- query_one: the first row, or nil without an error
out.first = assert(db:query_one("SELECT v FROM t ORDER BY id DESC")).v
local none, none_err = db:query_one("SELECT * FROM t WHERE id = 99")
out.none = { none == nil, none_err == nil }

-- a prepared statement shares the connection
local count = assert(db:prepare("SELECT count(*) AS n FROM t"))
out.prepared = { assert(count:query_one()).n, #assert(count:query()) }
out.prepared_execute = db:prepare("DELETE FROM t WHERE v = 'b'"):execute()
out.after_delete = assert(count:query_one()).n

-- parameters: the arguments after the SQL, in order; nil binds NULL, the last one too
db:execute("CREATE TABLE p (a, b, c)")
out.param_inserted = {
  db:execute("INSERT INTO p VALUES (?, ?, ?)", 1, "one", 1.5),
  db:execute("INSERT INTO p VALUES (?, ?, ?)", 2, nil, nil),
  db:execute("INSERT INTO p VALUES (?, ?, ?)", nil, "three", nil),
  db:prepare("INSERT INTO p VALUES (?, ?, ?)"):execute(4, "\0\255", true),
}
out.params = {}
for i, r in assert(db:query("SELECT a, b, c, typeof(a) AS ta, typeof(b) AS tb, typeof(c) AS tc FROM p ORDER BY rowid")) do
  out.params[i] = { r.a == nil, r.b == nil, r.c == nil, r.ta, r.tb, r.tc } :: { any }
end
out.param_query = {
  #assert(db:query("SELECT a FROM p WHERE a >= ? AND a < ?", 2, 5)),
  assert(db:query_one("SELECT b FROM p WHERE a = ?", 1)).b,
} :: { any }
out.param_null = assert(db:query_one("SELECT ? IS NULL AS yes, ? AS two", nil, 2)).yes
out.param_prepared = {
  assert(db:prepare("SELECT b FROM p WHERE a = ?"):query_one(1)).b,
  #assert(db:prepare("SELECT a FROM p WHERE b IS ?"):query(nil)),
}

-- failures: nil and an error
local function fails(value: unknown, err: unknown): boolean
  return value == nil and err ~= nil
end
out.bad_sql =
  { fails(db:execute("NOT VALID SQL")), fails(db:query("NOT VALID SQL")), fails(db:query_one("NOT VALID SQL")) }
out.multiple = fails(db:execute("CREATE TABLE a (x); CREATE TABLE b (x)"))
out.multiple_ran_nothing = #assert(db:query("SELECT name FROM sqlite_master WHERE name IN ('a', 'b')"))
out.empty_sql = { fails(db:execute("")), fails(db:execute("-- just a comment")) }
out.constraint = {
  db:execute("CREATE TABLE u (v TEXT UNIQUE)"),
  db:execute("INSERT INTO u VALUES ('k')"),
  fails(db:execute("INSERT INTO u VALUES ('k')")),
} :: { any }
out.select_in_execute = fails(db:execute("SELECT 1"))
out.bad_prepared = fails(db:prepare("NOT VALID SQL"):execute())
out.wrong_count = {
  fails(db:execute("INSERT INTO p VALUES (?, ?, ?)", 1)),
  fails(db:query("SELECT ?", 1, 2)),
  fails(db:query_one("SELECT ?")),
  fails(db:prepare("SELECT ?"):query(1, nil)),
}

-- close: idempotent, and everything after it fails
db:close()
db:close()
out.closed =
  { fails(db:execute("SELECT 1")), fails(db:query("SELECT 1")), fails(db:query_one("SELECT 1")), fails(count:query()) }

-- what it asserts
do
  local db = assert(rusqlite.open(":memory:"))
  assert(db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT, n)") == 0)

  -- parameters are the arguments after the SQL
  assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", "a", 1) == 1)
  assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", "b", 2.5) == 1)
  assert(db:execute("UPDATE t SET n = ? WHERE v = ?", 10, "a") == 1)
  local rows = assert(db:query("SELECT id, v, n FROM t WHERE n > ? ORDER BY id", 2))
  assert(#rows == 2 and rows[1].v == "a" and rows[1].n == 10 and rows[2].n == 2.5)
  local row = assert(db:query_one("SELECT v FROM t WHERE id = ?", 2))
  assert(row.v == "b")
  assert(db:query_one("SELECT v FROM t WHERE id = ?", 99) == nil)

  -- how a value binds
  local kinds = assert(
    db:query_one(
      "SELECT typeof(?) AS whole, typeof(?) AS real, typeof(?) AS text, typeof(?) AS blob, typeof(?) AS flag, ? AS yes",
      7,
      7.5,
      "text",
      "\0\255",
      false,
      true
    )
  )
  assert(kinds.whole == "integer" and kinds.real == "real" and kinds.text == "text" and kinds.blob == "blob")
  assert(kinds.flag == "integer" and kinds.yes == 1)
  local echoed = assert(db:query_one("SELECT ? AS b", "\0\1\2\254\255"))
  assert(echoed.b == "\0\1\2\254\255")

  -- nil binds NULL: a value that may be nil, a nil first, a nil last
  local maybe: string? = nil
  assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", "c", maybe) == 1)
  assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", nil, 4) == 1)
  assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", nil, nil) == 1)
  local nulls = assert(db:query("SELECT v, n FROM t WHERE id > 2 ORDER BY id"))
  assert(nulls[1].v == "c" and nulls[1].n == nil)
  assert(nulls[2].v == nil and nulls[2].n == 4)
  assert(next(nulls[3]) == nil)

  -- more or fewer values than the statement has placeholders
  local n, count = db:execute("INSERT INTO t (v) VALUES (?)", "a", "b")
  assert(n == nil and count ~= nil and count.kind == "parameter_count")
  if count.kind == "parameter_count" then
    assert(count.given == 2 and count.expected == 1)
  end
  assert(count.op == "vnd:rusqlite.Database:execute")
  assert(
    tostring(count)
      == "vnd:rusqlite.Database:execute: parameter_count: 2 values were given, the statement has 1 placeholders"
  )
  local _, few = db:query("SELECT ?, ?", 1)
  assert(few ~= nil and few.kind == "parameter_count" and few.given == 1 and few.expected == 2)
  local _, trailing = db:query_one("SELECT ?", 1, nil)
  assert(trailing ~= nil and trailing.kind == "parameter_count" and trailing.given == 2)

  -- what is not a parameter is refused as an argument
  for _, param in { {}, print } :: { any } do
    local ok, refused = pcall(function()
      db:execute("SELECT ?, ?", 1, param)
    end)
    assert(not ok and tostring(refused):find("vnd:rusqlite.Database:execute: argument #3 'params'", 1, true))
  end
  local ok_sql, refused_sql = pcall(function()
    db:query(5 :: any)
  end)
  assert(not ok_sql and tostring(refused_sql):find("vnd:rusqlite.Database:query: argument #1 'sql'", 1, true))

  -- typed errors
  local _, syntax = db:execute("NOT VALID SQL")
  assert(syntax ~= nil and syntax.kind == "sql" and syntax.code == 1)
  assert(tostring(syntax):find("vnd:rusqlite.Database:execute: sql: ", 1, true) == 1)
  local _, multiple = db:query("SELECT 1 AS a; SELECT 2 AS a")
  assert(multiple ~= nil and multiple.kind == "multiple_statements" and multiple.op == "vnd:rusqlite.Database:query")
  local _, empty = db:query_one("-- just a comment")
  assert(empty ~= nil and empty.kind == "empty_sql")
  assert(tostring(empty) == "vnd:rusqlite.Database:query_one: empty_sql: SQL is empty or comment only")

  assert(db:execute("CREATE TABLE u (v TEXT UNIQUE)"))
  assert(db:execute("INSERT INTO u VALUES (?)", "k") == 1)
  local _, constraint = db:execute("INSERT INTO u VALUES (?)", "k")
  assert(constraint ~= nil and constraint.kind == "constraint" and constraint.code == 2067)
  assert(constraint.message == "UNIQUE constraint failed: u.v")

  local _, returned_rows = db:execute("SELECT 1")
  assert(returned_rows ~= nil and returned_rows.kind == "other")

  -- a prepared statement takes the values alone, and names itself in its errors
  local insert = db:prepare("INSERT INTO u VALUES (?)")
  assert(insert:execute("x") == 1)
  assert(insert:execute("y") == 1)
  assert(insert:execute(nil) == 1)
  local _, again = insert:execute("x")
  assert(again ~= nil and again.kind == "constraint" and again.op == "vnd:rusqlite.Statement:execute")
  local _, none_given = insert:execute()
  assert(none_given ~= nil and none_given.kind == "parameter_count" and none_given.given == 0)
  local select = db:prepare("SELECT v FROM u WHERE v >= ? ORDER BY v")
  assert(#assert(select:query("x")) == 2)
  assert(assert(select:query_one("x")).v == "x")

  -- a path that cannot be opened
  local none, cannot_open = rusqlite.open("no-such-directory-for-rusqlite/x.db")
  assert(none == nil and cannot_open ~= nil and cannot_open.kind == "cannot_open")
  assert(cannot_open.path == "no-such-directory-for-rusqlite/x.db" and cannot_open.op == "vnd:rusqlite.open")

  -- closed
  db:close()
  db:close()
  local _, closed = db:execute("SELECT 1")
  assert(closed ~= nil and closed.kind == "closed")
  assert(tostring(closed) == "vnd:rusqlite.Database:execute: closed: database is closed")
  local _, statement_closed = select:query("x")
  assert(
    statement_closed ~= nil
      and statement_closed.kind == "closed"
      and statement_closed.op == "vnd:rusqlite.Statement:query"
  )
end

return out
