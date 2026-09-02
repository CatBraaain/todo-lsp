// §リピート 自動リピートのスケジュール純粋ロジック (src/autoRepeatCore.mjs).
import test from "node:test";
import assert from "node:assert/strict";
import { EDITOR_SWITCH_DELAY_MS, msToNextMinute, shouldAutoRepeat } from "../src/autoRepeatCore.mjs";

test("自動リピート: fires only when the setting is on and a todo editor is active", () => {
  assert.equal(shouldAutoRepeat(true, "todo"), true);
  assert.equal(shouldAutoRepeat(false, "todo"), false);
  assert.equal(shouldAutoRepeat(true, "markdown"), false);
  assert.equal(shouldAutoRepeat(undefined, "todo"), false);
});

test("自動リピート: editor switch waits about 0.5s", () => {
  assert.equal(EDITOR_SWITCH_DELAY_MS, 500);
});

test("自動リピート: the next run is scheduled at second 0 of each minute", () => {
  assert.equal(msToNextMinute(new Date(2024, 5, 15, 12, 0, 0, 0)), 60000);
  assert.equal(msToNextMinute(new Date(2024, 5, 15, 12, 0, 30, 0)), 30000);
  assert.equal(msToNextMinute(new Date(2024, 5, 15, 12, 0, 59, 999)), 1);
});
