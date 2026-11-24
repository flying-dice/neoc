-- intentional: a failure is the typed Error (the same kinds; err.kind, err.message, err.op, and matches on ambiguous), so error_kind(err) and error_message(err) are gone; a service, user or password that is not UTF-8 text and a spec that is not text under text are refused as arguments (they were the returned error invalid_input).
local keyring = require("vnd:keyring")

local store = keyring.mock()
local entry = assert(store:entry("example.com", "ada"))

-- the functions that read an error string are gone: the error says it itself
assert((keyring :: any).error_kind == nil and (keyring :: any).error_message == nil)

-- a typed error: a secret that is not text, read as a password
assert(entry:set_secret("\255hunter2"))
local password, err = entry:get_password()
assert(password == nil and err ~= nil and err.kind == "bad_encoding")
assert(err.op == "vnd:keyring.Entry:get_password")
assert(err.message == "the secret is not UTF-8 text: read it with `get_secret`")
assert(tostring(err) == "vnd:keyring.Entry:get_password: bad_encoding: the secret is not UTF-8 text: read it with `get_secret`")
assert(not tostring(err):find("hunter2", 1, true))
assert(entry:get_secret() == "\255hunter2")

-- the kinds a script branches on, with their fields
local _, again = entry:get_password()
if again ~= nil then
	if again.kind == "ambiguous" then
		assert(again.matches > 1)
	elseif again.kind == "no_store" or again.kind == "no_access" or again.kind == "invalid_input" then
		assert(again.reason ~= "")
	else
		assert(again.kind == "bad_encoding")
	end
end

-- what is not text is refused as an argument, before the store
local ok_service, refused_service = pcall(function()
	return store:entry("\255", "ada")
end)
assert(not ok_service and tostring(refused_service):find("vnd:keyring.Store:entry: argument #1 'service': expected UTF-8 text", 1, true))
local ok_user, refused_user = pcall(function()
	return store:entry("example.com", 5 :: any)
end)
assert(not ok_user and tostring(refused_user):find("vnd:keyring.Store:entry: argument #2 'user'", 1, true))
local ok_password, refused_password = pcall(function()
	return entry:set_password("\255")
end)
assert(not ok_password and tostring(refused_password):find("vnd:keyring.Entry:set_password: argument #1 'password': expected UTF-8 text", 1, true))
assert(entry:get_secret() == "\255hunter2")
local ok_spec, refused_spec = pcall(function()
	return store:search({ service = 1 } :: any)
end)
assert(not ok_spec and tostring(refused_spec):find("vnd:keyring.Store:search: argument #1 'spec'", 1, true))

-- success is as it was: true, the value, or nil without an error
assert(entry:set_password("text") == true)
assert(entry:set_secret("bytes") == true)
local service, user = entry:get_specifiers()
assert(service == "example.com" and user == "ada")
assert(entry:delete_credential() == true)
local deleted, delete_err = entry:delete_credential()
assert(deleted == false and delete_err == nil)
local gone, gone_err = entry:get_password()
assert(gone == nil and gone_err == nil)
