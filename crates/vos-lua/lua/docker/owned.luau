--!strict
-- One container an app owns: the one thing the app has in Docker. The app declares what it is made from (its image,
-- what it runs, its environment, the ports it publishes, the folders it may read) and `apply` has Docker hold exactly
-- that. The container is never changed: a change of the declaration is the container removed and made again, which
-- takes a moment and loses nothing, because such a container keeps nothing. So it is always what its label says it
-- was made from, and "is it up to date" is one comparison.
--
-- The container is found by a label that carries the app's identity (`vos.app=<app.id>`) and then by its name: a
-- container of that name that is not the app's is never touched (Docker refuses the name, and the app says so), and
-- an app that owns several keeps them apart by their names.
--
-- A function that does something answers with a plain sentence when it could not, and with nothing when it did.
local app = require("vos:app")
local task = require("std:task")
local time = require("std:time")
local declaration = require("./declaration")
local engine = require("./engine")

local owned = {}

-- the label that says a container is an app's, and the one that says what it was made from
local OWNER = "vos.app"
local SIGNATURE = "vos.signature"
-- what changes how the container is doing; the rest of what Docker reports does not
local EVENTS = { "create", "start", "die", "destroy", "pause", "unpause", "restart" }
-- How long an `apply` that says nothing is waited for, in seconds: more than Docker is given to act
-- (`engine.actor`). A pull says it goes on with every step of its progress.
local BUSY_LIMIT = 180
export type Declaration = declaration.Declaration
-- How the container is doing. `current`: it was made from the declaration it is compared with. `problem`: why it
-- could not be made or started the last time that was tried; empty when it could.
export type Status = vos_docker_ContainerStatus
-- The container: its filters (Docker's, for what happens to the app's containers, for `engine.watch`) and what is
-- asked of it.
export type Container = vos_docker_Container

-- The container named `name` (a new one is given that name) that is this app's.
function owned.container(name: string): Container
  local filter = { `{OWNER}={app.id}` }
  -- why the container could not be made or started the last time it was tried; empty when it could
  local problem = ""

  -- Since when one `apply` is at work: a second waits, so that two never make the same container at once. It is a
  -- moment and not a flag because a call the host dropped half way (its caller gave up) never gets to say it is
  -- done: the next then waits no longer than a call can take.
  local since: typeof(time.now())? = nil
  -- which `apply` holds it: one that was taken over does not release it for the one that took over
  local turn = 0

  -- this app's containers of this name, as Docker lists them: one, or none
  local function list(api: engine.Client): ({ vnd_bollard_ContainerSummary }?, string?)
    local found, err = api:list_containers { all = true, filters = { label = filter } }
    if not found then
      return nil, engine.sentence(err)
    end
    local named: { vnd_bollard_ContainerSummary } = {}
    for _, summary in found do
      if summary.Names and table.find(summary.Names, `/{name}`) then
        table.insert(named, summary)
      end
    end
    return named, nil
  end

  local function removeAll(api: engine.Client): string?
    local found, unlisted = list(api)
    if not found then
      return unlisted
    end
    local failure: string? = nil
    for _, summary in found do
      if summary.Id then
        local _, err = api:remove_container(summary.Id, { force = true })
        failure = failure or (if err then engine.sentence(err) else nil)
      end
    end
    return failure
  end

  -- have the image, pulling it when Docker does not
  local function pulled(api: engine.Client, image: string): string?
    local images, err = api:list_images { filters = { reference = { image } } }
    if not images then
      return engine.sentence(err)
    end
    if #images > 0 then
      return nil
    end
    local puller, unreachable = engine.puller()
    if not puller then
      return unreachable
    end
    local pull = puller:pull_image(image)
    -- the pull runs as its progress is read: an image that cannot be pulled is the error of a step
    while true do
      local step, step_err = pull:next()
      -- the pull goes on: whoever waits for this `apply` waits on
      since = time.now()
      if step_err then
        return `The image {image} could not be pulled: {engine.sentence(step_err)}`
      end
      if not step then
        return nil
      end
    end
  end

  -- Whether the one container there is was made from `signature` and runs: there is nothing to make then.
  local function standing(api: engine.Client, signature: string): boolean
    local found = list(api)
    local only = if found and #found == 1 then found[1] else nil
    return only ~= nil and only.State == "running" and (only.Labels or {})[SIGNATURE] == signature
  end

  local function make(api: engine.Client, declared: Declaration, always: boolean): string?
    local wrong = declaration.refused(declared)
    if wrong then
      return wrong
    end
    local signature = declaration.signature(declared)
    if not always and standing(api, signature) then
      return nil
    end
    local missing = pulled(api, declared.image)
    if missing then
      return missing
    end
    local stale = removeAll(api)
    if stale then
      return stale
    end
    local body = declaration.body(declared, { [OWNER] = app.id, [SIGNATURE] = signature })
    local made, err = api:create_container(body, { name = name })
    if not made then
      return engine.sentence(err)
    end
    local _, start_err = api:start_container(made.Id)
    return if start_err then engine.sentence(start_err) else nil
  end

  local container = {} :: Container
  container.filters = { type = { "container" }, label = filter, event = EVENTS }

  -- How the container is doing, and whether it is what `declaration` says (never, for none). With Docker away there
  -- is nothing to say of it: it is absent.
  function container.status(_self: Container, declared: Declaration?): Status
    local status: Status = { state = "absent", status = "", current = false, problem = problem }
    local api = engine.reader()
    local found = if api then list(api) else nil
    local first = if found then found[1] else nil
    if not first then
      return status
    end
    status.state = if first.State == "running"
      then "running"
      elseif first.State == "restarting" then "restarting"
      else "stopped"
    status.status = if type(first.Status) == "string" then first.Status else ""
    status.current = declared ~= nil
      and declaration.refused(declared) == nil
      and (first.Labels or {})[SIGNATURE] == declaration.signature(declared)
    return status
  end

  -- Have the container be what `declaration` says: made and started; removed for none. A container that is that
  -- already, and runs, is left running, unless `always`. What Docker refused is kept, for `status` to say, and
  -- answered.
  function container.apply(_self: Container, declared: Declaration?, always: boolean?): string?
    while since and since:elapsed():as_secs() < BUSY_LIMIT do
      task.wait(0.05)
    end
    turn += 1
    local mine = turn
    since = time.now()
    -- asked for only now: another Docker may have been chosen while this waited
    local api, unreachable = engine.actor()
    local ran, answer = true, unreachable
    if api then
      ran, answer = pcall(function(): string?
        return if declared then make(api, declared, always == true) else removeAll(api)
      end)
    end
    local failure: string? = if ran then answer else "The container could not be made."
    -- an `apply` that another took over says nothing of the container as it now is
    if turn == mine then
      since = nil
      -- a container that is not wanted has no problem to show, whether or not Docker could remove it
      problem = if declared then failure or "" else ""
    end
    return failure
  end

  -- The end of what the container wrote (`engine.logs`). Nothing, and no reason, when there is no container.
  function container.logs(_self: Container, tail: number?): (engine.Logs?, string?)
    local api, unreachable = engine.reader()
    if not api then
      return nil, unreachable
    end
    local found, unlisted = list(api)
    if not found then
      return nil, unlisted
    end
    local id = found[1] and found[1].Id
    if not id then
      return nil, nil
    end
    return engine.logs(id, tail)
  end

  return container
end

return owned
