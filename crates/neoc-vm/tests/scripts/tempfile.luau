--!strict
-- lib:tempfile as a script sees it: its answers (tempfile.json), then what it asserts. Everything it makes is under the
-- system's temporary directory and is removed: closed here, or collected.
local tempfile = require("lib:tempfile")
local fs = require("std:fs")
local path = require("std:path")
local env = require("std:env")

local out = {}

local file = assert(tempfile.tempfile())
local file_path = file:path()
out.file = {
  type(file_path),
  fs.exists(file_path),
  assert(fs.metadata(file_path)) ~= nil,
  path.strip_prefix(file_path, env.temp_dir()) ~= nil,
  path.is_absolute(file_path),
  tostring(file) == "TempFile(" .. file_path .. ")",
  file:path() == file_path,
} :: { any }
assert(fs.write(file_path, "content"))
out.file_content = fs.read(file_path)
file:close()
file:close()
out.file_closed = { fs.exists(file_path), file:path() == file_path }

local dir = assert(tempfile.tempdir())
local dir_path = dir:path()
out.dir = {
  fs.exists(dir_path),
  #assert(fs.read_dir(dir_path)),
  path.strip_prefix(dir_path, env.temp_dir()) ~= nil,
  tostring(dir) == "TempDir(" .. dir_path .. ")",
} :: { any }
assert(fs.write(path.join(dir_path, "inner"), "x"))
out.dir_close = { dir:close(), dir:close(), fs.exists(dir_path) }

-- names
local named = assert(tempfile.tempfile { prefix = "neoc-", suffix = ".tmp" })
local name = assert(path.file_name(named:path()))
local named_dir = assert(tempfile.tempdir { prefix = "neoc-", suffix = ".d" })
local dir_name = assert(path.file_name(named_dir:path()))
out.names = {
  name:sub(1, 5),
  name:sub(-4),
  #name > 9,
  dir_name:sub(1, 5),
  dir_name:sub(-2),
  name ~= path.file_name(assert(tempfile.tempfile { prefix = "neoc-", suffix = ".tmp" }):path()),
} :: { any }
named:close()

-- a parent of the script's choosing
local inside = assert(tempfile.tempfile { dir = named_dir:path() })
local inside_dir = assert(tempfile.tempdir { dir = named_dir:path(), prefix = "sub" })
out.parent = {
  path.parent(inside:path()) == named_dir:path(),
  path.parent(inside_dir:path()) == named_dir:path(),
  #assert(fs.read_dir(named_dir:path())),
} :: { any }
inside:close()
inside_dir:close()
out.parent_after = #assert(fs.read_dir(named_dir:path()))
named_dir:close()

-- keep: the file and the directory outlive their handles
local kept_file = assert(tempfile.tempfile())
local kept_dir = assert(tempfile.tempdir())
local kept_file_path, kept_dir_path = (assert(kept_file:keep())), (assert(kept_dir:keep()))
kept_file:close()
kept_dir:close()
out.keep = {
  kept_file_path == kept_file:path(),
  kept_dir_path == kept_dir:path(),
  fs.exists(kept_file_path),
  fs.exists(kept_dir_path),
} :: { any }
-- a failure is `(nil, err)`
local function failed(value: unknown, err: unknown): { boolean }
  return { value == nil, err ~= nil }
end
out.keep_again = { failed(kept_file:keep()), failed(kept_dir:keep()), failed(file:keep()), failed(dir:keep()) }
assert(fs.remove_file(kept_file_path))
assert(fs.remove_dir(kept_dir_path))

out.missing_parent = {
  failed(tempfile.tempfile { dir = "/does/not/exist/anywhere" }),
  failed(tempfile.tempdir { dir = "/does/not/exist/anywhere" }),
}
out.empty_options = { tempfile.tempfile {} ~= nil, tempfile.tempdir {} ~= nil, tempfile.tempfile(nil) ~= nil }

-- what it asserts
do
  -- typed errors
  local none, missing = tempfile.tempfile { dir = "/does/not/exist/anywhere" }
  assert(none == nil and missing ~= nil and missing.kind == "not_found" and missing.op == "lib:tempfile.tempfile")
  assert(
    missing.message == missing.reason and tostring(missing) == "lib:tempfile.tempfile: not_found: " .. missing.reason
  )
  local _, missing_dir = tempfile.tempdir { dir = "/does/not/exist/anywhere" }
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
end

return out
