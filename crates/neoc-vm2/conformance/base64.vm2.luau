--!strict
-- intentional: a decode failure is a typed error table { kind, message, op, offset?, byte?, length? }, not a "base64.decode: <text>" string.
local b64 = require("lib:base64")

local value, err = b64.decode("not*base64!")
assert(value == nil and err ~= nil)
assert(err.kind == "invalid_byte" and err.offset == 3 and err.byte == 42)
assert(err.op == "lib:base64.decode")
assert(tostring(err):find("^lib:base64%.decode: invalid_byte: "))

local _, at = b64.decode("ab*d")
assert(at ~= nil and at.kind == "invalid_byte" and at.offset == 2 and at.byte == 42)

local _, padding = b64.decode_no_pad("aGVsbG8=")
assert(padding ~= nil)
assert(padding.kind == "invalid_padding", padding.kind)
assert(padding.op == "lib:base64.decode_no_pad")

local _, length = b64.decode("a")
assert(length ~= nil and length.kind == "invalid_length" and length.length == 1)

-- bytes in and out: any byte, `\0` included
local data = assert(b64.decode("AAECgP8="))
assert(data == "\0\1\2\128\255")

-- a wrong argument is refused with its place
local ok, refused = pcall(b64.encode, 1 :: any) -- deliberate refusal test: a number is not bytes
assert(not ok and tostring(refused):find("lib:base64.encode: argument #1 'input': expected a string, got 1", 1, true))
