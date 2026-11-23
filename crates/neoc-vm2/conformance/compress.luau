-- lib:compress on neoc-vm and neoc-vm2: what both must answer the same, the bytes each encoder writes included.
-- A format or an option of the wrong type is refused as an argument on neoc-vm2 (compress.vm2.luau) and is not
-- here.
local compress = require("lib:compress")
local hex = require("lib:hex")
local out = {}

local formats = { "gzip", "zlib", "zstd", "brotli" }
local function fails(value, err)
	return { value == nil, err ~= nil }
end

local text = string.rep("neoc byte-stability probe 0123456789\n", 20)
local large = string.rep("The quick brown fox jumps over the lazy dog. ", 5000)

for _, format in formats do
	local packed = compress.encode(format, "hello world")
	out[format] = {
		round_trip = compress.decode(format, packed),
		binary = hex.encode((compress.decode(format, (compress.encode(format, "\0\1\2\254\255"))))),
		empty_has_a_stream = #compress.encode(format, "") > 0,
		empty_round_trip = compress.decode(format, (compress.encode(format, ""))),
		large = compress.decode(format, (compress.encode(format, large))) == large,
		large_is_smaller = #compress.encode(format, large) < #large,
		default_level = hex.encode((compress.encode(format, text))),
		decode_empty = fails(compress.decode(format, "")),
		decode_corrupt = fails(compress.decode(format, "not compressed data")),
		too_large = fails(compress.decode(format, compress.encode(format, string.rep("a", 1000)), { max_size = 10 })),
		fits = #compress.decode(format, compress.encode(format, string.rep("a", 1000)), { max_size = 1000 }),
		max_size_zero = fails(compress.decode(format, packed, { max_size = 0 })),
		level_too_high = fails(compress.encode(format, "hello", { level = 99 })),
	}
end

-- every level each format has, byte for byte
local levels = { gzip = { 0, 9 }, zlib = { 0, 9 }, zstd = { 1, 22 }, brotli = { 0, 11 } }
for format, range in levels do
	local bytes = {}
	for level = range[1], range[2] do
		local packed = compress.encode(format, text, { level = level })
		table.insert(bytes, hex.encode(packed))
		assert(compress.decode(format, packed) == text)
	end
	out[format .. "_levels"] = bytes
end
out.zstd_level_zero = fails(compress.encode("zstd", "hello", { level = 0 }))
out.zlib_stored = hex.encode((compress.encode("zlib", "", { level = 0 })))

out.cross_format = fails(compress.decode("zlib", (compress.encode("gzip", "hello"))))

return out
