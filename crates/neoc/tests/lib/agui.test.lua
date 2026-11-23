local _t       = require("lib:test")
local describe = _t.describe
local test     = _t.test
local expect   = _t.expect

local agui  = require("lib:agui")
local hyper = require("vnd:hyper")
local net   = require("std:net")

-- An agent in this process: it answers every run with the events of `script[path]`, and keeps what it was sent.
local received = {}
local script = {
    ["/text"] = {
        '{"type":"RUN_STARTED","threadId":"t","runId":"r"}',
        '{"type":"TEXT_MESSAGE_START","messageId":"m","role":"assistant"}',
        '{"type":"TEXT_MESSAGE_CONTENT","messageId":"m","delta":"Hi "}',
        '{"type":"TEXT_MESSAGE_CONTENT","messageId":"m","delta":"there"}',
        '{"type":"TEXT_MESSAGE_END","messageId":"m"}',
        '{"type":"RUN_FINISHED","threadId":"t","runId":"r"}',
    },
    ["/activity"] = {
        '{"type":"ACTIVITY_SNAPSHOT","messageId":"a","activityType":"PLAN","content":{"steps":["read"]}}',
        '{"type":"ACTIVITY_DELTA","messageId":"a","activityType":"PLAN","patch":[{"op":"add","path":"/steps/-","value":"write"}]}',
    },
}

local listener = assert(net.TcpListener.bind("127.0.0.1:0"))
local server = assert(hyper.serve(listener, function(req)
    received[req.path] = { body = req.body, accept = req:header("accept"), token = req:header("authorization") }
    local events = script[req.path]
    if not events then
        return { status = 404, body = "no such agent" }
    end
    local lines = {}
    for _, event in ipairs(events) do
        table.insert(lines, "data: " .. event .. "\n\n")
    end
    return { status = 200, headers = { ["content-type"] = "text/event-stream" }, body = table.concat(lines) }
end))
local base = "http://" .. server:addr()

local function input()
    return { thread_id = "t", run_id = "r", messages = { { id = "u1", role = "user", content = "hello" } } }
end

-- every event of a run, which must end without an error
local function all(run)
    local events = {}
    while true do
        local event, err = run:next()
        assert(err == nil, err)
        if not event then
            return events
        end
        table.insert(events, event)
    end
end

describe("lib:agui", function()
    test("a run sends the input and yields the agent's events", function()
        local run = assert(agui.run({ url = base .. "/text", input = input(), headers = { Authorization = "Bearer k" } }))
        local text, types = "", {}
        for _, event in ipairs(all(run)) do
            table.insert(types, event.type)
            if event.type == "TEXT_MESSAGE_CONTENT" then
                expect(event.message_id).toBe("m")
                text = text .. event.delta
            end
        end
        expect(text).toBe("Hi there")
        expect(types[1]).toBe("RUN_STARTED")
        expect(types[#types]).toBe("RUN_FINISHED")

        local sent = received["/text"]
        expect(sent.accept).toBe("text/event-stream")
        expect(sent.token).toBe("Bearer k")
        -- the wire is camelCase
        expect(sent.body).toBe(
            '{"threadId":"t","runId":"r","messages":[{"role":"user","id":"u1","content":"hello"}],"tools":[],"context":[],"forwardedProps":{}}'
        )
    end)

    test("an activity is a snapshot and the patches to it", function()
        local run = assert(agui.run({ url = base .. "/activity", input = input() }))
        local events = all(run)
        expect(events[1].activity_type).toBe("PLAN")
        expect(events[1].replace).toBe(true)
        local content = assert(agui.apply_patch(events[1].content, events[2].patch))
        expect(#content.steps).toBe(2)
        expect(content.steps[2]).toBe("write")
        expect(#events[1].content.steps).toBe(1)
    end)

    test("a failure has a kind", function()
        local run, err = agui.run({ url = base .. "/nowhere", input = input() })
        expect(run).toBe(nil)
        expect(agui.error_kind(err)).toBe("status")
        run, err = agui.run({ url = base .. "/text", input = { run_id = "r" } })
        expect(run).toBe(nil)
        expect(agui.error_kind(err)).toBe("invalid_input")
        expect(agui.error_kind("something else")).toBe(nil)
    end)

    test("a closed run has no more events", function()
        local run = assert(agui.run({ url = base .. "/text", input = input() }))
        run:close()
        local event, err = run:next()
        expect(event).toBe(nil)
        expect(err).toBe(nil)
    end)

    test("the module lists the event types it reads", function()
        expect(#agui.EVENT_TYPES).toBe(36)
        expect(agui.EVENT_TYPES[1]).toBe("TEXT_MESSAGE_START")
    end)
end)

server:close()
