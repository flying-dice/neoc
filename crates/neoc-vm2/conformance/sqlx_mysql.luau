-- vnd:sqlx_mysql on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- Only what needs no MySQL server: a URL the driver refuses before it connects. Queries are tested against a real
-- server by the module's ignored Rust test; parameters and the errors differ on purpose: see sqlx_mysql.vm2.luau.
local sqlx_mysql = require("vnd:sqlx_mysql")
local out = {}

local function fails(value, err)
	return value == nil and err ~= nil
end

out.not_a_url = fails(sqlx_mysql.open("not-a-url"))
out.empty_url = fails(sqlx_mysql.open(""))
out.bad_port = fails(sqlx_mysql.open("mysql://root@127.0.0.1:port/test"))

return out
