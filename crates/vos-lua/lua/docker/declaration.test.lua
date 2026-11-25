-- What an app declares its container with: what is refused before Docker hears of it, what Docker is asked for, and
-- when two declarations are the same container. Run by scripts/docker-luau.test.ts (neoc runs it, bundled; the module needs nothing of vos).
local _t = require("lib:test")
local describe = _t.describe
local test = _t.test
local expect = _t.expect

local declaration = require("./declaration")

local function nginx(): any
  return {
    image = "nginx:1.30.0-alpine",
    entrypoint = { "/bin/sh", "-c", "exec nginx" },
    environment = { "CONF=worker_processes auto;" },
    ports = { { host = 8081, container = 80 }, { host = 5353, container = 53, udp = true } },
    mounts = { { source = "/srv/sites/blog", target = "/sites/1" } },
  }
end

describe("a declaration", function()
  test("that says what the container is made from is taken", function()
    expect(declaration.refused(nginx())):toBeNil()
    expect(declaration.refused { image = "nginx" }):toBeNil()
  end)

  test("is refused with a sentence, never an error, whatever it is given", function()
    local wrong: { any } = {
      "nginx",
      {},
      { image = "" },
      { image = 7 },
      { image = "nginx latest" },
      { image = "nginx\0" },
      { image = "nginx", command = "serve" },
      { image = "nginx", command = { 1 } },
      { image = "nginx", command = { "a\0b" } },
      { image = "nginx", entrypoint = { {} } },
      { image = "nginx", environment = { "NO_VALUE" } },
      { image = "nginx", environment = { "=x" } },
      { image = "nginx", ports = "80" },
      { image = "nginx", ports = { 80 } },
      { image = "nginx", ports = { { host = 0, container = 80 } } },
      { image = "nginx", ports = { { host = 80, container = 65536 } } },
      { image = "nginx", ports = { { host = 80.5, container = 80 } } },
      { image = "nginx", ports = { { host = "80", container = 80 } } },
      { image = "nginx", ports = { { host = 80, container = 80, udp = "yes" } } },
      { image = "nginx", ports = { { host = 0 / 0, container = 80 } } },
      { image = "nginx", mounts = { "/srv" } },
      { image = "nginx", mounts = { { source = "/srv" } } },
      { image = "nginx", mounts = { { source = "data", target = "/data" } } },
      { image = "nginx", mounts = { { source = "../srv", target = "/data" } } },
      { image = "nginx", mounts = { { source = "/srv", target = "data" } } },
      { image = "nginx", mounts = { { source = "/srv\0", target = "/data" } } },
      { image = "nginx", mounts = { { source = 1, target = "/data" } } },
    }
    for index, declared in wrong do
      local ok, sentence = pcall(declaration.refused, declared)
      expect(ok):toBe(true)
      if type(sentence) ~= "string" or sentence == "" then
        error(`declaration {index} was taken`)
      end
    end
  end)

  test("more than a container is declared with is refused", function()
    local words = table.create(5000, "x")
    expect(type(declaration.refused { image = "nginx", command = words })):toBe("string")
    local ports = table.create(300, { host = 80, container = 80 })
    expect(type(declaration.refused { image = "nginx", ports = ports })):toBe("string")
  end)

  test("a list with a key that is no place in it is refused", function()
    local environment: any = { "A=b", extra = "X=y" }
    expect(type(declaration.refused { image = "nginx", environment = environment })):toBe("string")
    local ports: any = { { host = 80, container = 80 }, more = { host = 81, container = 81 } }
    expect(type(declaration.refused { image = "nginx", ports = ports })):toBe("string")
  end)
end)

describe("what Docker is asked for", function()
  test("is what was declared, under the name and the labels given", function()
    local body = declaration.body(nginx(), "vos-pages", { ["vos.app"] = "pages" })
    expect(body.name):toBe("vos-pages")
    expect(body.Image):toBe("nginx:1.30.0-alpine")
    expect(body.Entrypoint[3]):toBe("exec nginx")
    expect(body.Cmd):toBeNil()
    expect(body.Env[1]):toBe("CONF=worker_processes auto;")
    expect(body.Labels["vos.app"]):toBe("pages")
    expect(body.HostConfig.RestartPolicy.Name):toBe("unless-stopped")
  end)

  test("publishes the declared ports and no other, TCP unless UDP is said", function()
    local bindings = declaration.body(nginx(), "c", {}).HostConfig.PortBindings
    local keys = {}
    for key in bindings do
      table.insert(keys, key)
    end
    table.sort(keys)
    expect(table.concat(keys, " ")):toBe("53/udp 80/tcp")
    expect(bindings["80/tcp"][1].HostPort):toBe("8081")
    expect(bindings["53/udp"][1].HostPort):toBe("5353")
  end)

  test("gives the declared folders and no other, each only to read", function()
    local host = declaration.body(nginx(), "c", {}).HostConfig
    expect(#host.Mounts):toBe(1)
    expect(host.Mounts[1].Type):toBe("bind")
    expect(host.Mounts[1].Source):toBe("/srv/sites/blog")
    expect(host.Mounts[1].Target):toBe("/sites/1")
    expect(host.Mounts[1].ReadOnly):toBe(true)
    -- nothing else of the machine is asked for
    for key in host do
      if key ~= "PortBindings" and key ~= "Mounts" and key ~= "RestartPolicy" then
        error(`the container was given {key}`)
      end
    end
    expect(#declaration.body({ image = "nginx" }, "c", {}).HostConfig.Mounts):toBe(0)
  end)
end)

describe("a signature", function()
  test("is the same for the same declaration, whatever order its ports are written in", function()
    local one, other = nginx(), nginx()
    other.ports = { other.ports[2], other.ports[1] }
    expect(declaration.signature(one)):toBe(declaration.signature(nginx()))
    expect(declaration.signature(one)):toBe(declaration.signature(other))
    expect(#declaration.signature(one)):toBe(64)
  end)

  test("is another for any change of what the container is made from", function()
    local changes: { (any) -> () } = {
      function(d)
        d.image = "nginx:1.31.0-alpine"
      end,
      function(d)
        d.entrypoint[3] = "exec nginx -q"
      end,
      function(d)
        d.command = { "serve" }
      end,
      function(d)
        d.environment[1] = "CONF=worker_processes 1;"
      end,
      function(d)
        d.ports[1].host = 8082
      end,
      function(d)
        d.ports[2].udp = false
      end,
      function(d)
        table.remove(d.ports)
      end,
      function(d)
        d.mounts[1].source = "/srv/sites/wiki"
      end,
      function(d)
        d.mounts[1].target = "/sites/2"
      end,
      function(d)
        d.mounts = nil
      end,
    }
    local base = declaration.signature(nginx())
    for index, change in changes do
      local changed = nginx()
      change(changed)
      if declaration.signature(changed) == base then
        error(`change {index} left the signature as it was`)
      end
    end
  end)

  test("does not take two words for one", function()
    local joined = declaration.signature { image = "i", command = { "a b" } }
    local apart = declaration.signature { image = "i", command = { "a", "b" } }
    expect(joined == apart):toBe(false)
    local moved = declaration.signature { image = "i", entrypoint = { "a" }, command = {} }
    expect(moved == declaration.signature { image = "i", entrypoint = {}, command = { "a" } }):toBe(false)
  end)
end)
