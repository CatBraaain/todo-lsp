// Pure scheduling logic for 自動リピート (§リピート), free of vscode imports
// so it can be exercised directly by `node --test`. extension.ts wires these
// into timers and editor events.

// アクティブエディタの切替から約 0.5 秒後.
export const EDITOR_SWITCH_DELAY_MS = 500;

/**
 * Whether an auto-repeat run should fire for the current state: the setting
 * `todo-language.repeatTask.autoRepeat` must be true and the active editor
 * must be a todo document.
 */
export function shouldAutoRepeat(autoRepeatEnabled, activeLanguageId) {
  return autoRepeatEnabled === true && activeLanguageId === "todo";
}

/**
 * Milliseconds from `now` until the next minute boundary (second 0) — the
 * 毎分 0 秒 schedule.
 */
export function msToNextMinute(now) {
  return (60 - now.getSeconds()) * 1000 - now.getMilliseconds();
}

/**
 * 自動リピート (§リピート): wires the three trigger timings — 拡張の起動時,
 * アクティブエディタの切替から約 0.5 秒後, 毎分 0 秒 — onto environment
 * callbacks, so `node --test` can drive them with fakes instead of a VS Code
 * harness. Every trigger runs one Repeat Tasks attempt. A rejected attempt is
 * swallowed and retried on the next trigger.
 *
 * `env`: onStartup(cb), onEditorSwitch(cb), now(), setTimeout(cb, ms),
 * clearTimeout(timer), fire()
 */
export function setupAutoRepeatTriggers(env) {
  let disposed = false;
  let switchTimer;
  let minuteTimer;

  const fire = () => {
    try {
      void Promise.resolve(env.fire()).catch(() => {});
    } catch {
      // A synchronous failure retries on the next trigger, like a rejected
      // client request.
    }
  };

  // 拡張の起動時.
  env.onStartup(() => {
    if (!disposed) {
      fire();
    }
  });

  // アクティブエディタの切替から約 0.5 秒後 (a rapid switch cancels the
  // pending wait).
  env.onEditorSwitch(() => {
    if (disposed) {
      return;
    }
    if (switchTimer !== undefined) {
      env.clearTimeout(switchTimer);
    }
    switchTimer = env.setTimeout(fire, EDITOR_SWITCH_DELAY_MS);
  });

  // 毎分 0 秒.
  const scheduleMinute = () => {
    minuteTimer = env.setTimeout(() => {
      if (disposed) {
        return;
      }
      fire();
      scheduleMinute();
    }, msToNextMinute(env.now()));
  };
  scheduleMinute();

  return {
    dispose() {
      disposed = true;
      if (switchTimer !== undefined) {
        env.clearTimeout(switchTimer);
      }
      if (minuteTimer !== undefined) {
        env.clearTimeout(minuteTimer);
      }
    },
  };
}
