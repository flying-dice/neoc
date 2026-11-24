-- vnd:rusqlite on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- The errors differ on purpose: see rusqlite.vm2.luau.
local rusqlite = require("vnd:rusqlite")
local out = {}

local db, open_err = rusqlite.open(":memory:")
out.opened = db ~= nil and open_err == nil

-- rows affected
out.created = db:execute("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT, r REAL, b BLOB, n TEXT)")
out.inserted = db:execute("INSERT INTO t (v, r, b, n) VALUES ('a', 3.25, X'0001FEFF', NULL)")
db:execute("INSERT INTO t (v, r) VALUES ('b', 2), ('a', -0.5)")
out.updated = db:execute("UPDATE t SET v = 'x' WHERE v = 'a'")
out.deleted_none = db:execute("DELETE FROM t WHERE id = 99")

-- rows, in order, column by column
local rows, query_err = db:query("SELECT id, v, r FROM t ORDER BY id")
out.query_ok = query_err == nil
out.rows = {}
for i, row in rows do
	out.rows[i] = { row.id, row.v, row.r }
end
out.empty = #db:query("SELECT * FROM t WHERE id > 99")

-- the type of each value
local row = db:query_one("SELECT id, v, r, b, n, typeof(b) AS tb, 1.5 AS real, 7 AS whole, 'txt' AS text FROM t WHERE id = 1")
out.types = { type(row.id), type(row.v), type(row.r), type(row.b), type(row.n), row.tb }
out.blob = { #row.b, string.byte(row.b, 1, -1) }
out.literals = { row.real, row.whole, row.text }
out.null_is_absent = row.n == nil

-- query_one: the first row, or nil without an error
out.first = db:query_one("SELECT v FROM t ORDER BY id DESC").v
local none, none_err = db:query_one("SELECT * FROM t WHERE id = 99")
out.none = { none == nil, none_err == nil }

-- a prepared statement shares the connection
local count = db:prepare("SELECT count(*) AS n FROM t")
out.prepared = { count:query_one().n, #count:query() }
out.prepared_execute = db:prepare("DELETE FROM t WHERE v = 'b'"):execute()
out.after_delete = count:query_one().n

-- parameters: the arguments after the SQL, in order; nil binds NULL, the last one too
db:execute("CREATE TABLE p (a, b, c)")
out.param_inserted = {
	db:execute("INSERT INTO p VALUES (?, ?, ?)", 1, "one", 1.5),
	db:execute("INSERT INTO p VALUES (?, ?, ?)", 2, nil, nil),
	db:execute("INSERT INTO p VALUES (?, ?, ?)", nil, "three", nil),
	db:prepare("INSERT INTO p VALUES (?, ?, ?)"):execute(4, "\0\255", true),
}
out.params = {}
for i, r in db:query("SELECT a, b, c, typeof(a) AS ta, typeof(b) AS tb, typeof(c) AS tc FROM p ORDER BY rowid") do
	out.params[i] = { r.a == nil, r.b == nil, r.c == nil, r.ta, r.tb, r.tc }
end
out.param_query = { #db:query("SELECT a FROM p WHERE a >= ? AND a < ?", 2, 5), db:query_one("SELECT b FROM p WHERE a = ?", 1).b }
out.param_null = db:query_one("SELECT ? IS NULL AS yes, ? AS two", nil, 2).yes
out.param_prepared = { db:prepare("SELECT b FROM p WHERE a = ?"):query_one(1).b, #db:prepare("SELECT a FROM p WHERE b IS ?"):query(nil) }

-- failures: nil and an error
local function fails(value, err)
	return value == nil and err ~= nil
end
out.bad_sql = { fails(db:execute("NOT VALID SQL")), fails(db:query("NOT VALID SQL")), fails(db:query_one("NOT VALID SQL")) }
out.multiple = fails(db:execute("CREATE TABLE a (x); CREATE TABLE b (x)"))
out.multiple_ran_nothing = #db:query("SELECT name FROM sqlite_master WHERE name IN ('a', 'b')")
out.empty_sql = { fails(db:execute("")), fails(db:execute("-- just a comment")) }
out.constraint = { db:execute("CREATE TABLE u (v TEXT UNIQUE)"), db:execute("INSERT INTO u VALUES ('k')"), fails(db:execute("INSERT INTO u VALUES ('k')")) }
out.select_in_execute = fails(db:execute("SELECT 1"))
out.bad_prepared = fails(db:prepare("NOT VALID SQL"):execute())
out.wrong_count = { fails(db:execute("INSERT INTO p VALUES (?, ?, ?)", 1)), fails(db:query("SELECT ?", 1, 2)), fails(db:query_one("SELECT ?")), fails(db:prepare("SELECT ?"):query(1, nil)) }

-- close: idempotent, and everything after it fails
db:close()
db:close()
out.closed = { fails(db:execute("SELECT 1")), fails(db:query("SELECT 1")), fails(db:query_one("SELECT 1")), fails(count:query()) }

return out
