--!strict
-- The image recipes, in the app's database: each the name an image is built as (`tools:1`) and the Dockerfile it is
-- built from. The Dockerfile is the whole build context: what the image needs, it downloads. A recipe keeps what its
-- last build left: the id of the image the last build that worked made (`image`, with `built`, a hash of the name and
-- the Dockerfile it was built from, so a recipe changed since says so), and why the last one failed (`problem`).
--
-- A build runs as a task of its own, one at a time per recipe, and the request that starts it answers at once. How it
-- goes reaches the open windows as `_building` notifications, a line at a time; the last says it is done, or why it
-- failed. Building again moves the name to the new image, as `docker build -t` does: the image it had before keeps
-- what it was (with no name, unless it has another), for the person to prune. Removing a recipe leaves its image.
local docker = require("vos:docker")
local hash = require("lib:hash")
local hex = require("lib:hex")
local pack = require("./pack")
local random = require("lib:random")
local spec = require("./spec")
local task = require("std:task")
local time = require("std:time")
local db = app.db

export type Recipe = containers_Recipe

local recipes = {}

local COLUMNS = "id, tag, dockerfile, image, built, problem"
-- how often the windows are told how a pull the build makes goes, at most, in milliseconds
local PROGRESS_EVERY = 1000
-- the labels every image a recipe builds carries: the app's, and the recipe's id
local OWNER = "vos.app"
local RECIPE = "vos.recipe"

-- The recipes being built.
local building: { [string]: boolean } = {}

-- What the image of a recipe was built from: its name and its Dockerfile.
local function fingerprint(tag: string, dockerfile: string): string
  return hex.encode(hash.hash("sha256", `{tag}\n{dockerfile}`))
end

local function recipeOf(row: { [string]: any }): Recipe
  return {
    id = row.id,
    tag = row.tag,
    dockerfile = row.dockerfile,
    image = row.image,
    current = row.image ~= "" and row.built == fingerprint(row.tag, row.dockerfile),
    problem = row.problem,
    building = building[row.id] == true,
  }
end

-- The recipes, by the name of their image.
function recipes.all(): { Recipe }
  local list: { Recipe } = {}
  for _, row in db:all(`select {COLUMNS} from recipes order by tag, id`) do
    table.insert(list, recipeOf(row))
  end
  return list
end

function recipes.find(id: string): Recipe?
  local row = db:one(`select {COLUMNS} from recipes where id = :id`, { id = id })
  return if row then recipeOf(row) else nil
end

-- The name and the Dockerfile as they are kept, or what is wrong with them in the person's words. `except` is the
-- recipe being changed, whose own name is no other recipe's.
local function checked(tag: string, dockerfile: string, except: string?): (string?, string?)
  local name = spec.trim(tag)
  if name == "" then
    return nil, "Say what the image is called, like tools:1."
  end
  if string.find(name, "@", 1, true) then
    return nil, `{name} names a digest: a build gives an image a name and a tag, like tools:1.`
  end
  if not spec.isImage(name) then
    return nil, `{name} is not the name of an image. An image is named like tools, tools:1 or ghcr.io/owner/tools:1.`
  end
  name = spec.tagged(name)
  local taken =
    db:one("select 1 as found from recipes where tag = :tag and id <> :id", { tag = name, id = except or "" })
  if taken then
    return nil, `Another recipe builds {name} already.`
  end
  if spec.trim(dockerfile) == "" then
    return nil, "Write the Dockerfile the image is built from."
  end
  return name, nil
end

-- Add a recipe. Returns the problem, when there is one, in the person's words.
function recipes.add(tag: string, dockerfile: string): string?
  local name, problem = checked(tag, dockerfile, nil)
  if not name then
    return problem
  end
  db:exec(
    "insert into recipes(id, tag, dockerfile) values (:id, :tag, :dockerfile)",
    { id = random.uuid_v4(), tag = name, dockerfile = dockerfile }
  )
  return nil
end

-- Change a recipe's name or Dockerfile. What its last build left stays: a build that is under way builds what it was
-- given.
function recipes.update(recipe: Recipe, tag: string, dockerfile: string): string?
  local name, problem = checked(tag, dockerfile, recipe.id)
  if not name then
    return problem
  end
  db:exec(
    "update recipes set tag = :tag, dockerfile = :dockerfile where id = :id",
    { id = recipe.id, tag = name, dockerfile = dockerfile }
  )
  return nil
end

-- Remove a recipe. The images it built stay: they are Docker's, removed in the list of images.
function recipes.remove(recipe: Recipe)
  db:exec("delete from recipes where id = :id", { id = recipe.id })
end

local function tell(id: string, line: string, status: containers_BuildStatus)
  RPC.send_notification("_building", { recipe = id, line = line, status = status })
end

-- The build itself, telling the windows how it goes. Answers with the id of the image it made, or why it failed.
local function make(recipe: Recipe): (string?, string?)
  local api, problem = docker.puller()
  if not api then
    return nil, problem or "Docker cannot be reached."
  end
  local context, unpacked = pack.tar { { path = "Dockerfile", text = recipe.dockerfile } }
  if not context then
    return nil, unpacked or "The Dockerfile could not be packed."
  end
  tell(recipe.id, `Building {recipe.tag}`, "running")
  local labels = { [OWNER] = app.id, [RECIPE] = recipe.id }
  local stream = api:build_image(context, { t = recipe.tag, labels = labels, rm = true, forcerm = true })
  local told: typeof(time.now())? = nil
  local made: string? = nil
  -- the build runs as its progress is read: a step that fails is the error of a `next()`
  while true do
    local step, err = stream:next()
    if err then
      return nil, `The image could not be built: {docker.sentence(err)}`
    end
    if not step then
      break
    end
    if step.stream then
      for line in string.gmatch(step.stream, "[^\r\n]+") do
        tell(recipe.id, line, "running")
      end
    elseif step.status then
      -- a pull the build makes: each status is told, its progress (every few kilobytes) now and then
      local detail = step.progressDetail
      local moving = detail ~= nil and detail.total ~= nil
      if not moving or told == nil or told:elapsed():as_millis() >= PROGRESS_EVERY then
        told = if moving then time.now() else told
        tell(recipe.id, if step.id then `{step.id}: {step.status}` else step.status, "running")
      end
    end
    if step.aux and step.aux.ID then
      made = step.aux.ID
    end
  end
  if made then
    return made, nil
  end
  -- a builder that does not say which image it made: the name says
  local found, err = api:list_images { filters = { reference = { recipe.tag } } }
  local first = found and found[1]
  if not first then
    return nil, if err then docker.sentence(err) else `The build did not make {recipe.tag}.`
  end
  return first.Id, nil
end

-- Start a build of the recipe as it is now, and return at once. A build of it that is under way goes on, and no second
-- one starts. Once it is over, what it made, or why it failed, is kept before the windows hear that it is over.
function recipes.build(recipe: Recipe)
  if building[recipe.id] then
    return
  end
  building[recipe.id] = true
  task.defer(function()
    local ok, made, failed = pcall(make, recipe)
    local problem: string? = if ok then failed else `The build stopped: {tostring(made)}`
    if ok and made then
      db:exec(
        "update recipes set image = :image, built = :built, problem = '' where id = :id",
        { image = made, built = fingerprint(recipe.tag, recipe.dockerfile), id = recipe.id }
      )
    else
      problem = problem or "The image was not built."
      db:exec("update recipes set problem = :problem where id = :id", { problem = problem, id = recipe.id })
    end
    building[recipe.id] = nil
    if problem then
      tell(recipe.id, problem, "failed")
    else
      tell(recipe.id, `Built {recipe.tag}.`, "done")
    end
  end)
end

return recipes
