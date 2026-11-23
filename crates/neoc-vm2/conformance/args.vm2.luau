-- intentional: parse returns a typed union: { kind = "parsed", flags, positional, rest, subcommand? } with a flag's value under flags and a subcommand as { name, flags, positional, rest }, or { kind = "help", text } (it was one table with the flags beside positional/rest and help = true, help_text); flags are a list of { name, type, ... } only (the map form is refused); a flag is a union on type, so default has the flag's type and a boolean has no default, required or multiple; a config of the wrong shape is refused as an argument; errors are typed (invalid_config, unknown_argument, invalid_value, missing_required, missing_subcommand, invalid_subcommand, conflict, usage) with clap's words in text.
local args = require("lib:args")

local tool = {
	name = "mytool",
	description = "does the work",
	flags = {
		{ name = "verbose", short = "v", type = "boolean" },
		{ name = "output", short = "o", type = "string", default = "stdout" },
		{ name = "count", type = "number" },
		{ name = "include", type = "string", multiple = true },
	},
} :: lib_args_Config

-- the outcome says what it is, and a flag's value is under `flags`
local parsed = assert(args.parse({ "-v", "--count=3", "--include", "a", "--include", "b", "in.txt", "--", "-x" }, tool))
assert(parsed.kind == "parsed")
assert(parsed.flags.verbose == true and parsed.flags.output == "stdout" and parsed.flags.count == 3)
local include = parsed.flags.include
assert(type(include) == "table" and #include == 2 and include[1] == "a" and include[2] == "b")
assert(parsed.positional[1] == "in.txt" and #parsed.positional == 1 and parsed.rest[1] == "-x")
assert(parsed.subcommand == nil)
-- a flag that was not given and has no default is not there; a multiple one is an empty list
local bare = assert(args.parse({}, tool))
assert(bare.kind == "parsed" and bare.flags.count == nil and bare.flags.verbose == false)
local none = bare.flags.include
assert(type(none) == "table" and #none == 0)

-- help is its own outcome
local help = assert(args.parse({ "--help" }, tool))
assert(help.kind == "help" and help.text:find("does the work", 1, true) ~= nil)
assert(help.text == assert(args.help(tool)))

-- a subcommand is a value of its own
local git = {
	name = "git",
	flags = { { name = "verbose", type = "boolean" } },
	subcommands = {
		build = { description = "build it", flags = { { name = "release", type = "boolean" } } },
		test = {},
	},
} :: lib_args_Config
local built = assert(args.parse({ "--verbose", "build", "--release", "input.txt" }, git))
assert(built.kind == "parsed" and built.flags.verbose == true and #built.positional == 0)
local subcommand = assert(built.subcommand)
assert(subcommand.name == "build" and subcommand.flags.release == true and subcommand.positional[1] == "input.txt")

-- errors say what went wrong
local function kind_of(argv: { string }, config: lib_args_Config): string
	local value, err = args.parse(argv, config)
	assert(value == nil and err ~= nil)
	assert(err.op == "lib:args.parse" and tostring(err) == `lib:args.parse: {err.kind}: {err.message}`)
	return err.kind
end
assert(kind_of({ "--nope" }, tool) == "unknown_argument")
assert(kind_of({ "--count=abc" }, tool) == "invalid_value")
assert(kind_of({ "--output", "a", "--output", "b" }, tool) == "conflict")
assert(kind_of({}, { name = "t", flags = { { name = "out", type = "string", required = true } } }) == "missing_required")
assert(kind_of({}, git) == "missing_subcommand")
assert(kind_of({ "deploy" }, git) == "invalid_subcommand")
local _, unknown = args.parse({ "--nope" }, tool)
assert(unknown ~= nil and unknown.kind == "unknown_argument" and unknown.text:find("--nope", 1, true) ~= nil)

-- what only clap's rules forbid is invalid_config
local twice = { name = "t", flags = { { name = "a", type = "boolean" }, { name = "a", type = "string" } } } :: lib_args_Config
assert(kind_of({}, twice) == "invalid_config")
local _, config_err = args.help(twice)
assert(config_err ~= nil and config_err.kind == "invalid_config")
assert(tostring(config_err) == "lib:args.help: invalid_config: t: flag '--a' is declared twice")

-- a config of the wrong shape is refused as an argument, with its place
for _, config in {
	{ flags = {} },
	{ name = "t", flags = { verbose = { type = "boolean" } } },
	{ name = "t", flags = { { name = "x", type = "object" } } },
	{ name = "t", flags = { { name = "x" } } },
	{ name = "t", flags = { { name = "v", type = "boolean", multiple = true } } },
	{ name = "t", flags = { { name = "v", type = "boolean", default = true } } },
	{ name = "t", flags = { { name = "n", type = "number", default = "three" } } },
	{ name = "t", subcommands = { build = { subcommands = { x = {} } } } },
	{ name = "t", flagz = {} },
} :: { any } do
	local ok, refused = pcall(function()
		return args.parse({}, config)
	end)
	assert(not ok and tostring(refused):find("lib:args.parse: argument #2 'config'", 1, true), tostring(refused))
	local ok_help, refused_help = pcall(function()
		return args.help(config)
	end)
	assert(not ok_help and tostring(refused_help):find("lib:args.help: argument #1 'config'", 1, true))
end
local ok, refused = pcall(function()
	return args.parse({ "a", 5 } :: any, tool)
end)
assert(not ok and tostring(refused):find("lib:args.parse: argument #1 'argv'", 1, true))
