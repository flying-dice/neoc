--!strict
-- nginx, as this app uses it: the web server inside the container. Its whole configuration is one text, which the
-- container is given in its environment and writes down before it starts nginx, so nothing has to be written on the
-- machine or into the container: the container is what its configuration, its ports and its folders say, and a
-- change of any is the container made again.
--
-- Nothing here talks to Docker or keeps anything: this is where a hostname is checked, and where the configuration
-- is written.
local hash = require("lib:hash")
local hex = require("lib:hex")
local rpc = require("../.app/rpc")
local trim = require("lib:text").trim

local nginx = {}

-- The image: the official nginx on Alpine. It is named by version so that every machine runs the same server.
nginx.IMAGE = "nginx:1.30.0-alpine"
-- the variable of the container's environment the configuration travels in
nginx.CONFIG = "VOS_NGINX_CONF"
-- The program, whatever the image starts by default: the configuration is written where nginx reads it, and nginx
-- stays in front as a container's process must.
nginx.ENTRYPOINT = {
  "/bin/sh",
  "-c",
  `printf '%s' "${nginx.CONFIG}" > /etc/nginx/nginx.conf && exec nginx -g 'daemon off;'`,
}

-- a folder of the machine, and where the container sees it
export type Mount = { source: string, target: string }
-- What the container is made from: nginx's configuration, the ports it listens on (each published under its own
-- number), and the folders it reads.
export type Plan = { config: string, ports: { number }, mounts: { Mount } }

-- A hostname as it is kept: lower case, without the dot at its end. The second result says what is wrong with one
-- that is not a name. Only letters, digits, hyphens and dots pass, so a name cannot say anything else to nginx.
function nginx.hostname(raw: string): (string?, string?)
  local text = string.lower((string.gsub(trim(raw), "%.$", "")))
  if text == "" then
    return nil, "A hostname is blank."
  elseif #text > 253 then
    return nil, "A hostname is longer than 253 characters."
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
        `{text} is not a hostname: letters, digits and hyphens, in parts of up to 63 joined by dots, like blog.home. A part does not start or end with a hyphen.`
    end
  end
  return text, nil
end

-- where the container sees a site's folder
local function root(site: rpc.Site): string
  return `/sites/{site.id}`
end

-- One of nginx's servers for a site: its files as they are, `index.html` for a folder, and the site's own
-- `404.html` (nginx's when it has none) for what is not there.
local function server(listen: string, names: { string }, site: rpc.Site): string
  local lines = { "  server {", `    listen {listen};` }
  if #names > 0 then
    table.insert(lines, `    server_name {table.concat(names, " ")};`)
  end
  table.insert(lines, `    root {root(site)};`)
  table.insert(lines, "    index index.html index.htm;")
  table.insert(lines, "    error_page 404 /404.html;")
  table.insert(lines, "    location / { try_files $uri $uri/ =404; }")
  table.insert(lines, "  }")
  return table.concat(lines, "\n")
end

-- What the container is made from, for `sites`: the ones to serve, in the order they are written. A site with a
-- port is the only server of that port, whatever name it is asked for; the sites with names share `settings.port`,
-- where a name that is nobody's is answered with 404 rather than with the first site.
function nginx.plan(settings: rpc.Settings, sites: { rpc.Site }): Plan
  local servers: { string } = {}
  local ports: { number } = {}
  local mounts: { Mount } = {}
  local named = false
  for _, site in sites do
    table.insert(mounts, { source = site.folder, target = root(site) })
    if site.port > 0 then
      table.insert(ports, site.port)
      table.insert(servers, server(tostring(site.port), {}, site))
    end
    if #site.hostnames > 0 then
      named = true
      table.insert(servers, server(tostring(settings.port), site.hostnames, site))
    end
  end
  if named then
    table.insert(ports, settings.port)
    table.insert(servers, `  server \{\n    listen {settings.port} default_server;\n    return 404;\n  }`)
  end
  table.sort(ports)
  local config = table.concat({
    "worker_processes auto;",
    -- its log goes where Docker reads it
    "error_log /dev/stderr warn;",
    "events { worker_connections 1024; }",
    "http {",
    "  include /etc/nginx/mime.types;",
    "  default_type application/octet-stream;",
    "  sendfile on;",
    "  server_tokens off;",
    "  access_log /dev/stdout;",
    table.concat(servers, "\n"),
    "}",
    "",
  }, "\n")
  return { config = config, ports = ports, mounts = mounts }
end

-- What a container is made from, as one short word: two containers with the same signature are the same server.
function nginx.signature(plan: Plan): string
  local parts = { nginx.IMAGE, plan.config }
  for _, port in plan.ports do
    table.insert(parts, tostring(port))
  end
  for _, mount in plan.mounts do
    table.insert(parts, `{mount.source}\0{mount.target}`)
  end
  local digest = hash.hash("sha256", table.concat(parts, "\n"))
  return hex.encode(digest or "")
end

return nginx
