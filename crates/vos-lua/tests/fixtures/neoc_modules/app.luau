--!strict
-- Requires the host's neoc modules (std:, lib:, vnd:) next to a relative module and `vos:rpc`; the test
-- builds it through the product's pipeline (darklua), which must leave all of those requires in the bundle.
local RPC = require("vos:rpc")
local fs = require("std:fs")
local json = require("lib:json")
local base64 = require("lib:base64")
local yaml = require("vnd:serde_yaml")
local helpers = require("./helpers")

RPC.on_request("probe", function()
  local object = json.object()
  object:set("n", 6)
  return {
    encoded = helpers.encode("hi"),
    same_module = base64 == require("lib:base64"),
    n = object:get("n"),
    has_fs = type(fs.read) == "function",
    yaml = type(yaml) == "table",
  }
end)
