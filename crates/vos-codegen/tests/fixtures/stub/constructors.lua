-- Runs the generated `ui.lua` constructors and asserts what the docs promise. Executed by the codegen contract
-- test with a `lua` runner: `lua constructors.lua <ui.lua>`; prints `ok`.
local ui = assert(loadfile((...)))()

local function check(condition, message)
  assert(condition, message)
end

local function fails(body, fragment)
  local ok, err = pcall(body)
  check(not ok, "expected an error containing '" .. fragment .. "'")
  check(string.find(tostring(err), fragment, 1, true) ~= nil, "error was: " .. tostring(err))
end

-- positional entries are children, a trailing nil and nested lists are flattened, children get auto keys
local stack = ui.VStack {
  key = "s",
  spacing = 2,
  ui.Text { "a" },
  { ui.Text { "b" }, ui.Text { "c", key = "named" } },
  nil,
}
check(stack.type == "Stack" and stack.key == "s", "VStack is a Stack")
check(stack.props.axis == "v" and stack.props.spacing == 2, "VStack sets axis")
check(#stack.children == 3, "children are flattened")
check(stack.children[1].key == "_0" and stack.children[2].key == "_1", "auto keys by position")
check(stack.children[3].key == "named", "explicit keys are kept")
check(stack.children[1].props.text == "a", "Text is positional")
check(ui.HStack({}).props.axis == "h", "HStack sets axis")
check(ui.Stack({ axis = "h" }).props.axis == "h", "Stack takes an axis")

-- handlers stay functions in `on`; an action becomes a one-element list
local tapped = function() end
local button = ui.Button { key = "b", title = "Go", onTap = tapped, onLongPress = ui.act.pop() }
check(button.on.tap == tapped, "functions stay in on")
check(type(button.on.longPress) == "table" and button.on.longPress[1]["do"] == "nav.pop", "an action is a list of one")
check(button.props.title == "Go", "named title")
local mixed = ui.Button { key = "m", title = "x", onTap = { ui.act.pop(), tapped } }
check(#mixed.on.tap == 2, "a list mixing actions and functions is kept")

-- slots, dotted slot groups, prefixed slots
local screen = ui.Screen {
  title = "T",
  toolbar = { trailing = ui.Button { key = "b", title = "x" }, leading = { ui.Spacer {} } },
  ui.Spacer {},
}
check(screen.slots["toolbar.trailing"][1].key == "b", "toolbar.trailing slot")
check(screen.slots["toolbar.leading"][1].key == "_0", "slot children get auto keys")
check(#screen.children == 1 and screen.props.title == "T", "children and props")
local nav = ui.NavigationStack { root = ui.Spacer {}, dest = { note = ui.Spacer {} } }
check(nav.slots.root ~= nil and nav.slots["dest:note"] ~= nil, "root and dest:<route> slots")
local split = ui.SplitView { columns = 3, sidebar = ui.Spacer {}, detail = ui.Spacer {} }
check(split.slots.sidebar[1].key == "_0" and split.props.columns == 3, "named slots")
local show = ui.Show { when = true, ui.Spacer {}, otherwise = ui.Divider {} }
check(#show.children == 1 and show.slots.otherwise[1].type == "Divider", "Show then and otherwise")

-- a name that is both a prop and a slot is routed by the kind of value
local plain = ui.Section { header = "Title" }
check(plain.props.header == "Title" and plain.slots == nil, "a string is the prop")
local rich = ui.Section { header = ui.Text { "Title" } }
check(rich.props == nil and rich.slots.header[1].type == "Text", "a node is the slot")

-- sources become ids, row templates are called once with a ref proxy
local each = ui.ForEach {
  source = { ["$source"] = "s1" },
  row = function(r)
    return ui.Row { tag = r.id, title = r.title }
  end,
}
check(each.props.source == "s1", "a source handle becomes its id")
local row = each.slots.row[1]
check(row.key == "_0" and row.props.tag["$ref"] == "row.id" and row.props.title["$ref"] == "row.title", "row refs")
check(#each.slots.row == 1, "the template is built once")

-- bindings take refs
local ref = { ["$ref"] = "client.q" }
check(ui.TextField({ key = "q", text = ref, label = "Search" }).props.text == ref, "a ref binds")
fails(function()
  ui.TextField { key = "q", text = "literal", label = "Search" }
end, "two-way")

-- errors name the constructor and the prop
fails(function()
  ui.Button { key = "b", title = "x", bogus = 1 }
end, "ui.Button: unknown prop 'bogus'")
fails(function()
  ui.Text { "a", "b" }
end, "more than one value for 'text'")
fails(function()
  ui.VStack { 5 }
end, "expected a node")
fails(function()
  ui.Button { key = "b", title = "x", onTap = 5 }
end, "a handler is")
fails(function()
  ui.Screen { toolbar = { nowhere = ui.Spacer {} } }
end, "no slot 'nowhere'")
fails(function()
  ui.Stack(5)
end, "expected a table")

-- client actions
local set = ui.act.set(ref, 1)
check(set["do"] == "set" and set.ref == "client.q" and set.value == 1, "act.set takes a ref")
check(ui.act.set("client.q", 2).ref == "client.q", "act.set takes a path")
check(ui.act.seq({ set, ui.act.toggle(ref) }).actions[2]["do"] == "toggle", "act.seq")
check(ui.act.push({ name = "note", id = 7 }).route.id == 7, "act.push")
check(ui.act.emit("done", { x = 1 }).payload.x == 1, "act.emit")
for _, name in ipairs { "pop", "popToRoot" } do
  check(ui.act[name]()["do"] ~= nil, name)
end

-- expressions
local cond = ui.when(ui.isEmpty(ref), "a", "b")
check(cond["$fn"] == "if" and cond.args[1]["$fn"] == "isEmpty" and #cond.args == 3, "when and isEmpty")
check(ui.all(true, false)["$fn"] == "and" and ui.any(1, 2)["$fn"] == "or", "all and any")
check(ui.negate(true)["$fn"] == "not", "negate")
check(ui.concat(ref, " x", "y")["$fn"] == "concat" and #ui.concat(ref, " x", "y").args == 3, "variadic concat")
check(ui.fmt.relative(ref)["$fn"] == "fmt.relative" and #ui.fmt.relative(ref).args == 1, "optional args are not passed")
check(#ui.fmt.date(ref, { style = "short" }).args == 2, "fmt.date with options")
check(ui.eq(ref, 0)["$fn"] == "eq" and ui.add(1, 2)["$fn"] == "add", "eq and add")

-- ui.each
local mapped = ui.each({ 1, 2, 3 }, function(n)
  if n ~= 2 then
    return ui.Text { tostring(n), key = "n" .. n }
  end
  return nil
end)
check(#mapped == 2 and mapped[2].key == "n3", "ui.each skips nil")

print("ok")
