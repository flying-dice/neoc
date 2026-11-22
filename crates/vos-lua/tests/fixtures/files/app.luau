--!strict
-- A representative file workflow through `app.files` alone, with no `std:fs`: create, list, read, write, move,
-- copy, inspect and delete. `crates/vos-lua/tests/files.rs` runs it, also in a VM that refuses `std:fs`, and
-- checks what each step returned. The test loads it followed by `return {}` (and, in one case, by one more tool
-- first): the handlers a backend returns, here none.
local app = require("vos:app")

local docs = app.files.documents

type Step = { ok: boolean, value: any?, kind: string? }

-- One call's outcome: whether it worked, what it returned, and the kind of its error.
local function step(value: any?, err: string?): Step
  return { ok = value ~= nil, value = value, kind = app.files.errorKind(err) }
end

-- The size of what a read returned (a binary string is not JSON-friendly).
local function sized(data: string?, err: string?): Step
  return step(if data then #data else nil, err)
end

app.tool {
  name = "workflow",
  title = "Run the file workflow",
  description = "Creates, lists, reads, writes, moves, copies, inspects and deletes files in the app's documents.",
  -- writes the app's files
  run = function()
    local r: { [string]: Step } = {}
    r.createDir = step(docs:createDir("notes/2026"))
    r.createAgain = step(docs:createDir("notes/2026"))
    r.writeNoParent = step(docs:write("drafts/a.txt", "x"))
    r.write = step(docs:write("notes/2026/todo.txt", "buy milk"))
    r.overwrite = step(docs:write("notes/2026/todo.txt", "buy milk\nwalk the dog"))
    r.read = step(docs:read("notes/2026/todo.txt"))
    r.readMissing = step(docs:read("notes/2026/nope.txt"))
    r.readDir = step(docs:read("notes"))
    r.list = step(docs:list("notes/2026"))
    r.info = step(docs:info("notes/2026/todo.txt"))
    r.infoDir = step(docs:info("notes"))
    r.move = step(docs:move("notes/2026/todo.txt", "notes/2026/done.txt"))
    r.readMoved = step(docs:read("notes/2026/done.txt"))
    r.oldGone = step(docs:info("notes/2026/todo.txt"))
    r.copyFile = step(docs:copy("notes/2026/done.txt", "notes/2026/copy.txt"))
    r.copyTree = step(docs:copy("notes", "backup"))
    r.listBackup = step(docs:list("backup/2026"))
    r.copyOverTree = step(docs:copy("notes", "backup"))
    r.removeNotEmpty = step(docs:remove("backup"))
    r.removeTree = step(docs:remove("backup", { recursive = true }))
    r.removeFile = step(docs:remove("notes/2026/copy.txt"))
    r.removeMissing = step(docs:remove("notes/2026/copy.txt"))
    r.root = step(docs:list())
    r.cacheWrite = step(app.files.cache:write("thumb.bin", "\0\1\2"))
    r.cacheRead = sized(app.files.cache:read("thumb.bin"))
    r.tempEmpty = step(app.files.temp:list())
    return r
  end,
}
