// §リピート 自動リピートのスケジュール・トリガー純粋ロジック (src/autoRepeatCore.mjs).
import test from "node:test";
import assert from "node:assert/strict";
import {
  EDITOR_SWITCH_DELAY_MS,
  msToNextMinute,
  setupAutoRepeatTriggers,
  shouldAutoRepeat,
} from "../src/autoRepeatCore.mjs";

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

// §リピート 自動リピートのトリガー結線: a fake environment capturing the
// callbacks and timers so the three trigger timings are driven directly.
function fakeEnv() {
  const timers = [];
  const cleared = [];
  const fires = [];
  const env = {
    startupCallbacks: [],
    switchCallbacks: [],
    now: () => new Date(2024, 5, 15, 12, 0, 30, 0),
    onStartup: (cb) => env.startupCallbacks.push(cb),
    onEditorSwitch: (cb) => env.switchCallbacks.push(cb),
    setTimeout: (cb, ms) => {
      const timer = { cb, ms };
      timers.push(timer);
      return timer;
    },
    clearTimeout: (timer) => {
      cleared.push(timer);
      const index = timers.indexOf(timer);
      if (index >= 0) {
        timers.splice(index, 1);
      }
    },
    fire: () => {
      fires.push(env.now());
      return Promise.resolve();
    },
  };
  return { env, timers, cleared, fires };
}

test("自動リピート: startup fires immediately and schedules the minute tick", async () => {
  const { env, timers, fires } = fakeEnv();
  setupAutoRepeatTriggers(env);

  // 拡張の起動時.
  env.startupCallbacks[0]();
  assert.equal(fires.length, 1);
  await new Promise((resolve) => setImmediate(resolve));

  // 毎分 0 秒: initial schedule from now (:30 -> 30s) fires and reschedules.
  assert.equal(timers[0].ms, 30000);
  timers[0].cb();
  assert.equal(fires.length, 2);
  assert.equal(timers[1].ms, 30000, "minute tick reschedules at second 0");
});

test("自動リピート: editor switch fires after the switch delay and cancels a pending wait", () => {
  const { env, timers, cleared, fires } = fakeEnv();
  setupAutoRepeatTriggers(env);

  // アクティブエディタの切替から約 0.5 秒後.
  env.switchCallbacks[0]();
  const switchTimer = timers.at(-1);
  assert.equal(switchTimer.ms, EDITOR_SWITCH_DELAY_MS);

  // A rapid second switch cancels the pending wait and re-arms it.
  env.switchCallbacks[0]();
  assert.deepEqual(cleared, [switchTimer]);
  assert.equal(timers.at(-1).ms, EDITOR_SWITCH_DELAY_MS);

  timers.at(-1).cb();
  assert.equal(fires.length, 1);
});

test("自動リピート: a pending run does not suppress another trigger", () => {
  const { env, timers } = fakeEnv();
  let runs = 0;
  env.fire = () => {
    runs += 1;
    return new Promise(() => {});
  };

  setupAutoRepeatTriggers(env);

  env.startupCallbacks[0]();
  assert.equal(runs, 1);

  // Every trigger runs Repeat Tasks even while a prior request is pending.
  env.switchCallbacks[0]();
  timers.at(-1).cb();
  assert.equal(runs, 2);
});

test("自動リピート: a rejected run is swallowed and the next trigger retries", async () => {
  const { env } = fakeEnv();
  let attempts = 0;
  env.fire = () => {
    attempts += 1;
    return attempts === 1 ? Promise.reject(new Error("server down")) : Promise.resolve();
  };

  setupAutoRepeatTriggers(env);
  env.startupCallbacks[0]();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(attempts, 1);

  env.startupCallbacks[0]();
  await new Promise((resolve) => setImmediate(resolve));
  assert.equal(attempts, 2, "next trigger retries after a failure");
});

test("自動リピート: dispose cancels pending timers and stops firing", () => {
  const { env, timers, cleared, fires } = fakeEnv();
  const triggers = setupAutoRepeatTriggers(env);

  env.switchCallbacks[0]();
  triggers.dispose();
  assert.equal(cleared.length, 2, "switch + minute timers cleared");
  assert.equal(timers.length, 0);

  env.startupCallbacks[0]();
  env.switchCallbacks[0]();
  assert.equal(fires.length, 0);
});
