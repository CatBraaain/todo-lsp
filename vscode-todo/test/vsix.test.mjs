import assert from "node:assert/strict";
import { execFileSync, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const vsix = join(root, "dist", "todo-0.1.0.vsix");
const server = "extension/server/node_modules/@todo-lsp/todo-lsp/dist/main.js";

test("VSIX: ships the Node 20 requirement, LSP, core WASM, and runtime dependencies", () => {
  assert.ok(existsSync(vsix), "run npm run package before the VSIX test");
  const contents = execFileSync("unzip", ["-Z1", vsix], { encoding: "utf8" }).split("\n");
  const manifest = JSON.parse(execFileSync("unzip", ["-p", vsix, "extension/package.json"]));
  assert.equal(manifest.engines.node, ">=20");
  for (const file of [
    server,
    "extension/server/node_modules/@todo-lsp/todo-core/dist/tree-sitter-todo.wasm",
    "extension/server/node_modules/web-tree-sitter/tree-sitter.js",
    "extension/server/node_modules/vscode-languageserver/lib/node/main.js",
  ]) {
    assert.ok(contents.includes(file), `VSIX missing ${file}`);
  }
  assert.equal(contents.some((file) => /(^|\/)target\/|todo-lsp\.exe$|^extension\/bin\//.test(file)), false);
});

test("VSIX: packaged Node LSP accepts an initialize request over stdio", async (context) => {
  assert.ok(existsSync(vsix), "run npm run package before the VSIX test");
  const extractedServer = execFileSync("unzip", ["-p", vsix, server], { encoding: "buffer" });
  assert.ok(extractedServer.length > 0, "packaged LSP entrypoint is readable");

  const temp = execFileSync("mktemp", ["-d"], { encoding: "utf8" }).trim();
  context.after(() => execFileSync("rm", ["-rf", temp]));
  execFileSync("unzip", ["-q", vsix, "-d", temp]);

  const serverProcess = spawn(globalThis.process.execPath, [join(temp, server)]);
  context.after(() => serverProcess.kill());
  const response = await request(serverProcess, {
    id: 1,
    method: "initialize",
    params: { processId: null, capabilities: {} },
  });
  assert.equal(response.id, 1);
  assert.equal(response.result.capabilities.positionEncoding, "utf-16");
});

function request(serverProcess, message) {
  return new Promise((resolve, reject) => {
    let buffer = Buffer.alloc(0);
    const timer = setTimeout(() => reject(new Error("timed out waiting for packaged LSP")), 5_000);
    serverProcess.once("error", reject);
    serverProcess.stdout.on("data", (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      const separator = buffer.indexOf("\r\n\r\n");
      if (separator === -1) return;

      const length = Number(/^Content-Length: (\d+)$/im.exec(buffer.subarray(0, separator).toString())?.[1]);
      const start = separator + 4;
      if (!Number.isSafeInteger(length) || buffer.length < start + length) return;

      clearTimeout(timer);
      resolve(JSON.parse(buffer.subarray(start, start + length).toString()));
    });
    const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", ...message }));
    serverProcess.stdin.write(`Content-Length: ${body.length}\r\n\r\n${body}`);
  });
}
