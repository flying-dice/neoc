local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local env = require("std:env")
local task = require("std:task")
local process = require("std:process")

-- Real processes: `sh` and what every Unix has beside it. (The Rust tests of the module cover the handle being
-- collected and the engine shutting down.) On Windows only what is refused before anything starts is run: the
-- commands below are Unix's.
local unix = env.family == "unix"

-- read until the output holds `needle`
local function read_until(child, needle)
    local seen = ""
    while not string.find(seen, needle, 1, true) do
        local output, err = child:read()
        assert(err == nil, err)
        assert(output, "the output ended before `" .. needle .. "`: " .. seen)
        seen = seen .. output
    end
    return seen
end

-- read to the end of the output
local function read_all(child, method)
    local seen = ""
    while true do
        local output, err = child[method or "read"](child)
        assert(err == nil, err)
        if not output then
            return seen
        end
        seen = seen .. output
    end
end

local function sh(script, opts)
    opts = opts or {}
    opts.program = "/bin/sh"
    opts.args = { "-c", script }
    return assert(process.spawn(opts))
end

describe("process", function()

    describe("spawn", function()
        test("a missing program is a spawn error", function()
            local child, err = process.spawn({ program = "/nowhere/neoc-no-such-program" })
            expect(child).toBeNil()
            expect(err).toContain("process.spawn: spawn:")
            expect(process.error_kind(err)).toBe("spawn")
        end)

        test("a directory that is not there is a spawn error", function()
            local child, err = process.spawn({ program = "sh", cwd = "/nowhere/neoc-no-such-directory" })
            expect(child).toBeNil()
            expect(process.error_kind(err)).toBe("spawn")
        end)

        test("a command needs a program", function()
            for _, opts in ipairs({ {}, { args = { "-l" } }, { program = "" } }) do
                local child, err = process.spawn(opts)
                expect(child).toBeNil()
                expect(err).toContain("`program`")
                expect(process.error_kind(err)).toBe("invalid_input")
            end
        end)

        test("an unknown option is refused", function()
            local child, err = process.spawn({ progam = "sh" })
            expect(child).toBeNil()
            expect(err).toContain("unknown option `progam`")
            expect(process.error_kind(err)).toBe("invalid_input")
        end)

        test("a stream is piped, null or inherit", function()
            local child, err = process.spawn({ program = "sh", stdout = "pipe" })
            expect(child).toBeNil()
            expect(err).toContain("`stdout`")
            expect(process.error_kind(err)).toBe("invalid_input")
        end)

        test("error_kind is nil for what is not one of its errors", function()
            expect(process.error_kind(nil)).toBeNil()
            expect(process.error_kind("fs.read: not_found: x")).toBeNil()
        end)
    end)

    if not unix then
        return
    end

    describe("a child", function()
        test("what is written is its input, and what it writes is read, line after line", function()
            local child = sh('while read -r line; do echo "re:$line"; done')
            expect(type(child:id())).toBe("number")
            for turn = 1, 3 do
                expect(child:write("turn " .. turn .. "\n")).toBe(true)
                expect(read_until(child, "\n")).toBe("re:turn " .. turn .. "\n")
            end
            expect(child:close_stdin()).toBe(true)
            expect(read_all(child)).toBe("")
            expect(assert(child:wait()).success).toBe(true)
            child:close()
        end)

        test("the two outputs are apart, and wait returns the exit code", function()
            local child = sh("printf out; printf err >&2; exit 7")
            local status, err = child:wait()
            expect(err).toBeNil()
            expect(status.code).toBe(7)
            expect(status.success).toBe(false)
            expect(status.signal).toBeNil()
            expect(child:id()).toBeNil()
            expect(read_all(child)).toBe("out")
            expect(read_all(child, "read_stderr")).toBe("err")
            child:close()
        end)

        test("the command gets its arguments whole, its directory and its environment", function()
            local child = assert(process.spawn({
                program = "/bin/sh",
                args = { "-c", 'printf "%s|%s|%s" "$1" "$NEOC_PROCESS_TEST" "$(pwd)"', "sh", "a b $HOME *" },
                cwd = "/",
                env = { NEOC_PROCESS_TEST = "set here" },
            }))
            expect(read_all(child)).toBe("a b $HOME *|set here|/")
            child:close()
        end)

        test("clear_env leaves only what env gives", function()
            local child = assert(process.spawn({ program = "/usr/bin/env", env = { ONLY = "this" }, clear_env = true }))
            expect(read_all(child)).toBe("ONLY=this\n")
            child:close()
        end)

        test("a stream that is not a pipe is not read or written", function()
            local child = sh("cat; echo done", { stdin = "null", stderr = "null" })
            local written, write_err = child:write("x")
            expect(written).toBeNil()
            expect(process.error_kind(write_err)).toBe("closed")
            local errors, read_err = child:read_stderr()
            expect(errors).toBeNil()
            expect(process.error_kind(read_err)).toBe("closed")
            expect(read_all(child)).toBe("done\n")
            child:close()
        end)

        test("kill ends the process, and wait says a signal did", function()
            local child = sh("echo ready; exec sleep 1000")
            read_until(child, "ready")
            child:kill()
            local status = assert(child:wait())
            expect(status.code).toBeNil()
            expect(status.signal).toBe(9)
            expect(status.success).toBe(false)
            child:close()
        end)

        test("close ends a waiting read and refuses a write", function()
            local child = sh("echo ready; exec sleep 1000")
            read_until(child, "ready")
            local reading = task.spawn(function()
                return child:read()
            end)
            task.wait(0.05)
            child:close()
            local output, err = reading:await()
            expect(output).toBeNil()
            expect(err).toBeNil()
            local written, write_err = child:write("x")
            expect(written).toBeNil()
            expect(process.error_kind(write_err)).toBe("closed")
            expect(assert(child:wait()).success).toBe(false)
        end)

        test("two processes are talked to at once", function()
            local slow = sh("sleep 0.2; echo slow")
            local fast = sh("echo fast")
            local order = {}
            local tasks = {}
            for name, child in pairs({ slow = slow, fast = fast }) do
                table.insert(tasks, task.spawn(function()
                    read_until(child, name)
                    table.insert(order, name)
                end))
            end
            for _, waiting in ipairs(tasks) do
                waiting:await()
            end
            expect(order[1]).toBe("fast")
            expect(order[2]).toBe("slow")
            slow:close()
            fast:close()
        end)
    end)
end)
