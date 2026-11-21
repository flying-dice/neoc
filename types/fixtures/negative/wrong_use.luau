--!strict
-- Deliberately wrong uses of the `std:`, `lib:` and `vnd:` modules. The luau-lsp test requires a diagnostic on
-- every line that ends in an expect-error marker (the marker names the text the message must contain), and no
-- diagnostic on any other line; a line with two findings carries two markers. The marker is matched against the
-- message up to its `caused by:` lines. If the declarations stop loading, or `require("lib:json")` stops being
-- typed, nothing here is flagged (the module is `any` again) and the test fails.
local base64 = require("lib:base64")
local chrono = require("lib:chrono")
local collections = require("std:collections")
local fs = require("std:fs")
local hyper = require("vnd:hyper")
local json = require("lib:json")
local net = require("std:net")
local rusqlite = require("vnd:rusqlite")
local semver = require("lib:semver")
local serde_json = require("vnd:serde_json")
local task = require("std:task")
local test = require("lib:test")
local time = require("std:time")
local url = require("lib:url")

-- an unknown function, a missing argument, an argument of the wrong type, one argument too many
base64.encod("x") -- expect-error: Key 'encod' not found in table 'lib_base64'
base64.encode() -- expect-error: expects 1 argument, but none are specified
base64.encode({}) -- expect-error: Expected this to be 'string', but got '{|  |}'
time.sleep("soon") -- expect-error: Expected this to be 'std_time_Duration', but got 'string'
collections.Counter.new("ten") -- expect-error: Expected this to be 'number?', but got 'string'
base64.encode("x", "y") -- expect-error: expects 1 argument, but 2 are specified

-- a result of the wrong type: a fallible function returns an optional value
local _encoded: number = base64.encode("x") -- expect-error: Expected this to be 'number', but got 'string'
local _maybe: string = base64.decode("eA==") -- expect-error: Expected this to be 'string', but got 'string?'

-- an optional result used without a check
print(fs.read("/tmp/x"):upper()) -- expect-error: Value of type 'string?' could be nil

-- an unknown method, an unknown constructor, a wrong argument to a method
json.object():nope() -- expect-error: Key 'nope' not found in table 'lib_json_Object'
json.object():set(1, 2) -- expect-error: Expected this to be 'string', but got 'number'
collections.Map.create() -- expect-error: Key 'create' not found in table 'std_collections_MapStatics'
time.Duration.from_secs(5):as_years() -- expect-error: Key 'as_years' not found in table 'std_time_Duration'
chrono.Date.new("2026", 5, 20) -- expect-error: Expected this to be 'number', but got 'string'

-- a method called with a dot passes no receiver
local _object = json.object()
_object.set("name", "Ada") -- expect-error: Expected this to be 'lib_json_Object', but got 'string'

-- a field that does not exist, and a field read as the wrong type
local _parsed = assert(url.Url.parse("https://example.com"))
print(_parsed.nope) -- expect-error: Key 'nope' not found in table 'lib_url_Url'
local _major: string = assert(semver.Version.parse("1.0.0")).major -- expect-error: Expected this to be 'string', but got 'number'

-- trailing arguments are only for the functions that take them
assert(rusqlite.open(":memory:")):query() -- expect-error: Argument count mismatch. Function expects at least 2 arguments, but only 1 is specified
task.wait() -- expect-error: expects 1 argument, but none are specified
test.describe() -- expect-error: expects 1 to 2 arguments, but none are specified
test.expect(1, 2).toBe(1) -- expect-error: expects 0 to 1 argument, but 2 are specified
test.expect(1).toBeAbout(1) -- expect-error: Key 'toBeAbout' not found in table 'lib_test_Matcher'

-- a handler that is not a function, a listener that is not one
local _listener = assert(net.TcpListener.bind("127.0.0.1:0"))
hyper.serve(_listener, "handler") -- expect-error: Expected this to be '(vnd_hyper_Request) -> (...(string | { body: string?, headers: { [string]: string }?, status: number? })?)', but got 'string'
-- a handler returns a body, a table or nothing; a number is none of them, and the optional header is checked
hyper.serve(_listener, function(_req) return 42 end) -- expect-error: Expected this to be '(string | { body: string?, headers: { [string]: string }?, status: number? })?', but got 'number'
hyper.serve(_listener, function(_req) return { status = "ok" } end) -- expect-error: but got '{| status: string |}'
hyper.serve(_listener, function(req) return req:header("accept"):upper() end) -- expect-error: Value of type 'string?' could be nil
hyper.serve("127.0.0.1:0", function(_req) return nil end) -- expect-error: Expected this to be 'std_net_TcpListener', but got 'string'

-- a module that does not exist, and a module name that is not a literal
local _missing = require("lib:jsno") -- expect-error: Expected this to be '"std:collections"', but got '"lib:jsno"' -- expect-error: Other overloads are also not viable

local _name: string = "lib:json"
local _dynamic = require(_name) -- expect-error: Expected this to be '"std:collections"', but got 'string' -- expect-error: Other overloads are also not viable

local _fine = base64.encode("this line is correct")

-- the filesystem functions: a missing argument, an option of the wrong type, a metadata or watcher method that does not exist
fs.copy_dir("a") -- expect-error: Function 'fs.copy_dir' expects 2 arguments, but only 1 is specified
fs.remove_file("a", { missing_ok = "yes" }) -- expect-error: Expected this to be '{ missing_ok: boolean? }?' but got '{| missing_ok: string |}'
assert(fs.metadata("a")):size() -- expect-error: Key 'size' not found in table 'std_fs_Metadata'
assert(fs.watch("a")):recv() -- expect-error: Key 'recv' not found in table 'std_fs_Watcher'

-- a `lib:json` path is keys and indexes, and needs at least one
json.object():get() -- expect-error: Function expects at least 2 arguments, but only 1 is specified
json.object():get(true) -- expect-error: Expected this to be 'number | string', but got 'boolean'
json.array():get(1, {}) -- expect-error: Expected this to be 'number | string', but got '{|  |}'

-- a parsed document may be a scalar or the null sentinel, so it is not a handle until it is narrowed
assert(serde_json.from_str("{}")):get("a") -- expect-error: Key 'get' is missing from
local _document: lib_json_Object = assert(serde_json.from_str("{}")) -- expect-error: Expected this to be 'lib_json_Object'
