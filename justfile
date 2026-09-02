tree_sitter_dsl_def_path := "https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/crates/cli/npm/dsl.d.ts"

bin_dir := if os() == "windows" { "win32-x64" } else { "linux-x64" }
bin_ext := if os() == "windows" { ".exe" } else { "" }

_:
  @just --list --unsorted

prod:
  cargo build --release -p todo-lsp
  mkdir -p vscode-todo/bin/{{bin_dir}}
  cp target/release/todo-lsp{{bin_ext}} vscode-todo/bin/{{bin_dir}}/todo-lsp{{bin_ext}}
  just build-vscode

dev:
  cargo build -p todo-lsp
  mkdir -p vscode-todo/bin/{{bin_dir}}
  cp target/debug/todo-lsp{{bin_ext}} vscode-todo/bin/{{bin_dir}}/todo-lsp{{bin_ext}}
  just build-vscode

[working-directory("vscode-todo")]
build-vscode:
  npm install
  npm run build

test:
  cargo test --workspace
  cd vscode-todo && npm test

[working-directory("tree-sitter-todo")]
generate-grammar:
  tree-sitter generate

[working-directory("tree-sitter-todo")]
test-grammar:
  tree-sitter test

[working-directory("tree-sitter-todo")]
update-grammar-types:
  curl -L {{tree_sitter_dsl_def_path}} -o grammar.d.ts
