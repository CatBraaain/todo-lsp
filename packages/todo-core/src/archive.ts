import { formatDocument, joinLines, splitLines } from "./format.js";
import {
  grayOf,
  indentForLevel,
  isArchiveHeading,
  isBlank,
  normalizeBody,
  parseLine,
} from "./line.js";

interface Entry {
  line: number;
  level: number;
}

/** Archive selected all-gray top-level blocks. */
export function archive(source: string, selection: readonly number[]): string {
  const lines = splitLines(source);
  if (lines.length === 0) return source;

  const blocks = topLevelBlocks(lines);
  const archiveLine = rootArchiveLine(lines);
  const selected = new Set(selection);
  const moved = blocks
    .filter(
      (block) =>
        block.length > 0 &&
        !block.includes(archiveLine ?? -1) &&
        blockIsAllGray(lines, block) &&
        block.some((line) => selected.has(line)),
    )
    .flat();
  if (moved.length === 0) return source;

  const movedLines = moved.map((line) => shiftLine(lines, line, 1));
  const output: string[] = [];
  if (archiveLine === undefined) {
    for (let line = 0; line < lines.length; line++) {
      if (!moved.includes(line)) output.push(lines[line]);
    }
    output.push("Archive:", ...movedLines);
  } else {
    const insertAfter = blockEnd(lines, archiveLine);
    for (let line = 0; line < lines.length; line++) {
      if (moved.includes(line)) continue;
      output.push(lines[line]);
      if (line === insertAfter) output.push(...movedLines);
    }
  }
  return formatDocument(joinLines(output, source));
}

/** Unarchive selected all-gray direct children of the root Archive heading. */
export function unarchive(source: string, selection: readonly number[]): string {
  const lines = splitLines(source);
  const archiveLine = rootArchiveLine(lines);
  if (archiveLine === undefined) return source;

  const end = blockEnd(lines, archiveLine);
  const blocks = directChildBlocks(lines, archiveLine, end);
  const selected = new Set(selection);
  const movedBlocks = blocks.filter(
    (block) => blockIsAllGray(lines, block) && block.some((line) => selected.has(line)),
  );
  const moved = movedBlocks.flat();
  if (moved.length === 0) return source;

  const output = lines.filter((_, line) => !moved.includes(line));
  for (const block of movedBlocks) {
    const rootDepth = relativeDepth(lines, block[0]);
    output.push(...block.map((line) => shiftLine(lines, line, -rootDepth)));
  }
  return formatDocument(joinLines(output, source));
}

function topLevelBlocks(lines: readonly string[]): number[][] {
  const blocks: number[][] = [];
  for (const entry of entries(lines)) {
    if (entry.level === 0) blocks.push([]);
    blocks.at(-1)?.push(entry.line);
  }
  return blocks;
}

function directChildBlocks(lines: readonly string[], head: number, end: number): number[][] {
  const blocks: number[][] = [];
  for (const entry of entries(lines)) {
    if (entry.line <= head || entry.line > end) continue;
    if (entry.level === 1) blocks.push([]);
    blocks.at(-1)?.push(entry.line);
  }
  return blocks;
}

function entries(lines: readonly string[]): Entry[] {
  return lines.flatMap((line, index) => {
    const parts = parseLine(line);
    return isBlank(parts) ? [] : [{ line: index, level: parts.level }];
  });
}

function blockIsAllGray(lines: readonly string[], block: readonly number[]): boolean {
  return block.length > 0 && block.every((line) => grayOf(parseLine(lines[line])) !== undefined);
}

function rootArchiveLine(lines: readonly string[]): number | undefined {
  const index = lines.findIndex((line) => {
    const parts = parseLine(line);
    return parts.level === 0 && isArchiveHeading(parts, line);
  });
  return index === -1 ? undefined : index;
}

function blockEnd(lines: readonly string[], head: number): number {
  const headLevel = parseLine(lines[head]).level;
  let end = head;
  for (const entry of entries(lines)) {
    if (entry.line <= head) continue;
    if (entry.level > headLevel) end = entry.line;
    else break;
  }
  return end;
}

function relativeDepth(lines: readonly string[], target: number): number {
  const targetLevel = parseLine(lines[target]).level;
  let currentLevel = targetLevel;
  let depth = 0;
  for (const entry of entries(lines).reverse()) {
    if (entry.line >= target || entry.level >= currentLevel) continue;
    depth++;
    currentLevel = entry.level;
  }
  return depth;
}

function shiftLine(lines: readonly string[], index: number, delta: number): string {
  const line = lines[index];
  const parts = parseLine(line);
  const level = Math.max(0, relativeDepth(lines, index) + delta);
  return `${indentForLevel(level)}${normalizeBody(parts, line)}`;
}
