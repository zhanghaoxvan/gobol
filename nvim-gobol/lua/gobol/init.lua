-- Public API for the gobol Neovim plugin.
-- Usage:
--   require("gobol").setup({ lsp_path = "/path/to/gobol-lsp" })
local config = require("gobol.config")
local lsp = require("gobol.lsp")
local grape = require("gobol.grape")

local M = {}

M.grape = grape
M.lsp = lsp

--- Set up the gobol plugin with user options.
function M.setup(opts)
    config.setup(opts)

    local conf = config.get()

    -- Configure diagnostics display.
    vim.diagnostic.config(conf.diagnostic)

    -- Register a user command to restart the language server.
    vim.api.nvim_create_user_command("GobolRestart", function()
        lsp.restart()
    end, { desc = "Restart the gobol language server" })

    -- Register a user command to show detected project info.
    vim.api.nvim_create_user_command("GobolProject", function()
        local info = lsp.get_project()
        if not info then
            vim.notify("[gobol] No grape.toml project detected in the current workspace.",
                vim.log.levels.WARN)
            return
        end
        local lines = {
            "Gobol Project: " .. info.name .. " v" .. info.version,
            "Entry:    " .. info.entry,
            "Root:     " .. info.root,
            "License:  " .. (info.license or "(none)"),
            "Deps (" .. vim.tbl_count(info.dependencies) .. "):",
        }
        for name, spec in pairs(info.dependencies) do
            table.insert(lines, string.format("  %s @ %s (%s)", name, spec.tag, spec.repo))
        end
        vim.schedule(function()
            vim.api.nvim_echo(
                vim.tbl_map(function(l) return { l .. "\n", "Normal" } end, lines),
                false, {}
            )
        end)
    end, { desc = "Show detected grape.toml project info" })

    return M
end

--- Manually start the language server for the current buffer.
function M.start()
    return lsp.start(0)
end

--- Stop the language server.
function M.stop()
    lsp.stop()
end

return M
