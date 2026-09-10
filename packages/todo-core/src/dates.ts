import { Cron } from "croner";

// Date and cron classification for `@start` / `@due` / `@repeat` tags
// (SPEC 期限タグ / 繰り返しタグ). All parsing is UTC.

/** Parse a `@start`/`@due` argument and classify it relative to `now`.
 *
 * Accepted formats (first match wins, all parsed as UTC): `YYYY-MM-DD HH:mm`
 * then `YYYY-MM-DD`. `<= now` is `past`, `> now` is `future`, and anything
 * that fails to parse is `invalid`. `now` is a parameter so tests can
 * inject a fixed instant. */
export function classifyDate(arg: string, now: Date): "past" | "future" | "invalid" {
  const trimmed = arg.trim();
  const parsed = parseDateTime(trimmed) ?? parseDateOnly(trimmed);
  if (parsed === undefined) return "invalid";
  return parsed.getTime() <= now.getTime() ? "past" : "future";
}

function parseDateTime(arg: string): Date | undefined {
  const match = /^(\d{4})-(\d{2})-(\d{2}) (\d{2}):(\d{2})$/.exec(arg);
  if (!match) return undefined;
  return makeUtcDate(match, Number(match[4]), Number(match[5]));
}

function parseDateOnly(arg: string): Date | undefined {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(arg);
  if (!match) return undefined;
  return makeUtcDate(match, 0, 0);
}

function makeUtcDate(match: RegExpExecArray, hours: number, minutes: number): Date | undefined {
  const date = new Date(
    Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3]), hours, minutes),
  );
  // Reject rollovers like 2024-02-31 (the components must survive).
  if (
    date.getUTCFullYear() !== Number(match[1]) ||
    date.getUTCMonth() !== Number(match[2]) - 1 ||
    date.getUTCDate() !== Number(match[3])
  ) {
    return undefined;
  }
  return date;
}

/** Parse a supported five-field cron expression. */
export function cronFor(arg: string): Cron | undefined {
  try {
    return new Cron(arg.trim(), { mode: "5-part" });
  } catch {
    return undefined;
  }
}

/** Whether `arg` is a valid five-field cron expression. */
export function isValidCron(arg: string): boolean {
  return cronFor(arg) !== undefined;
}
