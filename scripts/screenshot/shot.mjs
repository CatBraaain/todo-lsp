// Connect to VSCode over CDP and save a screenshot of the workbench.
// Usage: node shot.mjs <out.png> [--port 9222] [--wait 2000]
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { chromium } from "playwright-core";

const argValue = (name) => {
  const index = process.argv.indexOf(name);
  return index === -1 ? undefined : process.argv[index + 1];
};

const out = process.argv[2] ?? "shot.png";
const port = argValue("--port") ?? "9222";
const settleMs = Number(argValue("--wait") ?? 2000);

const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser
  .contexts()
  .flatMap((context) => context.pages())
  .find((candidate) => candidate.url().startsWith("vscode-file://"));
if (!page) {
  throw new Error("workbench page not found; is VSCode running with --remote-debugging-port?");
}

await page.waitForTimeout(settleMs);
mkdirSync(dirname(out), { recursive: true });
await page.screenshot({ path: out });
console.log(out);
await browser.close();
