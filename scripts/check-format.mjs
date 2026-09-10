import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const roots = ["packages", "vscode-todo", "scripts"];
const files = roots.flatMap(sourceFiles);
const failures = files.filter((file) => {
  const text = readFileSync(file, "utf8");
  return !text.endsWith("\n") || /[ \t]+$/m.test(text);
});

if (failures.length > 0) {
  console.error(`Formatting check failed:\n${failures.join("\n")}`);
  process.exitCode = 1;
}

function sourceFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = join(directory, entry.name);
    if (entry.isDirectory()) return entry.name === "node_modules" || entry.name === "dist" ? [] : sourceFiles(file);
    return /\.(?:js|json|mjs|ts)$/.test(entry.name) ? [file] : [];
  });
}
