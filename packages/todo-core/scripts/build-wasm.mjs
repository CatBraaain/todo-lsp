import { mkdirSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { delimiter } from "node:path";
import { env } from "node:process";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";

const packageDirectory = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const grammarDirectory = resolve(packageDirectory, "../../tree-sitter-todo");
const outputDirectory = resolve(packageDirectory, "dist");
const outputPath = resolve(outputDirectory, "tree-sitter-todo.wasm");

mkdirSync(outputDirectory, { recursive: true });

const require = createRequire(import.meta.url);
const cliPath = require.resolve("tree-sitter-cli/cli.js");
// tree-sitter build --wasm shells out to `emcc`; the emsdk npm package
// links it into the workspace root's .bin directory when no system
// emcc/docker/podman is available.
const emsdkBin = resolve(packageDirectory, "../../node_modules/.bin");
const path = `${emsdkBin}${delimiter}${env.PATH}`;

function run(args) {
  const result = spawnSync(process.execPath, [cliPath, ...args], {
    cwd: grammarDirectory,
    stdio: "inherit",
    env: { ...env, PATH: path },
  });

  if (result.error) {
    throw result.error;
  }

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

run(["generate"]);
run(["build", "--wasm", "--output", outputPath]);
rmSync(resolve(grammarDirectory, "src", "node-types.json"), { force: true });
