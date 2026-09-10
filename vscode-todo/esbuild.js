// @ts-check
const { rmSync } = require("node:fs");
const esbuild = require("esbuild");

const production = process.argv.includes("--production");
if (production) rmSync("dist/extension.js.map", { force: true });

esbuild
  .build({
    entryPoints: ["src/extension.ts"],
    bundle: true,
    outfile: "dist/extension.js",
    platform: "node",
    format: "cjs",
    external: ["vscode"],
    minify: production,
    sourcemap: !production,
  })
  .catch(() => process.exit(1));
