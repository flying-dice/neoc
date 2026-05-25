--!strict
-- A small Luau module exercising the binding generator.

type Rgb = { r: number, g: number, b: number }

local M = {}

-- Typed constant.
local WHITE: string = "ffffff"

-- Typed exported function: maps cleanly.
function M.lighten(color: string, amount: number): string
	return color
end

-- Returns a declared type: opaque handle.
function M.toRgb(color: string): Rgb
	return { r = 0, g = 0, b = 0 }
end

-- Untyped export: no type info, falls back to LuaValue.
function M.debug(value)
	return value
end

return {
	lighten = M.lighten,
	toRgb = M.toRgb,
	debug = M.debug,
	WHITE = WHITE,
}
