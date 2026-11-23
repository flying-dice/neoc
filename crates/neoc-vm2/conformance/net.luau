-- std:net on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- It binds loopback sockets on ports the operating system picks. What an error is differs on purpose: see
-- net.vm2.luau.
local net = require("std:net")
local out = {}

local listener, err = net.TcpListener.bind("127.0.0.1:0")
local addr = listener:addr()
local port = tonumber(addr:match("^127%.0%.0%.1:(%d+)$"))
out.bind = { listener ~= nil, err == nil, port ~= nil and port > 0 and port < 65536, listener:addr() == addr }

local other = assert(net.TcpListener.bind("127.0.0.1:0"))
out.another_port = other:addr() ~= addr
out.v6 = (function()
	-- a machine without IPv6 has no `::1`: then there is nothing to compare
	local six = net.TcpListener.bind("[::1]:0")
	return six == nil or six:addr():match("^%[::1%]:%d+$") ~= nil
end)()

-- a failure is `(nil, err)`; what `err` is differs
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
out.wrong_arguments = { (pcall(net.TcpListener.bind)), (pcall(net.TcpListener.bind, {})) }

return out
