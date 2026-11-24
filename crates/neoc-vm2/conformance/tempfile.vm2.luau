--!strict
-- intentional: a failure is a typed TempError (not_found, permission_denied, invalid_input, closed, other), TempFile.new and TempDir.new are gone (they were tempfile and tempdir under a second name) and so is gettempdir (it is std:env's temp_dir), TempFile:close reports a failure to remove the file as TempDir:close always did (it returned nothing), and an option that does not exist or has the wrong type is refused as a wrong argument with its place.
local tempfile = require("lib:tempfile")
local fs = require("std:fs")

-- typed errors
local none, missing = tempfile.tempfile({ dir = "/does/not/exist/anywhere" })
assert(none == nil and missing ~= nil and missing.kind == "not_found" and missing.op == "lib:tempfile.tempfile")
assert(missing.message == missing.reason and tostring(missing) == "lib:tempfile.tempfile: not_found: " .. missing.reason)
local _, missing_dir = tempfile.tempdir({ dir = "/does/not/exist/anywhere" })
assert(missing_dir ~= nil and missing_dir.kind == "not_found" and missing_dir.op == "lib:tempfile.tempdir")

-- a handle that was kept or closed owns nothing
local file = assert(tempfile.tempfile())
local kept = assert(file:keep())
local again, closed = file:keep()
assert(again == nil and closed ~= nil and closed.kind == "closed")
assert(tostring(closed) == "lib:tempfile.TempFile:keep: closed: already closed")
assert(fs.remove_file(kept))
local dir = assert(tempfile.tempdir())
assert(dir:close() == true)
local _, dir_closed = dir:keep()
assert(dir_closed ~= nil and dir_closed.kind == "closed" and dir_closed.op == "lib:tempfile.TempDir:keep")

-- close says whether it removed the file
local closing = assert(tempfile.tempfile())
local ok, err = closing:close()
assert(ok == true and err == nil and closing:close() == true and fs.exists(closing:path()) == false)

-- one name each
local module = tempfile :: any
assert(module.TempFile == nil and module.TempDir == nil and module.gettempdir == nil)

-- a wrong option is a wrong argument
for _, opts in { { prefx = "a" }, { dir = 5 }, { prefix = {} }, "name" } :: { any } do
	for _, make in { tempfile.tempfile, tempfile.tempdir } :: { any } do
		local made, refused = pcall(make, opts)
		assert(not made and tostring(refused):find("argument #1 'opts'", 1, true), tostring(refused))
	end
end
