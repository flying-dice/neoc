--!strict
-- std:net as a script sees it: its answers (net.json), then what it asserts. It binds loopback sockets on ports the
-- operating system picks.
local net = require("std:net")
local out = {}

local bound, err = net.TcpListener.bind("127.0.0.1:0")
local listener = assert(bound)
local addr = assert(listener:addr())
local port = tonumber(addr:match("^127%.0%.0%.1:(%d+)$"))
out.bind = { (bound :: any) ~= nil, err == nil, port ~= nil and port > 0 and port < 65536, listener:addr() == addr }

local other = assert(net.TcpListener.bind("127.0.0.1:0"))
out.another_port = other:addr() ~= addr
out.v6 = (function()
  -- a machine without IPv6 has no `::1`: then there is nothing to compare
  local six = net.TcpListener.bind("[::1]:0")
  return six == nil or assert(six:addr()):match("^%[::1%]:%d+$") ~= nil
end)()

-- a failure is `(nil, err)`
local function failed(value: unknown, why: unknown): { boolean }
  return { value == nil, why ~= nil }
end
out.in_use = failed(net.TcpListener.bind(addr))
out.not_an_address = {
  failed(net.TcpListener.bind("no port")),
  failed(net.TcpListener.bind("")),
  failed(net.TcpListener.bind("127.0.0.1")),
  failed(net.TcpListener.bind("127.0.0.1:99999")),
  failed(net.TcpListener.bind("192.0.2.1:0")),
}
out.wrong_arguments = { (pcall(net.TcpListener.bind :: any)), (pcall(net.TcpListener.bind :: any, {})) }

-- what it asserts
do
  local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
  local addr, addr_err = listener:addr()
  assert(addr ~= nil and addr_err == nil)

  -- the address is taken
  local taken, in_use = net.TcpListener.bind(addr)
  assert(taken == nil and in_use ~= nil and in_use.kind == "addr_in_use" and in_use.addr == addr)
  assert(in_use.op == "std:net.TcpListener.bind" and in_use.message == addr .. ": " .. in_use.reason)
  assert(tostring(in_use) == "std:net.TcpListener.bind: addr_in_use: " .. in_use.message)

  -- text that is not an address
  for _, text in { "no port", "", "127.0.0.1", "127.0.0.1:99999" } do
    local value, invalid = net.TcpListener.bind(text)
    assert(value == nil and invalid ~= nil and invalid.kind == "invalid_input", text)
    assert(invalid.addr == text)
  end

  -- an address this machine does not have (192.0.2.0/24 is documentation space)
  local _, not_ours = net.TcpListener.bind("192.0.2.1:0")
  assert(not_ours ~= nil and not_ours.kind == "addr_not_available")

  -- a wrong argument is refused with its place
  local ok, refused = pcall(function()
    net.TcpListener.bind(8080 :: any) -- deliberate refusal test: a number is not an address
  end)
  assert(not ok and tostring(refused):find("std:net.TcpListener.bind: argument #1 'addr': expected a string", 1, true))
end

return out
