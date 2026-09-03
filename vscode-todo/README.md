# Todo for Visual Studio Code

Language support for `.todo` files: syntax highlighting, outline (document
symbols), folding, and diagnostics. These features are provided by a bundled
[`todo-lsp`](https://github.com/CatBraaain/todo-lsp/tree/main/todo-lsp)
language server connected over stdio.

## Status

Supports **Windows x64** and **Linux x64**: the extension resolves
`bin/<platform>/todo-lsp` and launches it over stdio. Binaries are built
locally with `just dev` / `just prod` from the repository root. Other
platforms fail with an explicit unsupported-platform error.
