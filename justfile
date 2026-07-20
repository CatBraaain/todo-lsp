tree_sitter_dsl_def_path := "https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/crates/cli/npm/dsl.d.ts"

_:
  @just --list --unsorted

prod:
  cargo build --release -p todo-lsp
  mkdir -p vscode-todo/bin/win32-x64
  cp target/release/todo-lsp.exe vscode-todo/bin/win32-x64/todo-lsp.exe
  cd vscode-todo && npm install && npm run build

dev:
  cargo build -p todo-lsp
  mkdir -p vscode-todo/bin/win32-x64
  cp target/debug/todo-lsp.exe vscode-todo/bin/win32-x64/todo-lsp.exe
  cd vscode-todo && npm install && npm run build

test:
  cargo test --workspace

generate:
  cd tree-sitter-todo && tree-sitter generate

test-grammar:
  cd tree-sitter-todo && tree-sitter test

update-grammar-types:
  cd tree-sitter-todo && curl -L {{tree_sitter_dsl_def_path}} -o grammar.d.ts
