-- lib:args on neoc-vm and neoc-vm2: what both must answer the same. The configs are the list form both take; what
-- parse returns was reshaped (args.vm2.luau), so `normal` below reads either shape into one, and everything read
-- from the arguments is compared. A failure is compared as a failure: its words are clap's on both, but neoc-vm2
-- wraps them in a typed error.
local args = require("lib:args")
local out = {}

-- either VM's result as { help?, flags, positional, rest, subcommand? }
local function normal(parsed: any): any
	if parsed.kind == "help" or parsed.help == true then
		return { help = parsed.text or parsed.help_text }
	end
	local function command(config: any, from: any, flags: any): any
		local values = {}
		for _, flag in config.flags or {} do
			local value = flags[flag.name]
			values[flag.name] = if value == nil then "<nil>" else value
		end
		return { flags = values, positional = from.positional, rest = from.rest }
	end
	return command
end

local function parse(argv: { string }, config: any): any
	local parsed, err = args.parse(argv, config)
	if parsed == nil then
		return { failed = err ~= nil }
	end
	local read = normal(parsed)
	if type(read) == "table" then
		return read
	end
	local top = read(config, parsed, parsed.flags or parsed)
	local subcommand = parsed.subcommand
	if type(subcommand) == "table" then -- neoc-vm2
		top.subcommand = subcommand.name
		top.sub = read(config.subcommands[subcommand.name], subcommand, subcommand.flags)
	elseif subcommand ~= nil then -- neoc-vm
		top.subcommand = subcommand
		top.sub = read(config.subcommands[subcommand], parsed[subcommand], parsed[subcommand])
	end
	return top
end

local tool = {
	name = "mytool",
	description = "does the work",
	flags = {
		{ name = "verbose", short = "v", type = "boolean", desc = "say more" },
		{ name = "output", short = "o", type = "string", default = "stdout" },
		{ name = "count", type = "number" },
		{ name = "include", short = "I", type = "string", multiple = true },
		{ name = "weight", type = "number", multiple = true },
	},
}

out.flags = parse({ "--verbose", "--output=out.txt", "--count", "42" }, tool)
out.defaults = parse({}, tool)
out.shorts = parse({ "-v", "-o", "x.txt", "-I", "a", "-I", "b" }, tool)
out.negative = parse({ "--count=-7", "--weight", "1.5", "--weight=-2" }, tool)
out.positional = parse({ "a", "--verbose", "b" }, tool)
out.rest = parse({ "x", "--", "--y", "z" }, tool)
out.failures = {
	parse({ "--count=abc" }, tool),
	parse({ "--nope" }, tool),
	parse({ "--output" }, tool),
	parse({ "--output", "a", "--output", "b" }, tool),
	parse({}, { name = "t", flags = { { name = "out", type = "string", required = true } } }),
}

-- subcommands
local git = {
	name = "git",
	flags = { { name = "verbose", type = "boolean" } },
	subcommands = {
		build = { description = "build it", flags = { { name = "release", short = "r", type = "boolean" } } },
		test = { flags = { { name = "filter", type = "string" } } },
	},
}
out.subcommand = parse({ "--verbose", "build", "-r", "input.txt", "--", "-x" }, git)
out.subcommand_other = parse({ "test", "--filter=fast" }, git)
out.subcommand_failures = { parse({}, git), parse({ "deploy" }, git), parse({ "build", "--filter", "x" }, git) }

-- help: before a subcommand it is the top command's, after it the subcommand's
out.help = {
	parse({ "--help" }, tool),
	parse({ "-h" }, tool),
	parse({ "--help" }, { name = "t", flags = { { name = "out", type = "string", required = true } } }),
	parse({ "--help", "build" }, git),
	parse({ "build", "--help" }, git),
}
out.help_text = { args.help(tool), args.help(git), (args.help({ name = "bare" })) }

-- a flag may claim -h: --help still asks for help
local host = {
	name = "t",
	description = "tool",
	flags = { { name = "host", short = "h", type = "string" }, { name = "port", short = "p", type = "number" } },
	subcommands = nil :: any,
}
out.claims_h = { parse({ "-h", "example.org", "-p", "80" }, host), parse({ "--help" }, host) }

-- what clap would panic on is an error
for i, config in {
	{ name = "t", flags = { { name = "a", type = "boolean" }, { name = "a", type = "string" } } },
	{ name = "t", flags = { { name = "help", type = "boolean" } } },
	{ name = "t", flags = { { name = "__rest", type = "string" } } },
	{ name = "t", flags = { { name = "a", short = "b", type = "boolean" }, { name = "b", short = "b", type = "boolean" } } },
	{ name = "t", flags = { { name = "--a", type = "boolean" } } },
	{ name = "t", flags = { { name = "a", short = "-", type = "boolean" } } },
	{ name = "t", flags = { { name = "a", short = "ab", type = "boolean" } } },
	{ name = "t", subcommands = { help = {} } },
} :: { any } do
	local help, help_err = args.help(config)
	out["hostile_" .. i] = { parse({}, config), parse({ "-h" }, config), help == nil, help_err ~= nil }
end

return out
