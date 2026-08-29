" Basic syntax highlighting for the GoBol programming language.
" This is a fallback; a Tree-sitter grammar or the TextMate grammar ported
" from the VS Code extension provides richer highlighting when available.
if exists("b:current_syntax")
    finish
endif

" Keywords
syntax keyword gobolKeyword if else for while return break continue
syntax keyword gobolKeyword func var val import from as export extern
syntax keyword gobolKeyword struct impl trait enum constructor new
syntax keyword gobolKeyword match convert operator self true false null
syntax keyword gobolKeyword int float str bool

" Types / builtins (highlighted as Type)
syntax keyword gobolType Vec Ref Option Result

" Import module names — `import mod`, `import a::b::c`, `from mod import ...`.
" The module identifier(s) get their own highlight group so `import xxx`'s
" `xxx` stands out even without an LSP (rust-analyzer-style coloring is
" provided by the LSP semantic tokens; this is the no-LSP fallback).
syntax match gobolImportModule "\(import\|from\)\s\+\zs[[:alpha:]_][[:word:]]*"
syntax match gobolImportModule "\zs[[:alpha:]_][[:word:]]*\ze\s*::"
syntax match gobolImportModule "::\s*\zs[[:alpha:]_][[:word:]]*"

" Comments
syntax match gobolComment "//.*" contains=gobolTodo
syntax region gobolBlockComment start="/\*" end="\*/" contains=gobolTodo
syntax keyword gobolTodo contained TODO FIXME XXX NOTE

" Strings — escape sequences (`\n`, `\t`, `\x41`, `\u{...}`) are highlighted
" inside double-quoted and format (@") strings as well as char literals.
syntax match gobolEscape /\\\(u{[0-9a-fA-F]*}\|x[0-9a-fA-F]\{2}\|.\)/ contained
syntax region gobolString start=+"+ skip=+\\\\\|\\"+ end=+"+ contains=gobolEscape
syntax region gobolString start=+'+ skip=+\\\\\|\\'+ end=+'+ contains=gobolEscape

" Numbers
syntax match gobolNumber "\v<\d+>"
syntax match gobolNumber "\v<\d+\.\d+>"

" Operators / punctuation
syntax match gobolOperator "->"
syntax match gobolOperator "::"

" Attributes
syntax match gobolAttribute "#\[.*\]"

highlight default link gobolKeyword Keyword
highlight default link gobolType Type
highlight default link gobolImportModule Identifier
highlight default link gobolComment Comment
highlight default link gobolBlockComment Comment
highlight default link gobolTodo Todo
highlight default link gobolString String
highlight default link gobolEscape Special
highlight default link gobolNumber Number
highlight default link gobolOperator Operator
highlight default link gobolAttribute PreProc

let b:current_syntax = "gobol"
