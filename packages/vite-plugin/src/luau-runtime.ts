/**
 * The part of the generated Luau that does not depend on the definition: the checker that holds a value to a schema
 * and the router that checks a call, dispatches it to the developer's handler and checks what the handler returns.
 * It is appended to the generated types and tables, which name `COMPONENTS` and `METHODS` for it.
 */
export const LUAU_RUNTIME = String.raw`
local function describe(value: any): string
  if value == nil then
    return "null"
  end
  local kind = type(value)
  if kind == "table" then
    return "an object or array"
  end
  return "a " .. kind
end

-- a table is an array when every key is 1..n; an empty table is both an array and an object
local function isArray(value: { [any]: any }): boolean
  local count = 0
  for key in value do
    if type(key) ~= "number" then
      return false
    end
    count += 1
  end
  for index = 1, count do
    if value[index] == nil then
      return false
    end
  end
  return true
end

local check: (value: any, schema: any, path: string) -> string?

-- a null is an absent key in Lua: a required property whose schema allows null may be absent
local function allowsNull(schema: any): boolean
  if schema.kind == "ref" then
    return allowsNull(COMPONENTS[schema.name])
  end
  return schema.kind == "nullable" or schema.kind == "null"
end

check = function(value: any, schema: any, path: string): string?
  local kind = schema.kind
  if kind == "ref" then
    return check(value, COMPONENTS[schema.name], path)
  elseif kind == "nullable" then
    if value == nil then
      return nil
    end
    return check(value, schema.inner, path)
  elseif kind == "string" then
    if type(value) ~= "string" then
      return path .. " must be a string, not " .. describe(value)
    end
    if schema.enum ~= nil and not table.find(schema.enum, value) then
      return path .. " must be one of " .. table.concat(schema.enum, ", ")
    end
    local length = utf8.len(value) or #value
    if schema.minLength ~= nil and length < schema.minLength then
      return path .. " must have at least " .. tostring(schema.minLength) .. " characters"
    end
    if schema.maxLength ~= nil and length > schema.maxLength then
      return path .. " must have at most " .. tostring(schema.maxLength) .. " characters"
    end
  elseif kind == "number" then
    if type(value) ~= "number" or value ~= value then
      return path .. " must be a number, not " .. describe(value)
    end
    if schema.integer and value % 1 ~= 0 then
      return path .. " must be a whole number"
    end
    if schema.minimum ~= nil and value < schema.minimum then
      return path .. " must be at least " .. tostring(schema.minimum)
    end
    if schema.maximum ~= nil and value > schema.maximum then
      return path .. " must be at most " .. tostring(schema.maximum)
    end
  elseif kind == "boolean" then
    if type(value) ~= "boolean" then
      return path .. " must be true or false, not " .. describe(value)
    end
  elseif kind == "null" then
    if value ~= nil then
      return path .. " must be null"
    end
  elseif kind == "array" then
    if type(value) ~= "table" or not isArray(value) then
      return path .. " must be an array, not " .. describe(value)
    end
    for index, item in value do
      local problem = check(item, schema.items, path .. "[" .. tostring(index - 1) .. "]")
      if problem ~= nil then
        return problem
      end
    end
  elseif kind == "object" then
    if type(value) ~= "table" then
      return path .. " must be an object, not " .. describe(value)
    end
    for key, item in value do
      if type(key) ~= "string" then
        return path .. " must be an object, not an array"
      end
      local property = schema.properties[key]
      if property == nil then
        return path .. "." .. key .. " is not a property of the definition"
      end
      local problem = check(item, property, path .. "." .. key)
      if problem ~= nil then
        return problem
      end
    end
    for _, key in schema.required do
      if value[key] == nil and not allowsNull(schema.properties[key]) then
        return path .. "." .. key .. " is required"
      end
    end
  end
  return nil
end

local function router(implementation: Handlers): vos_Handlers
  local provided: { [string]: any } = implementation :: any
  for name in provided do
    if METHODS[name] == nil then
      error('the handlers implement "' .. name .. '", which the definition does not declare')
    end
  end
  local handlers: vos_Handlers = {}
  for name, method in METHODS do
    local handler = provided[name]
    if type(handler) ~= "function" then
      error('the handlers do not implement "' .. name .. '"')
    end
    handlers[name] = function(ctx, args)
      local params: any = args
      if params == nil then
        params = {}
      end
      local problem = check(params, method.params, "params")
      if problem ~= nil then
        return ctx:fail("Invalid params: " .. problem, { code = -32602 })
      end
      local result = handler(ctx, params)
      local wrong = check(result, method.result, "result")
      if wrong ~= nil then
        return ctx:fail(
          "Internal error: the result of " .. name .. " does not match the definition (" .. wrong .. ")",
          { code = -32603 }
        )
      end
      return result
    end
  end
  return handlers
end

return { router = router }
`
