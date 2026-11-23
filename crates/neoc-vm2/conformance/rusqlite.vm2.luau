-- intentional: parameters are one list (db:execute(sql, { a, b }), stmt:execute({ a, b })) and not trailing arguments; a position without a value binds NULL (so a short list is not an error) and a value past the placeholders is unknown_parameter; a parameter that is not a boolean, number or string is refused as an argument (it was a returned error); a failure is a typed error (OpenError cannot_open; Error closed, multiple_statements, empty_sql, unknown_parameter, constraint, busy, sql, other) and a statement's op is vnd:rusqlite.Statement:execute.
local rusqlite = require("vnd:rusqlite")

local db = assert(rusqlite.open(":memory:"))
assert(db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT, n)") == 0)

-- parameters are a list
assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", { "a", 1 }) == 1)
assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", { "b", 2.5 }) == 1)
assert(db:execute("UPDATE t SET n = ? WHERE v = ?", { 10, "a" }) == 1)
local rows = assert(db:query("SELECT id, v, n FROM t WHERE n > ? ORDER BY id", { 2 }))
assert(#rows == 2 and rows[1].v == "a" and rows[1].n == 10 and rows[2].n == 2.5)
local row = assert(db:query_one("SELECT v FROM t WHERE id = ?", { 2 }))
assert(row.v == "b")
assert(db:query_one("SELECT v FROM t WHERE id = ?", { 99 }) == nil)

-- how a value binds
local kinds = assert(db:query_one(
	"SELECT typeof(?) AS whole, typeof(?) AS real, typeof(?) AS text, typeof(?) AS blob, typeof(?) AS flag, ? AS yes",
	{ 7, 7.5, "text", "\0\255", false, true }
))
assert(kinds.whole == "integer" and kinds.real == "real" and kinds.text == "text" and kinds.blob == "blob")
assert(kinds.flag == "integer" and kinds.yes == 1)
local echoed = assert(db:query_one("SELECT ? AS b", { "\0\1\2\254\255" }))
assert(echoed.b == "\0\1\2\254\255")

-- a position without a value binds NULL: a nil in the list, a list that ends early, no list
local maybe: string? = nil
assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", { "c", maybe }) == 1)
assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)", { nil, 4 }) == 1)
assert(db:execute("INSERT INTO t (v, n) VALUES (?, ?)") == 1)
local nulls = assert(db:query("SELECT v, n FROM t WHERE id > 2 ORDER BY id"))
assert(nulls[1].v == "c" and nulls[1].n == nil)
assert(nulls[2].v == nil and nulls[2].n == 4)
assert(next(nulls[3]) == nil)

-- a value the statement has no placeholder for
local n, unknown = db:execute("INSERT INTO t (v) VALUES (?)", { "a", "b" })
assert(n == nil and unknown ~= nil and unknown.kind == "unknown_parameter")
assert(unknown.position == 2 and unknown.count == 1)
assert(unknown.op == "vnd:rusqlite.Database:execute")
assert(tostring(unknown) == "vnd:rusqlite.Database:execute: unknown_parameter: a value was given for parameter 2, the statement has 1")

-- what is not a parameter is refused as an argument
for _, params in { { {} }, { print }, "a", 5 } :: { any } do
	local ok, refused = pcall(function()
		return db:execute("SELECT ?", params)
	end)
	assert(not ok and tostring(refused):find("vnd:rusqlite.Database:execute: argument #2 'params'", 1, true))
end
local ok_sql, refused_sql = pcall(function()
	return db:query(5 :: any)
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
assert(db:execute("INSERT INTO u VALUES (?)", { "k" }) == 1)
local _, constraint = db:execute("INSERT INTO u VALUES (?)", { "k" })
assert(constraint ~= nil and constraint.kind == "constraint" and constraint.code == 2067)
assert(constraint.message == "UNIQUE constraint failed: u.v")

local _, returned_rows = db:execute("SELECT 1")
assert(returned_rows ~= nil and returned_rows.kind == "other")

-- a prepared statement takes the list alone, and names itself in its errors
local insert = db:prepare("INSERT INTO u VALUES (?)")
assert(insert:execute({ "x" }) == 1)
assert(insert:execute({ "y" }) == 1)
local _, again = insert:execute({ "x" })
assert(again ~= nil and again.kind == "constraint" and again.op == "vnd:rusqlite.Statement:execute")
local select = db:prepare("SELECT v FROM u WHERE v >= ? ORDER BY v")
assert(#assert(select:query({ "x" })) == 2)
assert(assert(select:query_one({ "x" })).v == "x")

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
local _, statement_closed = select:query({ "x" })
assert(statement_closed ~= nil and statement_closed.kind == "closed" and statement_closed.op == "vnd:rusqlite.Statement:query")
