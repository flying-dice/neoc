local _t = require("lib:test")
local describe = _t.describe
local test = _t.test
local expect = _t.expect

local args = require("lib:args")

describe("args.parse", function()
    test("boolean and string flags", function()
        local parsed, err = args.parse({ "--verbose", "--output=out.txt" }, {
            name = "mytool",
            flags = {
                verbose = { short = "v", type = "boolean" },
                output = { short = "o", type = "string" },
            },
        })
        expect(err).toBeNil()
        expect(parsed.verbose).toBe(true)
        expect(parsed.output).toBe("out.txt")
    end)

    test("defaults applied when absent", function()
        local parsed = args.parse({}, {
            name = "mytool",
            flags = {
                verbose = { type = "boolean" },
                count = { type = "number", default = 3 },
                output = { type = "string", default = "stdout" },
            },
        })
        expect(parsed.verbose).toBe(false)
        expect(parsed.count).toBe(3)
        expect(parsed.output).toBe("stdout")
    end)

    test("short alias accepted", function()
        local parsed = args.parse({ "-v", "-o", "x.txt" }, {
            name = "mytool",
            flags = {
                verbose = { short = "v", type = "boolean" },
                output = { short = "o", type = "string" },
            },
        })
        expect(parsed.verbose).toBe(true)
        expect(parsed.output).toBe("x.txt")
    end)

    test("number coercion accepts negative", function()
        local parsed = args.parse({ "--count=-7" }, {
            name = "mytool",
            flags = { count = { type = "number" } },
        })
        expect(parsed.count).toBe(-7)
    end)

    test("number coercion rejects garbage", function()
        local _, err = args.parse({ "--count=abc" }, {
            name = "mytool",
            flags = { count = { type = "number" } },
        })
        expect(err).toContain("abc")
    end)

    test("string flag space-separated value", function()
        local parsed = args.parse({ "--output", "file.txt" }, {
            name = "mytool",
            flags = { output = { type = "string" } },
        })
        expect(parsed.output).toBe("file.txt")
    end)

    test("multiple = true accumulates", function()
        local parsed = args.parse({ "--include=*.rs", "--include=*.toml" }, {
            name = "mytool",
            flags = { include = { type = "string", multiple = true } },
        })
        expect(#parsed.include).toBe(2)
        expect(parsed.include[1]).toBe("*.rs")
        expect(parsed.include[2]).toBe("*.toml")
    end)

    test("missing required flag is named", function()
        local _, err = args.parse({}, {
            name = "mytool",
            flags = { config = { type = "string", required = true } },
        })
        expect(err).toContain("--config")
    end)

    test("unknown flag errors", function()
        local _, err = args.parse({ "--mystery" }, {
            name = "mytool",
            flags = {},
        })
        expect(err).toContain("--mystery")
    end)

    test("rest after -- captured", function()
        local parsed = args.parse({ "--verbose", "--", "raw", "passthrough" }, {
            name = "mytool",
            flags = { verbose = { type = "boolean" } },
        })
        expect(#parsed.rest).toBe(2)
        expect(parsed.rest[1]).toBe("raw")
        expect(parsed.rest[2]).toBe("passthrough")
    end)

    test("rest is always a table", function()
        local parsed = args.parse({}, {
            name = "mytool",
            flags = {},
        })
        expect(type(parsed.rest)).toBe("table")
        expect(#parsed.rest).toBe(0)
    end)

    test("positional collected", function()
        local parsed = args.parse({ "--verbose", "a.txt", "b.txt" }, {
            name = "mytool",
            flags = { verbose = { type = "boolean" } },
        })
        expect(#parsed.positional).toBe(2)
        expect(parsed.positional[1]).toBe("a.txt")
        expect(parsed.positional[2]).toBe("b.txt")
    end)

    test("subcommand scoped namespace", function()
        local parsed = args.parse({ "build", "--output=out.txt", "src/main.lua" }, {
            name = "mytool",
            subcommands = {
                build = { flags = { output = { type = "string" } } },
                init = { flags = {} },
            },
        })
        expect(parsed.subcommand).toBe("build")
        expect(parsed.build.output).toBe("out.txt")
        expect(parsed.build.positional[1]).toBe("src/main.lua")
    end)

    test("unknown subcommand errors", function()
        local _, err = args.parse({ "frobnicate" }, {
            name = "mytool",
            subcommands = { build = { flags = {} } },
        })
        expect(err).toContain("frobnicate")
    end)

    test("missing subcommand errors", function()
        local _, err = args.parse({}, {
            name = "mytool",
            subcommands = { build = { flags = {} }, init = { flags = {} } },
        })
        expect(type(err)).toBe("string")
    end)

    test("--help short-circuits required-flag check", function()
        local parsed = args.parse({ "--help" }, {
            name = "mytool",
            description = "Build and deploy",
            flags = { config = { type = "string", required = true, desc = "Config path" } },
        })
        expect(parsed.help).toBe(true)
        expect(parsed.help_text).toContain("Build and deploy")
        expect(parsed.help_text).toContain("--config")
    end)

    test("-h short alias same as --help", function()
        local parsed = args.parse({ "-h" }, {
            name = "mytool",
            flags = {},
        })
        expect(parsed.help).toBe(true)
    end)

    test("help mode still emits positional and rest tables", function()
        local parsed = args.parse({ "--help" }, {
            name = "mytool",
            flags = {},
        })
        expect(parsed.help).toBe(true)
        expect(type(parsed.positional)).toBe("table")
        expect(#parsed.positional).toBe(0)
        expect(type(parsed.rest)).toBe("table")
        expect(#parsed.rest).toBe(0)
    end)

    test("--help after subcommand prints subcommand help", function()
        local parsed = args.parse({ "build", "--help" }, {
            name = "mytool",
            subcommands = {
                build = {
                    description = "Compile the project",
                    flags = { config = { type = "string", desc = "Config path" } },
                },
                init = { description = "Scaffold a project", flags = {} },
            },
        })
        expect(parsed.help).toBe(true)
        expect(parsed.help_text).toContain("Compile the project")
        expect(parsed.help_text).toContain("--config")
    end)

    test("--help before subcommand prints root help", function()
        local parsed = args.parse({ "--help" }, {
            name = "mytool",
            description = "Top-level tool",
            subcommands = {
                build = { description = "Compile the project", flags = {} },
            },
        })
        expect(parsed.help).toBe(true)
        expect(parsed.help_text).toContain("Top-level tool")
    end)
end)

describe("args.help", function()
    test("renders header, flags, and built-in help", function()
        local text = args.help({
            name = "mytool",
            description = "Build and deploy Lua projects",
            flags = {
                verbose = { short = "v", type = "boolean", desc = "Enable verbose output" },
                output = { short = "o", type = "string", desc = "Output file path" },
            },
        })
        expect(text).toContain("Build and deploy Lua projects")
        expect(text).toContain("--verbose")
        expect(text).toContain("Enable verbose output")
        expect(text).toContain("--output")
        expect(text).toContain("Output file path")
        expect(text).toContain("--help")
    end)

    test("array-form flags preserve definition order", function()
        local text = args.help({
            name = "mytool",
            flags = {
                { name = "zeta", short = "z", type = "boolean" },
                { name = "alpha", short = "a", type = "boolean" },
            },
        })
        local zpos = assert(string.find(text, "--zeta"), "zeta not in help")
        local apos = assert(string.find(text, "--alpha"), "alpha not in help")
        expect(zpos < apos).toBe(true)
    end)

    test("subcommand listed in help", function()
        local text = args.help({
            name = "mytool",
            subcommands = {
                build = { description = "Build the project", flags = {} },
                init = { description = "Initialise a project", flags = {} },
            },
        })
        expect(text).toContain("build")
        expect(text).toContain("Build the project")
    end)
end)
