-- Detect GoBol source files (.gbl) and set the filetype to "gobol".
vim.filetype.add({
    extension = {
        gbl = "gobol",
    },
    filename = {
        ["grape.toml"] = "gobol.project",
    },
})
