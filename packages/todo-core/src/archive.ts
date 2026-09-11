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
  units: number;
  /** Index into the entry list of the 親行 — the nearest earlier non-blank
   * row of shallower indent. Undefined for ルート直下 rows (SPEC §構文要素). */
  parent: number | undefined;
}

/** Archive selected all-gray top-level blocks. */
export function archive(source: string, selection: readonly number[], tabSize = 4): string {
  const lines = splitLines(source);
  if (lines.length === 0) return source;

  const blocks = topLevelBlocks(lines, tabSize);
  const archiveLine = rootArchiveLine(lines, tabSize);
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

  const movedLines = moved.map((line) => shiftLine(lines, line, 1, tabSize));
  const output: string[] = [];
  if (archiveLine === undefined) {
    for (let line = 0; line < lines.length; line++) {
      if (!moved.includes(line)) output.push(lines[line]);
    }
    output.push("Archive:", ...movedLines);
  } else {
    const insertAfter = blockEnd(lines, archiveLine, tabSize);
    for (let line = 0; line < lines.length; line++) {
      if (moved.includes(line)) continue;
      output.push(lines[line]);
      if (line === insertAfter) output.push(...movedLines);
    }
  }
  return formatDocument(joinLines(output, source), tabSize);
}

/** Unarchive selected all-gray direct children of the root Archive heading. */
export function unarchive(source: string, selection: readonly number[], tabSize = 4): string {
  const lines = splitLines(source);
  const archiveLine = rootArchiveLine(lines, tabSize);
  if (archiveLine === undefined) return source;

  const end = blockEnd(lines, archiveLine, tabSize);
  const blocks = directChildBlocks(lines, archiveLine, end, tabSize);
  const selected = new Set(selection);
  const movedBlocks = blocks.filter(
    (block) => blockIsAllGray(lines, block) && block.some((line) => selected.has(line)),
  );
  const moved = movedBlocks.flat();
  if (moved.length === 0) return source;

  const output = lines.filter((_, line) => !moved.includes(line));
  for (const block of movedBlocks) {
    const rootDepth = relativeDepth(lines, block[0], tabSize);
    output.push(...block.map((line) => shiftLine(lines, line, -rootDepth, tabSize)));
  }
  return formatDocument(joinLines(output, source), tabSize);
}

/** トップレベルブロック: blocks headed by ルート直下 rows — rows with no
 * earlier non-blank row of shallower indent. */
function topLevelBlocks(lines: readonly string[], tabSize: number): number[][] {
  const blocks: number[][] = [];
  for (const entry of entries(lines, tabSize)) {
    if (entry.parent === undefined) blocks.push([]);
    blocks.at(-1)?.push(entry.line);
  }
  return blocks;
}

function directChildBlocks(
  lines: readonly string[],
  head: number,
  end: number,
  tabSize: number,
): number[][] {
  const structure = entries(lines, tabSize);
  const headIndex = structure.findIndex((entry) => entry.line === head);
  const blocks: number[][] = [];
  for (const entry of structure) {
    if (entry.line <= head || entry.line > end) continue;
    if (entry.parent === headIndex) blocks.push([]);
    blocks.at(-1)?.push(entry.line);
  }
  return blocks;
}

/** Non-blank rows with their indent units and parent index. */
function entries(lines: readonly string[], tabSize: number): Entry[] {
  const out: Entry[] = [];
  for (const [line, text] of lines.entries()) {
    const parts = parseLine(text, tabSize);
    if (isBlank(parts)) continue;
    let parent: number | undefined;
    for (let index = out.length - 1; index >= 0; index--) {
      if (out[index].units < parts.units) {
        parent = index;
        break;
      }
    }
    out.push({ line, units: parts.units, parent });
  }
  return out;
}

function blockIsAllGray(lines: readonly string[], block: readonly number[]): boolean {
  return block.length > 0 && block.every((line) => grayOf(parseLine(lines[line])) !== undefined);
}

/** The first `Archive:` 見出し行 at ルート直下 (an earlier row of shallower
 * indent would make it a child, not the root archive). */
function rootArchiveLine(lines: readonly string[], tabSize: number): number | undefined {
  for (const entry of entries(lines, tabSize)) {
    if (entry.parent !== undefined) continue;
    const line = lines[entry.line];
    if (isArchiveHeading(parseLine(line), line)) return entry.line;
  }
  return undefined;
}

/** The last row of `head`'s block: `head` plus its following deeper rows. */
function blockEnd(lines: readonly string[], head: number, tabSize: number): number {
  const structure = entries(lines, tabSize);
  const headUnits = structure.find((entry) => entry.line === head)?.units;
  let end = head;
  if (headUnits === undefined) return end;
  for (const entry of structure) {
    if (entry.line <= head) continue;
    if (entry.units > headUnits) end = entry.line;
    else break;
  }
  return end;
}

/** The depth of `target` in its parent chain (ルート直下 = 0). */
function relativeDepth(lines: readonly string[], target: number, tabSize: number): number {
  const structure = entries(lines, tabSize);
  let index = structure.findIndex((entry) => entry.line === target);
  let depth = 0;
  while (index >= 0 && structure[index].parent !== undefined) {
    index = structure[index].parent!;
    depth++;
  }
  return depth;
}

function shiftLine(
  lines: readonly string[],
  index: number,
  delta: number,
  tabSize: number,
): string {
  const line = lines[index];
  const parts = parseLine(line);
  const level = Math.max(0, relativeDepth(lines, index, tabSize) + delta);
  return `${indentForLevel(level, tabSize)}${normalizeBody(parts, line)}`;
}
