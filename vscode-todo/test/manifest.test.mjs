// Machine-checked coverage for SPEC.md's extension-manifest behaviors:
// ファイル関連付け / 言語名 / 括弧の自動入力 / 囲みペア / エディタ規定値 /
// コマンドとキーバインド / 設定 contribution. Run via `npm test`.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { COMMAND_SPECS } from "../src/commandSpecs.mjs";
import {
  platformDirectoryName,
  serverBinaryName,
} from "../src/platform.mjs";

const root = dirname(fileURLToPath(import.meta.url));
const pkg = JSON.parse(readFileSync(join(root, "..", "package.json"), "utf8"));
const langConfig = JSON.parse(
  readFileSync(join(root, "..", "language-configuration.json"), "utf8"),
);

test("ファイル関連付け: .todo and .tasks open as language `todo`", () => {
  const language = pkg.contributes.languages.find((l) => l.id === "todo");
  assert.ok(language, "language `todo` contributed");
  assert.deepEqual([...language.extensions].sort(), [".tasks", ".todo"]);
  assert.deepEqual(language.aliases, ["Todo", "Tasks"]);
  assert.equal(language.configuration, "./language-configuration.json");
});

test("括弧の自動入力: { [ ( and ` auto-close", () => {
  const opens = langConfig.autoClosingPairs.map((p) => p.open).sort();
  assert.deepEqual(opens, ["(", "[", "`", "{"]);
});

test("囲みペア: {} [] () ` and * surround selections", () => {
  const opens = langConfig.surroundingPairs.map((p) => p.open).sort();
  assert.deepEqual(opens, ["(", "*", "[", "`", "{"]);
});

test("エディタ規定値 for [todo]", () => {
  const defaults = pkg.contributes.configurationDefaults["[todo]"];
  assert.deepEqual(defaults, {
    "editor.semanticHighlighting.enabled": true,
    "editor.tabSize": 4,
    "editor.bracketPairColorization.enabled": false,
    "editor.matchBrackets": "never",
    "editor.occurrencesHighlight": "off",
    "editor.selectionHighlight": false,
    "editor.stickyScroll.enabled": false,
    "editor.foldingHighlight": false,
  });
});

// §コマンド: command ID / palette title / default key triples.
const SPEC_COMMANDS = [
  ["todo-language.toggleDone", "Toggle Line Done", "alt+d"],
  ["todo-language.toggleCancelled", "Toggle Line Cancelled", "alt+c"],
  ["todo-language.toggleStart", "Toggle Line Start", "alt+s"],
  ["todo-language.toggleDue", "Toggle Line Due", "alt+shift+d"],
  ["todo-language.toggleQueue", "Toggle Line Queue", "alt+q"],
  ["todo-language.toggleQueueUnshift", "Toggle Line Queue Unshift", "alt+shift+q"],
  ["todo-language.toggleWaiting", "Toggle Line Waiting", "alt+w"],
  ["todo-language.togglePending", "Toggle Line Pending", "alt+p"],
  ["todo-language.toggleHide", "Toggle Line Hide", "alt+h"],
  ["todo-language.toggleRepeat", "Toggle Line Repeat", "alt+r"],
  ["todo-language.indent", "Indent Lines", "tab"],
  ["todo-language.dedent", "Dedent Lines", "shift+tab"],
  ["todo-language.repeatTasks", "Repeat Tasks", null],
  ["todo-language.archive", "Archive Task Blocks", "alt+a"],
  ["todo-language.unarchive", "Unarchive Task Blocks", "alt+shift+a"],
];

test("コマンド: all 15 commands contributed with palette titles", () => {
  const commands = pkg.contributes.commands;
  assert.equal(commands.length, SPEC_COMMANDS.length);
  for (const [id, title] of SPEC_COMMANDS) {
    const entry = commands.find((c) => c.command === id);
    assert.ok(entry, `command ${id} missing`);
    assert.equal(entry.title, title);
  }
});

test("コマンド: default keys bound only while editing a writable todo file", () => {
  const bindings = pkg.contributes.keybindings;
  const todoBindings = bindings.filter((b) =>
    b.command.startsWith("todo-language."),
  );
  assert.equal(todoBindings.length, SPEC_COMMANDS.filter((c) => c[2]).length);
  for (const [id, , key] of SPEC_COMMANDS) {
    const binding = todoBindings.find((b) => b.command === id);
    if (key === null) {
      assert.equal(binding, undefined, `${id} has no default key`);
      continue;
    }
    assert.ok(binding, `keybinding for ${id} missing`);
    assert.equal(binding.key, key);
    assert.equal(
      binding.when,
      "editorTextFocus && editorLangId == todo && !editorReadonly",
    );
  }
});

test("コマンド: Alt+F folds all comment ranges while editing a writable todo file", () => {
  const binding = pkg.contributes.keybindings.find(
    (b) => b.command === "editor.foldAllBlockComments",
  );
  assert.ok(binding, "editor.foldAllBlockComments keybinding missing");
  assert.equal(binding.key, "alt+f");
  assert.equal(
    binding.when,
    "editorTextFocus && editorLangId == todo && !editorReadonly",
  );
});

test("コマンド: the extension registers every contributed command", () => {
  const contributed = pkg.contributes.commands.map((c) => c.command).sort();
  const registered = COMMAND_SPECS.map((s) => s.id).sort();
  assert.deepEqual(registered, contributed);
  // Repeat Tasks is document-wide; every other command passes the selection.
  for (const spec of COMMAND_SPECS) {
    assert.equal(
      spec.needsSelection,
      spec.id !== "todo-language.repeatTasks",
      `needsSelection wrong for ${spec.id}`,
    );
  }
});

test("設定: todo-language.repeatTask.autoRepeat contributed, default true", () => {
  const property =
    pkg.contributes.configuration.properties["todo-language.repeatTask.autoRepeat"];
  assert.deepEqual(property, {
    type: "boolean",
    default: true,
    description: property.description,
  });
  assert.equal(property.default, true);
});

test("利用環境: supported platforms resolve their binary directory and name", () => {
  assert.equal(platformDirectoryName("win32", "x64"), "win32-x64");
  assert.equal(platformDirectoryName("linux", "x64"), "linux-x64");
  assert.equal(serverBinaryName("win32"), "todo-lsp.exe");
  assert.equal(serverBinaryName("linux"), "todo-lsp");
});

test("利用環境: unsupported platforms fail with the spec error message", () => {
  assert.throws(
    () => platformDirectoryName("darwin", "arm64"),
    /^Error: todo-lsp: unsupported platform darwin\/arm64\. Ship a matching binary under bin\/\.$/,
  );
});

// §表示: the semantic-token color rules mirror SPEC.md's color tables.
test("表示: semantic token color rules match the spec colors", () => {
  const rules =
    pkg.contributes.configurationDefaults["editor.semanticTokenColorCustomizations"].rules;
  assert.deepEqual(rules, {
    "todo-line": "#666666",
    "todo-line.italic": { foreground: "#666666", italic: true },
    "todo-heading-content": { foreground: "#66d9ef", bold: true },
    "todo-heading-symbol": { bold: true },
    "todo-tag": "#ACA7CB",
    "todo-tag.queue1": "#F9F871",
    "todo-bold": { foreground: "#D57F6D", bold: true },
    "todo-italic": { italic: true },
    "todo-code": "#FAB565",
    "start-tag.future": "#ACA7CB",
    "start-tag.past": "#F9F871",
    "start-tag.invalid": { foreground: "#FF0000", underline: true },
    "due-tag.future": "#ACA7CB",
    "due-tag.past": "#FF0000",
    "due-tag.invalid": { foreground: "#FF0000", underline: true },
    "repeat-tag.valid": "#FAB565",
    "repeat-tag.invalid": { foreground: "#FF0000", underline: true },
  });
});
