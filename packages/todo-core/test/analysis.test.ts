import assert from "node:assert/strict";
import test from "node:test";

import {
  createTodoParser,
  diagnostics,
  documentLinks,
  documentSymbols,
  foldingRanges,
  semanticTokens,
  semanticTokensAt,
  semanticTokensLegend,
  type Diagnostic,
  type DocumentSymbol,
  type FoldingRange,
} from "../src/index.js";

const SAMPLE = `Inbox:
  buy milk
  call mom @done(2024-01-01)
  Project:
    draft spec @priority(high)
    review @done
  wrap up
Archive:
  old task
`;

/** Fixed "now" for deterministic @start/@due classification. */
function fixedNow(): Date {
  return new Date(Date.UTC(2024, 5, 15, 12, 0));
}

/** Decode delta-encoded tokens back to absolute (line, col, len, type,
 * modifiers) tuples — easier to assert than raw deltas. */
function absPositions(
  tokens: ReturnType<typeof semanticTokens>,
): [number, number, number, number, number][] {
  let line = 0;
  let col = 0;
  return tokens.map((t) => {
    line += t.deltaLine;
    col = t.deltaLine === 0 ? col + t.deltaStart : t.deltaStart;
    return [line, col, t.length, t.tokenType, t.tokenModifiers];
  });
}

function tokensAt(text: string, now: Date) {
  return absPositions(semanticTokensAt(text, now));
}

test("outline: sample document symbols", async () => {
  const parser = await createTodoParser();
  const symbols = documentSymbols(parser.parse(SAMPLE).rootNode, SAMPLE);
  assert.equal(symbols.length, 2); // top-level: Inbox, Archive

  const inbox = symbols[0];
  assert.equal(inbox.name, "Inbox");
  assert.equal(inbox.kind, "module");

  const inboxChildren = inbox.children!;
  assert.equal(inboxChildren.length, 4);
  assert.equal(inboxChildren[0].name, "buy milk");
  assert.equal(inboxChildren[1].name, "call mom");
  assert.equal(inboxChildren[2].name, "Project");
  assert.equal(inboxChildren[2].kind, "module");
  assert.equal(inboxChildren[3].name, "wrap up");

  const projectChildren = inboxChildren[2].children!;
  assert.equal(projectChildren.length, 2);
  assert.equal(projectChildren[0].name, "draft spec");
  assert.equal(projectChildren[1].name, "review");

  const archive = symbols[1];
  assert.equal(archive.name, "Archive");
  assert.equal(archive.children!.length, 1);
  assert.equal(archive.children![0].name, "old task");
});

test("outline: symbol ranges are UTF-16 positions", async () => {
  const text = "タスク見出し:\n  子供のタスク\n";
  const parser = await createTodoParser();
  const symbols = documentSymbols(parser.parse(text).rootNode, text);
  // web-tree-sitter parses strings as UTF-16, so node indices slice the
  // source directly (the name extraction below) and columns pass through.
  assert.deepEqual(symbols[0].range.start, { line: 0, character: 0 });
  // The heading block consumes the trailing newline run: end at row 2 col 0.
  assert.deepEqual(symbols[0].range.end, { line: 2, character: 0 });
  assert.deepEqual(symbols[0].selectionRange.start, { line: 0, character: 0 });
  assert.deepEqual(symbols[0].selectionRange.end, { line: 1, character: 0 });
  assert.equal(symbols[0].name, "タスク見出し");
  assert.equal(symbols[0].children![0].name, "子供のタスク");
});

const symbols = (text: string) =>
  createTodoParser().then((p) => documentSymbols(p.parse(text).rootNode, text));

const topNames = (symbols_: DocumentSymbol[]) => symbols_.map((s) => s.name);

test("outline: simple task line", async () => {
  const s = await symbols("buy milk\n");
  assert.deepEqual(topNames(s), ["buy milk"]);
  assert.equal(s[0].kind, "string");
  assert.equal(s[0].children, undefined);
});

test("outline: tab indentation", async () => {
  const s = await symbols("A:\n\ttask\n");
  assert.deepEqual(topNames(s), ["A"]);
  assert.equal(s[0].kind, "module");
  assert.deepEqual(
    s[0].children!.map((c) => c.name),
    ["task"],
  );
  assert.deepEqual(
    s[0].children!.map((c) => c.kind),
    ["string"],
  );
});

test("outline: blank lines ignored", async () => {
  const s = await symbols("task a\n\ntask b\n");
  assert.deepEqual(topNames(s), ["task a", "task b"]);
});

test("outline: tag-only line yields empty symbol", async () => {
  const s = await symbols("@done\n");
  assert.deepEqual(topNames(s), [""]);
  assert.equal(s[0].kind, "string");
});

test("outline: colon-only line is a heading symbol", async () => {
  const s = await symbols(":\n");
  assert.deepEqual(topNames(s), [""]);
  assert.equal(s[0].kind, "module");
});

test("outline: leading tag column on heading is body, on task is not in name", async () => {
  const s = await symbols("@done Project:\n  @waiting buy milk @queue(1)\n");
  assert.deepEqual(topNames(s), ["@done Project"]);
  assert.deepEqual(
    s[0].children!.map((c) => c.name),
    ["buy milk"],
  );
});

test("outline: colon in body is task not heading", async () => {
  const s = await symbols("time is 12:30\n");
  assert.deepEqual(topNames(s), ["time is 12:30"]);
  assert.equal(s[0].kind, "string");
});

test("outline: nested headings", async () => {
  const s = await symbols(
    "Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n",
  );
  assert.deepEqual(topNames(s), ["Project"]);
  assert.deepEqual(
    s[0].children!.map((c) => c.name),
    ["Phase 1", "kickoff meeting"],
  );
  assert.deepEqual(
    s[0].children!.map((c) => c.kind),
    ["module", "string"],
  );
  assert.deepEqual(
    s[0].children![0].children!.map((c) => c.name),
    ["design spec", "prototype"],
  );
});

test("outline: sibling headings", async () => {
  const s = await symbols("List A:\n  task 1\nList B:\n  task 2\n");
  assert.deepEqual(topNames(s), ["List A", "List B"]);
  assert.deepEqual(
    s[0].children!.map((c) => c.name),
    ["task 1"],
  );
});

test("outline: heading without body has no children", async () => {
  const s = await symbols("Inbox:\n");
  assert.deepEqual(topNames(s), ["Inbox"]);
  assert.equal(s[0].children, undefined);
});

test("outline: tag arguments stripped from name", async () => {
  for (const input of [
    "task @done\n",
    "task @done(2024-01-01)\n",
    "task @done(2024-01-01) @folding @priority(high)\n",
  ]) {
    const s = await symbols(input);
    assert.deepEqual(topNames(s), ["task"], input);
    assert.equal(s[0].kind, "string", input);
  }
});

test("outline: tag edge arguments", async () => {
  for (const input of [
    "task @flag()\n",
    "task @link(http://example.com/path?q=1)\n",
    "task @note(remember to follow up tomorrow)\n",
    "task @ref(@other)\n",
  ]) {
    const s = await symbols(input);
    assert.deepEqual(topNames(s), ["task"], input);
  }
  // A tag on a heading line does not change the heading name.
  const s = await symbols("List: @collapsed\n  item one\n");
  assert.deepEqual(topNames(s), ["List"]);
  assert.deepEqual(
    s[0].children!.map((c) => c.name),
    ["item one"],
  );
});

// ----- folding ranges -----

const folds = (text: string): Promise<FoldingRange[]> =>
  createTodoParser().then((p) => foldingRanges(p.parse(text).rootNode, text));

test("folding: sample has Inbox, Project, Archive region folds", async () => {
  const r = await folds(SAMPLE);
  assert.equal(r.length, 3);
  assert.ok(r.every((f) => f.kind === "region"));
  // 0-based rows: Inbox 0..6, Project 3..5, Archive 7..8.
  assert.deepEqual(
    r.map((f) => [f.startLine, f.endLine]),
    [
      [0, 6],
      [3, 5],
      [7, 8],
    ],
  );
});

test("folding: heading with only gray child is a comment fold", async () => {
  const r = await folds(
    "Archive:\n  Alt + A to move selected grayed blocks to archive @done(2024-01-01) @folding\n",
  );
  assert.deepEqual(r, [{ startLine: 0, endLine: 1, kind: "comment" }]);
});

test("folding: leading gray children replace heading region with comment", async () => {
  const r = await folds("Project:\n  a @done\n  b @cancelled\n  c\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 2, kind: "comment" }]);
});

test("folding: gray run excludes trailing blank lines", async () => {
  const r = await folds("Archive:\n  a @done\n  b @done\n\n  c\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 2, kind: "comment" }]);

  const r2 = await folds("x @done\ny @done\n\nz\n");
  assert.deepEqual(r2, [{ startLine: 0, endLine: 1, kind: "comment" }]);
});

test("folding: gray children with leading tag columns", async () => {
  const r = await folds("@done x\n@hide y\nc\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 1, kind: "comment" }]);
});

test("folding: gray run including tag-only lines", async () => {
  const r = await folds("@done\n@cancelled\nplain\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 1, kind: "comment" }]);
});

test("folding: ignores gray children after a plain first child", async () => {
  const r = await folds("Project:\n  a\n  b @done\n  c @hide\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 3, kind: "region" }]);
});

test("folding: leading gray children across blank lines", async () => {
  const r = await folds("a @done\n\nb @hide\nc\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 2, kind: "comment" }]);
});

test("folding: all-gray heading and children as one comment fold", async () => {
  const r = await folds("Archive:\n  old @done\n  old2 @hide\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 2, kind: "comment" }]);
});

test("folding: task line owns its leading gray child run", async () => {
  const r = await folds("task\n  a @done\n  b @done\nc\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 2, kind: "comment" }]);
});

test("folding: task line with plain first child owns no gray run", async () => {
  const r = await folds("task\n  a\n  b @done\nc\n");
  assert.deepEqual(r, []);
});

test("folding: task-owned gray run inside a heading", async () => {
  const r = await folds("H:\n  task\n    a @done\n    b @done\n  z\n");
  assert.deepEqual(r, [
    { startLine: 0, endLine: 4, kind: "region" },
    { startLine: 1, endLine: 3, kind: "comment" },
  ]);
});

test("folding: task-owned gray run continues across blank lines", async () => {
  const r = await folds("task\n  a @done\n\n  b @done\nc\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 3, kind: "comment" }]);
});

test("folding: gray child with deeper gray descendant is one block", async () => {
  const r = await folds("task\n  a @done\n    b @done\nc\n");
  // `b` stays inside `a`'s block (not a second child block of task); `a`
  // itself owns the run of its own gray child block.
  assert.deepEqual(r, [
    { startLine: 0, endLine: 2, kind: "comment" },
    { startLine: 1, endLine: 2, kind: "comment" },
  ]);
});

test("folding: task-owned gray run deduplicates against the document run", async () => {
  const r = await folds("x @done\n  y @done\nz\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 1, kind: "comment" }]);
});

test("folding: nested comment ranges do not partially overlap", async () => {
  const r = await folds("Outer:\n  a\n  Inner:\n    x @done\n    y @done\n  z\n");
  assert.deepEqual(r, [
    { startLine: 0, endLine: 5, kind: "region" },
    { startLine: 2, endLine: 4, kind: "comment" },
  ]);
});

test("folding: omits single-line gray runs and duplicate ranges", async () => {
  assert.deepEqual(await folds("a @done\nb\n"), []);

  const r = await folds("P: @done\n  a @done\nz\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 1, kind: "comment" }]);
});

test("folding: tab indent", async () => {
  const r = await folds("A:\n\ttask\n");
  assert.deepEqual(r, [{ startLine: 0, endLine: 1, kind: "region" }]);
});

test("folding: nested boundaries", async () => {
  const r = await folds(
    "Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n",
  );
  assert.deepEqual(r, [
    { startLine: 0, endLine: 4, kind: "region" },
    { startLine: 1, endLine: 3, kind: "region" },
  ]);
});

test("folding: dedent to top level", async () => {
  const r = await folds("A:\n  B:\n    task\nback to top\n");
  assert.deepEqual(r, [
    { startLine: 0, endLine: 2, kind: "region" },
    { startLine: 1, endLine: 2, kind: "region" },
  ]);
});

test("folding: dedent to intermediate", async () => {
  const r = await folds("A:\n  B:\n    task\n  sibling of B\n");
  assert.deepEqual(r, [
    { startLine: 0, endLine: 3, kind: "region" },
    { startLine: 1, endLine: 2, kind: "region" },
  ]);
});

test("folding: header without body is empty", async () => {
  assert.deepEqual(await folds("Inbox:\n"), []);
});

test("folding: top-level task lines is empty", async () => {
  assert.deepEqual(await folds("buy milk\ncall mom\n"), []);
});

// ----- diagnostics -----

const diagsOf = (text: string): Promise<Diagnostic[]> =>
  createTodoParser().then((p) => diagnostics(p.parse(text).rootNode));

test("diagnostics: valid corpus inputs are clean", async () => {
  const valid = [
    "buy milk\n",
    "task a\n\ntask b\n",
    "time is 12:30\n",
    "see http://example.com for details\n",
    "email me at user@example.com @done\n",
    "Project:\n  Phase 1:\n    design spec\n    prototype\n  kickoff meeting\n",
    "List A:\n  task 1\nList B:\n  task 2\n",
    "Inbox:\n",
    "A:\n\ttask\n",
    "A:\n  B:\n    task\n",
    "A:\n  B:\n    task\nback to top\n",
    "A:\n  B:\n    task\n  sibling of B\n",
    "task @done(2024-01-01) @folding @priority(high)\n",
    "task @flag()\n",
    "task @link(http://example.com/path?q=1)\n",
    "task @note(remember to follow up tomorrow)\n",
    "task @ref(@other)\n",
    "List: @collapsed\n  item one\n",
  ];
  for (const input of valid) {
    assert.deepEqual(await diagsOf(input), [], input);
  }
});

test("diagnostics: unclosed tag is body text and clean", async () => {
  for (const input of ["@done(", "task\n  @done(", "Project:\n  @done("]) {
    assert.deepEqual(await diagsOf(input), [], input);
  }
});

test("diagnostics: colon-initial lines are clean", async () => {
  for (const input of [":", ": @done", ":foo", ":foo:"]) {
    assert.deepEqual(await diagsOf(input), [], input);
  }
  for (const input of ["@done", "@done @waiting", "@done("]) {
    assert.deepEqual(await diagsOf(input), [], input);
  }
});

test("diagnostics: any document reports no errors", async () => {
  // SPEC 診断: “Todo 文書はどのような行構成でも構文エラーにならない”.
  // A tab-only line between differently indented rows still makes the
  // grammar produce an ERROR node, yet diagnostics stay empty.
  const parser = await createTodoParser();
  const tree = parser.parse("a\n\t\n  b\n");
  assert.ok(tree.rootNode.hasError, "expected the grammar to produce an ERROR node");
  assert.deepEqual(diagnostics(tree.rootNode), []);
});

// ----- semantic tokens -----

test("semantic tokens: legend order is pinned", () => {
  const legend = semanticTokensLegend();
  assert.deepEqual(legend.tokenTypes, [
    "todo-line",
    "todo-heading-content",
    "todo-heading-symbol",
    "todo-tag",
    "start-tag",
    "due-tag",
    "repeat-tag",
    "todo-bold",
    "todo-italic",
    "todo-code",
    "todo-url",
  ]);
  assert.deepEqual(legend.tokenModifiers, [
    "italic",
    "queue1",
    "past",
    "future",
    "invalid",
    "valid",
  ]);
});

const [
  TODO_LINE,
  TODO_HEADING_CONTENT,
  TODO_HEADING_SYMBOL,
  TODO_TAG,
  START_TAG,
  DUE_TAG,
  REPEAT_TAG,
  TODO_BOLD,
  TODO_ITALIC,
  TODO_CODE,
  TODO_URL,
] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
const [ITALIC, QUEUE1, PAST, FUTURE, INVALID, VALID] = [1, 2, 4, 8, 16, 32];

test("semantic tokens: empty document", () => {
  assert.deepEqual(semanticTokens(""), []);
});

test("semantic tokens: sample document", async () => {
  const abs = absPositions(semanticTokens(SAMPLE));
  assert.equal(abs.length, 8, "2 headings x2 tokens + 1 tag + 2 gray lines + archive");
  assert.deepEqual(abs[0], [0, 0, 5, TODO_HEADING_CONTENT, 0]); // L0 "Inbox"
  assert.deepEqual(abs[1], [0, 5, 1, TODO_HEADING_SYMBOL, 0]); // L0 ":"
  // L2 has @done in its tag column -> whole line gray.
  assert.deepEqual(abs[2], [2, 0, 28, TODO_LINE, 0]);
  assert.deepEqual(abs[3], [3, 2, 7, TODO_HEADING_CONTENT, 0]); // L3 "Project"
  assert.deepEqual(abs[4], [3, 9, 1, TODO_HEADING_SYMBOL, 0]); // L3 ":"
  assert.deepEqual(abs[5], [4, 15, 15, TODO_TAG, 0]); // L4 "@priority(high)"
  assert.deepEqual(abs[6], [5, 0, 16, TODO_LINE, 0]); // L5 gray (@done)
  assert.deepEqual(abs[7], [7, 0, 8, TODO_LINE, 0]); // L7 "Archive:"
});

test("semantic tokens: heading content and symbol", async () => {
  const abs = absPositions(semanticTokens("Inbox:\n"));
  assert.deepEqual(abs, [
    [0, 0, 5, TODO_HEADING_CONTENT, 0],
    [0, 5, 1, TODO_HEADING_SYMBOL, 0],
  ]);
});

test("semantic tokens: colon-only heading", async () => {
  const abs = absPositions(semanticTokens(":\n"));
  assert.deepEqual(abs, [[0, 0, 1, TODO_HEADING_SYMBOL, 0]]);
});

test("semantic tokens: heading shows tags not stylings", async () => {
  const abs = absPositions(semanticTokens("List: @priority(high)\n"));
  assert.deepEqual(abs, [
    [0, 0, 4, TODO_HEADING_CONTENT, 0],
    [0, 4, 1, TODO_HEADING_SYMBOL, 0],
    [0, 6, 15, TODO_TAG, 0],
  ]);
});

test("semantic tokens: non-tag suffix is task line", async () => {
  // `List: **bold**` is a task line and its styling shows.
  const abs = absPositions(semanticTokens("List: **bold**\n"));
  assert.deepEqual(abs, [[0, 6, 8, TODO_BOLD, 0]]);
  // `Foo: @a b` — the tag is mid-line, so no heading, no tag.
  assert.deepEqual(semanticTokens("Foo: @a b\n"), []);
});

test("semantic tokens: done with trailing text is plain", async () => {
  assert.deepEqual(semanticTokens("task @done trailing\n"), []);
});

test("semantic tokens: done at eol is grayed", async () => {
  const abs = absPositions(semanticTokens("task @done\n"));
  assert.deepEqual(abs, [[0, 0, 10, TODO_LINE, 0]]);
});

test("semantic tokens: done in leading column is grayed", async () => {
  const abs = absPositions(semanticTokens("@done buy milk\n"));
  assert.deepEqual(abs, [[0, 0, 14, TODO_LINE, 0]]);
});

test("semantic tokens: cancelled is grayed italic", async () => {
  const abs = absPositions(semanticTokens("@cancelled buy milk\n"));
  assert.deepEqual(abs, [[0, 0, 19, TODO_LINE, ITALIC]]);
  const abs2 = absPositions(semanticTokens("call mom @cancelled(2024-01-01)\n"));
  assert.deepEqual(abs2, [[0, 0, 31, TODO_LINE, ITALIC]]);
});

test("semantic tokens: hide grayed", async () => {
  const abs = absPositions(semanticTokens("task @hide\n"));
  assert.deepEqual(abs, [[0, 0, 10, TODO_LINE, 0]]);
});

test("semantic tokens: precedence done beats cancelled", async () => {
  const abs = absPositions(semanticTokens("task @cancelled @done\n"));
  assert.deepEqual(abs, [[0, 0, 21, TODO_LINE, 0]]);
});

test("semantic tokens: archive heading", async () => {
  assert.deepEqual(absPositions(semanticTokens("Archive:\n")), [[0, 0, 8, TODO_LINE, 0]]);
  // An indented Archive: heading is still recognized and outranks @done.
  assert.deepEqual(absPositions(semanticTokens("  Archive: @done\n")), [[0, 0, 16, TODO_LINE, 0]]);
  // A suffix that is not a tag column is not an Archive heading.
  assert.deepEqual(semanticTokens("  Archive: old stuff\n"), []);
});

test("semantic tokens: queue tiers", async () => {
  const abs = absPositions(semanticTokens("@queue(1)\n"));
  assert.deepEqual(abs, [[0, 0, 9, TODO_TAG, QUEUE1]]);
  for (const input of ["@queue(2)\n", "@queue(3)\n", "@queue(9)\n"]) {
    const a = absPositions(semanticTokens(input));
    assert.deepEqual(a, [[0, 0, 9, TODO_TAG, 0]], input);
  }
});

test("semantic tokens: generic tag", async () => {
  const abs = absPositions(semanticTokens("task @priority(high)\n"));
  assert.deepEqual(abs, [[0, 5, 15, TODO_TAG, 0]]);
});

test("semantic tokens: at in text is not tagged", async () => {
  assert.deepEqual(semanticTokens("email user@example.com\n"), []);
  const abs = absPositions(semanticTokens("send to a@b @priority(high)\n"));
  assert.deepEqual(abs, [[0, 12, 15, TODO_TAG, 0]]);
});

test("semantic tokens: multiple tags same line delta encoded", async () => {
  const tokens = semanticTokens("task @a @b\n");
  assert.deepEqual(tokens, [
    { deltaLine: 0, deltaStart: 5, length: 2, tokenType: TODO_TAG, tokenModifiers: 0 },
    { deltaLine: 0, deltaStart: 3, length: 2, tokenType: TODO_TAG, tokenModifiers: 0 },
  ]);
});

test("semantic tokens: bold italic code url", async () => {
  assert.deepEqual(absPositions(semanticTokens("**bold**\n")), [[0, 0, 8, TODO_BOLD, 0]]);
  assert.deepEqual(absPositions(semanticTokens("*italic*\n")), [[0, 0, 8, TODO_ITALIC, 0]]);
  assert.deepEqual(absPositions(semanticTokens("`code`\n")), [[0, 0, 6, TODO_CODE, 0]]);
  assert.deepEqual(absPositions(semanticTokens("<https://example.com>\n")), [
    [0, 0, 21, TODO_URL, 0],
  ]);
});

test("semantic tokens: bold then italic ordering", async () => {
  const abs = absPositions(semanticTokens("**b** *i*\n"));
  assert.deepEqual(abs, [
    [0, 0, 5, TODO_BOLD, 0],
    [0, 6, 3, TODO_ITALIC, 0],
  ]);
});

test("semantic tokens: code double backtick", async () => {
  const abs = absPositions(semanticTokens("``a`b``\n"));
  assert.deepEqual(abs, [[0, 0, 7, TODO_CODE, 0]]);
});

test("semantic tokens: unclosed stylings and urls have no tokens", async () => {
  assert.deepEqual(semanticTokens("*unclosed\n"), []);
  assert.deepEqual(semanticTokens("see **not bold\n"), []);
  assert.deepEqual(semanticTokens("`unclosed\n"), []);
  assert.deepEqual(semanticTokens("see <https://example.com\n"), []);
});

test("semantic tokens: start tag modifiers", async () => {
  const now = fixedNow();
  assert.deepEqual(tokensAt("@start(2000-01-01)\n", now), [[0, 0, 18, START_TAG, PAST]]);
  assert.deepEqual(tokensAt("@start(2100-01-01)\n", now), [[0, 0, 18, START_TAG, FUTURE]]);
  assert.deepEqual(tokensAt("@start(foo)\n", now), [[0, 0, 11, START_TAG, INVALID]]);
});

test("semantic tokens: argumentless start tag is invalid", async () => {
  assert.deepEqual(tokensAt("@start\n", fixedNow()), [[0, 0, 6, START_TAG, INVALID]]);
});

test("semantic tokens: due tag modifiers", async () => {
  const now = fixedNow();
  assert.deepEqual(tokensAt("@due(2000-01-01)\n", now), [[0, 0, 16, DUE_TAG, PAST]]);
  assert.deepEqual(tokensAt("@due(2100-01-01)\n", now), [[0, 0, 16, DUE_TAG, FUTURE]]);
  assert.deepEqual(tokensAt("@due(foo)\n", now), [[0, 0, 9, DUE_TAG, INVALID]]);
});

test("semantic tokens: argumentless due tag is invalid", async () => {
  assert.deepEqual(tokensAt("@due\n", fixedNow()), [[0, 0, 4, DUE_TAG, INVALID]]);
});

test("semantic tokens: now boundary is past", async () => {
  assert.deepEqual(tokensAt("@start(2024-06-15 12:00)\n", fixedNow()), [
    [0, 0, 24, START_TAG, PAST],
  ]);
});

test("semantic tokens: seconds format is invalid", async () => {
  assert.deepEqual(tokensAt("@start(2024-06-15 12:00:00)\n", fixedNow()), [
    [0, 0, 27, START_TAG, INVALID],
  ]);
});

test("semantic tokens: grayed suppresses date tag", async () => {
  const abs = absPositions(semanticTokens("task @done @due(2100-01-01)\n"));
  assert.deepEqual(abs, [[0, 0, 27, TODO_LINE, 0]]);
});

test("semantic tokens: repeat valid invalid", async () => {
  assert.deepEqual(absPositions(semanticTokens("@repeat(0 0 * * *)\n")), [
    [0, 0, 18, REPEAT_TAG, VALID],
  ]);
  assert.deepEqual(absPositions(semanticTokens("@repeat(notacron)\n")), [
    [0, 0, 17, REPEAT_TAG, INVALID],
  ]);
});

test("semantic tokens: argumentless repeat tag is invalid", async () => {
  assert.deepEqual(absPositions(semanticTokens("@repeat\n")), [
    [0, 0, 7, REPEAT_TAG, INVALID],
  ]);
});

test("semantic tokens: cron L W hash extensions are valid", async () => {
  for (const input of [
    "@repeat(0 0 * * 5L)\n",
    "@repeat(0 0 1W * *)\n",
    "@repeat(0 0 * * 1#1)\n",
  ]) {
    const abs = absPositions(semanticTokens(input));
    assert.equal(abs.length, 1, input);
    assert.equal(abs[0][3], REPEAT_TAG, input);
    assert.equal(abs[0][4], VALID, input);
  }
});

test("semantic tokens: cron validation matches repeat evaluation", () => {
  const valid = absPositions(semanticTokens("@repeat(0 0 * JAN MON)\n"));
  assert.equal(valid[0][3], REPEAT_TAG);
  assert.equal(valid[0][4], VALID);
  const invalid = absPositions(semanticTokens("@repeat(0 0 0 * * *)\n"));
  assert.equal(invalid[0][3], REPEAT_TAG);
  assert.equal(invalid[0][4], INVALID);
});

test("semantic tokens: non-ascii columns are UTF-16 code units", async () => {
  // "タスク" is 3 code points = 3 UTF-16 code units; " " then "@a" at 4.
  const abs = absPositions(semanticTokens("タスク @a\n"));
  assert.deepEqual(abs, [[0, 4, 2, TODO_TAG, 0]]);
  // A non-BMP character occupies 2 UTF-16 code units (Rust byte columns
  // differ: 4 bytes) — SPEC「LSP の位置」requires UTF-16.
  const abs2 = absPositions(semanticTokens("😀 @a\n"));
  assert.deepEqual(abs2, [[0, 3, 2, TODO_TAG, 0]]);
});

test("semantic tokens: hash line emits no token", async () => {
  assert.deepEqual(semanticTokens("# just a note\n"), []);
});

// ----- document links -----

test("document links: closed urls and display precedence", async () => {
  const links = documentLinks("see <https://example.com> <ftp://example.org/path>\n");
  assert.equal(links.length, 2);
  assert.deepEqual(links[0].range.start, { line: 0, character: 4 });
  assert.deepEqual(links[0].range.end, { line: 0, character: 25 });
  assert.equal(links[0].target, "https://example.com");
  assert.equal(links[1].target, "ftp://example.org/path");
  // Unclosed URLs and URLs in gray lines do not expose individual links.
  assert.deepEqual(documentLinks("see <http://example.com\n"), []);
  assert.deepEqual(documentLinks("see <http://example.com> @done\n"), []);
});

test("document links: closed http url", () => {
  const links = documentLinks("see <http://example.com>\n");
  assert.equal(links.length, 1);
  assert.deepEqual(links[0].range, {
    start: { line: 0, character: 4 },
    end: { line: 0, character: 24 },
  });
  assert.equal(links[0].target, "http://example.com");
});

test("document links: headings, gray lines and archive lines are excluded", async () => {
  // A URL pushes the rightmost colon off the line end, so these lines are
  // task lines (SPEC 見出し: the colon must end the line) and their URLs
  // link — even `Inbox:` / `Archive:` lookalikes and a mid-line `@done`.
  const inbox = documentLinks("Inbox: <https://example.com>\n");
  assert.equal(inbox.length, 1);
  assert.equal(inbox[0].target, "https://example.com");
  assert.equal(documentLinks("Archive: <https://example.com>\n").length, 1);
  assert.equal(documentLinks("  Archive: <https://example.com>\n").length, 1);
  assert.equal(documentLinks("task @done <https://example.com>\n").length, 1);
  // A gray line (@done in the leading tag column) exposes no links.
  assert.deepEqual(documentLinks("@done <https://example.com>\n"), []);
  // A true heading — the URL sits before the line-ending colon — exposes none.
  assert.deepEqual(documentLinks("see <https://example.com>:\n"), []);
  assert.deepEqual(documentLinks("see <https://example.com>: @tag\n"), []);
});

test("document links: indented plain task line keeps links with UTF-16 columns", async () => {
  const links = documentLinks("  see <https://example.com>\n");
  assert.equal(links.length, 1);
  assert.deepEqual(links[0].range.start, { line: 0, character: 6 });
  assert.deepEqual(links[0].range.end, { line: 0, character: 27 });
  assert.equal(links[0].target, "https://example.com");
});

test("document links: uri that cannot be parsed is skipped", async () => {
  // A scheme is required by the URI grammar; `http://` with an empty
  // authority still parses, but a malformed URI does not link.
  assert.deepEqual(documentLinks("see <https://exa mple.com>\n"), []);
  // Non-URL text in angle brackets never matches a supported scheme.
  assert.deepEqual(documentLinks("see <mailto:a@b.com>\n"), []);
});

test("document links: non-ascii columns are UTF-16 code units", async () => {
  const links = documentLinks("タスク <https://example.com>\n");
  assert.equal(links.length, 1);
  assert.deepEqual(links[0].range.start, { line: 0, character: 4 });
  assert.deepEqual(links[0].range.end, { line: 0, character: 25 });
});
