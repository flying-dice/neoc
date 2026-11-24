--!strict
-- lib:compress as a script sees it: its answers (compress.json), then what it asserts.
local compress = require("lib:compress")
local hex = require("lib:hex")
local out: { [string]: any } = {}

local formats = { "gzip", "zlib", "zstd", "brotli" }
local function fails(value: unknown, err: unknown): { boolean }
  return { value == nil, err ~= nil }
end

local text = string.rep("neoc byte-stability probe 0123456789\n", 20)
local large = string.rep("The quick brown fox jumps over the lazy dog. ", 5000)

for _, name in formats do
  local format: lib_compress_Format = name :: any
  local packed = assert(compress.encode(format, "hello world"))
  out[format] = {
    round_trip = compress.decode(format, packed),
    binary = hex.encode((assert(compress.decode(format, (assert(compress.encode(format, "\0\1\2\254\255"))))))),
    empty_has_a_stream = #assert(compress.encode(format, "")) > 0,
    empty_round_trip = compress.decode(format, (assert(compress.encode(format, "")))),
    large = compress.decode(format, (assert(compress.encode(format, large)))) == large,
    large_is_smaller = #assert(compress.encode(format, large)) < #large,
    default_level = hex.encode((assert(compress.encode(format, text)))),
    decode_empty = fails(compress.decode(format, "")),
    decode_corrupt = fails(compress.decode(format, "not compressed data")),
    too_large = fails(
      compress.decode(format, (assert(compress.encode(format, string.rep("a", 1000)))), { max_size = 10 })
    ),
    fits = #assert(
      compress.decode(format, (assert(compress.encode(format, string.rep("a", 1000)))), { max_size = 1000 })
    ),
    max_size_zero = fails(compress.decode(format, packed, { max_size = 0 })),
    level_too_high = fails(compress.encode(format, "hello", { level = 99 })),
  }
end

-- every level each format has, byte for byte
local levels: { [string]: { number } } = { gzip = { 0, 9 }, zlib = { 0, 9 }, zstd = { 1, 22 }, brotli = { 0, 11 } }
for name, range in levels do
  local format: lib_compress_Format = name :: any
  local bytes = {}
  for level = range[1], range[2] do
    local packed = assert(compress.encode(format, text, { level = level }))
    table.insert(bytes, hex.encode(packed))
    assert(compress.decode(format, packed) == text)
  end
  out[format .. "_levels"] = bytes
end
out.zstd_level_zero = fails(compress.encode("zstd", "hello", { level = 0 }))
out.zlib_stored = hex.encode((assert(compress.encode("zlib", "", { level = 0 }))))

out.cross_format = fails(compress.decode("zlib", (assert(compress.encode("gzip", "hello")))))

-- what it asserts
do
  local packed = assert(compress.encode("gzip", string.rep("a", 1000), { level = 9 }))
  assert(compress.decode("gzip", packed) == string.rep("a", 1000))

  -- typed errors
  local _, level = compress.encode("gzip", "hello", { level = 99 })
  assert(level ~= nil and level.kind == "level_out_of_range")
  if level.kind == "level_out_of_range" then
    assert(level.format == "gzip" and level.level == 99 and level.min == 0 and level.max == 9)
  end
  assert(
    tostring(level) == "lib:compress.encode: level_out_of_range: level out of range for gzip: 99 (it is from 0 to 9)"
  )
  local _, zstd_zero = compress.encode("zstd", "hello", { level = 0 })
  assert(zstd_zero ~= nil and zstd_zero.kind == "level_out_of_range" and zstd_zero.op == "lib:compress.encode")

  local _, empty = compress.decode("gzip", "")
  assert(empty ~= nil and empty.kind == "empty")
  local _, zero = compress.decode("gzip", packed, { max_size = 0 })
  assert(zero ~= nil and zero.kind == "max_size_zero")
  local _, large = compress.decode("gzip", packed, { max_size = 10 })
  assert(large ~= nil and large.kind == "too_large")
  if large.kind == "too_large" then
    assert(large.max_size == 10)
  end
  assert(tostring(large) == "lib:compress.decode: too_large: output exceeds max_size (10 bytes)")
  local formats: { lib_compress_Format } = { "gzip", "zlib", "zstd", "brotli" }
  for _, format: lib_compress_Format in formats do
    local _, corrupt = compress.decode(format, "not compressed data")
    assert(corrupt ~= nil and corrupt.kind == "corrupt")
    if corrupt.kind == "corrupt" then
      assert(corrupt.format == format and #corrupt.reason > 0)
    end
  end
  local _, crossed = compress.decode("zlib", packed)
  assert(crossed ~= nil and crossed.kind == "corrupt")

  -- what is not a format, a level or a size is refused as an argument, with its place
  local function refused(place: string, f: () -> ...unknown)
    local ok, err = pcall(f)
    assert(not ok and tostring(err):find(place, 1, true), tostring(err))
  end
  for _, format in { "lz4", "GZIP", "", 5 } :: { any } do
    refused("lib:compress.encode: argument #1 'format'", function()
      return compress.encode(format, "hello")
    end)
    refused("lib:compress.decode: argument #1 'format'", function()
      return compress.decode(format, "hello")
    end)
  end
  for _, options in { { level = 6.7 }, { level = "fast" }, { level = -1 }, { levle = 1 }, { max_size = 10 } } :: { any } do
    refused("lib:compress.encode: argument #3 'options'", function()
      return compress.encode("gzip", "hello", options)
    end)
  end
  for _, options in { { max_size = "1MB" }, { max_size = 1.5 }, { max_size = -1 }, { level = 1 } } :: { any } do
    refused("lib:compress.decode: argument #3 'options'", function()
      return compress.decode("gzip", packed, options)
    end)
  end
  refused("lib:compress.encode: argument #2 'data'", function()
    return compress.encode("gzip", 5 :: any)
  end)
end

return out
