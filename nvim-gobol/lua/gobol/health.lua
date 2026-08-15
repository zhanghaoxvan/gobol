-- :checkhealth gobol support.
local M = {}

function M.check()
    local health = vim.health or require("health")
    local start = health.start or health.report_start
    local ok = health.ok or health.report_ok
    local warn = health.warn or health.report_warn
    local err = health.error or health.report_error

    start("gobol-lsp")

    -- Neovim version (vim.lsp.start requires 0.8+).
    if vim.fn.has("nvim-0.8") == 1 then
        ok("Neovim version supports vim.lsp.start (>= 0.8)")
    else
        err("Neovim 0.8+ required for the built-in LSP client", {})
    end

    -- Locate the language server binary.
    local lsp = require("gobol.lsp")
    local cmd, _ = lsp.find_binary()
    if cmd then
        ok("gobol-lsp binary found: " .. cmd)
    else
        warn("gobol-lsp binary not found. Install it or set `lsp_path`.", {})
    end

    -- grape.toml detection.
    local grape = require("gobol.grape")
    local toml = grape.find_grape_toml(vim.fn.getcwd())
    if toml then
        ok("grape.toml detected: " .. toml)
        local info, parse_err = grape.load_project(toml)
        if info then
            ok(string.format("Project: %s v%s (entry: %s)", info.name, info.version, info.entry))
        elseif parse_err then
            err("Failed to parse grape.toml: " .. parse_err, {})
        end
    else
        warn("No grape.toml found in the current workspace", {})
    end
end

return M
