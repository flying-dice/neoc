-- Requires the host's neoc modules (std:, lib:, vnd:) next to a relative module and `@agentos/app`; the test
-- builds it through the product's pipeline (darklua), which must leave all of those requires in the bundle.
local app = require("@agentos/app")
local fs = require("std:fs")
local json = require("lib:json")
local base64 = require("lib:base64")
local yaml = require("vnd:serde_yaml")
local helpers = require("./helpers")

app.manifest {
  id = "neoc-modules",
  name = "Neoc modules",
  icon = "puzzlepiece",
  tint = "blue",
  window = { w = 320, h = 240, minW = 320, minH = 240 },
}

app.tool {
  name = "probe",
  readOnly = true,
  run = function()
    local object = json.object()
    object:set("n", 6)
    return {
      encoded = helpers.encode("hi"),
      same_module = base64 == require("lib:base64"),
      n = object:get("n"),
      has_fs = type(fs.read) == "function",
      yaml = type(yaml) == "table",
    }
  end,
}
