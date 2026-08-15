-- Plugin entry point: registers autocommands that start the gobol language
-- server when a .gbl file is opened, after parsing grape.toml if present.
local config = require("gobol.config")
local lsp = require("gobol.lsp")

local group = vim.api.nvim_create_augroup("gobol", { clear = true })

vim.api.nvim_create_autocmd({ "FileType" }, {
    group = group,
    pattern = "gobol",
    callback = function(args)
        local opts = config.get()
        if opts.auto_start then
            lsp.start(args.buf)
        end
    end,
    desc = "Start gobol-lsp for .gbl buffers",
})

-- When entering a workspace whose root contains grape.toml, eagerly surface
-- project info even before a .gbl file is opened.
vim.api.nvim_create_autocmd({ "VimEnter" }, {
    group = group,
    callback = function()
        local opts = config.get()
        if not opts.detect_grape then return end
        local grape = require("gobol.grape")
        local toml = grape.find_grape_toml(vim.fn.getcwd())
        if toml then
            local info, err = grape.load_project(toml)
            if info then
                vim.notify(string.format("[gobol] Detected project %s v%s (entry: %s)",
                    info.name, info.version, info.entry), vim.log.levels.INFO)
            elseif err then
                vim.notify("[gobol] " .. err, vim.log.levels.WARN)
            end
        end
    end,
    desc = "Detect grape.toml on startup",
})
