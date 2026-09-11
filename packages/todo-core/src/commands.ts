import { formatDocument, joinLines, splitLines } from "./format.js";
import { grayOf, indentForLevel, isBlank, parseLine, render, type Tag } from "./line.js";

/** Tag actions supported by the document-editing commands. */
export type Toggle =
  | "done"
  | "cancelled"
  | "start"
  | "due"
  | "queue"
  | "queueUnshift"
  | "waiting"
  | "pending"
  | "hide"
  | "repeat";

/** Named action values for callers that prefer enum-like constants. */
export const Toggle = {
  Done: "done",
  Cancelled: "cancelled",
  Start: "start",
  Due: "due",
  Queue: "queue",
  QueueUnshift: "queueUnshift",
  Waiting: "waiting",
  Pending: "pending",
  Hide: "hide",
  Repeat: "repeat",
} as const satisfies Record<string, Toggle>;

/** Toggle a tag on the selected non-blank lines. */
export function toggle(
  source: string,
  selection: readonly number[],
  action: Toggle,
  today: Date,
  tabSize = 4,
): string {
  const lines = splitLines(source);
  const targets = structureLines(lines, selection);
  if (targets.length === 0) return source;

  const name = action === "queueUnshift" ? "queue" : action;
  const allHave = targets.every((index) => hasTag(lines[index], name));
  const queueNumber = nextQueueNumber(lines);

  for (const index of targets) {
    if (allHave) {
      lines[index] = retag(
        lines[index],
        (leading, trailing) => {
          removeTags(leading, name);
          removeTags(trailing, name);
        },
        tabSize,
      );
      continue;
    }
    if (hasTag(lines[index], name)) continue;

    const removed =
      action === "done" || action === "cancelled"
        ? ["done", "cancelled", "queue", "waiting", "pending"]
        : [];
    const tag = newTag(tagForAction(action, today, queueNumber));
    lines[index] = retag(
      lines[index],
      (leading, trailing) => {
        removeTagsByName(leading, removed);
        removeTagsByName(trailing, removed);
        trailing.push(tag);
      },
      tabSize,
    );
  }

  if (action === "queue" || action === "queueUnshift") renumberQueues(lines, tabSize);
  return formatDocument(joinLines(lines, source), tabSize);
}

/** Indent or dedent selected non-blank lines by one or more levels. */
export function reindent(
  source: string,
  selection: readonly number[],
  delta: number,
  tabSize = 4,
): string {
  const lines = splitLines(source);
  for (const index of structureLines(lines, selection)) {
    const line = lines[index];
    const parts = parseLine(line, tabSize);
    const level = Math.max(0, parts.level + delta);
    lines[index] = `${indentForLevel(level, tabSize)}${line.slice(parts.indentLen)}`;
  }
  return joinLines(lines, source);
}

function tagForAction(action: Toggle, today: Date, queueNumber: number): string {
  switch (action) {
    case "done":
    case "cancelled":
    case "start":
    case "due":
      return `@${action}(${dateText(today)})`;
    case "queue":
      return `@queue(${queueNumber})`;
    case "queueUnshift":
      return "@queue(0)";
    case "waiting":
    case "pending":
    case "hide":
      return `@${action}`;
    case "repeat":
      return "@repeat(0 0 * * *)";
  }
}

function dateText(date: Date): string {
  const year = date.getFullYear().toString().padStart(4, "0");
  const month = (date.getMonth() + 1).toString().padStart(2, "0");
  const day = date.getDate().toString().padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function structureLines(lines: readonly string[], selection: readonly number[]): number[] {
  return [...new Set(selection.filter((index) => index >= 0 && index < lines.length))]
    .filter((index) => !isBlank(parseLine(lines[index])))
    .sort((a, b) => a - b);
}

function hasTag(line: string, name: string): boolean {
  const parts = parseLine(line);
  return [...parts.leadingTags, ...parts.tags].some((tag) => tag.name === name);
}

function nextQueueNumber(lines: readonly string[]): number {
  return (
    lines.filter((line) => {
      const parts = parseLine(line);
      return !isBlank(parts) && hasTag(line, "queue") && grayOf(parts) === undefined;
    }).length + 1
  );
}

function renumberQueues(lines: string[], tabSize: number): void {
  const numbers = [
    ...new Set(
      lines.flatMap((line) => {
        const parts = parseLine(line);
        return [...parts.leadingTags, ...parts.tags]
          .filter((tag) => tag.name === "queue" && tag.arg !== undefined && /^\d+$/.test(tag.arg))
          .map((tag) => Number(tag.arg));
      }),
    ),
  ].sort((a, b) => a - b);

  for (let index = 0; index < lines.length; index++) {
    if (!hasTag(lines[index], "queue")) continue;
    lines[index] = retag(
      lines[index],
      (leading, trailing) => {
        renumberTagColumn(leading, numbers);
        renumberTagColumn(trailing, numbers);
      },
      tabSize,
    );
  }
}

function renumberTagColumn(tags: Tag[], numbers: readonly number[]): void {
  for (const tag of tags) {
    if (tag.name !== "queue" || tag.arg === undefined || !/^\d+$/.test(tag.arg)) continue;
    tag.arg = String(numbers.indexOf(Number(tag.arg)) + 1);
  }
}

function retag(
  line: string,
  edit: (leading: Tag[], trailing: Tag[]) => void,
  tabSize: number,
): string {
  const parts = parseLine(line, tabSize);
  const leading = parts.leadingTags.map((tag) => ({ ...tag }));
  const trailing = parts.tags.map((tag) => ({ ...tag }));
  edit(leading, trailing);
  return `${indentForLevel(parts.level, tabSize)}${render(parts, line, leading, trailing)}`;
}

function removeTags(tags: Tag[], name: string): void {
  removeTagsByName(tags, [name]);
}

function removeTagsByName(tags: Tag[], names: readonly string[]): void {
  tags.splice(0, tags.length, ...tags.filter((tag) => !names.includes(tag.name)));
}

function newTag(text: string): Tag {
  const open = text.indexOf("(");
  return {
    name: open === -1 ? text.slice(1) : text.slice(1, open),
    arg: open === -1 ? undefined : text.slice(open + 1, -1),
    start: 0,
    end: 0,
  };
}
