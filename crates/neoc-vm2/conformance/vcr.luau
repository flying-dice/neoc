-- lib:vcr on neoc-vm and neoc-vm2: what both must answer the same. Returns plain values, compared by the harness.
-- A recorded call is `tape:wrap(key, f)(...)` on neoc-vm and `tape:play(key, function() return f(...) end)` on
-- neoc-vm2; `play` below is the one or the other, so that what is recorded and replayed can be compared. What
-- differs on purpose (errors, what is refused, the file) is in vcr.vm2.luau.
local vcr = require("lib:vcr")
local base64 = require("lib:base64")
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")

local function play(tape: any, key: string, call: () -> ...any): ...any
	if tape.play ~= nil then
		return tape:play(key, call)
	end
	return tape:wrap(key, call)()
end

local out = {}

local dir = path.join(env.temp_dir(), "neoc-conformance-vcr-" .. random.uuid_v4())
local function recorded()
	assert(fs.create_dir_all(dir))
	local file = path.join(dir, "tape.json")
	local result = {}

	-- record: the call's own result comes back, and the file exists from the start
	local rec = vcr.cassette({ path = file, mode = "record" })
	result.created = { rec ~= nil, fs.exists(file) }
	local decoded, decode_err = play(rec, "decode", function()
		return base64.decode("aGVsbG8=")
	end)
	local failed, failure = play(rec, "invalid", function()
		return base64.decode("not*valid!")
	end)
	result.record = {
		play(rec, "encode", function()
			return base64.encode("hello")
		end),
		decoded,
		decode_err == nil,
		failed == nil,
		failure ~= nil,
		play(rec, "number", function()
			return 42
		end),
		play(rec, "table", function()
			return { name = "x", list = { 1, 2, 3 }, nested = { flag = true } }
		end).nested.flag,
	}
	-- a call that raises is raised
	result.raises = (pcall(play, rec, "raises", function()
		error("the call failed")
	end))
	-- a buffer is not recorded
	result.buffer = (pcall(play, rec, "buffer", function()
		return { data = buffer.create(2) }
	end))

	-- replay: the same answers, and the call is not run
	local ran = false
	local function never(): any
		ran = true
		return "not recorded"
	end
	local rep = vcr.cassette({ path = file, mode = "replay" })
	local again, again_err = play(rep, "decode", never)
	local table_again = play(rep, "table", never)
	result.replay = {
		play(rep, "encode", never),
		again,
		again_err == nil,
		(play(rep, "invalid", never)) == nil,
		play(rep, "number", never),
		table_again.name,
		#table_again.list,
		table_again.nested.flag,
		ran,
	}
	-- a key the cassette does not have, and one it has but was not recorded (the call raised)
	result.missing = { (pcall(play, rep, "nope", never)), (pcall(play, rep, "raises", never)), (pcall(play, rep, "buffer", never)) }

	-- off: the call runs, nothing is written, a path is not needed
	local off = vcr.cassette({ mode = "off" })
	local off_file = path.join(dir, "off.json")
	result.off = {
		play(off, "encode", function()
			return base64.encode("hello")
		end),
		play(vcr.cassette({ mode = "off", path = off_file }), "encode", function()
			return base64.encode("again")
		end),
		fs.exists(off_file),
	}
	return result
end
local ok, result = pcall(recorded)
fs.remove_dir_all(dir, { missing_ok = true })
assert(ok, result)
out.recorded = result

out.wrong_arguments = { (pcall(vcr.cassette, { mode = "rewind", path = "x" })), (pcall(vcr.cassette, { mode = "record" })), (pcall(vcr.cassette)) }

return out
