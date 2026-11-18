--!nocheck
-- Runs one fixture app's window and prints what the host would send in `view.open`, as JSON:
--   luau harness.luau <app dir> [window]
-- The Luau closures in `on` become what the wire carries (`true`, or the client actions with an `emit` for each
-- function), and the source and client declarations come from the stub host.
local app = require("@agentos/app")

local OBJECT = {}
local function object(t)
  return setmetatable(t, OBJECT)
end

local function encode(value, out)
  local kind = type(value)
  if value == nil then
    table.insert(out, "null")
  elseif kind == "boolean" then
    table.insert(out, tostring(value))
  elseif kind == "number" then
    table.insert(out, string.format("%.14g", value))
  elseif kind == "string" then
    local escaped = string.gsub(value, '[%c"\\]', function(c)
      local named = { ['"'] = '\\"', ["\\"] = "\\\\", ["\n"] = "\\n", ["\r"] = "\\r", ["\t"] = "\\t" }
      return named[c] or string.format("\\u%04x", string.byte(c))
    end)
    table.insert(out, '"' .. escaped .. '"')
  elseif kind == "table" then
    local isObject = getmetatable(value) == OBJECT or (next(value) ~= nil and #value == 0)
    if isObject then
      local keys = {}
      for key in value do
        table.insert(keys, key)
      end
      table.sort(keys)
      table.insert(out, "{")
      for i, key in keys do
        if i > 1 then
          table.insert(out, ",")
        end
        encode(tostring(key), out)
        table.insert(out, ":")
        encode(value[key], out)
      end
      table.insert(out, "}")
    else
      table.insert(out, "[")
      for i = 1, #value do
        if i > 1 then
          table.insert(out, ",")
        end
        encode(value[i], out)
      end
      table.insert(out, "]")
    end
  else
    error("cannot encode " .. kind)
  end
end

-- What the host puts on the wire for a handler.
local function wireHandler(handler)
  if handler == true or type(handler) == "function" then
    return true
  end
  local wire = {}
  local sendsToApp = false
  for _, item in handler do
    if type(item) == "function" then
      sendsToApp = true
    else
      table.insert(wire, item)
    end
  end
  if sendsToApp then
    table.insert(wire, { ["do"] = "emit" })
  end
  return wire
end

local function wireNode(node)
  local copy = { type = node.type, key = node.key, props = node.props }
  if node.on then
    copy.on = {}
    for event, handler in node.on do
      copy.on[event] = wireHandler(handler)
    end
  end
  if node.children then
    copy.children = {}
    for _, child in node.children do
      table.insert(copy.children, wireNode(child))
    end
  end
  if node.slots then
    copy.slots = {}
    for name, list in node.slots do
      copy.slots[name] = {}
      for _, child in list do
        table.insert(copy.slots[name], wireNode(child))
      end
    end
  end
  return copy
end

local dir = ...
require(dir .. "/app")
local tree, clientDecls, sources = app.__render("main")

local decls = {}
for name, decl in clientDecls do
  decls[name] = { init = decl.init, persist = decl.persist, sync = decl.sync }
end
local sourceDecls = {}
for id, source in sources do
  sourceDecls[id] = {
    kind = source.kind,
    key = source.key,
    page = source.page,
    params = object(source.params),
  }
end

local out = {}
encode({
  tree = wireNode(tree),
  client = object(decls),
  sources = object(sourceDecls),
  manifest = app.__stub.manifest,
  tools = #app.__stub.tools,
}, out)
print(table.concat(out))
