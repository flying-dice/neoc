--!strict
-- The projects, their issues and their pages, in the app's own database. Everything that changes them is here, and
-- says why when it cannot: a function that changes something answers with a plain sentence when it refused (the
-- handler and the tool pass it on as it is), and with nothing when it did what it was asked.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local trim = require("lib:text").trim
local unicode = require("lib:unicode")
local db = app.db

-- what can change on an issue: only what is given changes
export type IssueChanges = {
  title: string?,
  description: string?,
  type: rpc.IssueType?,
  priority: rpc.Priority?,
  status: rpc.Status?,
}

local store = {}

local NOW = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"

local ISSUE = [[
  select i.id, i.project_id, p.key || '-' || i.number as key, i.title, i.description, i.type, i.priority, i.status,
         i.updated
  from issues i join projects p on p.id = i.project_id
]]

local PAGE = "select id, project_id, parent_id, title, updated from pages"

db:migrate {
  [1] = [[
    create table projects (
      id integer primary key,
      key text not null unique,
      name text not null,
      color text not null,
      next_number integer not null default 1
    );
    create table issues (
      id integer primary key,
      project_id integer not null,
      number integer not null,
      title text not null,
      description text not null default '',
      type text not null default 'task',
      priority text not null default 'medium',
      status text not null default 'todo',
      updated text not null
    );
    create index issues_by_project on issues(project_id, id);
    create table pages (
      id integer primary key,
      project_id integer not null,
      parent_id integer not null default 0,
      title text not null,
      body text not null default '',
      updated text not null
    );
    create index pages_by_project on pages(project_id, parent_id);
  ]],
}

-- a key is what a project's issues are numbered under, as in WEB-12: two to five capitals or digits, a letter first
local function isKey(text: string): boolean
  return string.match(text, "^%u[%u%d]+$") ~= nil and #text >= 2 and #text <= 5
end

local function issueOf(row: { [string]: any }): rpc.Issue
  return {
    id = row.id,
    project = row.project_id,
    key = row.key,
    title = row.title,
    description = row.description,
    type = row.type,
    priority = row.priority,
    status = row.status,
    updated = row.updated,
  }
end

local function pageOf(row: { [string]: any }): rpc.Page
  return { id = row.id, project = row.project_id, parent = row.parent_id, title = row.title, updated = row.updated }
end

function store.projects(): { rpc.Project }
  local projects: { rpc.Project } = {}
  for _, row in db:all("select id, key, name, color from projects order by id") do
    table.insert(projects, { id = row.id, key = row.key, name = row.name, color = row.color })
  end
  return projects
end

-- the project called `name` or keyed `name`, whatever its capitals: how an agent names a project
function store.projectNamed(name: string): rpc.Project?
  local wanted = trim(name)
  for _, project in store.projects() do
    if unicode.caseless_eq(project.key, wanted) or unicode.caseless_eq(project.name, wanted) then
      return project
    end
  end
  return nil
end

-- the issues in the order they were added; of one project when `project` is given
function store.issues(project: number?): { rpc.Issue }
  local issues: { rpc.Issue } = {}
  local rows = if project
    then db:all(ISSUE .. " where i.project_id = :p order by i.id", { p = project })
    else db:all(ISSUE .. " order by i.id")
  for _, row in rows do
    table.insert(issues, issueOf(row))
  end
  return issues
end

function store.issue(id: number): rpc.Issue?
  local row = db:one(ISSUE .. " where i.id = :id", { id = id })
  return if row then issueOf(row) else nil
end

-- the issue called `key` (WEB-12), whatever its capitals: how an agent names an issue
function store.issueKeyed(key: string): rpc.Issue?
  local row = db:one(ISSUE .. " where upper(p.key || '-' || i.number) = upper(:k)", { k = trim(key) })
  return if row then issueOf(row) else nil
end

-- the pages by title; of one project when `project` is given
function store.pages(project: number?): { rpc.Page }
  local pages: { rpc.Page } = {}
  local rows = if project
    then db:all(PAGE .. " where project_id = :p order by title collate nocase, id", { p = project })
    else db:all(PAGE .. " order by title collate nocase, id")
  for _, row in rows do
    table.insert(pages, pageOf(row))
  end
  return pages
end

function store.page(id: number): rpc.Page?
  local row = db:one(PAGE .. " where id = :id", { id = id })
  return if row then pageOf(row) else nil
end

-- the page of a project called `title`, whatever its capitals: how an agent names a page
function store.pageTitled(project: number, title: string): rpc.Page?
  local row =
    db:one(PAGE .. " where project_id = :p and title = :t collate nocase order by id", { p = project, t = trim(title) })
  return if row then pageOf(row) else nil
end

function store.body(id: number): string?
  local row = db:one("select body from pages where id = :id", { id = id })
  return if row then row.body else nil
end

function store.workspace(): rpc.Workspace
  return { projects = store.projects(), issues = store.issues(), pages = store.pages() }
end

function store.addProject(name: string, key: string, color: rpc.Color): string?
  local named = trim(name)
  local keyed = string.upper(trim(key))
  if named == "" then
    return "A project needs a name."
  elseif not isKey(keyed) then
    return "A key is two to five letters or digits, a letter first, like WEB."
  elseif db:one("select id from projects where key = :k", { k = keyed }) then
    return `Another project has the key {keyed}.`
  end
  db:exec("insert into projects(key, name, color) values (:k, :n, :c)", { k = keyed, n = named, c = color })
  return nil
end

function store.removeProject(id: number): string?
  if not db:one("select id from projects where id = :id", { id = id }) then
    return "There is no such project."
  end
  db:tx(function()
    db:exec("delete from issues where project_id = :id", { id = id })
    db:exec("delete from pages where project_id = :id", { id = id })
    db:exec("delete from projects where id = :id", { id = id })
    return nil
  end)
  return nil
end

-- Add an issue: answers with its id, or with nothing and why not. It takes the next number of its project.
function store.addIssue(project: number, title: string, with: IssueChanges?): (number?, string?)
  local given: IssueChanges = with or {}
  local titled = trim(title)
  if titled == "" then
    return nil, "An issue needs a title."
  end
  local id: number? = nil
  db:tx(function()
    local row = db:one("select next_number from projects where id = :id", { id = project })
    if not row then
      return nil
    end
    db:exec("update projects set next_number = next_number + 1 where id = :id", { id = project })
    local made = db:exec(
      "insert into issues(project_id, number, title, description, type, priority, status, updated) "
        .. `values (:p, :n, :t, :d, :ty, :pr, :st, {NOW})`,
      {
        p = project,
        n = row.next_number,
        t = titled,
        d = given.description or "",
        ty = given.type or "task",
        pr = given.priority or "medium",
        st = given.status or "todo",
      }
    )
    id = made.last_insert_id
    return nil
  end)
  if id == nil then
    return nil, "There is no such project."
  end
  return id, nil
end

function store.updateIssue(id: number, changes: IssueChanges): string?
  local issue = store.issue(id)
  if not issue then
    return "There is no such issue."
  end
  local title = if changes.title ~= nil then trim(changes.title) else issue.title
  if title == "" then
    return "An issue needs a title."
  end
  db:exec(
    `update issues set title = :t, description = :d, type = :ty, priority = :pr, status = :st, updated = {NOW} `
      .. "where id = :id",
    {
      id = id,
      t = title,
      d = if changes.description ~= nil then changes.description else issue.description,
      ty = changes.type or issue.type,
      pr = changes.priority or issue.priority,
      st = changes.status or issue.status,
    }
  )
  return nil
end

function store.removeIssue(id: number): string?
  local changed = db:exec("delete from issues where id = :id", { id = id })
  return if changed.changes == 0 then "There is no such issue." else nil
end

-- Add a page: answers with its id, or with nothing and why not. `parent` is the page it goes under, 0 for none.
function store.addPage(project: number, title: string, parent: number, body: string?): (number?, string?)
  local titled = trim(title)
  if titled == "" then
    return nil, "A page needs a title."
  elseif not db:one("select id from projects where id = :id", { id = project }) then
    return nil, "There is no such project."
  elseif
    parent ~= 0 and not db:one("select id from pages where id = :id and project_id = :p", { id = parent, p = project })
  then
    return nil, "The page it goes under is not in the project."
  end
  local made = db:exec(
    `insert into pages(project_id, parent_id, title, body, updated) values (:p, :u, :t, :b, {NOW})`,
    { p = project, u = parent, t = titled, b = body or "" }
  )
  return made.last_insert_id, nil
end

function store.writePage(id: number, title: string?, body: string?): string?
  local page = store.page(id)
  if not page then
    return "There is no such page."
  end
  local titled = if title ~= nil then trim(title) else page.title
  if titled == "" then
    return "A page needs a title."
  end
  if body ~= nil then
    db:exec(`update pages set title = :t, body = :b, updated = {NOW} where id = :id`, { id = id, t = titled, b = body })
  else
    db:exec(`update pages set title = :t, updated = {NOW} where id = :id`, { id = id, t = titled })
  end
  return nil
end

-- Delete a page and every page under it. A page goes under another only when it is made, so there is no circle.
function store.removePage(id: number): string?
  local changed = db:exec(
    [[with recursive under(id) as (
        select :id
        union all
        select p.id from pages p join under u on p.parent_id = u.id
      )
      delete from pages where id in (select id from under)]],
    { id = id }
  )
  return if changed.changes == 0 then "There is no such page." else nil
end

return store
