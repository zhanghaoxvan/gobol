# nvim-gobol

Neovim 语言服务支持 for the [GoBol](https://github.com/zhanghaoxvan/gobol) programming language.

When you open a folder containing a `grape.toml` file as your workspace root, this plugin automatically detects and parses it, starts the `gobol-lsp` language server, and provides code completion, diagnostics, go-to-definition, references, and document symbols.

## Features

- **grape.toml auto-detection** — walks up from the current buffer to find `grape.toml`, parses the project (name, version, entry, dependencies) and surfaces it via `:GobolProject`.
- **Built-in LSP client** — uses Neovim's native `vim.lsp` (no `nvim-lspconfig` dependency). Supports:
  - Code completion (`<C-x><C-o>` omnifunc, or integrate with [nvim-cmp](https://github.com/hrsh7th/nvim-cmp))
  - Diagnostics (syntax checking) via `vim.diagnostic`
  - Go-to-definition (`gd`), references (`gr`), hover (`K`)
  - Document symbols
- **Syntax highlighting** — a regex-based syntax file for `.gbl` files.
- **Filetype detection** — `.gbl` → `gobol`; `grape.toml` → `gobol.project`.
- **`:checkhealth gobol`** — verifies Neovim version, binary location, and grape.toml status.

## Requirements

- Neovim **0.8+** (for the built-in `vim.lsp.start` API)
- The `gobol-lsp` binary, discoverable via one of:
  1. `lsp_path` option in `setup()`
  2. `~/.gobol/bin/gobol-lsp`
  3. `target/debug/gobol-lsp` or `target/release/gobol-lsp` in the workspace
  4. `gobol-lsp` on `$PATH`

## Installation

### [lazy.nvim](https://github.com/folke/lazy.nvim)

```lua
{
    "zhanghaoxvan/gobol",
    dir = "/path/to/gobol/nvim-gobol",  -- or use the repo URL
    ft = "gobol",
    config = function()
        require("gobol").setup({
            -- lsp_path = "/usr/local/bin/gobol-lsp",  -- optional explicit path
        })
    end,
}
```

### [packer.nvim](https://github.com/wbthomason/packer.nvim)

```lua
use {
    "zhanghaoxvan/gobol",
    rtp = "nvim-gobol",
    ft = "gobol",
    config = function()
        require("gobol").setup({})
    end,
}
```

### Manual (rtp)

Add the `nvim-gobol` directory to your `runtimepath`:

```vim
set rtp+=/path/to/gobol/nvim-gobol
lua require("gobol").setup({})
```

## Configuration

```lua
require("gobol").setup({
    lsp_path = "",            -- explicit path to gobol-lsp; "" = auto-detect
    lsp_args = {},            -- extra args forwarded to the server
    cargo_run = false,        -- run via "cargo run --bin gobol-lsp --release"
    auto_start = true,        -- start LSP when a .gbl file is opened
    detect_grape = true,      -- parse grape.toml on workspace enter
    keymaps = {
        goto_def = "gd",
        hover = "K",
        references = "gr",
        rename = "<leader>rn",
        code_action = "<leader>ca",
        signature_help = "<C-k>",
        format = "<leader>gf",
    },
    diagnostic = {
        virtual_text = true,
        signs = true,
        underline = true,
        update_in_insert = false,
    },
})
```

## Commands

| Command | Description |
|---------|-------------|
| `:GobolRestart` | Restart the language server. |
| `:GobolProject` | Show detected `grape.toml` project info. |
| `:checkhealth gobol` | Run health checks. |

## Default keymaps (on LSP attach)

| Key | Action |
|-----|--------|
| `gd` | Go to definition |
| `K` | Hover documentation |
| `gr` | References |
| `<C-k>` | Signature help |
| `<leader>rn` | Rename symbol |
| `<leader>ca` | Code action |
| `<leader>gf` | Format buffer |

## Completion

The plugin sets `omnifunc` so that `<C-x><C-o>` triggers LSP completion. For
a richer completion UI, pair it with **nvim-cmp** and its `cmp-nvim-lsp` source —
the `gobol-lsp` server advertises standard `textDocument/completion`.

## How grape.toml detection works

1. On `VimEnter`, the plugin searches upward from `getcwd()` for `grape.toml`.
2. When a `.gbl` file is opened, it again searches upward from the buffer path.
3. If found, the TOML is parsed and the project root is set to the directory
   containing `grape.toml`, so the language server resolves imports correctly.
4. Project info (name, version, entry, dependencies) is logged and available
   via `:GobolProject`.
