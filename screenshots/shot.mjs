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

// Expand the Explorer Outline section and enlarge it so the heading tree fits
// on screen. VSCode sizes explorer views absolutely with no sash to drag, so
// the folder-tree and Timeline views are hidden and the Outline view is
// resized by style override instead. The Inbox and Project subtrees are
// collapsed in the outline so all seven headings fit at once.
// Expand the Explorer Outline section and enlarge it so the heading tree fits
// on screen. VSCode sizes explorer views absolutely with no sash to drag, so
// the folder-tree and Timeline views are hidden and the Outline view is
// resized by style override instead. The outline virtual list keeps
// rendering only a few rows unless the tree shrinks, so every expanded
// outline node is collapsed (real mouse clicks; synthetic element.click()
// is ignored by the tree) to bring all seven headings into view.
async function showOutline() {
  const done = await page.evaluate(() => {
    const header = [...document.querySelectorAll(".pane-header")].find((el) =>
      /outline/i.test(el.textContent ?? ""),
    );
    if (!header) return false;
    if (header.getAttribute("aria-expanded") === "false") header.click();
    for (const other of document.querySelectorAll(".pane-header")) {
      const label = other.textContent?.trim() ?? "";
      if (/^timeline$/i.test(label) || !/outline/i.test(label)) {
        other.closest(".split-view-view")?.setAttribute("style", "display: none");
      }
    }
    header.closest(".split-view-view")?.setAttribute("style", "top: 0px; height: 1000px");
    return true;
  });
  if (!done) throw new Error("outline section not found");
  await wait(500);
  // Collapse every heading subtree in order (Project before Inbox: Project
  // disappears once Inbox is folded; later headings only render after the
  // earlier ones fold away). After all folds the seven headings plus
  // "old task" fit within the rendered rows.
  for (const name of ["Project", "Inbox", "Dates", "Repeat", "Tags", "Decorations", "Archive"]) {
    const row = page
      .locator(".monaco-list-row")
      .filter({ has: page.locator(".label-name", { hasText: new RegExp(`^${name}$`) }) })
      .first();
    if (!(await row.count())) continue;
    await row.locator(".monaco-tl-twistie").click({ timeout: 3000 }).catch(() => {});
    await wait(300);
  }
  await wait(500);
}

const shot = async (name) => {
  const path = join(outDir, name);
  await page.screenshot({ path });
  console.log(path);
};

mkdirSync(outDir, { recursive: true });
await wait(settleMs); // extension activation + LSP startup + first semantic tokens
await resizeWindow(1600, 1400);

// complete.png: sample.todo with Outline and Problems (0 diagnostics) open.
await key("Control+Shift+M"); // View: Problems
await wait(1500);
await showOutline();
await wait(500);
await shot("complete.png");

// highlighting.png: editor only, side bar (Ctrl+B) and panel (Ctrl+J) closed.
await key("Control+J");
await wait(500);
await key("Control+B");
await wait(500);
await key("Control+Home"); // ensure the editor has focus and show the top
await wait(1000);
await shot("highlighting.png");

// fold-comments.png: comment (gray block) folds applied (SPEC §コマンド Alt+F).
await runCommand("editor.foldAllBlockComments");
await wait(1000);
await shot("fold-comments.png");

// fold-headings.png: every heading fold region collapsed. Fold Level 1
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
await shot("fold-headings.png");

await browser.close();
