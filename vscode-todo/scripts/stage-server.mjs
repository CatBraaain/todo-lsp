import { cpSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const extensionRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repositoryRoot = join(extensionRoot, "..");
const sourceNodeModules = join(repositoryRoot, "node_modules");
const targetNodeModules = join(extensionRoot, "server", "node_modules");
const staged = new Set();

rmSync(join(extensionRoot, "server"), { force: true, recursive: true });
mkdirSync(targetNodeModules, { recursive: true });
stagePackage("@todo-lsp/todo-lsp");

function stagePackage(name) {
  if (staged.has(name)) return;

  const source = join(sourceNodeModules, name);
  if (!existsSync(source)) {
    throw new Error(`Missing runtime dependency ${name}; run npm install in the repository root first.`);
  }

  staged.add(name);
  const manifest = awaitableJson(join(source, "package.json"));
  for (const dependency of Object.keys({ ...manifest.dependencies, ...manifest.optionalDependencies })) {
    stagePackage(dependency);
  }
  cpSync(source, join(targetNodeModules, name), { dereference: true, recursive: true });
}

function awaitableJson(file) {
  return JSON.parse(readFileSync(file, "utf8"));
}
