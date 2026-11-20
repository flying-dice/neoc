--!strict
-- Luau syntax and types with the module declarations: type annotations, `local x: T`, type aliases that
-- name a module's resource type (the declarations export them), `continue`, compound assignment, string interpolation, if-expressions and
-- generic functions. Checked clean like usage.luau.
local json = require("lib:json")
local semver = require("lib:semver")
local time = require("std:time")
local url = require("lib:url")

-- a module's resource types are named `<ns>_<module>_<Type>`; an alias of one reads better in a script
type Timeout = std_time_Duration
type Parsed = lib_url_Url

local function describe(version: lib_semver_Version): string
	return `v{version.major}.{version.minor}.{version.patch}`
end

local function budget(limit: Timeout?): number
	local fallback: Timeout = time.Duration.from_secs(30)
	return (limit or fallback):as_secs()
end

local function host_of(target: Parsed): string
	return target.host or "localhost"
end

-- a typed local, compound assignment, `continue`, string interpolation
local total: number = 0
for index, word in { "a", "bb", "ccc" } do
	if #word < 2 then
		continue
	end
	total += #word * index
end
local label = if total > 3 then "big" else "small"
print(`total {total} is {label}`)

-- a generic function over a module value
local function first<T>(items: { T }): T?
	return items[1]
end
local found = first({ json.object(), json.object() })
if found then
	found:set("x", 1)
end

-- the result pair as a typed local
local parsed: Parsed?, err: string? = url.Url.parse("https://example.com/path")
if parsed == nil then
	error(err)
end
local release = assert(semver.Version.parse("1.2.3"))
print(host_of(parsed), describe(release), budget(nil))
