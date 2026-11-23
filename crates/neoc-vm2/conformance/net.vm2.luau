-- intentional: a failure to bind is a typed NetError whose kind is the operating system's (addr_in_use, addr_not_available, permission_denied, invalid_input, other) with the address, and addr() of a listener the engine has closed returns a typed closed error (it raised).
local net = require("std:net")

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
	assert(invalid.kind ~= "closed" and invalid.addr == text)
end

-- an address this machine does not have (192.0.2.0/24 is documentation space)
local _, not_ours = net.TcpListener.bind("192.0.2.1:0")
assert(not_ours ~= nil and not_ours.kind == "addr_not_available")

-- a wrong argument is refused with its place
local ok, refused = pcall(net.TcpListener.bind, 8080 :: any)
assert(not ok and tostring(refused):find("std:net.TcpListener.bind: argument #1 'addr': expected a string", 1, true))
