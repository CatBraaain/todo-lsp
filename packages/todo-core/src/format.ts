import { indentForLevel, isBlank, isHeading, normalizeBody, parseLine } from "./line.js";

/** Split a document into lines without line endings. */
export function splitLines(source: string): string[] {
  const lines = source.split("\n").map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line));
  if (lines.at(-1) === "") lines.pop();
  return lines;
}

/** Join LF-normalized lines using the document's existing line ending. */
export function joinLines(lines: readonly string[], original: string): string {
  const eol = original.includes("\r\n") ? "\r\n" : "\n";
  if (lines.length === 0) return "";
  return lines.join(eol) + (original.endsWith("\n") ? eol : "");
}

/** Format a document according to SPEC.md §フォーマット. */
export function formatDocument(source: string): string {
  const lines = splitLines(source);
  if (lines.every((line) => isBlank(parseLine(line)))) return "";

  const normalized = normalizeLines(lines);
  const collapsed = collapseBlankLines(normalized);
  const formatted = addHeadingBlockBlanks(collapsed);
  const eol = source.includes("\r\n") ? "\r\n" : "\n";
  return formatted.join(eol) + eol;
}

interface NormalizedLine {
  text: string;
  level: number;
  heading: boolean;
}

function normalizeLines(lines: readonly string[]): NormalizedLine[] {
  const parents: Array<{ units: number; level: number }> = [];
  const normalized: NormalizedLine[] = [];

  for (const line of lines) {
    const parts = parseLine(line);
    if (isBlank(parts)) {
      normalized.push({ text: "", level: 0, heading: false });
      continue;
    }

    while (parents.length > 0 && parents.at(-1)!.units >= parts.units) {
      parents.pop();
    }
    const level = parents.at(-1)?.level ?? -1;
    const normalizedLevel = level + 1;
    parents.push({ units: parts.units, level: normalizedLevel });
    normalized.push({
      text: `${indentForLevel(normalizedLevel)}${normalizeBody(parts, line)}`,
      level: normalizedLevel,
      heading: isHeading(parts),
    });
  }
  return normalized;
}

function collapseBlankLines(lines: readonly NormalizedLine[]): NormalizedLine[] {
  const collapsed: NormalizedLine[] = [];
  for (const line of lines) {
    if (line.text === "") {
      if (collapsed.at(-1)?.text === "" || collapsed.length === 0) continue;
    }
    collapsed.push(line);
  }
  while (collapsed.at(-1)?.text === "") collapsed.pop();
  return collapsed;
}

function addHeadingBlockBlanks(lines: readonly NormalizedLine[]): string[] {
  const output: string[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];
    if (line.level !== 0 || !line.heading) {
      output.push(line.text);
      index++;
      continue;
    }

    if (output.at(-1) !== undefined && output.at(-1) !== "") output.push("");
    do {
      output.push(lines[index].text);
      index++;
    } while (index < lines.length && lines[index].level !== 0);
    if (index < lines.length && lines[index].text !== "") output.push("");
  }
  return output;
}
