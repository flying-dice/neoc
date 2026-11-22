--!strict
-- The Projects app's backend: projects, the issues on each one's board and the pages written beside them, in the app's
-- own database (store.luau). Every call that changes something answers with the whole workspace as it now stands, so
-- the window that asked draws what the backend has. A workspace lists a page by its title; `readPage` gives its text.
--
-- An agent can do what a person does most: read the issues (`list_issues`), raise one (`add_issue`), move one across
-- the board (`move_issue`), and find, read and write pages (`list_pages`, `read_page`, `write_page`). A tool that
-- changed something tells the open windows with a `projects.changed` event, and they read the workspace again.
local app = require("vos:app")
local rpc = require("../.app/rpc")
local store = require("./store")
local t = app.t

local handlers: rpc.Handlers = {
  workspace = function(_ctx)
    return store.workspace()
  end,

  addProject = function(ctx, params)
    local problem = store.addProject(params.name, params.key, params.color)
    return if problem then ctx:fail(problem) else store.workspace()
  end,

  removeProject = function(ctx, params)
    local problem = store.removeProject(params.project)
    return if problem then ctx:fail(problem) else store.workspace()
  end,

  addIssue = function(ctx, params)
    local _id, problem = store.addIssue(
      params.project,
      params.title,
      { type = params.type, priority = params.priority, status = params.status }
    )
    return if problem then ctx:fail(problem) else store.workspace()
  end,

  updateIssue = function(ctx, params)
    local problem = store.updateIssue(params.issue, {
      title = params.title,
      description = params.description,
      type = params.type,
      priority = params.priority,
      status = params.status,
    })
    return if problem then ctx:fail(problem) else store.workspace()
  end,

  removeIssue = function(ctx, params)
    local problem = store.removeIssue(params.issue)
    return if problem then ctx:fail(problem) else store.workspace()
  end,

  addPage = function(ctx, params)
    local _id, problem = store.addPage(params.project, params.title, params.parent or 0)
    return if problem then ctx:fail(problem) else store.workspace()
  end,

  readPage = function(ctx, params)
    local page = store.page(params.page)
    local body = store.body(params.page)
    if page == nil or body == nil then
      return ctx:fail("There is no such page.")
    end
    return { page = page, body = body }
  end,

  writePage = function(ctx, params)
    local problem = store.writePage(params.page, params.title, params.body)
    return if problem then ctx:fail(problem) else store.workspace()
  end,

  removePage = function(ctx, params)
    local problem = store.removePage(params.page)
    return if problem then ctx:fail(problem) else store.workspace()
  end,
}

local STATUSES: { [string]: rpc.Status } = { todo = "todo", doing = "doing", done = "done" }
local TYPES: { [string]: rpc.IssueType } = { task = "task", bug = "bug", story = "story" }
local PRIORITIES: { [string]: rpc.Priority } = { low = "low", medium = "medium", high = "high" }

app.tool {
  name = "list_issues",
  title = "List the issues",
  readOnly = true,
  description = "The projects, and the issues of one project or of all: each with its key (WEB-12), title, description, type (task, bug, story), priority (low, medium, high) and status (todo, doing, done). Give `status` for one column of the board only.",
  input = {
    project = t.string { optional = true, description = "The key or the name of a project; every project when left out" },
    status = t.string { optional = true, description = "todo, doing or done; every status when left out" },
  },
  run = function(input: { project: string?, status: string? }, ctx)
    local only: number? = nil
    if input.project ~= nil then
      local project = store.projectNamed(input.project)
      if not project then
        return ctx:fail(`There is no project called {input.project}.`)
      end
      only = project.id
    end
    if input.status ~= nil and STATUSES[input.status] == nil then
      return ctx:fail("A status is todo, doing or done.")
    end
    local issues = {}
    for _, issue in store.issues(only) do
      if input.status == nil or issue.status == input.status then
        table.insert(issues, {
          key = issue.key,
          title = issue.title,
          description = issue.description,
          type = issue.type,
          priority = issue.priority,
          status = issue.status,
        })
      end
    end
    local projects = {}
    for _, project in store.projects() do
      table.insert(projects, { key = project.key, name = project.name })
    end
    return { projects = projects, issues = issues }
  end,
}

app.tool {
  name = "add_issue",
  title = "Add an issue",
  description = "Raise an issue in a project: a task of medium priority, to do, unless told otherwise. Answers with its key (WEB-12).",
  input = {
    project = t.string { description = "The key or the name of the project" },
    title = t.string { description = "What has to be done", maxLength = 200 },
    description = t.string { optional = true, description = "What is known about it", maxLength = 5000 },
    type = t.string { optional = true, description = "task, bug or story" },
    priority = t.string { optional = true, description = "low, medium or high" },
  },
  run = function(
    input: { project: string, title: string, description: string?, type: string?, priority: string? },
    ctx
  )
    local project = store.projectNamed(input.project)
    if not project then
      return ctx:fail(`There is no project called {input.project}.`)
    elseif input.type ~= nil and TYPES[input.type] == nil then
      return ctx:fail("A type is task, bug or story.")
    elseif input.priority ~= nil and PRIORITIES[input.priority] == nil then
      return ctx:fail("A priority is low, medium or high.")
    end
    local id, problem = store.addIssue(project.id, input.title, {
      description = input.description,
      type = if input.type then TYPES[input.type] else nil,
      priority = if input.priority then PRIORITIES[input.priority] else nil,
    })
    local issue = if id then store.issue(id) else nil
    if problem or not issue then
      return ctx:fail(problem or "The issue could not be added.")
    end
    app.emit("projects.changed")
    return { key = issue.key, title = issue.title, status = issue.status }
  end,
}

app.tool {
  name = "move_issue",
  title = "Move an issue",
  description = "Move an issue to another column of the board, by its key (`list_issues` gives the keys): todo, doing or done.",
  input = {
    key = t.string { description = "The key of the issue, as WEB-12" },
    status = t.string { description = "todo, doing or done" },
  },
  run = function(input: { key: string, status: string }, ctx)
    local status: rpc.Status? = STATUSES[input.status]
    local issue = store.issueKeyed(input.key)
    if not status then
      return ctx:fail("A status is todo, doing or done.")
    elseif not issue then
      return ctx:fail(`There is no issue called {input.key}.`)
    end
    local problem = store.updateIssue(issue.id, { status = status })
    if problem then
      return ctx:fail(problem)
    end
    app.emit("projects.changed")
    return { key = issue.key, title = issue.title, status = status }
  end,
}

app.tool {
  name = "list_pages",
  title = "List the pages",
  readOnly = true,
  description = "The pages of a project: each with its title, the title of the page it is under (empty for none) and when it last changed.",
  input = { project = t.string { description = "The key or the name of the project" } },
  run = function(input: { project: string }, ctx)
    local project = store.projectNamed(input.project)
    if not project then
      return ctx:fail(`There is no project called {input.project}.`)
    end
    local found = store.pages(project.id)
    local titles: { [number]: string } = {}
    for _, page in found do
      titles[page.id] = page.title
    end
    local pages = {}
    for _, page in found do
      table.insert(pages, { title = page.title, under = titles[page.parent] or "", updated = page.updated })
    end
    return { project = project.key, pages = pages }
  end,
}

app.tool {
  name = "read_page",
  title = "Read a page",
  readOnly = true,
  description = "The text of a page of a project, by its title (`list_pages` gives the titles).",
  input = {
    project = t.string { description = "The key or the name of the project" },
    title = t.string { description = "The title of the page" },
  },
  run = function(input: { project: string, title: string }, ctx)
    local project = store.projectNamed(input.project)
    if not project then
      return ctx:fail(`There is no project called {input.project}.`)
    end
    local page = store.pageTitled(project.id, input.title)
    if not page then
      return ctx:fail(`There is no page called {input.title} in {project.name}.`)
    end
    return { title = page.title, body = store.body(page.id) or "", updated = page.updated }
  end,
}

app.tool {
  name = "write_page",
  title = "Write a page",
  description = "Write a page of a project: the page with that title is given the text in place of what it had, and a title no page has makes a new page, under the page named in `under` when one is.",
  input = {
    project = t.string { description = "The key or the name of the project" },
    title = t.string { description = "The title of the page", maxLength = 120 },
    body = t.string { description = "The whole text of the page", maxLength = 20000 },
    under = t.string { optional = true, description = "For a new page: the title of the page it goes under" },
  },
  run = function(input: { project: string, title: string, body: string, under: string? }, ctx)
    local project = store.projectNamed(input.project)
    if not project then
      return ctx:fail(`There is no project called {input.project}.`)
    end
    local page = store.pageTitled(project.id, input.title)
    local problem: string? = nil
    if page then
      problem = store.writePage(page.id, nil, input.body)
    else
      local parent = 0
      if input.under ~= nil then
        local under = store.pageTitled(project.id, input.under)
        if not under then
          return ctx:fail(`There is no page called {input.under} in {project.name}.`)
        end
        parent = under.id
      end
      local _id, refused = store.addPage(project.id, input.title, parent, input.body)
      problem = refused
    end
    if problem then
      return ctx:fail(problem)
    end
    app.emit("projects.changed")
    return { project = project.key, title = input.title, created = page == nil }
  end,
}

return handlers
