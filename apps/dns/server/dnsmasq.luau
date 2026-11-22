--!strict
-- dnsmasq, as this app uses it: the DNS server inside the container. It takes its whole configuration on its command
-- line, so the settings and the records become the container's arguments and nothing has to be written into it: the
-- container is what its arguments say, and a change of either is the container made again.
--
-- Nothing here talks to Docker or keeps anything: this is where a name and an address are checked, and where the
-- arguments are written.
local hash = require("lib:hash")
local hex = require("lib:hex")
local rpc = require("../.app/rpc")

local dnsmasq = {}

-- The image: Alpine's dnsmasq package and nothing else. It is named by version so that every machine runs the same
-- server; its Dockerfile exposes 53 (UDP and TCP), which is what lets Docker publish them.
dnsmasq.IMAGE = "4km3/dnsmasq:2.90-r3-alpine-3.22"
-- The program, whatever the image starts by default.
dnsmasq.ENTRYPOINT = { "dnsmasq" }
-- the port dnsmasq answers on inside the container
dnsmasq.PORT = 53

local function isV4(text: string): boolean
  local parts = { string.match(text, "^(%d%d?%d?)%.(%d%d?%d?)%.(%d%d?%d?)%.(%d%d?%d?)$") }
  if #parts ~= 4 then
    return false
  end
  for _, part in parts do
    if (tonumber(part) :: number) > 255 then
      return false
    end
  end
  return true
end

-- how many groups of one to four hex digits `text` is, joined by colons; nil when it is something else
local function groups(text: string): number?
  if text == "" then
    return 0
  end
  local count = 0
  for group in string.gmatch(text .. ":", "([^:]*):") do
    if not string.match(group, "^%x%x?%x?%x?$") then
      return nil
    end
    count += 1
  end
  return count
end

local function isV6(text: string): boolean
  if string.find(text, "[^%x:]") then
    return false
  end
  local gap = string.find(text, "::", 1, true)
  if not gap then
    return groups(text) == 8
  end
  local before, after = groups(string.sub(text, 1, gap - 1)), groups(string.sub(text, gap + 2))
  -- `::` stands for at least one group of zeros
  return before ~= nil and after ~= nil and before + after <= 7
end

-- An IP address as it is kept: IPv4, or IPv6 in lower case. The second result says what is wrong with one that is
-- not. Only digits, hex letters, dots and colons pass, so an address cannot say anything else to dnsmasq.
function dnsmasq.address(raw: string): (string?, string?)
  local text = string.lower(string.match(raw, "^%s*(.-)%s*$") :: string)
  if isV4(text) or isV6(text) then
    return text, nil
  end
  return nil, `{if text == "" then "That" else text} is not an IP address, like 192.168.1.10 or fd00::10.`
end

-- A name as it is kept: lower case, without the dot at its end. The second result says what is wrong with one that
-- is not a name. Only letters, digits, hyphens and dots pass, so a name cannot say anything else to dnsmasq.
function dnsmasq.name(raw: string): (string?, string?)
  local text = string.lower(string.match(raw, "^%s*(.-)%.?%s*$") :: string)
  if text == "" then
    return nil, "The name is blank."
  elseif #text > 253 then
    return nil, "The name is longer than 253 characters."
  elseif isV4(text) then
    return nil, "The name is an address: it goes in the address field."
  end
  for label in string.gmatch(text .. ".", "([^.]*)%.") do
    if
      #label == 0
      or #label > 63
      or string.find(label, "[^a-z0-9-]")
      or string.sub(label, 1, 1) == "-"
      or string.sub(label, -1) == "-"
    then
      return nil,
        "A name is letters, digits and hyphens, in parts of up to 63 joined by dots, like nas.home. A part does not start or end with a hyphen."
    end
  end
  return text, nil
end

-- What dnsmasq is started with. It reads no file (not the image's own, not the container's resolv.conf or hosts),
-- stays in front as a container's process must, and writes its log where Docker reads it.
function dnsmasq.arguments(settings: rpc.Settings, records: { rpc.Record }): { string }
  local arguments = {
    "--keep-in-foreground",
    "--conf-file=/dev/null",
    "--no-resolv",
    "--no-hosts",
    "--log-facility=-",
    "--cache-size=1000",
    -- what never belongs to the internet is not asked of it: a name without a dot, an address of a private network
    "--domain-needed",
    "--bogus-priv",
  }
  if settings.logQueries then
    table.insert(arguments, "--log-queries")
  end
  for _, upstream in settings.upstreams do
    table.insert(arguments, `--server={upstream}`)
  end
  for _, record in records do
    table.insert(
      arguments,
      if record.subdomains
        then `--address=/{record.name}/{record.address}`
        else `--host-record={record.name},{record.address}`
    )
  end
  return arguments
end

-- What a container is made from, as one short word: two containers with the same signature are the same server.
function dnsmasq.signature(port: number, arguments: { string }): string
  local digest = hash.hash("sha256", table.concat({ dnsmasq.IMAGE, tostring(port), table.unpack(arguments) }, "\n"))
  return hex.encode(digest or "")
end

return dnsmasq
