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
    -- Auto-highlight: same-file (standard documentHighlight) plus cross-file
    -- decoration via the custom gobol/highlights request.
    if client.server_capabilities.documentHighlightProvider then
        setup_highlight_autocmds(bufnr)
    end
    -- Automatic rust-analyzer-style semantic highlighting: Neovim's built-in
    -- semantic-token engine requests textDocument/semanticTokens/full and
    -- colors identifiers (import module names, types, functions, variables)
    -- with no manual enabling. Guarded for older nightlies where the API may
    -- not exist yet.
    if client.server_capabilities.semanticTokensProvider then
        if vim.lsp.semantic_tokens and vim.lsp.semantic_tokens.start then
            vim.lsp.semantic_tokens.start(bufnr, client.id)
        end
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

-- ============ Cross-file auto-highlight ============
-- Neovim namespaces and per-buffer extmark tables for cross-file highlight
-- decorations. Ranges come from the LSP's custom `gobol/highlights` request.
local cross_ns = vim.api.nvim_create_namespace("gobol_crossfile")
local cross_extmarks = {} -- bufnr -> list of extmark ids
local hl_seq = 0 -- monotonically increasing seq for cross-file requests
local hl_active_seq = 0 -- seq of the request currently allowed to apply (0 = none)

local function clear_cross(bufnr)
    local ids = cross_extmarks[bufnr]
    if ids then
        for _, id in ipairs(ids) do
            pcall(vim.api.nvim_buf_del_extmark, bufnr, cross_ns, id)
        end
        cross_extmarks[bufnr] = nil
    end
end

--- Drop every cross-file extmark across all buffers we ever decorated.
local function clear_all_cross()
    for b in pairs(cross_extmarks) do
        clear_cross(b)
    end
end

--- Convert a single LSP Range to neovim extmark coordinates:
--- start_row, start_col, end_row, end_col (0-based).
local function range_to_coords(range)
    return range.start.line, range.start.character,
           range["end"].line, range["end"].character
end

--- Request cross-file highlight ranges for the identifier under the cursor in
--- `bufnr` and decorate every loaded gobol buffer accordingly. Called from a
--- CursorHold autocommand (throttled by `updatetime`). Decoration state is
--- global, so a single module-level accepted-seq + clear-before-request mirrors
--- the VS Code client: stale responses (from a moved cursor or a newer request)
--- never re-apply.
local function refresh_cross(uri, bufnr, position)
    local seq = hl_seq + 1
    hl_seq = seq
    hl_active_seq = seq -- this request is now the only one allowed to apply
    local req_line, req_char = position.line, position.character
    -- Clear everything we own across all buffers up front, so stale cross-file
    -- extmarks never linger even if this request fails or is superseded.
    clear_all_cross()
    vim.lsp.buf_request(bufnr, "gobol/highlights", {
        textDocument = { uri = uri },
        position = position,
    }, function(err, result)
        if err or not result then
            return
        end
        -- Drop the response if a newer request took over or CursorMoved
        -- invalidated it (`hl_active_seq` was reset to 0).
        if hl_active_seq ~= seq then
            return
        end
        -- Only accept when we are still looking at the issuing buffer at the
        -- same cursor position.
        if vim.api.nvim_get_current_buf() ~= bufnr then
            return
        end
        local cur = vim.api.nvim_win_get_cursor(0)
        local cur_line = cur[1] - 1
        local cur_char = cur[2]
        if cur_line ~= req_line or cur_char ~= req_char then
            return
        end
        for _, item in ipairs(result) do
            local highs = item.highlights
            if highs and #highs > 0 then
                -- Resolve the buffer by name so we never trigger an implicit
                -- `bufadd` (which `vim.uri_to_bufnr` performs and would pollute
                -- the buffer list / register an unloaded buffer on CursorHold).
                local target = vim.fn.bufnr("^" .. vim.uri_to_fname(item.uri))
                -- Guard: buffer 0 is the current buffer, so only decorate real,
                -- already-loaded, non-scratch gobol buffers.
                if target < 1 then
                    goto continue
                end
                if not vim.api.nvim_buf_is_loaded(target) then
                    goto continue
                end
                local ids = {}
                for _, hl in ipairs(highs) do
                    local sr, sc, er, ec = range_to_coords(hl.range)
                    local ok, id = pcall(
                        vim.api.nvim_buf_set_extmark,
                        target, cross_ns, sr, sc,
                        { end_row = er, end_col = ec, hl_group = "LspReferenceText" }
                    )
                    if ok and id then
                        table.insert(ids, id)
                    end
                end
                cross_extmarks[target] = ids
            end
            ::continue::
        end
    end)
end

--- Register the document-highlight (same-file, standard) and cross-file
--- autocommands for a buffer once its LSP client attaches. Each buffer gets
--- its own augroup so re-attach (e.g. server restart) replaces rather than
--- stacks handlers and never affects other buffers.
local function setup_highlight_autocmds(bufnr)
    local aug = vim.api.nvim_create_augroup(
        "GobolLspHighlight_" .. bufnr, { clear = true }
    )
    vim.api.nvim_create_autocmd({ "CursorHold", "CursorHoldI" }, {
        group = aug,
        buffer = bufnr,
        callback = vim.lsp.buf.document_highlight,
    })
    vim.api.nvim_create_autocmd("CursorMoved", {
        group = aug,
        buffer = bufnr,
        callback = vim.lsp.buf.clear_references,
    })

    -- Cross-file queries: CursorHold triggers the request, CursorMoved only
    -- clears the previously applied cross-file extmarks and invalidates any
    -- response still in flight. (CursorHold is throttled by `updatetime`.)
    vim.api.nvim_create_autocmd({ "CursorHold", "CursorHoldI" }, {
        group = aug,
        buffer = bufnr,
        callback = function()
            local uri = vim.uri_from_bufnr(bufnr)
            local pos = vim.lsp.util.make_position_params(bufnr)
            refresh_cross(uri, bufnr, pos.position)
        end,
    })
    vim.api.nvim_create_autocmd("CursorMoved", {
        group = aug,
        buffer = bufnr,
        callback = function()
            -- Invalidate any in-flight response and clear all cross-file
            -- extmarks (decoration state is global, like the VS Code client's
            -- clearAllHighlights on every event).
            hl_active_seq = 0
            clear_all_cross()
        end,
    })
end

return M
