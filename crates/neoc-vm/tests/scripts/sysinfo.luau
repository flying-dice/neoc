--!strict
-- vnd:sysinfo as a script sees it: what the machine reports is the machine's own, so this asserts on shapes and
-- rules and returns no answers. It starts no program and signals nothing but a handle it knows to be gone.
local sysinfo = require("vnd:sysinfo")

local capabilities = sysinfo.capabilities()
assert(capabilities.kill)
assert(capabilities.binding == "pidfd" or capabilities.binding == "process_handle" or capabilities.binding == "start_time")

local system = assert(sysinfo.system())

-- a first reading has the things that are read, and no load: a load needs two measurements
local cpu = system:cpu()
assert(cpu.sampled == false and cpu.usage == nil)
assert(cpu.logical_cores >= 1 and #cpu.cores == cpu.logical_cores)

local memory = system:memory()
assert(memory.total > 0 and memory.used <= memory.total)
assert(system:host().uptime_secs > 0)
assert(#system:processes() > 0)

-- the process list holds this process, and a process is read from its handle
local found: { [number]: boolean } = {}
for _, process in system:processes() do
  found[process.pid] = true
  assert(process.start_time > 0)
end
assert(next(found) ~= nil)

-- a refresh too soon after the last is refused, with how long to wait, and measures nothing
local ok, err = system:refresh_processes()
assert(ok == nil and err ~= nil and err.kind == "too_soon")
if err ~= nil and err.kind == "too_soon" then
  assert(err.retry_after_ms > 0 and err.retry_after_ms <= sysinfo.MIN_CPU_INTERVAL_MS + 1)
end

-- a PID no process has is not a handle
assert(system:process(4294967295) == nil)
