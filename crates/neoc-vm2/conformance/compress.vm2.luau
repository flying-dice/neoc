-- intentional: the format is a string enum, so one that does not exist is refused as an argument (it was nil and "unknown format"); level and max_size are whole numbers from 0 up and anything else, or an option that does not exist, is refused as an argument (each was nil and a message); failures are typed: EncodeError (level_out_of_range, failed) and DecodeError (empty, max_size_zero, too_large, corrupt).
local compress = require("lib:compress")

local packed = assert(compress.encode("gzip", string.rep("a", 1000), { level = 9 }))
assert(compress.decode("gzip", packed) == string.rep("a", 1000))

-- typed errors
local _, level = compress.encode("gzip", "hello", { level = 99 })
assert(level ~= nil and level.kind == "level_out_of_range")
if level.kind == "level_out_of_range" then
	assert(level.format == "gzip" and level.level == 99 and level.min == 0 and level.max == 9)
end
assert(tostring(level) == "lib:compress.encode: level_out_of_range: level out of range for gzip: 99 (it is from 0 to 9)")
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
for _, format in { "gzip", "zlib", "zstd", "brotli" } :: { lib_compress_Format } do
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
