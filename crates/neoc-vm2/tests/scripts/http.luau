--!strict
-- lib:http as a script sees it: its answers (http.json), then what it asserts. `serve_static` blocks while its server
-- runs, so nothing is served here (the server is tested over a socket in Rust).
local http = require("lib:http")
local out = {}

out.surface = type(http.serve_static)
-- the pattern is an argument, and it is a string
out.wrong_arguments =
  { (pcall(http.serve_static :: any)), (pcall(http.serve_static :: any, {})), (pcall(http.serve_static :: any, true)) }

-- what it asserts
do
  local net = require("std:net")
  local tempfile = require("lib:tempfile")

  local dir = assert(tempfile.tempdir())
  local root = (dir:path():gsub("\\", "/"))

  -- a pattern that is not a glob
  local served, invalid = http.serve_static(root .. "/a[")
  assert(served == nil and invalid ~= nil and invalid.kind == "invalid_glob")
  assert(invalid.pattern == root .. "/a[" and invalid.op == "lib:http.serve_static")
  assert(invalid.message == invalid.pattern .. ": " .. invalid.reason)
  assert(tostring(invalid) == "lib:http.serve_static: invalid_glob: " .. invalid.message)

  -- a pattern whose fixed prefix is not a directory
  local _, walk = http.serve_static(root .. "/nope/**/*.png")
  assert(walk ~= nil and walk.kind == "walk" and walk.path ~= nil and walk.message == walk.reason)

  -- an address that is taken: nothing is served, and the call does not block
  local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
  local addr = assert(listener:addr())
  local port = assert(tonumber(addr:match(":(%d+)$")))
  local _, bind = http.serve_static(root .. "/**/*.png", { host = "127.0.0.1", port = port })
  assert(bind ~= nil and bind.kind == "bind" and bind.cause == "addr_in_use" and bind.addr == addr)
  assert(bind.message == "bind " .. addr .. ": " .. bind.reason)

  -- an option that is misspelt or of the wrong type is refused with its place, not ignored
  local function refused(opts: any)
    local ok, why = pcall(http.serve_static :: any, root .. "/**/*.png", opts)
    assert(not ok and tostring(why):find("lib:http.serve_static: argument #2 'opts'", 1, true), tostring(why))
  end
  refused { prot = port }
  refused { port = "8080" }
  refused { port = 70000 }
  refused { host = 127 }
  local ok, why = pcall(http.serve_static :: any, 8080)
  assert(not ok and tostring(why):find("lib:http.serve_static: argument #1 'glob_pattern'", 1, true))

  dir:close()
end

return out
