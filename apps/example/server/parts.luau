--!strict
-- What the Example's sidebar lists: the parts a vos app is made of. A plain table, so a copy can replace it with its
-- own data (the app's database, a vnd: module, a web request) without touching the handler that serves it.
local rpc = require("../.app/rpc")

local PARTS: { rpc.Part } = {
  {
    id = "contract",
    title = "Contract",
    icon = "doc",
    summary = "openrpc.json names the app, its window and icon, and every method the page may call, with the shape of what goes in and comes out.",
    facts = {
      { label = "File", value = "openrpc.json" },
      { label = "App id", value = "example" },
      { label = "Methods", value = "parts, greet" },
      { label = "Generates", value = "The page's typed client and the backend's router" },
    },
  },
  {
    id = "backend",
    title = "Backend",
    icon = "terminal",
    summary = "Luau, run by the host: one handler per method. It answers the page, keeps the app's data and does what the browser may not.",
    facts = {
      { label = "Entry", value = "server/main.luau" },
      { label = "Language", value = "Luau (strict)" },
      { label = "Refusals", value = "ctx:fail(message), code -32010" },
    },
  },
  {
    id = "page",
    title = "Page",
    icon = "square.grid",
    summary = "A Vite page in a frame of the app's window, drawn with the kit's ui-* elements so it looks and behaves like the rest of vos.",
    facts = {
      { label = "Entry", value = "src/main.ts" },
      { label = "State", value = "src/lib/example.svelte.ts" },
      { label = "Layers", value = "molecules, organisms, pages" },
      { label = "Client", value = "app:rpc (generated)" },
    },
  },
}

local function all(): { rpc.Part }
  return PARTS
end

return { all = all }
