tree_sitter_dsl_def_path := "https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/crates/cli/npm/dsl.d.ts"

_:
  @just --list --unsorted

# Re-fetch tree-sitter grammar type definitions into tree-sitter-todo/.
prepare:
  cd tree-sitter-todo && curl -L {{tree_sitter_dsl_def_path}} -o grammar.d.ts

# Build the LSP server (release).
build-lsp:
  cargo build --release -p todo-lsp

# Build the Windows x64 server binary and stage it under vscode-todo/bin/.
copy-binary: build-lsp
  mkdir -p vscode-todo/bin/win32-x64
  cp target/release/todo-lsp.exe vscode-todo/bin/win32-x64/todo-lsp.exe

# Install deps and bundle the VSCode extension.
build-ext:
  cd vscode-todo && npm install && npm run build

# Build everything and package a .vsix (run on Windows).
package: copy-binary build-ext
  cd vscode-todo && npm run package

# Run the Rust workspace tests.
test:
  cargo test --workspace

# Build a debug server binary, stage it, and rebuild the extension (F5 loop).
dev:
  cargo build -p todo-lsp
  mkdir -p vscode-todo/bin/win32-x64
  cp target/debug/todo-lsp.exe vscode-todo/bin/win32-x64/todo-lsp.exe
  cd vscode-todo && npm run build

# Regenerate parser sources via the tree-sitter Docker toolchain.
generate:
  cd tree-sitter-todo && bash tools/ts-docker.sh generate

# Run the tree-sitter corpus tests via the Docker toolchain.
test-grammar:
  cd tree-sitter-todo && bash tools/ts-docker.sh test
