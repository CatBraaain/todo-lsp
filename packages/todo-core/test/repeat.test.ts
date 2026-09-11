import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import test from "node:test";
import { promisify } from "node:util";

import { repeatTasks } from "../src/index.js";

const runNode = promisify(execFile);

const noon = new Date(2024, 5, 15, 12, 0);

test("generates the previous local occurrence and renders midnight as a date", () => {
  assert.equal(
    repeatTasks("sweep @repeat(0 0 * * *)\n", noon),
    "sweep @repeat(0 0 * * *)\nsweep @start(2024-06-15)\n",
  );
  assert.equal(
    repeatTasks("check @repeat(0 9 * * *)\n", noon),
    "check @repeat(0 9 * * *)\ncheck @start(2024-06-15 09:00)\n",
  );
});

test("uses the caller's current Date and ignores sub-minute precision", () => {
  const now = new Date(2024, 5, 15, 12, 0, 30, 500);
  const once = repeatTasks("task @repeat(0 12 * * *)\n", now);
  assert.equal(once, "task @repeat(0 12 * * *)\ntask @start(2024-06-15 12:00)\n");
  assert.equal(repeatTasks(once, now), once);
});

test("uses the current time when no Date is supplied", () => {
  const source = "task @repeat(* * * * *)\n";
  assert.notEqual(repeatTasks(source), source);
});

test("skips definitions whose start is later than the previous occurrence", () => {
  const source = "task @repeat(0 0 * * *) @start(2024-06-16)\n";
  assert.equal(repeatTasks(source, noon), source);
});

test("creates missing path parents and appends the task to the destination", () => {
  const source = "Home:\n    stuff\nHome/Kitchen/mop floor @repeat(0 0 * * *)\n";
  assert.equal(
    repeatTasks(source, noon),
    "Home:\n    stuff\n    Kitchen\n        mop floor @start(2024-06-15)\n\nHome/Kitchen/mop floor @repeat(0 0 * * *)\n",
  );
});

test("suppresses duplicate tasks anywhere in the document", () => {
  const source =
    "Elsewhere:\n    buy milk @start(2024-06-15 00:00)\nInbox/buy milk @repeat(0 0 * * *)\n";
  assert.equal(
    repeatTasks(source, noon),
    "Elsewhere:\n    buy milk @start(2024-06-15 00:00)\n\nInbox/buy milk @repeat(0 0 * * *)\n",
  );
});

test("resolves missing path parents before suppressing a duplicate", () => {
  const source = "Elsewhere:\n    name @start(2024-06-15)\n\nP/Q/name @repeat(0 0 * * *)\n";
  assert.equal(
    repeatTasks(source, noon),
    "Elsewhere:\n    name @start(2024-06-15)\n\nP/Q/name @repeat(0 0 * * *)\n    Q\n",
  );
});

test("uses a definition line as a matching path parent", () => {
  assert.equal(
    repeatTasks("Chores/mop floor @repeat(0 0 * * *)\n", noon),
    "Chores/mop floor @repeat(0 0 * * *)\n    mop floor @start(2024-06-15)\n",
  );
});

test("resolves direct path parents across blank lines", () => {
  const source = "Inbox:\n\nHome:\n    stuff\nHome/mop floor @repeat(0 0 * * *)\n";
  assert.equal(
    repeatTasks(source, noon),
    "Inbox:\n\nHome:\n    stuff\n    mop floor @start(2024-06-15)\n\nHome/mop floor @repeat(0 0 * * *)\n",
  );
});

test("resolves nested path parents across blank lines", () => {
  const source =
    "Home:\n\n    Kitchen:\n        dishes\nHome/Kitchen/mop floor @repeat(0 0 * * *)\n";
  assert.equal(
    repeatTasks(source, noon),
    "Home:\n\n    Kitchen:\n        dishes\n        mop floor @start(2024-06-15)\nHome/Kitchen/mop floor @repeat(0 0 * * *)\n",
  );
});

test("suppresses duplicates across blank lines", () => {
  const source =
    "intro\n\nInbox:\n    buy milk @start(2024-06-15)\n\nInbox/buy milk @repeat(0 0 * * *)\n";
  assert.equal(repeatTasks(source, noon), source);
});

test("processes definitions in document order before formatting", () => {
  const source = "a @repeat(0 0 * * *)\nb @repeat(0 12 * * *)\n";
  assert.equal(
    repeatTasks(source, noon),
    "a @repeat(0 0 * * *)\nb @repeat(0 12 * * *)\na @start(2024-06-15)\nb @start(2024-06-14 12:00)\n",
  );
});

test("evaluates L, #, and W cron extensions", () => {
  const cases: Array<[string, string]> = [
    ["last @repeat(0 0 L * *)\n", "last @start(2024-05-31)\n"],
    ["second Friday @repeat(0 0 * * 5#2)\n", "second Friday @start(2024-06-14)\n"],
    ["weekday @repeat(0 0 15W * *)\n", "weekday @start(2024-06-14)\n"],
    ["last Friday @repeat(0 0 * * 5L)\n", "last Friday @start(2024-05-31)\n"],
  ];
  for (const [source, generated] of cases) {
    assert.equal(repeatTasks(source, noon), source + generated);
  }
});

test("resolves the previous occurrence in the process local timezone", async (t) => {
  const now = Date.UTC(2024, 5, 15, 3, 30);
  const cases: Array<[string, string, string]> = [
    ["Asia/Tokyo", "sweep @repeat(0 9 * * *)\n", "sweep @start(2024-06-15 09:00)\n"],
    ["Asia/Tokyo", "sweep @repeat(0 0 * * *)\n", "sweep @start(2024-06-15)\n"],
    ["America/New_York", "sweep @repeat(0 9 * * *)\n", "sweep @start(2024-06-14 09:00)\n"],
    ["America/New_York", "sweep @repeat(0 0 * * *)\n", "sweep @start(2024-06-14)\n"],
  ];
  for (const [timezone, source, generated] of cases) {
    const probe = await runInTimezone(timezone, source, now);
    if (probe.timeZone.toLowerCase() !== timezone.toLowerCase()) {
      t.skip(`environment does not honor TZ=${timezone}`);
      return;
    }
    assert.equal(probe.output, source + generated);
  }
});

test("ignores invalid cron and treats an invalid start as not future", () => {
  const invalidCron = "task @repeat(nonsense)\n";
  const invalidStart = "task @repeat(0 0 * * *) @start(2024-02-30)\n";
  assert.doesNotThrow(() => repeatTasks(invalidCron, noon));
  assert.doesNotThrow(() => repeatTasks(invalidStart, noon));
  assert.equal(repeatTasks(invalidCron, noon), invalidCron);
  assert.equal(
    repeatTasks(invalidStart, noon),
    "task @repeat(0 0 * * *) @start(2024-02-30)\ntask @start(2024-06-15)\n",
  );
});

test("requires exactly five cron fields", () => {
  const source = "task @repeat(0 0 0 * * *)\n";
  assert.equal(repeatTasks(source, noon), source);
});

test("formats the completed document and preserves its line ending", () => {
  assert.equal(
    repeatTasks("  A:\r\n      deep\r\nflat @repeat(0 0 * * *)\r\n", noon),
    "A:\r\n    deep\r\n\r\nflat @repeat(0 0 * * *)\r\nflat @start(2024-06-15)\r\n",
  );
});

async function runInTimezone(
  timezone: string,
  source: string,
  now: number,
): Promise<{ timeZone: string; output: string }> {
  const entry = new URL("../src/index.js", import.meta.url).href;
  const script = `
import { repeatTasks } from ${JSON.stringify(entry)};
const { source, now } = JSON.parse(process.env.PROBE);
const output = repeatTasks(source, new Date(now));
console.log(JSON.stringify({ timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone, output }));
`;
  const { stdout } = await runNode(process.execPath, ["--input-type=module", "--eval", script], {
    env: { ...process.env, TZ: timezone, PROBE: JSON.stringify({ source, now }) },
  });
  return JSON.parse(stdout);
}
