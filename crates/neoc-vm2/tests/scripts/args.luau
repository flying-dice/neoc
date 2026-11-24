--!strict
-- lib:args as a script sees it: its answers (args.json), then what it asserts.
local args = require("lib:args")
local out = {}

-- what a command was given: every flag of its config (`"<nil>"` for one with no value), the positionals, the rest
local function read(config: any, from: any): any
  local values = {}
  for _, flag in config.flags or {} do
    local value = from.flags[flag.name]
    values[flag.name] = if value == nil then "<nil>" else value
  end
  return { flags = values, positional = from.positional, rest = from.rest }
end

-- a result as { failed } or { help } or { flags, positional, rest, subcommand?, sub? }
local function parse(argv: { string }, config: any): any
  local parsed, err = args.parse(argv, config)
  if parsed == nil then
    return { failed = err ~= nil }
  end
  if parsed.kind == "help" then
    return { help = parsed.text }
  end
  local top = read(config, parsed)
  local subcommand = parsed.subcommand
  if subcommand ~= nil then
    top.subcommand = subcommand.name
    top.sub = read(config.subcommands[subcommand.name], subcommand)
  end
  return top
end

local tool: any = {
  name = "mytool",
  description = "does the work",
  flags = {
    { name = "verbose", short = "v", type = "boolean", desc = "say more" },
    { name = "output", short = "o", type = "string", default = "stdout" },
    { name = "count", type = "number" },
    { name = "include", short = "I", type = "string", multiple = true },
    { name = "weight", type = "number", multiple = true },
  } :: { any },
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
local git: any = {
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
out.help_text = { args.help(tool), args.help(git), (args.help { name = "bare" }) }

-- a flag may claim -h: --help still asks for help
local host = {
  name = "t",
  description = "tool",
  flags = { { name = "host", short = "h", type = "string" }, { name = "port", short = "p", type = "number" } },
  subcommands = nil :: any,
}
out.claims_h = { parse({ "-h", "example.org", "-p", "80" }, host), parse({ "--help" }, host) }

-- what clap would panic on is an error
for i, config in
  {
    { name = "t", flags = { { name = "a", type = "boolean" }, { name = "a", type = "string" } } },
    { name = "t", flags = { { name = "help", type = "boolean" } } },
    { name = "t", flags = { { name = "__rest", type = "string" } } },
    {
      name = "t",
      flags = { { name = "a", short = "b", type = "boolean" }, { name = "b", short = "b", type = "boolean" } },
    },
    { name = "t", flags = { { name = "--a", type = "boolean" } } },
    { name = "t", flags = { { name = "a", short = "-", type = "boolean" } } },
    { name = "t", flags = { { name = "a", short = "ab", type = "boolean" } } },
    { name = "t", subcommands = { help = {} } },
  } :: { any }
do
  local help, help_err = args.help(config)
  out["hostile_" .. i] = { parse({}, config), parse({ "-h" }, config), help == nil, help_err ~= nil }
end

-- what it asserts
do
  -- a list of flags is annotated where it is written: the checker does not pick a flag's variant from a
  -- literal nested in a config
  local tool_flags: { lib_args_Flag } = {
    { name = "verbose", short = "v", type = "boolean" },
    { name = "output", short = "o", type = "string", default = "stdout" },
    { name = "count", type = "number" },
    { name = "include", type = "string", multiple = true },
  }
  local tool: lib_args_Config = {
    name = "mytool",
    description = "does the work",
    flags = tool_flags,
  }

  -- the outcome says what it is, and a flag's value is under `flags`
  local parsed =
    assert(args.parse({ "-v", "--count=3", "--include", "a", "--include", "b", "in.txt", "--", "-x" }, tool))
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
  local git_flags: { lib_args_Flag } = { { name = "verbose", type = "boolean" } }
  local build_flags: { lib_args_Flag } = { { name = "release", type = "boolean" } }
  local git: lib_args_Config = {
    name = "git",
    flags = git_flags,
    subcommands = {
      build = { description = "build it", flags = build_flags },
      test = {},
    },
  }
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
  local required_flags: { lib_args_Flag } = { { name = "out", type = "string", required = true } }
  assert(kind_of({}, { name = "t", flags = required_flags }) == "missing_required")
  assert(kind_of({}, git) == "missing_subcommand")
  assert(kind_of({ "deploy" }, git) == "invalid_subcommand")
  local _, unknown = args.parse({ "--nope" }, tool)
  assert(unknown ~= nil and unknown.kind == "unknown_argument" and unknown.text:find("--nope", 1, true) ~= nil)

  -- what only clap's rules forbid is invalid_config
  local twice_flags: { lib_args_Flag } = { { name = "a", type = "boolean" }, { name = "a", type = "string" } }
  local twice: lib_args_Config = { name = "t", flags = twice_flags }
  assert(kind_of({}, twice) == "invalid_config")
  local _, config_err = args.help(twice)
  assert(config_err ~= nil and config_err.kind == "invalid_config")
  assert(tostring(config_err) == "lib:args.help: invalid_config: t: flag '--a' is declared twice")

  -- a config of the wrong shape is refused as an argument, with its place
  for _, config in
    {
      { flags = {} },
      { name = "t", flags = { verbose = { type = "boolean" } } },
      { name = "t", flags = { { name = "x", type = "object" } } },
      { name = "t", flags = { { name = "x" } } },
      { name = "t", flags = { { name = "v", type = "boolean", multiple = true } } },
      { name = "t", flags = { { name = "v", type = "boolean", default = true } } },
      { name = "t", flags = { { name = "n", type = "number", default = "three" } } },
      { name = "t", subcommands = { build = { subcommands = { x = {} } } } },
      { name = "t", flagz = {} },
    } :: { any }
  do -- deliberate refusal test: each is a config of the wrong shape
    local ok, refused = pcall(function()
      args.parse({}, config)
    end)
    assert(not ok and tostring(refused):find("lib:args.parse: argument #2 'config'", 1, true), tostring(refused))
    local ok_help, refused_help = pcall(function()
      args.help(config)
    end)
    assert(not ok_help and tostring(refused_help):find("lib:args.help: argument #1 'config'", 1, true))
  end
  local ok, refused = pcall(function()
    args.parse({ "a", 5 :: any }, tool) -- deliberate refusal test: an argument that is not text
  end)
  assert(not ok and tostring(refused):find("lib:args.parse: argument #1 'argv'", 1, true))
end

return out
