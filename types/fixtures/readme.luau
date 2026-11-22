--!strict
-- The front-page example of README.md (section "neoc"), checked in strict mode. `the_readme_example_is_the_fixture`
-- in crates/neoc-vm/tests/luau_decls.rs holds the two to the same text: change both together.
-- notes.lua
local fs   = require("std:fs")
local json = require("vnd:serde_json")
local null = require("lib:json").null -- the JSON null sentinel is lib:json's; vnd:serde_json has none

-- a parse result is a handle, `null` or a scalar: cast a container to the handle you expect
local doc = assert(json.from_str('{"name": "neoc", "tags": ["lua", "rust"], "owner": null}')) :: lib_json_Object

local text = assert(json.to_string_pretty(doc))
assert(fs.write("notes.json", text))
print("saved " .. tostring(doc:get("name")))
if doc:get("owner") == null then
  print("no owner yet")
end
