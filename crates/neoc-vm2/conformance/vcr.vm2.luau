--!strict
-- intentional: tape:play(key, function() return f(x) end) replaces tape:wrap(key, f)(x) (mlua-ir has no function that returns a function, nor one of any number of values), so a call's arguments are not written to the cassette; a call returns at most a result and an error (too_many_results past that) and may wait; a key is played once per cassette (it was wrapped once and called any number of times); what is not plain data is refused (not_plain_data: only a buffer was, the rest was recorded as "<function>"); cassette returns a typed CassetteError (not_found, io, malformed; they were raised) and play raises a typed PlayError (duplicate_key, missing_key, too_many_results, not_plain_data, io); a wrong mode, a missing path and an unknown option are refused as arguments; replay gives nil for a recorded null; dump is gone.
local vcr = require("lib:vcr")
local base64 = require("lib:base64")
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")
local time = require("std:time")

local function raised(call: () -> ...unknown): string
	local ok, err = pcall(call)
	assert(not ok, "nothing was raised")
	return tostring(err)
end
local function has(text: string, part: string)
	assert(text:find(part, 1, true) ~= nil, text)
end

local dir = path.join(env.temp_dir(), "neoc-conformance-vcr-vm2-" .. random.uuid_v4())
local function checks(): ...unknown
	assert(fs.create_dir_all(dir))
	local file = path.join(dir, "tape.json")

	-- a cassette that cannot be opened is a returned, typed error
	local none, not_found = vcr.cassette({ mode = "replay", path = file })
	assert(none == nil and not_found ~= nil and not_found.kind == "not_found" and not_found.path == file)
	assert(tostring(not_found) == `lib:vcr.cassette: not_found: no cassette at {file}`)
	assert(fs.write(file, "{ not json"))
	local _, malformed = vcr.cassette({ mode = "replay", path = file })
	assert(malformed ~= nil and malformed.kind == "malformed" and malformed.op == "lib:vcr.cassette")
	local _, unwritable = vcr.cassette({ mode = "record", path = path.join(path.join(dir, "no-dir"), "tape.json") })
	assert(unwritable ~= nil and unwritable.kind == "io")

	-- record: a result and an error, a call that waits, and the file
	local rec = assert(vcr.cassette({ mode = "record", path = file }))
	local decoded, decode_err = rec:play("decode", function()
		return base64.decode("aGVsbG8=")
	end)
	assert(decoded == "hello" and decode_err == nil)
	local failed, failure = rec:play("invalid", function()
		return base64.decode("not*valid!")
	end)
	assert(failed == nil and failure.kind == "invalid_byte")
	local slow = rec:play("slow", function()
		time.sleep(time.Duration.from_millis(2))
		return "after the wait"
	end)
	assert(slow == "after the wait")
	local written = assert(fs.read(file))
	has(written, '"decode": {\n    "result": [\n      "hello",\n      null\n    ]\n  }')
	has(written, '"kind": "invalid_byte"')
	assert(written:find('"args"', 1, true) == nil, "a call's arguments are not written")

	-- a key once, two values at most, plain data only: raised, typed, and nothing recorded
	has(raised(function()
		return rec:play("decode", function()
			return "again"
		end)
	end), 'lib:vcr.Cassette:play: duplicate_key: duplicate key "decode"')
	has(raised(function()
		return rec:play("three", function()
			return 1, 2, 3
		end)
	end), 'lib:vcr.Cassette:play: too_many_results: the call "three" returned 3 values')
	local not_plain: { [string]: any } = { ["function"] = print, handle = rec, buffer = { buffer.create(2) } }
	for key, value in not_plain do
		has(raised(function()
			return rec:play(key, function()
				return value
			end)
		end), `lib:vcr.Cassette:play: not_plain_data: the call "{key}" returned what is not plain data: `)
	end

	-- replay: both values, nil for a null, and its own refusals
	local rep = assert(vcr.cassette({ mode = "replay", path = file }))
	local function never(): unknown
		error("replay ran the call")
	end
	local again, again_err = rep:play("decode", never)
	assert(again == "hello" and again_err == nil)
	local replayed, replayed_err = rep:play("invalid", never)
	assert(replayed == nil and replayed_err.kind == "invalid_byte" and replayed_err.op == "lib:base64.decode")
	has(raised(function()
		return rep:play("decode", never)
	end), 'lib:vcr.Cassette:play: duplicate_key: duplicate key "decode"')
	has(raised(function()
		return rep:play("three", never)
	end), 'lib:vcr.Cassette:play: missing_key: no cassette entry for key "three"')

	-- a cassette neoc-vm wrote replays: its args are not read, a null inside a table is nil
	local old = path.join(dir, "old.json")
	assert(fs.write(old, '{ "call": { "args": ["x", null], "result": [{ "a": null, "b": [1, 2] }, null] } }'))
	local value, err = assert(vcr.cassette({ mode = "replay", path = old })):play("call", never)
	assert(value.a == nil and #value.b == 2 and err == nil)

	-- off: any key any number of times, any value
	local off = assert(vcr.cassette({ mode = "off" }))
	for _ = 1, 2 do
		local live, second = off:play("same", function()
			return print, rec
		end)
		assert(live == print and second == rec)
	end
end
local ok, failure = pcall(checks)
fs.remove_dir_all(dir, { missing_ok = true })
assert(ok, tostring(failure))

-- there is no wrap and no dump
local tape = assert(vcr.cassette({ mode = "off" }))
assert(not pcall(function()
	return (tape :: any):wrap("key", print)
end) and not pcall(function()
	return (tape :: any):dump()
end))

-- arguments of the wrong shape are refused, with their place
for _, config in { { mode = "rewind", path = "x" }, { path = "x" }, { mode = "record" }, { mode = "off", strict = true }, "record" } :: { any } do
	has(raised(function()
		return vcr.cassette(config)
	end), "lib:vcr.cassette: argument #1 'config'")
end
has(raised(function()
	return tape:play("key", "not a call" :: any) -- deliberate refusal test: text is not a call
end), "lib:vcr.Cassette:play: argument #2 'call'")
