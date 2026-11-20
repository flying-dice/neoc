--!strict
-- A fallible function returns `(value, err)`, both optional: strict mode refuses a value used as if it were
-- there. Same marker rule as wrong_use.luau (an expect-error marker at the end of the line).
local fs = require("std:fs")
local hash = require("lib:hash")
local semver = require("lib:semver")
local url = require("lib:url")

-- the value is optional until it is checked
local content, read_err = fs.read("/tmp/x")
print(content:len(), read_err) -- expect-error: Value of type 'string?' could be nil

-- checking the error does not narrow the value
local digest, digest_err = hash.hash("sha256", "data")
if digest_err ~= nil then
	error(digest_err)
end
print(digest:upper()) -- expect-error: Value of type 'string?' could be nil

-- a constructor that can fail returns an optional handle
local parsed = url.Url.parse("https://example.com")
print(parsed.host) -- expect-error: Value of type 'lib_url_Url?' could be nil
print(semver.Version.parse("1.0.0"):increment("major")) -- expect-error: Value of type 'lib_semver_Version?' could be nil

-- a narrowed value is fine, an error string is not a value
local checked = url.Url.parse("https://example.com/a")
if checked == nil then
	return
end
local _host: string? = checked.host
local _wrong: lib_url_Url = read_err -- expect-error: Expected this to be 'lib_url_Url', but got 'string?'
