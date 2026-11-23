-- intentional: `==` is a:equals(b) (operators are not supported), query pairs are { key, value } records (were { key, value } arrays), the port is a u16 and the percent-encode set a "component" | "path" | "query" | "fragment" union (wrong values are refused as arguments; encode cannot fail), resolve takes a Url or a string as a typed union, and every failure is a typed error table (invalid_url, cannot_resolve, invalid_scheme, invalid_host, cannot_set_port, invalid_utf8).
local url = require("lib:url")

local a = url.parse("https://example.com/a")
assert(a:equals((url.parse("https://example.com/a"))) and not a:equals((url.parse("https://example.com/b"))))

-- query pairs are records, in order, repeats included
local pairs_out = url.parse("https://example.com/?a=1&b=2&a=3"):query_pairs()
assert(#pairs_out == 3 and pairs_out[1].key == "a" and pairs_out[3].value == "3")
local q = url.encode_query({ { key = "name", value = "ada lovelace" }, { key = "role", value = "engineer/lead" } })
assert(q == "name=ada+lovelace&role=engineer%2Flead")
local decoded = url.decode_query(q)
assert(decoded[1].key == "name" and decoded[1].value == "ada lovelace" and decoded[2].value == "engineer/lead")
local ok, refused = pcall(url.encode_query, { { "a", "b" } })
assert(not ok and tostring(refused):find("argument #1 'pairs'", 1, true))

-- arguments are checked by their types
for _, port in { 99999, -1, 8080.5 } do
	local fine, why = pcall(a.with_port, a, port)
	assert(not fine and tostring(why):find("lib:url.Url:with_port: argument #1 'port'", 1, true))
end
local fine, why = pcall(url.encode, "x", "bogus")
assert(not fine and tostring(why):find("lib:url.encode: argument #2 'set'", 1, true))
local fine2, why2 = pcall(url.resolve, 1, "x")
assert(not fine2 and tostring(why2):find("argument #1 'base'", 1, true))

-- typed errors
local _, invalid = url.parse("not a url")
assert(invalid.kind == "invalid_url" and invalid.input == "not a url" and invalid.op == "lib:url.parse")
assert(tostring(invalid):find("lib:url.parse: invalid_url:", 1, true))
local _, unresolved = url.resolve("mailto:ada@example.com", "x")
assert(unresolved.kind == "cannot_resolve" and unresolved.reference == "x")
local _, scheme = a:with_scheme("not a scheme")
assert(scheme.kind == "invalid_scheme" and scheme.scheme == "not a scheme" and scheme.op == "lib:url.Url:with_scheme")
local _, host = a:with_host("exa mple.com")
assert(host.kind == "invalid_host" and host.host == "exa mple.com")
local _, port = url.parse("mailto:ada@example.com"):with_port(25)
assert(port.kind == "cannot_set_port")
local _, utf8 = url.decode("%FF%FE")
assert(utf8.kind == "invalid_utf8" and utf8.op == "lib:url.decode")
