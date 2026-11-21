--!strict
-- Notes from docs/architecture/ui-system.md §4.4. The codegen contract test runs it (against the real
-- `ui.lua`) and validates the tree it renders.
local app = require("vos:app")
local ui = require("vos:ui")
local t = app.t
local db = app.db

app.manifest {
  id = "notes",
  name = "Notes",
  icon = "note.text",
  tint = "yellow",
  window = { w = 960, h = 640, minW = 480, minH = 360 },
}

db:migrate {
  [1] = [[
    create table folders (
      id integer primary key, name text not null, position real not null default 0
    );
    create table notes (
      id integer primary key,
      folder_id integer not null references folders(id) on delete cascade,
      body text not null default '',
      rev integer not null default 0,                      -- bumped on every write: conflict detection
      updated_at text not null default (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
      author text                                          -- who wrote last: a person or an agent
    );
    create index notes_by_folder on notes(folder_id, updated_at desc);
    insert into folders(name) values ('Notes');
  ]],
}

-- ── Data: prepared once; the SQL never leaves the host ──────────────────────

local folders = db:query(
  [[
  select f.id, f.name, (select count(*) from notes n where n.folder_id = f.id) as count
  from folders f order by f.position, f.id
]],
  { key = "id" }
)

local noteList = db:query(
  [[
  select id,
         coalesce(nullif(trim(substr(body, 1, instr(body || char(10), char(10)) - 1)), ''), 'New Note') as title,
         substr(replace(body, char(10), ' '), 1, 120) as preview,
         updated_at
  from notes
  where (:folder = 0 or folder_id = :folder)
    and (:q = '' or body like '%' || :q || '%')
  order by updated_at desc, id desc                        -- total order: required for windowed lists
]],
  { key = "id", page = 60 }
)

local note = db:row([[select id, body, rev, updated_at, author from notes where id = :id]], {
  key = "id",
  rev = "rev", -- compared with the client's baseRev on write-back
  write = function(id, field, value, ctx) -- the host calls this for a two-way bound field
    assert(field == "body")
    db:exec(
      [[update notes set body = :v, rev = rev + 1, updated_at = :now, author = :who where id = :id]],
      { v = value, id = id, now = ctx.now, who = ctx.actor.name }
    )
  end,
})

local function newNote(folder, who)
  local target = folder == 0 and 1 or folder
  return db:exec("insert into notes(folder_id, author) values (:f, :who)", { f = target, who = who }).last_insert_id
end

-- ── Window ─────────────────────────────────────────────────────────────────

app.window("main", {
  title = "Notes",

  client = { -- client-managed: instant, no round trip
    folder = { init = 0, persist = "app" }, -- 0 = All Notes; survives closing the window
    noteId = { init = 0, persist = "app" },
    mode = { init = "edit", persist = "app" },
    search = { init = "", persist = "session" },
    confirmDelete = { init = false }, -- the alert; never persisted
    deleteId = { init = 0, sync = "commit" }, -- mirrored so the alert's handler can read it
  },

  render = function(w)
    local c = w.client -- Refs, not values
    local current = note { id = c.noteId } -- live single-row source, parameterised by a client ref

    local function askDelete(id) -- an Action: runs on the client, then the Alert shows
      return ui.act.seq { ui.act.set(c.deleteId, id), ui.act.set(c.confirmDelete, true) }
    end

    return ui.Group {
      ui.SplitView {
        columns = 3,

        sidebar = ui.Screen {
          title = "Folders",
          ui.List {
            key = "folders",
            style = "sidebar",
            selection = c.folder,
            ui.Row { key = "all", tag = 0, icon = "tray.full", title = "All Notes" },
            ui.ForEach {
              source = folders {},
              row = function(r)
                return ui.Row { tag = r.id, icon = "folder", title = r.name, badge = r.count }
              end,
            },
          },
        },

        content = ui.Screen {
          title = "Notes",
          titleDisplay = "large",
          searchable = { text = c.search, placeholder = "Search", sync = "debounce:250" },
          toolbar = {
            trailing = ui.Button {
              key = "new-note",
              icon = "square.and.pencil",
              label = "New Note",
              shortcut = "cmd+n",
              onTap = function(e)
                w.client:set("noteId", newNote(w.client:get("folder"), e.actor.name))
              end,
            },
          },

          ui.List {
            key = "notes",
            style = "plain",
            selection = c.noteId,
            empty = ui.EmptyState {
              icon = "note.text",
              title = "No Notes",
              message = ui.when(ui.isEmpty(c.search), "Tap the compose button to start.", "No results."),
            },
            ui.ForEach {
              source = noteList { folder = c.folder, q = c.search },
              row = function(r)
                return ui.Row {
                  tag = r.id,
                  title = r.title,
                  subtitle = ui.concat(ui.fmt.relative(r.updated_at), "  ", r.preview),
                  swipeTrailing = {
                    ui.Button {
                      key = "delete",
                      icon = "trash",
                      label = "Delete",
                      role = "destructive",
                      onTap = askDelete(r.id),
                    },
                  },
                }
              end,
            },
          },
        },

        detail = ui.Screen {
          titleDisplay = "inline",
          toolbar = {
            principal = ui.Picker {
              key = "mode",
              style = "segmented",
              selection = c.mode,
              label = "Mode",
              options = { { value = "edit", label = "Edit" }, { value = "preview", label = "Preview" } },
            },
            trailing = ui.Button {
              key = "delete-current",
              icon = "trash",
              label = "Delete Note",
              role = "destructive",
              disabled = ui.eq(c.noteId, 0),
              onTap = askDelete(c.noteId),
            },
          },

          ui.Show {
            when = ui.eq(c.noteId, 0),
            ui.EmptyState { icon = "note.text", title = "No Note Selected" },
            otherwise = ui.VStack {
              spacing = 0,
              ui.Show {
                when = ui.eq(c.mode, "edit"),
                -- two-way: the field shows what you type at once; the host gets one debounced write
                ui.TextEditor {
                  key = "body",
                  text = current:bind("body"),
                  label = "Note",
                  placeholder = "Start writing…",
                  sync = "debounce:400",
                  grow = 1,
                },
                -- the preview reads the same ref: it follows every keystroke, with no round trip
                otherwise = ui.ScrollView {
                  grow = 1,
                  ui.Markdown { key = "preview", text = current.body, padding = 4 },
                },
              },
              ui.Text {
                key = "meta",
                padding = 2,
                style = "footnote",
                foreground = "secondary",
                ui.concat(
                  "Edited ",
                  ui.fmt.relative(current.updated_at),
                  ui.when(ui.isEmpty(current.author), "", ui.concat(" by ", current.author))
                ),
              },
            },
          },
        },
      },

      ui.Alert {
        key = "confirm-delete",
        presented = c.confirmDelete,
        title = "Delete this note?",
        message = "This can’t be undone.",
        actions = {
          ui.Button {
            key = "confirm",
            title = "Delete",
            role = "destructive",
            onTap = function()
              local id = w.client:get("deleteId")
              db:exec("delete from notes where id = :id", { id = id })
              if w.client:get("noteId") == id then
                w.client:set("noteId", 0)
              end
            end,
          },
          ui.Button { key = "cancel", title = "Cancel", role = "cancel" }, -- any action dismisses
        },
      },
    }
  end,
})

-- ── Agent tools ────────────────────────────────────────────────────────────

app.tool {
  name = "search",
  title = "Search Notes",
  readOnly = true,
  description = "Find notes whose text contains the query. Returns id, title and a preview.",
  input = { query = t.string {}, limit = t.integer { default = 10, min = 1, max = 50 } },
  run = function(input)
    return db:all(
      [[select id, substr(body, 1, 60) as title, updated_at from notes
                    where body like '%' || :q || '%' order by updated_at desc limit :n]],
      { q = input.query, n = input.limit }
    )
  end,
}

app.tool {
  name = "append",
  title = "Append to Note",
  description = "Append text (Markdown) on a new line at the end of a note. If the note is open the user sees it appear live.",
  input = { id = t.integer { description = "Note id from search" }, text = t.string {} },
  run = function(input, ctx)
    local r = db:exec(
      [[
      update notes set body = body || char(10) || :text, rev = rev + 1, updated_at = :now, author = :who
      where id = :id]],
      { text = input.text, id = input.id, now = ctx.now, who = ctx.actor.name }
    )
    if r.changes == 0 then
      return ctx:fail("No note with id " .. input.id)
    end
    return { id = input.id }
  end,
}
