local M = {}

---@generic S
---@param store S
---@param body string
---@return string
function M.create(store, body) end

---@generic S
---@param store S
---@return string
function M.list(store) end

return M
