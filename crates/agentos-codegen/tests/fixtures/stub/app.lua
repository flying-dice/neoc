-- A stand-in for the host's native `@agentos/app` module, just enough to load an app and render a window:
-- the codegen contract test runs the Calculator and Notes examples against the real generated `ui.lua` with it,
-- then validates the tree in Rust. It mimics what the host does (refs for client state, source handles, the
-- row proxy is in ui.lua itself); the real implementation lives in agentos-luau.
local stub = { windows = {}, sources = {}, tools = {}, store = {}, manifest = nil }

local function isDollar(key)
  return type(key) == "string" and string.sub(key, 1, 1) == "$"
end

-- A table that answers every field with a ref (`w.client.search` -> {"$ref": "client.search"}).
local function refs(prefix, own)
  return setmetatable(own or {}, {
    __index = function(_, key)
      if isDollar(key) then
        return nil
      end
      return { ["$ref"] = prefix .. key }
    end,
  })
end

local function handle(kind, id)
  return refs("src." .. id .. ".", {
    ["$source"] = id,
    bind = function(_, field)
      return { ["$ref"] = "src." .. id .. "." .. field }
    end,
  })
end

local count = 0
local function declare(kind, options, params)
  count = count + 1
  local id = "s" .. count
  stub.sources[id] = { kind = kind, key = options and options.key, page = options and options.page, params = params }
  return handle(kind, id)
end

local db = {}
function db.migrate() end
function db.exec()
  return { changes = 1, last_insert_id = 1 }
end
function db.all()
  return {}
end
function db.one()
  return nil
end
function db.tx(_, body)
  body()
end
function db.query(_, _, options)
  return function(params)
    return declare("rows", options, params or {})
  end
end
function db.row(_, _, options)
  return function(params)
    return declare("row", options, params or {})
  end
end
function db.scalar(_, _)
  return function(params)
    return declare("value", nil, params or {})
  end
end

local app = {
  db = db,
  kv = {
    get = function(_, key)
      return stub.store[key]
    end,
    set = function(_, key, value)
      stub.store[key] = value
    end,
    delete = function(_, key)
      stub.store[key] = nil
    end,
  },
  t = setmetatable({}, {
    __index = function(_, kind)
      return function(options)
        return { __schema = kind, options = options }
      end
    end,
  }),
}

function app.manifest(manifest)
  stub.manifest = manifest
end
function app.state(initial)
  return initial
end
function app.window(name, spec)
  stub.windows[name] = spec
end
function app.tool(def)
  table.insert(stub.tools, def)
end
function app.openWindow() end
function app.windows()
  return {}
end
function app.notify() end
function app.log() end

-- Render one window instance and return its tree, client declarations and source declarations.
function app.__render(name)
  local spec = stub.windows[name]
  assert(spec, "no window " .. name)
  local decls = spec.client or {}
  local client = refs("client.", {
    get = function(_, key)
      return decls[key] and decls[key].init
    end,
    set = function() end,
  })
  local w = {
    id = "w1",
    params = {},
    state = {},
    client = client,
    env = refs("env."),
    toast = function() end,
    focus = function() end,
    scrollTo = function() end,
    announce = function() end,
    copy = function() end,
    close = function() end,
    openWindow = function() end,
  }
  local tree = spec.render(w)
  tree.key = name
  return tree, decls, stub.sources
end

app.__stub = stub
return app
