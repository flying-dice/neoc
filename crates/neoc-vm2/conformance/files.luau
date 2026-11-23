-- Files as std:io readers and writers, on neoc-vm and neoc-vm2: `Writer:write` / `flush`, `Reader` over a file,
-- `io.copy`, and the format modules' reader and writer functions (`to_writer`, `encode_to`, `from_reader`, `decode`).
-- Returns plain values, compared by the harness. It works in a directory of its own under the system's temporary
-- directory and removes it at the end. `csv.reader` over a file is in fs.vm2.luau: its reader was reshaped.
local fs = require("std:fs")
local io = require("std:io")
local path = require("std:path")
local env = require("std:env")
local random = require("lib:random")
local serde_json = require("vnd:serde_json")
local serde_yaml = require("vnd:serde_yaml")
local toml = require("vnd:toml")
local quick_xml = require("vnd:quick_xml")

-- The one place the two VMs differ here, on purpose (see fs.vm2.luau): neoc-vm2 opens a file with `open`, `create`
-- or `append`; neoc-vm with `open { path, mode }`.
local reshaped = fs.create ~= nil
local function reader(file: string)
	return assert(if reshaped then fs.open(file) else fs.open({ path = file, mode = "r" }))
end
local function writer(file: string)
	return assert(if reshaped then fs.create(file) else fs.open({ path = file, mode = "w" }))
end
local function appender(file: string)
	return assert(if reshaped then fs.append(file) else fs.open({ path = file, mode = "a" }))
end

local dir = path.join(env.temp_dir(), "neoc-conformance-files-" .. random.uuid_v4())
local function p(name: string): string
	return path.join(dir, name)
end

-- a call's answer as plain values: the value (`false` for nil) and whether there was no error
local function answer(value: unknown, err: unknown): { unknown }
	return { if value == nil then false else value, err == nil }
end

local function body()
	local out = {}
	assert(fs.create_dir(dir))

	-- write and flush
	local w = writer(p("w"))
	out.created_empty = answer(fs.read(p("w")))
	out.write = { answer(w:write("hello ")), answer(w:write("")), answer(w:write("world\n")), answer(w:flush()), answer(w:flush()) }
	out.written = answer(fs.read(p("w")))
	local binary = writer(p("bin"))
	binary:write("\0\1\2\254\255")
	binary:flush()
	out.binary = fs.read(p("bin")) == "\0\1\2\254\255"
	local big = writer(p("big"))
	for _ = 1, 100 do
		big:write(string.rep("x", 1000))
	end
	big:flush()
	out.big = #assert(fs.read(p("big")))

	-- truncate and append
	local again = writer(p("w"))
	again:write("x")
	again:flush()
	local more = appender(p("w"))
	more:write("yz")
	more:flush()
	local fresh = appender(p("fresh"))
	fresh:write("new")
	fresh:flush()
	out.truncate_and_append = { answer(fs.read(p("w"))), answer(fs.read(p("fresh"))) }

	-- a file read through a Reader
	fs.write(p("lines"), "one\r\ntwo\n\nlast")
	local lines = reader(p("lines"))
	out.read_line = { answer(lines:read_line()), answer(lines:read_line()), answer(lines:read_line()), answer(lines:read_line()), answer(lines:read_line()) }
	local chunks = reader(p("lines"))
	out.read = { answer(chunks:read(3)), answer(chunks:read(2)), answer(chunks:read_all()), answer(chunks:read(1)), answer(chunks:read_all()) }
	out.read_big = #assert(reader(p("big")):read_all())

	-- io.copy: a string to a file, a file to a file, what is left of a reader
	local to = writer(p("copy1"))
	out.copy_string = answer(io.copy(io.from_string("from a string"), to))
	to:flush()
	local to2 = writer(p("copy2"))
	out.copy_file = answer(io.copy(reader(p("big")), to2))
	to2:flush()
	local partly = reader(p("lines"))
	partly:read_line()
	local to3 = writer(p("copy3"))
	out.copy_rest = { answer(io.copy(partly, to3)), answer(io.copy(partly, to3)) }
	to3:flush()
	out.copied = { answer(fs.read(p("copy1"))), #assert(fs.read(p("copy2"))), answer(fs.read(p("copy3"))) }

	-- the format modules write to a file and read it back
	local config = { name = "neoc", port = 8080, debug = true, tags = { "a", "b" }, server = { host = "localhost" } }

	local json = writer(p("config.json"))
	out.json_to_writer = answer(serde_json.to_writer(config, json))
	json:flush()
	out.json_file = fs.read(p("config.json")) == serde_json.to_string(config)
	out.json_from_reader = assert(serde_json.from_reader(reader(p("config.json")))):to_table()

	local yaml = writer(p("config.yaml"))
	out.yaml_encode_to = answer(serde_yaml.encode_to(config, yaml))
	yaml:flush()
	out.yaml_file = fs.read(p("config.yaml")) == serde_yaml.encode(config)
	out.yaml_decode = serde_yaml.decode(reader(p("config.yaml")))

	local tomlw = writer(p("config.toml"))
	out.toml_encode_to = answer(toml.encode_to(config, tomlw))
	tomlw:flush()
	out.toml_file = fs.read(p("config.toml")) == toml.encode(config)
	out.toml_decode = toml.decode(reader(p("config.toml")))

	local document = { config = { name = "neoc", server = { ["@port"] = "8080", host = "localhost" } } }
	local xml = writer(p("config.xml"))
	out.xml_encode_to = answer(quick_xml.encode_to(document, xml))
	xml:flush()
	out.xml_file = fs.read(p("config.xml")) == quick_xml.encode(document)
	out.xml_decode = quick_xml.decode(reader(p("config.xml")))

	-- several writes without a flush reach the file once the writers are flushed, in order
	local first, second = writer(p("order")), appender(p("order2"))
	first:write("1")
	second:write("a")
	first:write("2")
	second:write("b")
	first:flush()
	second:flush()
	out.order = { answer(fs.read(p("order"))), answer(fs.read(p("order2"))) }
	return out
end

local ok, out = pcall(body)
fs.remove_dir_all(dir, { missing_ok = true })
assert(ok, out)
return out
