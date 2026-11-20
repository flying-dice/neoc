-- lua-language-server plugin: resolves neoc's colon-named modules to the generated definitions.
--
-- `require("lib:json")` is `types/luals/lib/json.lua`, `require("vnd:rusqlite")` is
-- `types/luals/vnd/rusqlite.lua`. A colon is not allowed in a Windows file name, so the definition files cannot
-- be named after the module key (lua-language-server would otherwise look for `lib:json.lua`); this maps the key
-- to the file instead. Any other name falls through to the normal search.
--
-- Enabled by `runtime.plugin` in .luarc.json. The editor asks once whether to trust a workspace plugin; this one
-- only computes a path.

local furi = require("file-uri")

-- the folder this plugin sits in (`types/`), found from the chunk's own name (`@/path/to/types/luals-plugin.lua`)
local source = debug.getinfo(1, "S").source
local here = source:sub(2):gsub("[/\\][^/\\]*$", "")

---@param _ string the workspace uri
---@param name string the argument of `require`
---@return string[]? uris the definition file, or nil to leave the name to the normal search
function ResolveRequire(_, name)
  local namespace, module = name:match("^(%a+):([%w_]+)$")
  if namespace ~= "std" and namespace ~= "lib" and namespace ~= "vnd" then
    return nil
  end
  return { furi.encode(here .. "/luals/" .. namespace .. "/" .. module .. ".lua") }
end
