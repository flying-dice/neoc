--!strict
-- intentional: a parameter that is not a boolean, number, string or nil is refused as an argument; a failure is a typed error (OpenError invalid_url, connect; Error closed, constraint with its violation, database, connection, pool_timed_out, decode, other), a value that does not decode is the returned decode error (it was raised), and a statement's op is vnd:sqlx_mysql.Statement:execute; a wrong number of values is the typed error parameter_count (given, expected), checked before the statement is run
local sqlx_mysql = require("vnd:sqlx_mysql")

-- a URL the driver refuses before it connects
local db, err = sqlx_mysql.open("not-a-url")
assert(db == nil and err ~= nil and err.kind == "invalid_url")
assert(err.op == "vnd:sqlx_mysql.open")
assert(tostring(err) == "vnd:sqlx_mysql.open: invalid_url: " .. err.message)
if err.kind == "invalid_url" then
	assert(err.reason == err.message)
end

-- a URL that is not a string is refused as an argument
local ok, refused = pcall(function()
	sqlx_mysql.open(5 :: any)
end)
assert(not ok and tostring(refused):find("vnd:sqlx_mysql.open: argument #1 'url'", 1, true))

-- what a script writes against a server: checked against the declarations, not run here
local function _usage(url: string)
	local pool = assert(sqlx_mysql.open(url))
	local changed: number = assert(pool:execute("INSERT INTO users (id, name, nick) VALUES (?, ?, ?)", 1, nil, "al"))
	assert(pool:execute("UPDATE users SET nick = ? WHERE id = ? AND name <=> ?", "al", 1, nil))
	local rows = assert(pool:query("SELECT id, name FROM users WHERE id > ?", 0))
	for _, row in rows do
		local id = row.id
		if type(id) == "number" then
			changed += id
		end
	end
	local row, query_err = pool:query_one("SELECT name FROM users WHERE id = ?", 1)
	if query_err ~= nil then
		if query_err.kind == "constraint" then
			assert(query_err.violation == "unique" or query_err.code == "23000")
		elseif query_err.kind == "decode" then
			assert(query_err.column ~= "")
		end
	elseif row ~= nil then
		assert(row.name ~= nil)
	end
	local insert = pool:prepare("INSERT INTO users (id, name) VALUES (?, ?)")
	local _, stmt_err = insert:execute(2, "bob")
	assert(stmt_err == nil or stmt_err.kind ~= "closed")
	assert(insert:query(2))
	insert:query_one()
	pool:close()
	return changed
end
