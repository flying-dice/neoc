local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local env = require("std:env")
local task = require("std:task")
local portable_pty = require("vnd:portable_pty")

-- Real terminals with real processes on them: `sh` and what every Unix has beside it. (The Rust tests of the module
-- cover the handle being collected and the engine shutting down.) On Windows only what is refused before anything
-- starts is run: the commands below are Unix's.
local unix = env.family == "unix"

-- read until the output holds `needle`
local function read_until(pty, needle)
    local seen = ""
    while not string.find(seen, needle, 1, true) do
        local output, err = pty:read()
        assert(err == nil, err)
        assert(output, "the output ended before `" .. needle .. "`: " .. seen)
        seen = seen .. output
    end
    return seen
end

local function sh(script, opts)
    opts = opts or {}
    opts.program = "/bin/sh"
    opts.args = { "-c", script }
    return assert(portable_pty.spawn_command(opts))
end

describe("portable_pty", function()

    describe("spawn_command", function()
        test("a missing program is a spawn error", function()
            local pty, err = portable_pty.spawn_command({ program = "/nowhere/neoc-no-such-program" })
            expect(pty).toBeNil()
            expect(err).toContain("portable_pty.spawn_command: spawn:")
            expect(portable_pty.error_kind(err)).toBe("spawn")
        end)

        test("a directory that is not there is a spawn error", function()
            local pty, err = portable_pty.spawn_command({ cwd = "/nowhere/neoc-no-such-directory" })
            expect(pty).toBeNil()
            expect(portable_pty.error_kind(err)).toBe("spawn")
        end)

        test("an unknown option is refused", function()
            local pty, err = portable_pty.spawn_command({ progam = "sh" })
            expect(pty).toBeNil()
            expect(err).toContain("unknown option `progam`")
            expect(portable_pty.error_kind(err)).toBe("invalid_input")
        end)

        test("a size that is not a whole number from 1 is refused", function()
            for _, rows in ipairs({ 0, -1, 1.5, 40000 }) do
                local pty, err = portable_pty.spawn_command({ program = "sh", rows = rows })
                expect(pty).toBeNil()
                expect(portable_pty.error_kind(err)).toBe("invalid_input")
            end
        end)

        test("arguments without a program are refused", function()
            local pty, err = portable_pty.spawn_command({ args = { "-l" } })
            expect(pty).toBeNil()
            expect(portable_pty.error_kind(err)).toBe("invalid_input")
        end)

        test("error_kind is nil for what is not one of its errors", function()
            expect(portable_pty.error_kind(nil)).toBeNil()
            expect(portable_pty.error_kind("fs.read: not_found: x")).toBeNil()
        end)

        test("error_message is what follows the kind, and nil for what is not one of its errors", function()
            local pty, err = portable_pty.spawn_command({ args = { "-l" } })
            expect(pty).toBeNil()
            local message = portable_pty.error_message(err)
            expect(message ~= "").toBe(true)
            expect(err).toBe("portable_pty.spawn_command: invalid_input: " .. message)
            expect(portable_pty.error_message(nil)).toBeNil()
            expect(portable_pty.error_message("fs.read: not_found: x")).toBeNil()
        end)
    end)

    if not unix then
        return
    end

    describe("a terminal", function()
        test("what the command writes is read, and what is written is its input", function()
            local pty = sh("echo ready; cat")
            read_until(pty, "ready")
            expect(type(pty:process_id())).toBe("number")
            expect(pty:write("hello\n")).toBe(true)
            -- the terminal echoes what is typed, and `cat` says it again
            local output = read_until(pty, "hello\r\nhello")
            expect(output).toContain("hello")
            pty:close()
        end)

        test("the command gets its directory, its environment and the terminal's size", function()
            local pty = sh('echo "$NEOC_PTY_TEST|$TERM|$(stty size)|end"', {
                cwd = "/",
                env = { NEOC_PTY_TEST = "set here", TERM = "xterm-256color" },
                rows = 30,
                cols = 100,
            })
            expect(read_until(pty, "|end")).toContain("set here|xterm-256color|30 100|end")
            pty:close()
        end)

        test("resize tells the terminal its new size", function()
            local pty = sh("echo ready; read line; stty size; cat")
            read_until(pty, "ready")
            expect(pty:resize(40, 120)).toBe(true)
            assert(pty:write("\n"))
            expect(read_until(pty, "40 120")).toContain("40 120")
            local resized, err = pty:resize(0, 80)
            expect(resized).toBeNil()
            expect(portable_pty.error_kind(err)).toBe("invalid_input")
            pty:close()
        end)

        test("wait returns the command's exit code, and the output ends", function()
            local pty = sh("printf bye; exit 7")
            local status, err = pty:wait()
            expect(err).toBeNil()
            expect(status.exit_code).toBe(7)
            expect(status.success).toBe(false)
            expect(status.signal).toBeNil()
            expect(pty:process_id()).toBeNil()
            local output = ""
            while true do
                local piece, read_err = pty:read()
                expect(read_err).toBeNil()
                if not piece then
                    break
                end
                output = output .. piece
            end
            expect(output).toBe("bye")
            pty:close()
        end)

        test("kill ends the command, and wait says a signal did", function()
            local pty = sh("echo ready; cat")
            read_until(pty, "ready")
            pty:kill()
            local status = assert(pty:wait())
            expect(status.success).toBe(false)
            expect(type(status.signal)).toBe("string")
            pty:close()
        end)

        test("a read asks for at most what it is told", function()
            local pty = sh("printf 0123456789; cat")
            local output = assert(pty:read(4))
            expect(#output <= 4).toBe(true)
            local none, err = pty:read(0)
            expect(none).toBeNil()
            expect(portable_pty.error_kind(err)).toBe("invalid_input")
            pty:close()
        end)

        test("other tasks run while a read waits", function()
            local pty = sh("echo ready; cat")
            read_until(pty, "ready")
            local ticks = 0
            local ticking = task.spawn(function()
                for _ = 1, 3 do
                    task.wait(0.01)
                    ticks = ticks + 1
                end
            end)
            local reading = task.spawn(function()
                return pty:read()
            end)
            ticking:await()
            expect(ticks).toBe(3)
            assert(pty:write("x\n"))
            expect(reading:await()).toContain("x")
            pty:close()
        end)
    end)

    describe("a closed terminal", function()
        test("ends its reads, refuses writes and resizes, and still says how the command ended", function()
            local pty = sh("echo ready; cat; exit 3")
            read_until(pty, "ready")
            local waiting = task.spawn(function()
                return pty:read()
            end)
            task.wait(0.02)
            pty:close()
            local output, err = waiting:await()
            expect(output).toBeNil()
            expect(err).toBeNil()

            local written, write_err = pty:write("x")
            expect(written).toBeNil()
            expect(write_err).toContain("portable_pty.write: closed:")
            expect(portable_pty.error_kind(write_err)).toBe("closed")
            local resized, resize_err = pty:resize(10, 10)
            expect(resized).toBeNil()
            expect(portable_pty.error_kind(resize_err)).toBe("closed")
            expect(pty:process_id()).toBeNil()

            local status = assert(pty:wait())
            expect(status.success).toBe(false)
            -- closing and killing again do nothing
            pty:close()
            pty:kill()
        end)
    end)
end)
