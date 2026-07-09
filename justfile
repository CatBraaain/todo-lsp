tree_sitter_dsl_def_path := "https://raw.githubusercontent.com/tree-sitter/tree-sitter/master/crates/cli/npm/dsl.d.ts"

_:
  @just --list --unsorted

prepare:
  curl -L {{tree_sitter_dsl_def_path}} -o grammar.d.ts
