--!strict
-- The machine as the app reads it: one `vnd:sysinfo` System for the whole app, measured when a window asks, and
-- turned into the overview and the list of processes the windows draw. Nothing here is kept between two calls but the
-- System itself: a rate is the difference between two measurements of it, so the windows that ask share it, and a
-- window that asks sooner than the CPU can be measured again (`too_soon`) is given the last measurement.
--
-- A function that could not do what it was asked answers with a plain sentence (the handler passes it on as it is).
local sysinfo = require("vnd:sysinfo")
local rpc = require("../.app/rpc")

local monitor = {}

-- how many children of a process one answer names: a process like init has hundreds, and a window shows a few screens
local MAX_CHILDREN = 100

local system: vnd_sysinfo_System? = nil

-- The one System, made when it is first needed. Making it measures once, so the first reading has memory, disks and
-- the processes, and rates and loads start with the second.
local function open(): (vnd_sysinfo_System?, string?)
  local existing = system
  if existing then
    return existing, nil
  end
  local created, err = sysinfo.system()
  if err then
    return nil, err.message
  end
  -- another call may have made one while this waited for the machine: keep that one, as its measurements are older
  system = system or created
  return system, nil
end

-- Measure everything again. A refresh that is too soon is not a failure: the last measurement is as good.
function monitor.refresh(): (vnd_sysinfo_System?, string?)
  local current, problem = open()
  if not current then
    return nil, problem
  end
  local _, err = current:refresh()
  if err and err.kind ~= "too_soon" then
    return nil, err.message
  end
  return current, nil
end

-- The System as it was last measured, without measuring.
function monitor.system(): (vnd_sysinfo_System?, string?)
  return open()
end

function monitor.canTerminate(): boolean
  return sysinfo.capabilities().terminate
end

local function overviewOf(current: vnd_sysinfo_System, count: number): rpc.Overview
  local cpu = current:cpu()
  local cores: { rpc.Core } = {}
  for _, core in cpu.cores do
    table.insert(cores, { usage = core.usage })
  end
  local load = cpu.load_average
  local memory = current:memory()
  local swap = current:swap()
  local disks: { rpc.Disk } = {}
  for _, disk in current:disks() do
    table.insert(disks, {
      name = disk.name,
      mountPoint = disk.mount_point,
      fileSystem = disk.file_system,
      totalBytes = disk.total_bytes,
      availableBytes = disk.available_bytes,
      readRate = disk.read_rate,
      writeRate = disk.write_rate,
    })
  end
  local networks: { rpc.Network } = {}
  for _, network in current:networks() do
    table.insert(
      networks,
      { name = network.name, receiveRate = network.receive_rate, transmitRate = network.transmit_rate }
    )
  end
  local host = current:host()
  return {
    cpu = {
      sampled = cpu.sampled,
      usage = cpu.usage,
      cores = cores,
      logicalCores = cpu.logical_cores,
      physicalCores = cpu.physical_cores,
      brand = cpu.brand,
      loadAverage = if load then { one = load.one, five = load.five, fifteen = load.fifteen } else nil,
    },
    memory = { total = memory.total, used = memory.used, available = memory.available },
    swap = { total = swap.total, used = swap.used },
    disks = disks,
    networks = networks,
    machine = {
      name = host.name,
      osName = host.os_name,
      osVersion = host.os_version,
      uptimeSecs = host.uptime_secs,
      arch = host.arch,
    },
    unsupported = current:unsupported(),
    processCount = count,
    canTerminate = monitor.canTerminate(),
  }
end

local function rowOf(process: vnd_sysinfo_Process): rpc.ProcessRow
  return {
    pid = process.pid,
    startTime = process.start_time,
    observation = process.observation,
    name = process.name,
    user = process:user(),
    cpu = process:cpu_usage(),
    memory = process:memory() or 0,
    status = process:status(),
  }
end

local function relatedOf(process: vnd_sysinfo_Process): rpc.Related
  return {
    pid = process.pid,
    startTime = process.start_time,
    observation = process.observation,
    name = process.name,
    user = process:user(),
  }
end

-- One process in full. One that has exited is what it was when it was last seen: its name, and nothing else.
function monitor.detail(process: vnd_sysinfo_Process, control: rpc.Control): rpc.Detail
  local parent = process:parent()
  local children: { rpc.Related } = {}
  for _, child in process:children() do
    if #children >= MAX_CHILDREN then
      break
    end
    table.insert(children, relatedOf(child))
  end
  table.sort(children, function(a, b)
    return a.pid < b.pid
  end)
  return {
    pid = process.pid,
    startTime = process.start_time,
    observation = process.observation,
    name = process.name,
    alive = process:alive(),
    control = control,
    status = process:status(),
    user = process:user(),
    cpu = process:cpu_usage(),
    memory = process:memory(),
    virtualMemory = process:virtual_memory(),
    runTimeSecs = process:run_time(),
    command = process:command() or {},
    exe = process:exe(),
    parent = if parent then relatedOf(parent) else nil,
    children = children,
  }
end

-- The overview and every process, as last measured.
function monitor.read(current: vnd_sysinfo_System): (rpc.Overview, { rpc.ProcessRow })
  local rows: { rpc.ProcessRow } = {}
  for _, process in current:processes() do
    table.insert(rows, rowOf(process))
  end
  return overviewOf(current, #rows), rows
end

-- The process a window named, if it is still the one it saw: the PID, when it started, and which sighting of the PID it
-- is (a PID that was given to another process since, even in the same second, has another number).
function monitor.find(current: vnd_sysinfo_System, ref: rpc.ProcessRef): vnd_sysinfo_Process?
  local process = current:process(ref.pid)
  if process and process.start_time == ref.startTime and process.observation == ref.observation then
    return process
  end
  return nil
end

return monitor
