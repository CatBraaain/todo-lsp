// Semantic tokens: a line-based scanner over the raw document. Each line is
// classified via `./line.js` (SPEC.md's 用語 model) by the 適用規則
// precedence (archive -> done -> cancelled -> hide -> heading -> plain) and
// emits the corresponding token types. `@start`/`@due`/`@repeat` tags
// additionally carry past/future/valid/invalid modifiers computed from the
// argument (date parse / cron validation vs `now`). Positions are UTF-16
// code units (SPEC「LSP の位置」).

import { classifyDate, isValidCron } from "./dates.js";
import { colonOf, grayOf, isArchiveHeading, isBlank, parseLine, type Tag } from "./line.js";
import type { SemanticToken, SemanticTokensLegend } from "./lsp-types.js";

/** Token type indices. MUST stay in sync with `semanticTokensLegend`. */
const tt = {
  TODO_LINE: 0,
  TODO_HEADING_CONTENT: 1,
  TODO_HEADING_SYMBOL: 2,
  TODO_TAG: 3,
  START_TAG: 4,
  DUE_TAG: 5,
  REPEAT_TAG: 6,
  TODO_BOLD: 7,
  TODO_ITALIC: 8,
  TODO_CODE: 9,
  TODO_URL: 10,
} as const;

/** Token modifier bits. MUST stay in sync with `semanticTokensLegend`. */
const tm = {
  ITALIC: 1 << 0,
  QUEUE1: 1 << 1,
  PAST: 1 << 2,
  FUTURE: 1 << 3,
  INVALID: 1 << 4,
  VALID: 1 << 5,
} as const;

/** The semantic-token legend advertised to clients. The index of each type /
 * modifier MUST stay in sync with the `tt` / `tm` constants. */
export function semanticTokensLegend(): SemanticTokensLegend {
  return {
    tokenTypes: [
      "todo-line",
      "todo-heading-content",
      "todo-heading-symbol",
      "todo-tag",
      "start-tag",
      "due-tag",
      "repeat-tag",
      "todo-bold",
      "todo-italic",
      "todo-code",
      "todo-url",
    ],
    tokenModifiers: ["italic", "queue1", "past", "future", "invalid", "valid"],
  };
}

interface RawToken {
  line: number;
  /** UTF-16 code-unit column from the line start. */
  start: number;
  /** Length in UTF-16 code units. */
  length: number;
  type: number;
  modifiers: number;
}

/** Build the document's semantic tokens by scanning `source` line by line
 * and delta-encoding the result per the LSP spec. */
export function semanticTokens(source: string): SemanticToken[] {
  return semanticTokensAt(source, new Date());
}

/** Same as `semanticTokens` but with an injectable `now`, so the
 * past/future boundary for `@start`/`@due` is deterministic in tests. */
export function semanticTokensAt(source: string, now: Date): SemanticToken[] {
  const raw: RawToken[] = [];
  source.split("\n").forEach((rawLine, lineIdx) => {
    // Exclude a trailing CR so token spans stay within the line content.
    const line = rawLine.endsWith("\r") ? rawLine.slice(0, -1) : rawLine;
    classifyLine(line, lineIdx, now, raw);
  });

  // Stable sort by (line, start): ties keep insertion order.
  raw.sort((a, b) => a.line - b.line || a.start - b.start);

  // Delta-encode per LSP spec: deltaStart is relative to the previous
  // token's start on the same row, absolute on a new row.
  const tokens: SemanticToken[] = [];
  let prevLine = 0;
  let prevStart = 0;
  for (const { line, start, length, type, modifiers } of raw) {
    const deltaLine = line - prevLine;
    const deltaStart = deltaLine === 0 ? start - prevStart : start;
    tokens.push({ deltaLine, deltaStart, length, tokenType: type, tokenModifiers: modifiers });
    prevLine = line;
    prevStart = start;
  }
  return tokens;
}

/** Classify a single line (no trailing newline) and push its tokens. */
function classifyLine(line: string, lineIdx: number, now: Date, raw: RawToken[]): void {
  const parts = parseLine(line);
  if (isBlank(parts)) return;
  // 適用規則 1: `Archive:` 見出し行 — whole line gray.
  if (isArchiveHeading(parts, line)) {
    raw.push({ line: lineIdx, start: 0, length: line.length, type: tt.TODO_LINE, modifiers: 0 });
    return;
  }
  // 適用規則 2-4: done / cancelled / hide — the whole line is one grayed
  // token; inner tags and stylings are suppressed. cancelled is italic.
  const gray = grayOf(parts);
  if (gray === "done" || gray === "hide") {
    raw.push({ line: lineIdx, start: 0, length: line.length, type: tt.TODO_LINE, modifiers: 0 });
    return;
  }
  if (gray === "cancelled") {
    raw.push({
      line: lineIdx,
      start: 0,
      length: line.length,
      type: tt.TODO_LINE,
      modifiers: tm.ITALIC,
    });
    return;
  }
  // 適用規則 5: 見出し行 — content + symbol + the tag column; no stylings.
  // SPEC 表示: the `:` is bold and the trailing tag column is shown even
  // when the body is empty (SPEC 見出し: `:` で終わる行は本文が空でも見出し).
  const colon = colonOf(parts);
  if (colon !== undefined) {
    const [textStart, textEnd] = parts.textRange;
    for (const tag of parts.leadingTags) pushTagToken(tag, lineIdx, now, raw);
    if (textEnd > textStart) {
      raw.push({
        line: lineIdx,
        start: textStart,
        length: textEnd - textStart,
        type: tt.TODO_HEADING_CONTENT,
        modifiers: 0,
      });
    }
    raw.push({
      line: lineIdx,
      start: colon,
      length: 1,
      type: tt.TODO_HEADING_SYMBOL,
      modifiers: 0,
    });
    for (const tag of parts.tags) pushTagToken(tag, lineIdx, now, raw);
    return;
  }
  // 適用規則 6: 通常行 — both tag columns, plus stylings over the body
  // text. Tokens stay ordered by position: leading tags, body stylings,
  // trailing tags.
  for (const tag of parts.leadingTags) pushTagToken(tag, lineIdx, now, raw);
  const [textStart, textEnd] = parts.textRange;
  scanStyles(line.slice(textStart, textEnd), textStart, lineIdx, raw);
  for (const tag of parts.tags) pushTagToken(tag, lineIdx, now, raw);
}

/** Push one token for a tag-column `Tag`, with the date/cron modifiers for
 * `@start` / `@due` / `@repeat` and the yellow tier for `@queue(1)`. */
function pushTagToken(tag: Tag, lineIdx: number, now: Date, raw: RawToken[]): void {
  const arg = tag.arg ?? "";
  let type: number;
  let modifiers: number;
  switch (tag.name) {
    case "start":
      type = tt.START_TAG;
      modifiers = dateModifiers(classifyDate(arg, now));
      break;
    case "due":
      type = tt.DUE_TAG;
      modifiers = dateModifiers(classifyDate(arg, now));
      break;
    case "repeat":
      type = tt.REPEAT_TAG;
      modifiers = isValidCron(arg) ? tm.VALID : tm.INVALID;
      break;
    case "queue":
      type = tt.TODO_TAG;
      modifiers = arg === "1" ? tm.QUEUE1 : 0;
      break;
    default:
      type = tt.TODO_TAG;
      modifiers = 0;
  }
  raw.push({
    line: lineIdx,
    start: tag.start,
    length: tag.end - tag.start,
    type,
    modifiers,
  });
}

function dateModifiers(date: "past" | "future" | "invalid"): number {
  switch (date) {
    case "past":
      return tm.PAST;
    case "future":
      return tm.FUTURE;
    case "invalid":
      return tm.INVALID;
  }
}

/** Scan `text` for inline stylings (`**bold**`, `*italic*`, `` `code` ``,
 * `<url>`) and push one token per span. Bold is tried before italic at each
 * position to mirror the grammar's `#stylings` include order. Offsets are
 * byte-based within `text` but converted to UTF-16 columns via
 * `utf16ColFromByte`. */
function scanStyles(text: string, offset: number, lineIdx: number, raw: RawToken[]): void {
  const bytes = Buffer.from(text, "utf8");
  // Map each byte index inside `text` to the UTF-16 column relative to it.
  const utf16OfByte = (byteIndex: number): number =>
    utf16LengthOfBytes(bytes.subarray(0, byteIndex));

  let i = 0;
  while (i < bytes.length) {
    switch (bytes[i]) {
      case 0x2a /* * */: {
        if (i + 1 < bytes.length && bytes[i + 1] === 0x2a) {
          // bold: content is [^*]* then a closing `**`.
          let j = i + 2;
          while (j < bytes.length && bytes[j] !== 0x2a) j++;
          if (j + 1 < bytes.length && bytes[j + 1] === 0x2a) {
            const end = j + 2;
            pushStyleToken(utf16OfByte(i), utf16OfByte(end), tt.TODO_BOLD);
            i = end;
          } else {
            i += 2;
          }
        } else {
          // italic: content is [^*]* then a closing `*`.
          let j = i + 1;
          while (j < bytes.length && bytes[j] !== 0x2a) j++;
          if (j < bytes.length) {
            const end = j + 1;
            pushStyleToken(utf16OfByte(i), utf16OfByte(end), tt.TODO_ITALIC);
            i = end;
          } else {
            i += 1;
          }
        }
        break;
      }
      case 0x60 /* ` */: {
        // code: a run of n backticks, content, then a run of exactly n.
        let n = 0;
        while (i + n < bytes.length && bytes[i + n] === 0x60) n++;
        let k = i + n;
        let close: number | undefined;
        while (k + n <= bytes.length) {
          const runIsBackticks = bytes.subarray(k, k + n).every((b) => b === 0x60);
          const leftOk = k === 0 || bytes[k - 1] !== 0x60;
          const rightOk = k + n >= bytes.length || bytes[k + n] !== 0x60;
          if (runIsBackticks && leftOk && rightOk) {
            close = k;
            break;
          }
          k += 1;
        }
        if (close !== undefined) {
          const end = close + n;
          pushStyleToken(utf16OfByte(i), utf16OfByte(end), tt.TODO_CODE);
          i = end;
        } else {
          i += n;
        }
        break;
      }
      case 0x3c /* < */: {
        // url: <scheme://...> up to the next `>`.
        const rest = bytes.subarray(i + 1);
        const hasScheme =
          startsWithAscii(rest, "https://") ||
          startsWithAscii(rest, "http://") ||
          startsWithAscii(rest, "ftp://");
        if (hasScheme) {
          const gt = rest.indexOf(0x3e);
          if (gt !== -1) {
            const end = i + 1 + gt + 1;
            pushStyleToken(utf16OfByte(i), utf16OfByte(end), tt.TODO_URL);
            i = end;
            break;
          }
        }
        i += 1;
        break;
      }
      default:
        i += 1;
    }
  }

  function pushStyleToken(start: number, end: number, type: number): void {
    raw.push({ line: lineIdx, start: offset + start, length: end - start, type, modifiers: 0 });
  }
}

function startsWithAscii(bytes: Buffer, prefix: string): boolean {
  const expected = Buffer.from(prefix, "ascii");
  return bytes.subarray(0, expected.length).equals(expected);
}

/** UTF-16 code-unit length of a byte slice decoded as UTF-8. */
function utf16LengthOfBytes(bytes: Buffer): number {
  return bytes.toString("utf8").length;
}
