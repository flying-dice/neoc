-- lib:http on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- `serve_static` blocks while its server runs, so nothing is served here (the server is tested over a socket in
-- Rust). How a server that cannot start says so differs on purpose: see http.vm2.luau.
local http = require("lib:http")
local out = {}

out.surface = type(http.serve_static)
-- the pattern is an argument, and it is a string
out.wrong_arguments = { (pcall(http.serve_static)), (pcall(http.serve_static, {})), (pcall(http.serve_static, true)) }

return out
