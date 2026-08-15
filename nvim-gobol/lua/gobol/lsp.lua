-- LSP client management for the gobol language server.
-- Uses Neovim's built-in vim.lsp API (requires Neovim 0.8+).
local config = require("gobol.config")
local grape = require("gobol.grape")

local M = {}

local client_id = nil
local project_info = nil

--- Locate the gobol-lsp binary using the same search order as the VS Code extension.
function M.find_binary()
    local opts = config.get()
    if opts.lsp_path and opts.lsp_path ~= "" and vim.fn.executable(opts.lsp_path) == 1 then
        return opts.lsp_path, {}
    end
    local home = vim.loop.os_homedir()
    local home_bin = home .. "/.gobol/bin/gobol-lsp"
    if vim.fn.executable(home_bin) == 1 then
        return home_bin, {}
    end
    -- workspace target/debug or target/release
    local cwd = vim.fn.getcwd()
    if cwd and cwd ~= "" then
        for _, mode in ipairs({ "debug", "release" }) do
            local bin = cwd .. "/target/" .. mode .. "/gobol-lsp"
            if vim.fn.executable(bin) == 1 then
                return bin, {}
            end
        end
        if opts.cargo_run then
            return "cargo", { "run", "--bin", "gobol-lsp", "--release" }
        end
    end
    if vim.fn.executable("gobol-lsp") == 1 then
        return "gobol-lsp", {}
    end
    return nil, nil
end

--- Compute the workspace root for a buffer: nearest ancestor containing grape.toml,
--- else the current working directory.
function M.find_root(bufnr)
    bufnr = bufnr or 0
    local path = vim.api.nvim_buf_get_name(bufnr)
    if path == "" then
        path = vim.fn.getcwd()
    end
    local start_dir = vim.fn.fnamemodify(path, ":h")
    local toml = grape.find_grape_toml(start_dir)
    if toml then
        return vim.fn.fnamemodify(toml, ":h"), toml
    end
    return vim.fn.getcwd(), nil
end

--- Apply buffer-local keymaps and omnifunc when the LSP attaches.
local function on_attach(client, bufnr)
    local opts = config.get()
    local km = opts.keymaps
    local function map(lhs, rhs)
        if lhs and lhs ~= false then
            vim.keymap.set("n", lhs, rhs, { buffer = bufnr, silent = true, desc = "gobol-lsp" })
        end
    end
    if client.server_capabilities.definitionProvider then
        map(km.goto_def, vim.lsp.buf.definition)
    end
    if client.server_capabilities.declarationProvider then
        map(km.goto_decl, vim.lsp.buf.declaration)
    end
    if client.server_capabilities.hoverProvider then
        map(km.hover, vim.lsp.buf.hover)
    end
    if client.server_capabilities.referencesProvider then
        map(km.references, vim.lsp.buf.references)
    end
    if client.server_capabilities.renameProvider then
        map(km.rename, vim.lsp.buf.rename)
    end
    if client.server_capabilities.codeActionProvider then
        map(km.code_action, vim.lsp.buf.code_action)
    end
    if client.server_capabilities.signatureHelpProvider then
        map(km.signature_help, vim.lsp.buf.signature_help)
    end
    if client.server_capabilities.documentFormattingProvider then
        map(km.format, function()
            vim.lsp.buf.format({ bufnr = bufnr })
        end)
    end
    -- Completion via omnifunc (<C-x><C-o>) as a built-in fallback.
    if client.server_capabilities.completionProvider then
        vim.bo[bufnr].omnifunc = "v:lua.vim.lsp.omnifunc"
    end
end

--- Start (or reuse) the gobol language server for the given workspace root.
--- Returns the client id, or nil on failure.
function M.start(bufnr)
    bufnr = bufnr or 0
    local opts = config.get()

    local cmd, args = M.find_binary()
    if not cmd then
        vim.notify(
            "gobol-lsp binary not found. Set `lsp_path` in require('gobol').setup() " ..
            "or install gobol-lsp to ~/.gobol/bin.",
            vim.log.levels.ERROR
        )
        return nil
    end

    local root_dir, toml_path = M.find_root(bufnr)

    -- Parse grape.toml if present so we can surface project info and use its
    -- entry point as the effective root context.
    if toml_path and opts.detect_grape then
        local info, err = grape.load_project(toml_path)
        if info then
            project_info = info
            root_dir = info.root
            vim.notify(
                string.format("[gobol] Project: %s v%s (entry: %s, deps: %d)",
                    info.name, info.version, info.entry, vim.tbl_count(info.dependencies)),
                vim.log.levels.INFO
            )
        elseif err then
            vim.notify("[gobol] " .. err, vim.log.levels.WARN)
        end
    end

    -- Reuse an existing client bound to the same root.
    for _, c in ipairs(vim.lsp.get_active_clients({ bufnr = bufnr })) do
        if c.name == "gobol-lsp" and c.config.root_dir == root_dir then
            return c.id
        end
    end

    local full_cmd = vim.list_extend({ cmd }, args)

    local ok, id = pcall(vim.lsp.start, {
        name = "gobol-lsp",
        cmd = full_cmd,
        root_dir = root_dir,
        cmd_env = {},
        on_attach = on_attach,
        handlers = {
            ["textDocument/publishDiagnostics"] = function(err, result, ctx, conf)
                vim.lsp.handlers["textDocument/publishDiagnostics"](err, result, ctx, conf)
                if result and result.diagnostics and #result.diagnostics > 0 then
                    vim.diagnostic.setloclist({ open = false })
                end
            end,
        },
    }, {
        bufnr = bufnr,
    })

    if not ok then
        vim.notify("[gobol] Failed to start LSP: " .. tostring(id), vim.log.levels.ERROR)
        return nil
    end
    client_id = id
    return id
end

--- Stop all gobol-lsp clients.
function M.stop()
    for _, c in ipairs(vim.lsp.get_active_clients({ name = "gobol-lsp" })) do
        vim.lsp.stop_client(c.id)
    end
    client_id = nil
end

--- Restart the language server.
function M.restart()
    M.stop()
    vim.defer_fn(function()
        M.start(0)
    end, 100)
end

--- Return cached project info (from grape.toml), if any.
function M.get_project()
    return project_info
end

return M
