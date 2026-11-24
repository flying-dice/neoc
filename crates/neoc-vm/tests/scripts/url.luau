--!strict
-- lib:url as a script sees it: its answers (url.json), then what it asserts.
local url = require("lib:url")
local out = {}

local u = assert(url.parse("https://ada:secret@example.com:8443/a/b?x=1&y=2#frag"))
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
} :: { any }
local plain = assert(url.parse("https://example.com/"))
out.absent =
  { plain.port == nil, plain.password == nil, plain.query == nil, plain.fragment == nil, plain.username } :: { any }
out.static_parse = assert(url.Url.parse("https://example.com/a%20b?x=1#frag")).href

local bad, bad_err = url.parse("not a url")
out.invalid = { bad == nil, bad_err ~= nil }
local static_bad, static_bad_err = url.Url.parse("not a url")
out.static_invalid = { static_bad == nil, static_bad_err ~= nil }

local old = assert(url.parse("https://example.com/old"))
out.with = {
  tostring(old:with_path("/new"):with_query("a=1"):with_fragment("here")),
  tostring(old),
  tostring(assert(url.parse("https://example.com/?a=1#f")):with_query(nil):with_fragment(nil)),
  tostring((assert(url.parse("https://example.com/")):with_port(8443))),
  tostring((assert(url.parse("https://example.com:9999/")):with_port(nil))),
  tostring((assert(url.parse("https://example.com/")):with_host("other.org"))),
  tostring((assert(url.parse("http://example.com/")):with_scheme("https"))),
}
local no_scheme, no_scheme_err = assert(url.parse("https://example.com/")):with_scheme("not a scheme")
local no_host, no_host_err = assert(url.parse("https://example.com/")):with_host("exa mple.com")
out.with_failures = { no_scheme == nil, no_scheme_err ~= nil, no_host == nil, no_host_err ~= nil }

local base = assert(url.parse("https://example.com/dir/page"))
out.resolve = {
  tostring((base:resolve("other"))),
  tostring((url.resolve(base, "/abs"))),
  tostring((url.resolve("https://example.com/dir/page", "?q=1"))),
  tostring((url.resolve("https://example.com/dir/page", "../up"))),
}

local with_repeats = assert(url.parse("https://example.com/?a=1&b=2&a=3"))
local map = with_repeats:query_map()
out.query_map = { map.a, map.b }

out.encode = {
  url.encode("hello world!/?"),
  url.encode("a-b_c.d~e"),
  url.encode('a b"c#d', "query"),
  url.encode("a b?c{d}", "path"),
  url.encode("a b<c>d", "fragment"),
}
out.decode = { (url.decode("hello%20world%21")), (url.decode("a+b")) }
local decoded, decode_err = url.decode("%FF%FE")
out.decode_invalid = { decoded == nil, decode_err ~= nil }

out.origins = {
  assert(url.parse("blob:https://example.com:8443/uuid")).origin,
  assert(url.parse("blob:blob:https://example.com/uuid")).origin,
  assert(url.parse("data:text/plain,hi")).origin,
  assert(url.parse("ws://example.com/")).origin,
}

-- what it asserts
do
  local a = assert(url.parse("https://example.com/a"))
  assert(
    a:equals((assert(url.parse("https://example.com/a"))))
      and not a:equals((assert(url.parse("https://example.com/b"))))
  )

  -- query pairs are records, in order, repeats included
  local pairs_out = assert(url.parse("https://example.com/?a=1&b=2&a=3")):query_pairs()
  assert(#pairs_out == 3 and pairs_out[1].key == "a" and pairs_out[3].value == "3")
  local q = url.encode_query { { key = "name", value = "ada lovelace" }, { key = "role", value = "engineer/lead" } }
  assert(q == "name=ada+lovelace&role=engineer%2Flead")
  local decoded = url.decode_query(q)
  assert(decoded[1].key == "name" and decoded[1].value == "ada lovelace" and decoded[2].value == "engineer/lead")
  local ok, refused = pcall(url.encode_query, { { "a", "b" } } :: any) -- deliberate refusal test: the arrays pairs were
  assert(not ok and tostring(refused):find("argument #1 'pairs'", 1, true))

  -- arguments are checked by their types
  for _, port in { 99999, -1, 8080.5 } do
    local fine, why = pcall(function()
      a:with_port(port)
    end)
    assert(not fine and tostring(why):find("lib:url.Url:with_port: argument #1 'port'", 1, true))
  end
  local fine, why = pcall(url.encode, "x", "bogus" :: any) -- deliberate refusal test: not a set
  assert(not fine and tostring(why):find("lib:url.encode: argument #2 'set'", 1, true))
  local fine2, why2 = pcall(function()
    url.resolve(1 :: any, "x") -- deliberate refusal test: a number is not a base
  end)
  assert(not fine2 and tostring(why2):find("argument #1 'base'", 1, true))

  -- typed errors
  local _, invalid = url.parse("not a url")
  assert(invalid ~= nil)
  assert(invalid.kind == "invalid_url" and invalid.input == "not a url" and invalid.op == "lib:url.parse")
  assert(tostring(invalid):find("lib:url.parse: invalid_url:", 1, true))
  local _, unresolved = url.resolve("mailto:ada@example.com", "x")
  assert(unresolved ~= nil and unresolved.kind == "cannot_resolve" and unresolved.reference == "x")
  local _, scheme = a:with_scheme("not a scheme")
  assert(scheme ~= nil)
  assert(scheme.kind == "invalid_scheme" and scheme.scheme == "not a scheme" and scheme.op == "lib:url.Url:with_scheme")
  local _, host = a:with_host("exa mple.com")
  assert(host ~= nil and host.kind == "invalid_host" and host.host == "exa mple.com")
  local _, port = assert(url.parse("mailto:ada@example.com")):with_port(25)
  assert(port ~= nil and port.kind == "cannot_set_port")
  local _, utf8 = url.decode("%FF%FE")
  assert(utf8 ~= nil and utf8.kind == "invalid_utf8" and utf8.op == "lib:url.decode")
end

return out
