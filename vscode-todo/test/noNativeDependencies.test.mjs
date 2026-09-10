import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const extension = readFileSync(join(root, "src", "extension.ts"), "utf8");

test("拡張: Node stdio server replaces native platform binaries", () => {
  assert.match(extension, /command: process\.execPath/);
  assert.match(extension, /path\.join\("server", "node_modules", "@todo-lsp", "todo-lsp", "dist", "main\.js"\)/);
  assert.doesNotMatch(extension, /path\.join\("bin"|platformDirectoryName|serverBinaryName|todo-lsp\.exe/);
  assert.equal(existsSync(join(root, "src", "platform.mjs")), false);
});

test("拡張: packaged runtime excludes native server artifacts", () => {
  const packageJson = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
  const justfile = readFileSync(join(root, "..", "justfile"), "utf8");
  assert.deepEqual(Object.keys(packageJson.dependencies).sort(), [
    "@todo-lsp/todo-core",
    "@todo-lsp/todo-lsp",
    "vscode-languageclient",
  ]);
  assert.doesNotMatch(justfile, /todo-lsp\.exe|vscode-todo\/bin|cargo/i);
});

test("リポジトリ: native Rust LSPとgrammar Rust bindingは存在しない", () => {
  const repositoryRoot = join(root, "..");
  for (const file of ["Cargo.toml", "Cargo.lock", "todo-lsp"]) {
    assert.equal(existsSync(join(repositoryRoot, file)), false, `${file} must be removed`);
  }

  const grammarConfig = JSON.parse(
    readFileSync(join(repositoryRoot, "tree-sitter-todo", "tree-sitter.json"), "utf8"),
  );
  assert.equal(grammarConfig.bindings.rust, false);
});
