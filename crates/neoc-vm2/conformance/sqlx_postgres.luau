-- vnd:sqlx_postgres on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Only what needs no Postgres server: a URL the driver refuses before it connects. Queries are tested against a
-- real server by the module's ignored Rust test; parameters and the errors differ on purpose: see
-- sqlx_postgres.vm2.luau.
local sqlx_postgres = require("vnd:sqlx_postgres")
local out = {}

local function fails(value, err)
	return value == nil and err ~= nil
end

out.not_a_url = fails(sqlx_postgres.open("not-a-url"))
out.empty_url = fails(sqlx_postgres.open(""))
out.bad_port = fails(sqlx_postgres.open("postgres://postgres@127.0.0.1:port/test"))

return out
