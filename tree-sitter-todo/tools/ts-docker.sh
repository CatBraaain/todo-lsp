#!/usr/bin/env bash
# tree-sitter をメモリ上限付きコンテナで実行するラッパー。
# メモリ爆発が起きてもコンテナ内で OOM-Kill され、ホスト OS は保護される。
# --memory-swap == --memory が、Windows のページファイル膨張(ホスト全体のフリーズ)を防ぐ鍵。

set -euo pipefail

# Git Bash (MSYS2) が -v / -w の引数を Windows パスへ勝手に変換するのを抑止。
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL="*"

# Git Bash のパス(/c/...)を Docker Desktop が扱える形式へ。
HOST_DIR="$(pwd)"
if [[ "$HOST_DIR" == /c/* ]]; then
  HOST_DIR="C:${HOST_DIR#/c}"
fi

exec docker run --rm \
  -v "${HOST_DIR}:/work" \
  -w /work \
  --memory=1g \
  --memory-swap=1g \
  --cpus=1 \
  todo-tree-sitter "$@"
