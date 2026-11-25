--!strict
-- lib:vcr as a script sees it: its answers (vcr.json), then what it asserts.
local vcr = require("lib:vcr")
local base64 = require("lib:base64")
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")

local out = {}

local dir = path.join(env.temp_dir(), "neoc-scripts-vcr-" .. random.uuid_v4())
local function recorded()
  assert(fs.create_dir_all(dir))
  local file = path.join(dir, "tape.json")
  local result = {}

  -- record: the call's own result comes back, and the file exists from the start
  local made = vcr.cassette { path = file, mode = "record" }
  result.created = { (made :: any) ~= nil, fs.exists(file) } :: { any }
  local rec = assert(made)
  local decoded, decode_err = rec:play("decode", function()
    return base64.decode("aGVsbG8=")
  end)
  local failed, failure = rec:play("invalid", function()
    return base64.decode("not*valid!")
  end)
  result.record = {
    rec:play("encode", function()
      return base64.encode("hello")
    end),
    decoded,
    decode_err == nil,
    failed == nil,
    failure ~= nil,
    rec:play("number", function()
      return 42
    end),
    rec:play("table", function()
      return { name = "x", list = { 1, 2, 3 }, nested = { flag = true } }
    end).nested.flag,
  }
  -- a call that raises is raised
  result.raises = (
    pcall(function()
      return rec:play("raises", function()
        error("the call failed")
      end)
    end)
  )
  -- a buffer is not recorded
  result.buffer = (
    pcall(function()
      return rec:play("buffer", function()
        return { data = buffer.create(2) }
      end)
    end)
  )

  -- replay: the same answers, and the call is not run
  local ran = false
  local function never(): any
    ran = true
    return "not recorded"
  end
  local rep = assert(vcr.cassette { path = file, mode = "replay" })
  local again, again_err = rep:play("decode", never)
  local table_again = rep:play("table", never)
  result.replay = {
    rep:play("encode", never),
    again,
    again_err == nil,
    (rep:play("invalid", never)) == nil,
    rep:play("number", never),
    table_again.name,
    #table_again.list,
    table_again.nested.flag,
    ran,
  }
  -- a key the cassette does not have, and one it has but was not recorded (the call raised)
  local function replays(key: string): boolean
    return (pcall(function()
      return rep:play(key, never)
    end))
  end
  result.missing = { replays("nope"), replays("raises"), replays("buffer") }

  -- off: the call runs, nothing is written, a path is not needed
  local off = assert(vcr.cassette { mode = "off" })
  local off_file = path.join(dir, "off.json")
  result.off = {
    off:play("encode", function()
      return base64.encode("hello")
    end),
    assert(vcr.cassette { mode = "off", path = off_file }):play("encode", function()
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

out.wrong_arguments = {
  (pcall(vcr.cassette :: any, { mode = "rewind", path = "x" })),
  (pcall(vcr.cassette :: any, { mode = "record" })),
  (pcall(vcr.cassette :: any)),
}

-- what it asserts
do
  local time = require("std:time")

  local function raised(call: () -> ...unknown): string
    local ok, err = pcall(call)
    assert(not ok, "nothing was raised")
    return tostring(err)
  end
  local function has(text: string, part: string)
    assert(text:find(part, 1, true) ~= nil, text)
  end

  local dir = path.join(env.temp_dir(), "neoc-scripts-vcr-asserts-" .. random.uuid_v4())
  local function checks(): ...unknown
    assert(fs.create_dir_all(dir))
    local file = path.join(dir, "tape.json")

    -- a cassette that cannot be opened is a returned, typed error
    local none, not_found = vcr.cassette { mode = "replay", path = file }
    assert(none == nil and not_found ~= nil and not_found.kind == "not_found" and not_found.path == file)
    assert(tostring(not_found) == `lib:vcr.cassette: not_found: no cassette at {file}`)
    assert(fs.write(file, "{ not json"))
    local _, malformed = vcr.cassette { mode = "replay", path = file }
    assert(malformed ~= nil and malformed.kind == "malformed" and malformed.op == "lib:vcr.cassette")
    local _, unwritable = vcr.cassette { mode = "record", path = path.join(path.join(dir, "no-dir"), "tape.json") }
    assert(unwritable ~= nil and unwritable.kind == "io")

    -- record: a result and an error, a call that waits, and the file
    local rec = assert(vcr.cassette { mode = "record", path = file })
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
    has(
      raised(function()
        return rec:play("decode", function()
          return "again"
        end)
      end),
      'lib:vcr.Cassette:play: duplicate_key: duplicate key "decode"'
    )
    has(
      raised(function()
        return rec:play("three", function()
          return 1, 2, 3
        end)
      end),
      'lib:vcr.Cassette:play: too_many_results: the call "three" returned 3 values'
    )
    local not_plain: { [string]: any } = { ["function"] = print, handle = rec, buffer = { buffer.create(2) } }
    for key, value in not_plain do
      has(
        raised(function()
          return rec:play(key, function()
            return value
          end)
        end),
        `lib:vcr.Cassette:play: not_plain_data: the call "{key}" returned what is not plain data: `
      )
    end

    -- replay: both values, nil for a null, and its own refusals
    local rep = assert(vcr.cassette { mode = "replay", path = file })
    local function never(): unknown
      error("replay ran the call")
    end
    local again, again_err = rep:play("decode", never)
    assert(again == "hello" and again_err == nil)
    local replayed, replayed_err = rep:play("invalid", never)
    assert(replayed == nil and replayed_err.kind == "invalid_byte" and replayed_err.op == "lib:base64.decode")
    has(
      raised(function()
        return rep:play("decode", never)
      end),
      'lib:vcr.Cassette:play: duplicate_key: duplicate key "decode"'
    )
    has(
      raised(function()
        return rep:play("three", never)
      end),
      'lib:vcr.Cassette:play: missing_key: no cassette entry for key "three"'
    )

    -- a null inside a table is nil
    local nulls = path.join(dir, "nulls.json")
    assert(fs.write(nulls, '{ "call": { "result": [{ "a": null, "b": [1, 2] }, null] } }'))
    local value, err = assert(vcr.cassette { mode = "replay", path = nulls }):play("call", never)
    assert(value.a == nil and #value.b == 2 and err == nil)

    -- an entry holds its result and nothing else
    local extra = path.join(dir, "extra.json")
    assert(fs.write(extra, '{ "call": { "args": ["x"], "result": ["v", null] } }'))
    local no_tape, extra_err = vcr.cassette { mode = "replay", path = extra }
    assert(no_tape == nil and extra_err ~= nil and extra_err.kind == "malformed")

    -- off: any key any number of times, any value
    local off = assert(vcr.cassette { mode = "off" })
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

  local tape = assert(vcr.cassette { mode = "off" })

  -- arguments of the wrong shape are refused, with their place
  for _, config in
    { { mode = "rewind", path = "x" }, { path = "x" }, { mode = "record" }, { mode = "off", strict = true }, "record" } :: { any }
  do
    has(
      raised(function()
        return vcr.cassette(config)
      end),
      "lib:vcr.cassette: argument #1 'config'"
    )
  end
  has(
    raised(function()
      return tape:play("key", "not a call" :: any) -- deliberate refusal test: text is not a call
    end),
    "lib:vcr.Cassette:play: argument #2 'call'"
  )
end

return out
