--!strict
-- `vos:docker`: the container framework an app that runs on Docker requires, over `vnd:bollard`. The host bundles
-- these modules into one chunk (`bun run codegen`: types/generated/docker.luau) and runs it in the app's own VM the
-- first time the app requires it, so what it keeps (the chosen Docker, its clients) is that app's alone.
--
-- Which Docker and how it is reached, why a call failed and the watch for changes are engine.luau; one container the
-- app owns, kept as its declaration says, is owned.luau.
local engine = require("./engine")
local owned = require("./owned")

-- held to what an app is told of it: the module's declared type (crates/vos-lua/src/docker.rs)
local docker: vos_docker = {
  host = engine.host,
  use = engine.use,
  status = engine.status,
  sentence = engine.sentence,
  reader = engine.reader,
  actor = engine.actor,
  puller = engine.puller,
  logs = engine.logs,
  watch = engine.watch,
  container = owned.container,
}

return docker
