-- Default configuration for the gobol Neovim plugin.
-- Users override these via require("gobol").setup({ ... }).
local M = {}

local defaults = {
    -- Path to the gobol-lsp binary. When empty, the plugin searches a list
    -- of candidate locations (see lsp.find_binary).
    lsp_path = "",
    -- Extra arguments forwarded to the language server process.
    lsp_args = {},
    -- When true, run the server via "cargo run --bin gobol-lsp --release"
    -- instead of a pre-built binary (slow; for development only).
    cargo_run = false,
    -- Whether to enable automatic LSP startup when a .gbl file is opened.
    auto_start = true,
    -- Whether to parse grape.toml on workspace enter and surface project info.
    detect_grape = true,
    -- Key mappings applied on LSP attach (set to false to disable a mapping).
    keymaps = {
        goto_def = "gd",
        goto_decl = "gD",
        hover = "K",
        references = "gr",
        rename = "<leader>rn",
        code_action = "<leader>ca",
        signature_help = "<C-k>",
        format = "<leader>gf",
    },
    -- Diagnostic display options (merged with vim.diagnostic.config defaults).
    diagnostic = {
        virtual_text = true,
        signs = true,
        underline = true,
        update_in_insert = false,
    },
}

M.options = {}

function M.setup(opts)
    opts = opts or {}
    M.options = vim.tbl_deep_extend("force", defaults, opts)
    return M.options
end

function M.get()
    if vim.tbl_isempty(M.options) then
        M.setup({})
    end
    return M.options
end

return M
