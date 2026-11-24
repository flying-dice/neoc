--!strict
-- vnd:sqlx_postgres as a script sees it: its answers (sqlx_postgres.json), then what it asserts. Only what needs no
-- Postgres server: a URL the driver refuses before it connects.
local sqlx_postgres = require("vnd:sqlx_postgres")
local out = {}

local function fails(value, err)
  return value == nil and err ~= nil
end

out.not_a_url = fails(sqlx_postgres.open("not-a-url"))
out.empty_url = fails(sqlx_postgres.open(""))
out.bad_port = fails(sqlx_postgres.open("postgres://postgres@127.0.0.1:port/test"))

-- what it asserts
do
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
    sqlx_postgres.open(5 :: any)
  end)
  assert(not ok and tostring(refused):find("vnd:sqlx_postgres.open: argument #1 'url'", 1, true))

  -- what a script writes against a server: checked against the declarations, not run here
  local function _usage(url: string)
    local pool = assert(sqlx_postgres.open(url))
    local changed: number =
      assert(pool:execute("INSERT INTO users (id, score, nick) VALUES ($1, $2, $3)", 1, nil, "al"))
    assert(pool:execute("UPDATE users SET nick = $1 WHERE id = $2 AND score IS NOT DISTINCT FROM $3", "al", 1, nil))
    local rows = assert(pool:query("SELECT id, name FROM users WHERE id > $1", 0))
    for _, row in rows do
      local id = row.id
      if type(id) == "number" then
        changed += id
      end
    end
    local row, query_err = pool:query_one("SELECT name FROM users WHERE id = $1", 1)
    if query_err ~= nil then
      if query_err.kind == "constraint" then
        assert(query_err.violation == "unique" or query_err.code == "23505")
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
end

return out
