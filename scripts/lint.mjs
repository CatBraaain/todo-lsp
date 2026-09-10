import { execFileSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { join } from "node:path";

for (const file of ["packages", "vscode-todo", "scripts"].flatMap(sourceFiles)) {
  execFileSync(process.execPath, ["--check", file], { stdio: "inherit" });
}

function sourceFiles(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const file = join(directory, entry.name);
    if (entry.isDirectory()) {
      return ["node_modules", "dist", ".git", "target"].includes(entry.name) ? [] : sourceFiles(file);
    }
    return /\.(?:js|mjs)$/.test(entry.name) ? [file] : [];
  });
}
