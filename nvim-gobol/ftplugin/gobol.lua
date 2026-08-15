-- Buffer-local settings for gobol source files (.gbl).
vim.bo.commentstring = "// %s"
vim.bo.expandtab = true
vim.bo.shiftwidth = 4
vim.bo.tabstop = 4
vim.bo.softtabstop = 4

-- Indentation rules (basic): align with the previous non-blank line.
vim.bo.indentexpr = "v:lua.gobol_indent()"

-- 使用 vim.fn.indent(prev) 没问题，但为了避免递归，可以改用：
_G.gobol_indent = function()
    local line = vim.fn.getline(vim.v.lnum)
    local prev = vim.fn.prevnonblank(vim.v.lnum - 1)
    if prev == 0 then return 0 end
    local prev_line = vim.fn.getline(prev)
    -- 直接用空格计数，避免调用 indent() 递归
    local indent = vim.fn.indent(prev)
    if prev_line:match("{%s*$") or prev_line:match(":%s*$") then
        indent = indent + vim.bo.shiftwidth
    end
    if line:match("^%s*}") then
        indent = math.max(0, indent - vim.bo.shiftwidth)
    end
    return indent
end
