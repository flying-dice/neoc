-- ── Runtime ─────────────────────────────────────────────────────────────────────────────────────────────────
-- Constructors are thin: they validate their arguments, assign auto keys and return the plain node table of the
-- wire form (type, key, props, children, slots, on). Handlers stay Luau functions in `on` (the host replaces them
-- with `true` on the wire and keeps the closure, keyed by path#event). The host validates every tree again.

local function isRef(value: any): boolean
	return type(value) == "table" and value["$ref"] ~= nil
end

local function isSource(value: any): boolean
	return type(value) == "table" and type(value["$source"]) == "string"
end

local function isExpr(value: any): boolean
	return type(value) == "table" and (value["$ref"] ~= nil or value["$fn"] ~= nil or value["$lit"] ~= nil)
end

local function isNode(value: any): boolean
	return type(value) == "table" and type(value.type) == "string"
end

local function isAction(value: any): boolean
	return type(value) == "table" and type(value["do"]) == "string"
end

-- A node, or a list whose first entry is a node (an empty list is a valid, empty slot).
local function isNodeList(value: any): boolean
	if type(value) ~= "table" or isExpr(value) then
		return false
	end
	if isNode(value) then
		return true
	end
	return #value == 0 or isNodeList(value[1])
end

-- Flatten nodes, lists of nodes and nil into `out`.
local function collect(out: { any }, value: any, where: string)
	if value == nil then
		return
	end
	if isNode(value) then
		table.insert(out, value)
		return
	end
	if type(value) == "table" and not isExpr(value) then
		for i = 1, #value do
			collect(out, value[i], where)
		end
		return
	end
	error(`{where}: expected a node, got {typeof(value)}`, 4)
end

local rowRefs = setmetatable({}, {
	__index = function(_, field: any): any
		return { ["$ref"] = "row." .. tostring(field) }
	end,
})

local function toSlot(value: any, isRowTemplate: boolean, where: string): { any }
	local list: { any } = {}
	if isRowTemplate and type(value) == "function" then
		collect(list, (value :: any)(rowRefs), where)
	else
		collect(list, value, where)
	end
	return list
end

local function toHandler(value: any, where: string): any
	if value == true or type(value) == "function" then
		return value
	end
	if isAction(value) then
		return { value }
	end
	if type(value) == "table" then
		local list = {}
		for i = 1, #value do
			local item = value[i]
			if type(item) ~= "function" and not isAction(item) then
				error(`{where}: a handler list holds functions and ui.act actions, got {typeof(item)}`, 4)
			end
			table.insert(list, item)
		end
		return list
	end
	error(`{where}: a handler is a function, a ui.act action or a list of them, got {typeof(value)}`, 4)
end

local function assignKeys(list: { any })
	for index, child in ipairs(list) do
		if child.key == nil then
			child.key = "_" .. tostring(index - 1)
		end
	end
end

local function build(def: any, props: any): Node
	local label = "ui." .. def.label
	if type(props) ~= "table" then
		error(label .. ": expected a table, e.g. " .. label .. " { ... }", 3)
	end
	local nodeProps: { [string]: any } = {}
	local children: { any } = {}
	local slots: { [string]: { any } } = {}
	local on: { [string]: any } = {}

	if def.axis ~= nil then
		nodeProps.axis = def.axis
	end

	-- positional entries: the positional prop (text, title, ...) or children
	for i = 1, #props do
		local entry = props[i]
		if entry ~= nil then
			local isChild = isNode(entry) or (type(entry) == "table" and not isExpr(entry))
			if def.positional ~= nil and not isChild then
				if nodeProps[def.positional] ~= nil then
					error(`{label}: more than one value for '{def.positional}'`, 3)
				end
				nodeProps[def.positional] = entry
			else
				collect(children, entry, label)
			end
		end
	end

	for name, value in pairs(props) do
		if type(name) == "number" or name == "key" then
			continue
		end
		local where = `{label}.{name}`
		local event = def.events[name]
		if event ~= nil then
			on[event] = toHandler(value, where)
		elseif def.slots[name] ~= nil and (def.props[name] == nil or isNodeList(value) or type(value) == "function") then
			slots[name] = toSlot(value, def.rows[name] ~= nil, where)
		elseif def.groups[name] ~= nil then
			for sub, inner in pairs(value) do
				local full = def.groups[name][sub]
				if full == nil then
					error(`{where}: no slot '{sub}'`, 3)
				end
				slots[full] = toSlot(inner, false, `{where}.{sub}`)
			end
		elseif def.prefixes[name] ~= nil then
			for sub, inner in pairs(value) do
				slots[def.prefixes[name] .. sub] = toSlot(inner, false, `{where}.{sub}`)
			end
		elseif def.props[name] ~= nil then
			if def.bindings[name] ~= nil and not isRef(value) then
				error(`{where} is two-way: pass a ref (w.client.x, source:bind("field")), not a value`, 3)
			end
			if def.sources[name] ~= nil and isSource(value) then
				value = value["$source"]
			end
			if name == def.positional and nodeProps[name] ~= nil then
				error(`{where}: set both positionally and by name`, 3)
			end
			nodeProps[name] = value
		else
			error(`{label}: unknown prop '{name}'`, 3)
		end
	end

	assignKeys(children)
	for _, list in pairs(slots) do
		assignKeys(list)
	end

	local node: any = { type = def.name, key = props.key }
	if next(nodeProps) ~= nil then
		node.props = nodeProps
	end
	if #children > 0 then
		node.children = children
	end
	if next(slots) ~= nil then
		node.slots = slots
	end
	if next(on) ~= nil then
		node.on = on
	end
	return node
end

local function sugar(def: any, label: string, axis: string): any
	local copy = table.clone(def)
	copy.label = label
	copy.axis = axis
	return copy
end

local function call(name: string, ...: any): Fn
	return { ["$fn"] = name, args = { ... } }
end

local function refPath(value: any): string
	if type(value) == "string" then
		return value
	end
	if isRef(value) then
		return value["$ref"]
	end
	error("expected a ref (w.client.x, w.state paths are values and cannot be targets)", 3)
end
