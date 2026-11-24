-- intentional: parameters are one list (db:execute(sql, { a, b })) and not trailing arguments, so a trailing nil no longer binds NULL (a nil before the last value does); a parameter that is not a boolean, number or string is refused as an argument; a failure is a typed error (OpenError invalid_url, connect; Error closed, invalid_parameter, constraint with its violation, database, connection, pool_timed_out, unsupported_column, decode, other), and a column of an unsupported type or a value that does not decode is a returned error (it was raised).
local sqlx_postgres = require("vnd:sqlx_postgres")

-- a URL the driver refuses before it connects
local db, err = sqlx_postgres.open("not-a-url")
assert(db == nil and err ~= nil and err.kind == "invalid_url")
assert(err.op == "vnd:sqlx_postgres.open")
assert(tostring(err) == "vnd:sqlx_postgres.open: invalid_url: " .. err.message)
if err.kind == "invalid_url" then
	assert(err.reason == err.message)
end

-- a URL that is not a string is refused as an argument
local ok, refused = pcall(function()
	return sqlx_postgres.open(5 :: any)
end)
assert(not ok and tostring(refused):find("vnd:sqlx_postgres.open: argument #1 'url'", 1, true))

-- what a script writes against a server: checked against the declarations, not run here
local function _usage(url: string)
	local pool = assert(sqlx_postgres.open(url))
	local changed: number = assert(pool:execute("INSERT INTO users (id, score, nick) VALUES ($1, $2, $3)", { 1, nil, "al" }))
	local rows = assert(pool:query("SELECT id, name FROM users WHERE id > $1", { 0 }))
	for _, row in rows do
		local id = row.id
		if type(id) == "number" then
			changed += id
		end
	end
	local row, query_err = pool:query_one("SELECT name FROM users WHERE id = $1", { 1 })
	if query_err ~= nil then
		if query_err.kind == "constraint" then
			assert(query_err.violation == "unique" or query_err.code == "23505")
		elseif query_err.kind == "invalid_parameter" then
			assert(query_err.position == 0)
		elseif query_err.kind == "unsupported_column" then
			assert(query_err.column ~= "" and query_err.type_name ~= "")
		elseif query_err.kind == "decode" then
			assert(query_err.column ~= "")
		end
	elseif row ~= nil then
		assert(row.name ~= nil)
	end
	pool:close()
	return changed
end
