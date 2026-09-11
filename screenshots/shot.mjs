// Connect to VSCode over CDP and take the SPEC screenshots.
// Usage: node shot.mjs <outdir> [--port 9222] [--wait 2000]
// The shots and their on-screen state are specified by SPEC.md §スクショ (撮影対象);
// the sample opened in the editor is screenshots/sample.todo (SPEC.md §サンプル).
//
// Commands are issued via default keybindings where possible (Ctrl+Shift+M,
// Ctrl+J, Ctrl+B, Ctrl+K chords); typing command ids into the palette is
// avoided because the fuzzy match sometimes picks a different command.
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { chromium } from "playwright-core";

const argValue = (name) => {
  const index = process.argv.indexOf(name);
  return index === -1 ? undefined : process.argv[index + 1];
};

const outDir = process.argv[2] ?? "screenshots/dist";
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

const wait = (ms) => page.waitForTimeout(ms);
const key = (combo) => page.keyboard.press(combo);

// The Electron window would otherwise open at its default size (much smaller
// than the Xvfb screen), so it is resized from inside the renderer, which in
// Electron is allowed for the main window.
async function resizeWindow(width, height) {
  await page.evaluate(`window.resizeTo(${width}, ${height})`);
  await wait(1000);
}

async function closeSecondarySidebar() {
  const auxiliaryBar = page.locator('[id="workbench.parts.auxiliarybar"]');
  if (!(await auxiliaryBar.count())) return;
  const isVisible = await auxiliaryBar.evaluate((element) => {
    const style = getComputedStyle(element);
    const { width, height } = element.getBoundingClientRect();
    return style.display !== "none" && style.visibility !== "hidden" && width > 0 && height > 0;
  });
  if (isVisible) {
    await key("Control+Alt+B");
    await wait(500);
  }
}

// Run a command by its id through the command palette. Only for commands
// without a usable default keybinding.
async function runCommand(id) {
  await key("Control+Shift+P");
  await page.waitForSelector(".quick-input-widget", { state: "visible" });
  await page.keyboard.type(`>${id}`);
  await wait(500);
  await key("Enter");
  await wait(1000);
}

const shot = async (name) => {
  const path = join(outDir, name);
  await page.screenshot({ path });
  console.log(path);
};

mkdirSync(outDir, { recursive: true });
await wait(settleMs); // extension activation + LSP startup + first semantic tokens
await resizeWindow(1600, 1400);
await closeSecondarySidebar();

// 01-complete.png: sample.todo with Problems (0 diagnostics) open.
await key("Control+Shift+M"); // View: Problems
await wait(1500);
await shot("01-complete.png");

// 02-highlighting.png: editor only, side bar (Ctrl+B) and panel (Ctrl+J) closed.
await key("Control+J");
await wait(500);
await key("Control+B");
await wait(500);
await key("Control+Home"); // ensure the editor has focus and show the top
await wait(1000);
await shot("02-highlighting.png");

// 03-fold-comments.png: comment (gray block) folds applied (SPEC §コマンド Alt+F).
await runCommand("editor.foldAllBlockComments");
await wait(1000);
await shot("03-fold-comments.png");

// 04-fold-headings.png: every heading fold region collapsed. Fold Level 1
// (Ctrl+K Ctrl+1) folds all top-level regions. Inbox: has no heading fold
// region by design (SPEC §灰色ブロックの折りたたみ: a heading whose run starts
// with gray children only gets the comment fold), so its children stay
// visible; that is the expected state.
await key("Control+K");
await key("Control+J"); // Unfold All
await wait(800);
await key("Control+K");
await key("Control+1"); // Fold Level 1
await wait(800);
await shot("04-fold-headings.png");

await browser.close();
