import assert from "node:assert/strict";
import test from "node:test";

import {
  archive,
  formatDocument,
  indentUnits,
  lineEdits,
  matchEol,
  reindent,
  toggle,
  unarchive,
  type TextEdit,
  type ToggleAction,
  Toggle,
} from "../src/index.js";

const today = new Date(Date.UTC(2024, 5, 15, 12, 0));

test("toggles every supported tag with its specified value", () => {
  const cases: Array<[ToggleAction, string]> = [
    [Toggle.Done, "task @done(2024-06-15)\n"],
    [Toggle.Cancelled, "task @cancelled(2024-06-15)\n"],
    [Toggle.Start, "task @start(2024-06-15)\n"],
    [Toggle.Due, "task @due(2024-06-15)\n"],
    [Toggle.Queue, "task @queue(1)\n"],
    [Toggle.QueueUnshift, "task @queue(1)\n"],
    [Toggle.Waiting, "task @waiting\n"],
    [Toggle.Pending, "task @pending\n"],
    [Toggle.Hide, "task @hide\n"],
    [Toggle.Repeat, "task @repeat(0 0 * * *)\n"],
  ];
  for (const [action, expected] of cases) {
    assert.equal(toggle("task\n", [0], action, today), expected, action);
  }
});

test("dated toggles use the execution date in the local timezone", (t) => {
  const originalTimezone = process.env.TZ;
  process.env.TZ = "America/Los_Angeles";
  t.after(() => {
    if (originalTimezone === undefined) delete process.env.TZ;
    else process.env.TZ = originalTimezone;
  });

  const dateNearUtcMidnight = new Date("2024-06-15T00:30:00Z");
  const datedActions: Array<[ToggleAction, string]> = [
    [Toggle.Done, "done"],
    [Toggle.Cancelled, "cancelled"],
    [Toggle.Start, "start"],
    [Toggle.Due, "due"],
  ];
  for (const [action, tag] of datedActions) {
    assert.equal(toggle("task\n", [0], action, dateNearUtcMidnight), `task @${tag}(2024-06-14)\n`);
  }
});

test("removes a tag only when every selected line has it", () => {
  assert.equal(
    toggle("a @waiting\nb\n", [0, 1], Toggle.Waiting, today),
    "a @waiting\nb @waiting\n",
  );
  assert.equal(toggle("a @waiting\nb @waiting\n", [0, 1], Toggle.Waiting, today), "a\nb\n");
});

test("done and cancelled replace conflicting state tags in both columns", () => {
  const input = "@waiting task @cancelled(2024-01-01) @queue(2) @pending\n";
  assert.equal(toggle(input, [0], Toggle.Done, today), "task @done(2024-06-15)\n");
  assert.equal(
    toggle("task @done @queue(2)\n", [0], Toggle.Cancelled, today),
    "task @cancelled(2024-06-15)\n",
  );
});

test("queue toggle renumbers distinct numeric values and excludes gray rows", () => {
  const input = "a @queue(0)\nb @queue(2)\nc @queue(5)\nd @queue(5)\ne @queue(8) @done\n";
  assert.equal(
    toggle(input, [0], Toggle.Queue, today),
    "a\nb @queue(1)\nc @queue(2)\nd @queue(2)\ne @queue(3) @done\n",
  );
  assert.equal(
    toggle("a @queue(1) @done\nb\n", [1], Toggle.Queue, today),
    "a @queue(1) @done\nb @queue(1)\n",
  );
});

test("indent and dedent use four spaces and clamp at level zero", () => {
  assert.equal(reindent("a\n\tb\n", [0, 1], 1), "    a\n        b\n");
  assert.equal(reindent("    a\n  b\n", [0, 1], -1), "a\nb\n");
  assert.equal(reindent("a\n\n", [0, 1], -5), "a\n\n");
});

test("indent and dedent write one level as tab-size spaces", () => {
  assert.equal(reindent("a\n    b\n", [0, 1], 1, 2), "  a\n      b\n");
  assert.equal(reindent("  a\n\n", [0, 1], -5, 2), "a\n\n");
});

test("indent measurement advances a tab to the next tab-size multiple", () => {
  assert.equal(indentUnits("\t", 2), 2);
  assert.equal(indentUnits(" \t", 2), 2);
  assert.equal(indentUnits("\t\t", 4), 8);
  assert.equal(indentUnits("\t", 4), 4);
});

test("format normalizes structure, whitespace, blank lines, and heading spacing", () => {
  const input = "\n  A:\n        child   @done\n\n\nplain\nB:\n  b\n";
  assert.equal(formatDocument(input), "A:\n    child @done\n\nplain\n\nB:\n    b\n");
  assert.equal(formatDocument("\n\n"), "");
  assert.equal(formatDocument("plain"), "plain\n");
  const once = formatDocument(input);
  assert.equal(formatDocument(once), once);
});

test("format rounds a partially indented child up to its parent level + 1", () => {
  const input = "A:\n   child\n      mid\n";
  assert.equal(formatDocument(input, 4), "A:\n    child\n        mid\n");
});

test("format writes one level as tab-size spaces", () => {
  assert.equal(formatDocument("A:\n  child\n", 2), "A:\n  child\n");
  assert.equal(formatDocument("A:\n child\n", 2), "A:\n  child\n");
});

test("toggle applies document formatting after updating the selected tag", () => {
  const source = "\n  Inbox:\n        task   \n\n\n  Archive:\n      old @done\n";
  assert.equal(
    toggle(source, [2], Toggle.Done, today),
    "Inbox:\n    task @done(2024-06-15)\n\nArchive:\n    old @done\n",
  );
});

test("formatting and command results preserve CRLF", () => {
  const source = "A:\r\n  task   \r\n";
  assert.equal(formatDocument(source), "A:\r\n    task\r\n");
  assert.equal(toggle("task\r\n", [0], Toggle.Done, today), "task @done(2024-06-15)\r\n");
  assert.equal(reindent("task\r\n", [0], 1), "    task\r\n");
});

test("format resolves mixed CRLF and LF to CRLF", () => {
  // SPEC §改行コード: a document mixing CRLF and LF formats as CRLF.
  assert.equal(
    formatDocument("A:\n   child\nB:\r\n  b\n"),
    "A:\r\n    child\r\n\r\nB:\r\n    b\r\n",
  );
  assert.equal(
    reindent("A:\n   child\nB:\r\n  b\n", [0, 1], 1),
    "    A:\r\n    child\r\nB:\r\n  b\r\n",
  );
});

test("archive moves selected all-gray top-level blocks under Archive", () => {
  const source = "keep\na @done\nb @cancelled\nArchive:\n    old @hide\n";
  assert.equal(
    archive(source, [2]),
    "keep\na @done\n\nArchive:\n    old @hide\n    b @cancelled\n",
  );
  assert.equal(archive("active\ndone @done\n", [1]), "active\n\nArchive:\n    done @done\n");
});

test("archive recognizes a partially indented root Archive heading", () => {
  const source = "  Archive:\n    old @done\nnew @done\n";
  assert.equal(archive(source, [2]), "Archive:\n    old @done\n    new @done\n");
});

test("archive leaves a block unchanged when any member is not gray", () => {
  const source = "Parent:\n    done @done\n    active\n";
  assert.equal(archive(source, [1]), source);
});

test("unarchive recognizes a partially indented root Archive heading", () => {
  const source = "   Archive:\n       old @done\n";
  assert.equal(unarchive(source, [1]), "Archive:\n\nold @done\n");
});

test("unarchive moves selected gray Archive children to the document end", () => {
  const source =
    "Inbox:\n    active\nArchive:\n    old @done\n        note @hide\n    keep @done\n";
  assert.equal(
    unarchive(source, [3]),
    "Inbox:\n    active\n\nArchive:\n    keep @done\n\nold @done\n    note @hide\n",
  );
  assert.equal(unarchive("task @done\n", [0]), "task @done\n");
});

test("line edits touch changed lines only and use UTF-16 positions", () => {
  const oldText = "😀 task\nkeep\nthird\n";
  const newText = "😀 task @done\nkeep\nthird\n";
  const edits = lineEdits(oldText, newText);
  assert.equal(edits.length, 1);
  assert.deepEqual(edits[0].range, {
    start: { line: 0, character: 0 },
    end: { line: 0, character: 7 },
  });
  assert.equal(applyEdits(oldText, edits), newText);
  assert.equal(applyEdits("", lineEdits("", "new\n")), "new\n");
  assert.equal(applyEdits("a", lineEdits("a", "a\n")), "a\n");
  assert.equal(applyEdits("a\n", lineEdits("a\n", "a")), "a");

  const scattered = lineEdits("a @queue(2)\nb\nc @queue(5)\n", "a @queue(1)\nb\nc @queue(2)\n");
  assert.equal(scattered.length, 2);
  assert.deepEqual(
    scattered.map((edit) => edit.range.start.line),
    [0, 2],
  );
});

test("line edits preserve CRLF and insert/delete whole changed lines", () => {
  const oldText = "keep\r\nold @done\r\n";
  const newText = "keep\nArchive:\n    old @done\n";
  const edits = lineEdits(oldText, newText);
  assert.equal(edits.length, 1);
  assert.equal(edits[0].newText, "Archive:\r\n    old @done");
  assert.equal(applyEdits(oldText, edits), matchEol(oldText, newText));
});

function applyEdits(source: string, edits: readonly TextEdit[]): string {
  let result = source;
  for (const edit of [...edits].reverse()) {
    const start = offsetAt(source, edit.range.start);
    const end = offsetAt(source, edit.range.end);
    result = result.slice(0, start) + edit.newText + result.slice(end);
  }
  return result;
}

function offsetAt(source: string, position: { line: number; character: number }): number {
  const lines = source.split("\n");
  let offset = 0;
  for (let line = 0; line < position.line; line++) offset += lines[line].length + 1;
  return offset + position.character;
}
