import type { Position, TextEdit } from "./lsp-types.js";

/** Convert a command result to edits whose ranges contain changed lines only. */
export function lineEdits(oldText: string, newText: string): TextEdit[] {
  const target = matchEol(oldText, newText);
  const oldLines = linesWithNewline(oldText);
  const newLines = linesWithNewline(target);
  const prefix = commonPrefix(oldLines, newLines);
  const suffix = commonSuffix(oldLines, newLines, prefix);
  const oldMiddle = oldLines.slice(prefix, oldLines.length - suffix);
  const newMiddle = newLines.slice(prefix, newLines.length - suffix);
  const matches = lcsMatches(oldMiddle, newMiddle, prefix);
  const edits: TextEdit[] = [];
  let oldCursor = prefix;
  let newCursor = prefix;

  for (const [oldMatch, newMatch] of [
    ...matches,
    [oldMiddle.length + prefix, newMiddle.length + prefix],
  ]) {
    const oldEnd = oldMatch;
    const newEnd = newMatch;
    if (oldEnd > oldCursor || newEnd > newCursor) {
      edits.push(makeRunEdit(oldLines, oldCursor, oldEnd, newLines, newCursor, newEnd));
    }
    oldCursor = oldMatch + 1;
    newCursor = newMatch + 1;
  }
  return edits;
}

/** Match the document's existing LF/CRLF convention in generated text. */
export function matchEol(model: string, text: string): string {
  const lfText = text.replace(/\r\n/g, "\n");
  return model.includes("\r\n") ? lfText.replace(/\n/g, "\r\n") : lfText;
}

function linesWithNewline(text: string): string[] {
  return text === "" ? [] : text.split(/(?<=\n)/);
}

function commonPrefix(oldLines: readonly string[], newLines: readonly string[]): number {
  let length = 0;
  while (
    length < oldLines.length &&
    length < newLines.length &&
    oldLines[length] === newLines[length]
  ) {
    length++;
  }
  return length;
}

function commonSuffix(
  oldLines: readonly string[],
  newLines: readonly string[],
  prefix: number,
): number {
  let length = 0;
  while (
    length < oldLines.length - prefix &&
    length < newLines.length - prefix &&
    oldLines[oldLines.length - 1 - length] === newLines[newLines.length - 1 - length]
  ) {
    length++;
  }
  return length;
}

function lcsMatches(
  oldLines: readonly string[],
  newLines: readonly string[],
  offset: number,
): Array<[number, number]> {
  const rows = oldLines.length;
  const columns = newLines.length;
  const table = Array.from({ length: rows + 1 }, () => new Uint32Array(columns + 1));
  for (let row = rows - 1; row >= 0; row--) {
    for (let column = columns - 1; column >= 0; column--) {
      table[row][column] =
        oldLines[row] === newLines[column]
          ? table[row + 1][column + 1] + 1
          : Math.max(table[row + 1][column], table[row][column + 1]);
    }
  }

  const matches: Array<[number, number]> = [];
  let row = 0;
  let column = 0;
  while (row < rows && column < columns) {
    if (oldLines[row] === newLines[column]) {
      matches.push([offset + row, offset + column]);
      row++;
      column++;
    } else if (table[row + 1][column] >= table[row][column + 1]) {
      row++;
    } else {
      column++;
    }
  }
  return matches;
}

function makeRunEdit(
  oldLines: readonly string[],
  oldStart: number,
  oldEnd: number,
  newLines: readonly string[],
  newStart: number,
  newEnd: number,
): TextEdit {
  if (newStart === newEnd) {
    const end =
      oldEnd < oldLines.length || hasTrailingNewline(oldLines)
        ? { line: oldEnd, character: 0 }
        : { line: oldEnd - 1, character: utf16Length(lineContent(oldLines[oldEnd - 1])) };
    return { range: { start: { line: oldStart, character: 0 }, end }, newText: "" };
  }

  if (oldStart === oldEnd) {
    if (oldLines.length === 0) {
      return {
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
        newText: newLines.slice(newStart, newEnd).join(""),
      };
    }
    if (oldEnd < oldLines.length || hasTrailingNewline(oldLines)) {
      return {
        range: { start: { line: oldEnd, character: 0 }, end: { line: oldEnd, character: 0 } },
        newText: newLines.slice(newStart, newEnd).join(""),
      };
    }
    const last = oldLines.length - 1;
    const position = { line: last, character: utf16Length(lineContent(oldLines[last])) };
    return {
      range: { start: position, end: position },
      newText: `${eolOf(newLines)}${newLines.slice(newStart, newEnd).join("")}`,
    };
  }

  const lastOld = oldEnd - 1;
  const lastNew = newEnd - 1;
  const eol = eolOf(newLines);
  const newLastLine = lineContent(newLines[lastNew]);
  const needsFinalNewline = newLines[lastNew].endsWith("\n") && !oldLines[lastOld].endsWith("\n");
  const end =
    oldLines[lastOld].endsWith("\n") && !newLines[lastNew].endsWith("\n")
      ? { line: lastOld + 1, character: 0 }
      : { line: lastOld, character: utf16Length(lineContent(oldLines[lastOld])) };
  return {
    range: {
      start: { line: oldStart, character: 0 },
      end,
    },
    newText: `${newLines.slice(newStart, lastNew).join("")}${newLastLine}${needsFinalNewline ? eol : ""}`,
  };
}

function eolOf(lines: readonly string[]): string {
  return lines.some((line) => line.endsWith("\r\n")) ? "\r\n" : "\n";
}

function hasTrailingNewline(lines: readonly string[]): boolean {
  return lines.at(-1)?.endsWith("\n") ?? false;
}

function lineContent(line: string): string {
  return line.endsWith("\n") ? line.slice(0, -1).replace(/\r$/, "") : line;
}

function utf16Length(text: string): number {
  return text.length;
}

export function endPosition(text: string): Position {
  const lines = text.split("\n");
  const line = lines.length - 1;
  const content = lines[line].replace(/\r$/, "");
  return { line, character: content.length };
}
