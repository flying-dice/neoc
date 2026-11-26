--!strict
-- The Projects app's backend: projects, the issues on each one's board and the pages written beside them, in the app's
-- own database (store.luau). Every call that changes something answers with the whole workspace as it now stands, so
-- the window that asked draws what the backend has. A workspace lists a page by its title; `_get_page` gives its text.
-- It serves on the RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau`.
--
-- The page's methods are private (`_workspace`, `_add_project`, ...). An agent can do what a person does most: read the
-- issues (`list_issues`), raise one (`add_issue`), move one across the board (`move_issue`), and find, read and write
-- pages (`list_pages`, `read_page`, `write_page`). A tool that changed something tells the open windows with the
-- `projects_changed` notification, and they read the workspace again.
local store = require("./store")

RPC.on_request("_workspace", function(): projects_WorkspaceResult
  return store.workspace()
end)

RPC.on_request(
  "_add_project",
  function(params: projects_AddProjectParams, ctx: vos_app_Context): projects_AddProjectResult
    local problem = store.addProject(params.name, params.key, params.color)
    return if problem then ctx:fail(problem) else store.workspace()
  end
)

RPC.on_request(
  "_remove_project",
  function(params: projects_RemoveProjectParams, ctx: vos_app_Context): projects_RemoveProjectResult
    local problem = store.removeProject(params.project)
    return if problem then ctx:fail(problem) else store.workspace()
  end
)

RPC.on_request(
  "_create_issue",
  function(params: projects_CreateIssueParams, ctx: vos_app_Context): projects_CreateIssueResult
    local _id, problem = store.addIssue(
      params.project,
      params.title,
      { type = params.type, priority = params.priority, status = params.status }
    )
    return if problem then ctx:fail(problem) else store.workspace()
  end
)

RPC.on_request(
  "_update_issue",
  function(params: projects_UpdateIssueParams, ctx: vos_app_Context): projects_UpdateIssueResult
    local problem = store.updateIssue(params.issue, {
      title = params.title,
      description = params.description,
      type = params.type,
      priority = params.priority,
      status = params.status,
    })
    return if problem then ctx:fail(problem) else store.workspace()
  end
)

RPC.on_request(
  "_remove_issue",
  function(params: projects_RemoveIssueParams, ctx: vos_app_Context): projects_RemoveIssueResult
    local problem = store.removeIssue(params.issue)
    return if problem then ctx:fail(problem) else store.workspace()
  end
)

RPC.on_request("_add_page", function(params: projects_AddPageParams, ctx: vos_app_Context): projects_AddPageResult
  local _id, problem = store.addPage(params.project, params.title, params.parent or 0)
  return if problem then ctx:fail(problem) else store.workspace()
end)

RPC.on_request("_get_page", function(params: projects_GetPageParams, ctx: vos_app_Context): projects_GetPageResult
  local page = store.page(params.page)
  local body = store.body(params.page)
  if page == nil or body == nil then
    return ctx:fail("There is no such page.")
  end
  return { page = page, body = body }
end)

RPC.on_request("_save_page", function(params: projects_SavePageParams, ctx: vos_app_Context): projects_SavePageResult
  local problem = store.writePage(params.page, params.title, params.body)
  return if problem then ctx:fail(problem) else store.workspace()
end)

RPC.on_request(
  "_remove_page",
  function(params: projects_RemovePageParams, ctx: vos_app_Context): projects_RemovePageResult
    local problem = store.removePage(params.page)
    return if problem then ctx:fail(problem) else store.workspace()
  end
)

local STATUSES: { [string]: projects_Status } = { todo = "todo", doing = "doing", done = "done" }
local TYPES: { [string]: projects_IssueType } = { task = "task", bug = "bug", story = "story" }
local PRIORITIES: { [string]: projects_Priority } = { low = "low", medium = "medium", high = "high" }

RPC.on_request(
  "list_issues",
  function(params: projects_ListIssuesParams, ctx: vos_app_Context): projects_ListIssuesResult
    local only: number? = nil
    if params.project ~= nil then
      local project = store.projectNamed(params.project)
      if not project then
        return ctx:fail(`There is no project called {params.project}.`)
      end
      only = project.id
    end
    if params.status ~= nil and STATUSES[params.status] == nil then
      return ctx:fail("A status is todo, doing or done.")
    end
    local issues = {}
    for _, issue in store.issues(only) do
      if params.status == nil or issue.status == params.status then
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
  end
)

RPC.on_request("add_issue", function(params: projects_AddIssueParams, ctx: vos_app_Context): projects_AddIssueResult
  local project = store.projectNamed(params.project)
  if not project then
    return ctx:fail(`There is no project called {params.project}.`)
  elseif params.type ~= nil and TYPES[params.type] == nil then
    return ctx:fail("A type is task, bug or story.")
  elseif params.priority ~= nil and PRIORITIES[params.priority] == nil then
    return ctx:fail("A priority is low, medium or high.")
  end
  local id, problem = store.addIssue(project.id, params.title, {
    description = params.description,
    type = if params.type then TYPES[params.type] else nil,
    priority = if params.priority then PRIORITIES[params.priority] else nil,
  })
  local issue = if id then store.issue(id) else nil
  if problem or not issue then
    return ctx:fail(problem or "The issue could not be added.")
  end
  RPC.send_notification("projects_changed")
  return { key = issue.key, title = issue.title, status = issue.status }
end)

RPC.on_request("move_issue", function(params: projects_MoveIssueParams, ctx: vos_app_Context): projects_MoveIssueResult
  local status: projects_Status? = STATUSES[params.status]
  local issue = store.issueKeyed(params.key)
  if not status then
    return ctx:fail("A status is todo, doing or done.")
  elseif not issue then
    return ctx:fail(`There is no issue called {params.key}.`)
  end
  local problem = store.updateIssue(issue.id, { status = status })
  if problem then
    return ctx:fail(problem)
  end
  RPC.send_notification("projects_changed")
  return { key = issue.key, title = issue.title, status = status }
end)

RPC.on_request("list_pages", function(params: projects_ListPagesParams, ctx: vos_app_Context): projects_ListPagesResult
  local project = store.projectNamed(params.project)
  if not project then
    return ctx:fail(`There is no project called {params.project}.`)
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
end)

RPC.on_request("read_page", function(params: projects_ReadPageParams, ctx: vos_app_Context): projects_ReadPageResult
  local project = store.projectNamed(params.project)
  if not project then
    return ctx:fail(`There is no project called {params.project}.`)
  end
  local page = store.pageTitled(project.id, params.title)
  if not page then
    return ctx:fail(`There is no page called {params.title} in {project.name}.`)
  end
  return { title = page.title, body = store.body(page.id) or "", updated = page.updated }
end)

RPC.on_request("write_page", function(params: projects_WritePageParams, ctx: vos_app_Context): projects_WritePageResult
  local project = store.projectNamed(params.project)
  if not project then
    return ctx:fail(`There is no project called {params.project}.`)
  end
  local page = store.pageTitled(project.id, params.title)
  local problem: string? = nil
  if page then
    problem = store.writePage(page.id, nil, params.body)
  else
    local parent = 0
    if params.under ~= nil then
      local under = store.pageTitled(project.id, params.under)
      if not under then
        return ctx:fail(`There is no page called {params.under} in {project.name}.`)
      end
      parent = under.id
    end
    local _id, refused = store.addPage(project.id, params.title, parent, params.body)
    problem = refused
  end
  if problem then
    return ctx:fail(problem)
  end
  RPC.send_notification("projects_changed")
  return { project = project.key, title = params.title, created = page == nil }
end)
