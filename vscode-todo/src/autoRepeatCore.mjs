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
