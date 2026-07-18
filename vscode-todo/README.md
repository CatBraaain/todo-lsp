# Todo for Visual Studio Code

Language support for `.todo` files: syntax highlighting, outline (document
symbols), folding, and diagnostics. These features are provided by a bundled
[`todo-lsp`](../todo-lsp) language server connected over stdio.

## Status

v1 ships a **Windows x64** server binary only. The extension resolves
`bin/win32-x64/todo-lsp.exe` and launches it with stdio transport. Other
platforms are not supported in v1.
