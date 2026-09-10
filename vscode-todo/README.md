# Todo for Visual Studio Code

Language support for `.todo` and `.tasks` files: syntax highlighting, outline,
folding, diagnostics, commands, formatting, archive, and repeat tasks.

The VSIX ships the Node.js [`@todo-lsp/todo-lsp`](../packages/todo-lsp) server,
`@todo-lsp/todo-core`, its Tree-sitter WASM grammar, and the server's runtime
dependencies. The extension starts that server with Node stdio, so one VSIX runs
on Windows, macOS, and Linux where its VS Code extension host supplies Node.js
20 or later. Activation otherwise reports: `Todo requires Node.js 20 or later.`

## Development

From the repository root, build the workspace packages first, then build or
package the extension:

```sh
npm run build
cd vscode-todo
npm ci
npm run build
npm run package
npm run test:vsix
```
