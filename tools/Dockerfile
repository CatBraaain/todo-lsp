# tree-sitter CLI を安全に実行するための隔離イメージ。
# Ubuntu 24.04 (glibc 2.39) + node 24 公式バイナリ。
# node:20/24-slim は glibc 2.36 で tree-sitter 0.26.9 が要求する GLIBC_2.39 を満たさないため、
# Ubuntu 24.04 をベースに node 公式バイナリを展開する。
FROM ubuntu:24.04

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl gzip xz-utils build-essential \
 && curl -fsSL https://nodejs.org/dist/v24.18.0/node-v24.18.0-linux-x64.tar.xz \
    | tar -xJ -C /usr/local --strip-components=1 \
 && curl -fsSL https://github.com/tree-sitter/tree-sitter/releases/download/v0.26.9/tree-sitter-linux-x64.gz \
    -o /usr/local/bin/tree-sitter.gz \
 && gunzip /usr/local/bin/tree-sitter.gz \
 && chmod +x /usr/local/bin/tree-sitter \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /work
ENTRYPOINT ["tree-sitter"]
