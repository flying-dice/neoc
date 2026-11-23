-- lib:url on neoc-vm and neoc-vm2: what both must answer the same.
local url = require("lib:url")
local out = {}

local u = url.parse("https://ada:secret@example.com:8443/a/b?x=1&y=2#frag")
out.fields = {
	u.scheme,
	u.username,
	u.password,
	u.host,
	u.port,
	u.path,
	u.query,
	u.fragment,
	u.origin,
	u.href,
	tostring(u),
}
local plain = url.parse("https://example.com/")
out.absent = { plain.port == nil, plain.password == nil, plain.query == nil, plain.fragment == nil, plain.username }
out.static_parse = url.Url.parse("https://example.com/a%20b?x=1#frag").href

local bad, bad_err = url.parse("not a url")
out.invalid = { bad == nil, bad_err ~= nil }
local static_bad, static_bad_err = url.Url.parse("not a url")
out.static_invalid = { static_bad == nil, static_bad_err ~= nil }

local old = url.parse("https://example.com/old")
out.with = {
	tostring(old:with_path("/new"):with_query("a=1"):with_fragment("here")),
	tostring(old),
	tostring(url.parse("https://example.com/?a=1#f"):with_query(nil):with_fragment(nil)),
	tostring(url.parse("https://example.com/"):with_port(8443)),
	tostring(url.parse("https://example.com:9999/"):with_port(nil)),
	tostring(url.parse("https://example.com/"):with_host("other.org")),
	tostring(url.parse("http://example.com/"):with_scheme("https")),
}
local no_scheme, no_scheme_err = url.parse("https://example.com/"):with_scheme("not a scheme")
local no_host, no_host_err = url.parse("https://example.com/"):with_host("exa mple.com")
out.with_failures = { no_scheme == nil, no_scheme_err ~= nil, no_host == nil, no_host_err ~= nil }

local base = url.parse("https://example.com/dir/page")
out.resolve = {
	tostring(base:resolve("other")),
	tostring(url.resolve(base, "/abs")),
	tostring(url.resolve("https://example.com/dir/page", "?q=1")),
	tostring(url.resolve("https://example.com/dir/page", "../up")),
}

local with_repeats = url.parse("https://example.com/?a=1&b=2&a=3")
local map = with_repeats:query_map()
out.query_map = { map.a, map.b }

out.encode = {
	url.encode("hello world!/?"),
	url.encode("a-b_c.d~e"),
	url.encode("a b\"c#d", "query"),
	url.encode("a b?c{d}", "path"),
	url.encode("a b<c>d", "fragment"),
}
out.decode = { (url.decode("hello%20world%21")), (url.decode("a+b")) }
local decoded, decode_err = url.decode("%FF%FE")
out.decode_invalid = { decoded == nil, decode_err ~= nil }

out.origins = {
	url.parse("blob:https://example.com:8443/uuid").origin,
	url.parse("blob:blob:https://example.com/uuid").origin,
	url.parse("data:text/plain,hi").origin,
	url.parse("ws://example.com/").origin,
}
return out
