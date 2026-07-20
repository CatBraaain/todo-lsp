tree_sitter_dsl_def_path := "https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/crates/cli/npm/dsl.d.ts"

_:
  @just --list --unsorted

prod:
  cargo build --release -p todo-lsp
  mkdir -p vscode-todo/bin/win32-x64
  cp target/release/todo-lsp.exe vscode-todo/bin/win32-x64/todo-lsp.exe
  just build-vscode

dev:
  cargo build -p todo-lsp
  mkdir -p vscode-todo/bin/win32-x64
  cp target/debug/todo-lsp.exe vscode-todo/bin/win32-x64/todo-lsp.exe
  just build-vscode

[working-directory("vscode-todo")]
build-vscode:
  npm install
  npm run build

test:
  cargo test --workspace

[working-directory("tree-sitter-todo")]
generate-grammar:
  tree-sitter generate

[working-directory("tree-sitter-todo")]
test-grammar:
  tree-sitter test

[working-directory("tree-sitter-todo")]
update-grammar-types:
  curl -L {{tree_sitter_dsl_def_path}} -o grammar.d.ts
