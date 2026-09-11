// Line-level lexical model of the Todo language, shared by highlighting,
// commands, formatting, archiving and repeat generation.
//
// This module mirrors SPEC.md's 用語 definitions verbatim:
// 見出し行 / タスク行 / タグ列 / 灰色行 / インデントレベル. All consumers
// classify a physical line through `parseLine`; nothing here talks to the
// syntax tree.

/** A `@name` / `@name(arg)` token inside a line's leading or trailing tag
 * column. */
export interface Tag {
  name: string;
  arg: string | undefined;
  /** Index of the leading `@` within the line, in UTF-16 code units
   * (JS string indices; byte offsets in the Rust original). */
  start: number;
  /** Index one past the token's last character, in UTF-16 code units. */
  end: number;
}

/** The token as it appears in a line: `@name` or `@name(arg)`. */
export function tagText(tag: Tag): string {
  return tag.arg === undefined ? `@${tag.name}` : `@${tag.name}(${tag.arg})`;
}

/** How a physical line classifies per SPEC.md's line grammar. */
export type LineKind =
  | { kind: "blank" }
  /** 見出し行: non-empty body, then `:`, then an optional trailing tag
   * column. Leading tag-like tokens are body text (SPEC 行頭タグ列 applies
   * to task lines only). The value is the index of the `:` in UTF-16
   * code units. */
  | { kind: "heading"; colon: number }
  /** タスク行: an optional leading tag column, a body that may be empty (a
   * tag-only line), and an optional trailing tag column. */
  | { kind: "task" };

/** The whole-line gray rule that applies to a line, in SPEC.md 適用規則
 * order (done before cancelled before hide; `Archive:` headings are handled
 * separately via `isArchiveHeading`). */
export type Gray = "done" | "cancelled" | "hide";

/** The parsed structure of one physical line (without its newline). */
export interface LineParts {
  lineKind: LineKind;
  /** Length of the leading whitespace (spaces and tabs), in UTF-16 code
   * units. */
  indentLen: number;
  /** Indent measurement (tabSize spaces per level; tab = to the next
   * multiple of tabSize). Structure is decided by comparing these
   * relatively. */
  units: number;
  /** The level per SPEC.md's formula (`units / tabSize`), used when writing
   * indents (indent / dedent commands). */
  level: number;
  /** The leading tag column (行頭タグ列), in line order. Empty when the
   * line starts with body text. */
  leadingTags: Tag[];
  /** Range of the trimmed body text — before the `:` for headings, before
   * the trailing tag column for tasks — in UTF-16 code units. Empty range
   * for blank lines and tag-only lines. */
  textRange: [number, number];
  /** The trailing tag column (行末タグ列), in line order. */
  tags: Tag[];
}

export function isBlank(parts: LineParts): boolean {
  return parts.lineKind.kind === "blank";
}

export function isHeading(parts: LineParts): boolean {
  return parts.lineKind.kind === "heading";
}

/** Byte index of the heading `:`, if this line is a 見出し行. */
export function colonOf(parts: LineParts): number | undefined {
  return parts.lineKind.kind === "heading" ? parts.lineKind.colon : undefined;
}

export function isStructure(parts: LineParts): boolean {
  return parts.lineKind.kind !== "blank";
}

/** The trimmed body text (heading text excludes the `:`). Empty for
 * tag-only lines. */
export function textOf(parts: LineParts, line: string): string {
  return line.slice(parts.textRange[0], parts.textRange[1]);
}

/** タスクテキスト per SPEC.md: the line minus tags and whitespace, with
 * runs of whitespace collapsed to single spaces. Heading lines keep their
 * trailing `:`. Empty for tag-only lines. */
export function taskTextOf(parts: LineParts, line: string): string {
  const end = parts.lineKind.kind === "heading" ? parts.lineKind.colon + 1 : parts.textRange[1];
  return collapseAsciiWhitespace(line.slice(parts.textRange[0], end));
}

/** All tags on the line: the leading tag column then the trailing tag
 * column, in line order. */
export function allTags(parts: LineParts): Tag[] {
  return [...parts.leadingTags, ...parts.tags];
}

/** Whether the line has a `@name` tag in either tag column. */
export function hasTag(parts: LineParts, name: string): boolean {
  return allTags(parts).some((t) => t.name === name);
}

/** The first `@name` argument on the line, if any (leading column first). */
export function tagArg(parts: LineParts, name: string): string | undefined {
  return allTags(parts).find((t) => t.name === name)?.arg;
}

/** The 灰色行 rule for this line: `@done` / `@cancelled` / `@hide` in
 * either tag column, first match in 適用規則 order. */
export function grayOf(parts: LineParts): Gray | undefined {
  if (hasTag(parts, "done")) return "done";
  if (hasTag(parts, "cancelled")) return "cancelled";
  if (hasTag(parts, "hide")) return "hide";
  return undefined;
}

/** Whether this is an `Archive:` 見出し行 (the display §行単位の色付け and
 * §アーカイブ target). Indented archives qualify; the heading text must be
 * exactly `Archive`. */
export function isArchiveHeading(parts: LineParts, line: string): boolean {
  return isHeading(parts) && textOf(parts, line) === "Archive";
}

/** Rule 2 of §フォーマット for this line alone: every token (leading tags,
 * text, `:`, trailing tags) separated by single spaces, no leading/trailing
 * whitespace. Returns an empty string for blank lines. The caller prepends
 * the normalized indent. */
export function normalizeBody(parts: LineParts, line: string): string {
  return render(parts, line, parts.leadingTags, parts.tags);
}

/** Render a line's content from its parsed parts and (possibly edited) tag
 * columns: leading tags, whitespace-normalized text, `:` for headings,
 * trailing tags — each joined with single spaces. Blank lines render as
 * empty strings. */
export function render(parts: LineParts, line: string, leading: Tag[], trailing: Tag[]): string {
  const text = collapseAsciiWhitespace(textOf(parts, line));
  const out: string[] = [];
  for (const tag of leading) out.push(tagText(tag));
  if (text !== "") out.push(text);
  let joined = out.join(" ");
  if (parts.lineKind.kind === "heading") joined += ":";
  for (const tag of trailing) {
    joined += (joined === "" ? "" : " ") + tagText(tag);
  }
  return joined;
}

/** Split on ASCII whitespace runs (mirroring Rust's
 * `split_ascii_whitespace`) and rejoin with single spaces. */
function collapseAsciiWhitespace(text: string): string {
  return text
    .split(/[ \t\n\v\f\r]+/)
    .filter(Boolean)
    .join(" ");
}

/** SPEC.md インデントレベルの測定単位: tabSize spaces per level; a tab
 * advances to the next multiple of tabSize (so up to tabSize-1 preceding
 * spaces merge with it). Structure (parent / child / sibling) is decided by
 * comparing these units relatively — any deeper indent makes a child. */
export function indentUnits(indent: string, tabSize = 4): number {
  let units = 0;
  for (const ch of indent) {
    units = ch === "\t" ? (Math.floor(units / tabSize) + 1) * tabSize : units + 1;
  }
  return units;
}

/** The level per SPEC.md's formula: `units / tabSize` (the canonical level
 * used when writing indents — tabSize spaces per level). */
export function indentLevel(indent: string, tabSize = 4): number {
  return Math.floor(indentUnits(indent, tabSize) / tabSize);
}

/** The canonical indentation for a level: tabSize spaces per level
 * (§インデント). */
export function indentForLevel(level: number, tabSize = 4): string {
  return " ".repeat(tabSize * level);
}

/** Parse one physical line (newline already stripped) into `LineParts`. */
export function parseLine(line: string, tabSize = 4): LineParts {
  let indentLen = 0;
  while (indentLen < line.length && (line[indentLen] === " " || line[indentLen] === "\t")) {
    indentLen++;
  }
  const indent = line.slice(0, indentLen);
  const units = indentUnits(indent, tabSize);
  const level = indentLevel(indent, tabSize);
  const blank: LineParts = {
    lineKind: { kind: "blank" },
    indentLen,
    units,
    level,
    leadingTags: [],
    textRange: [indentLen, indentLen],
    tags: [],
  };
  if (indentLen === line.length) return blank;

  // The leading tag column (行頭タグ列): tags from the line start up to the
  // first non-tag token. A line the leading column fills entirely is a
  // tag-only task line with no body and no trailing column.
  const [scannedLeadingTags, contentStart] = scanLeadingTagsForward(line, indentLen);
  if (contentStart === line.length) {
    return {
      lineKind: { kind: "task" },
      indentLen,
      units,
      level,
      leadingTags: scannedLeadingTags,
      textRange: [contentStart, contentStart],
      tags: [],
    };
  }

  // The trailing tag column is the line-end suffix of tags (SPEC 行末タグ列).
  // Tags are parsed backward from the line end so that arguments containing
  // spaces (`@repeat(0 0 * * *)`) stay intact.
  const tags = scanTagColumnBackward(line, contentStart);
  let bodyEnd = line.length;
  while (bodyEnd > contentStart && (line[bodyEnd - 1] === " " || line[bodyEnd - 1] === "\t")) {
    bodyEnd--;
  }
  const body = line.slice(contentStart, tags.length > 0 ? tags[0].start : bodyEnd);

  // 見出し行: the body's rightmost `:` with only whitespace after it (up to
  // the tag column). Any earlier `:` sits inside the body text, so only the
  // rightmost colon can qualify — this mirrors the external scanner, where
  // the last valid colon wins. The text before the `:` may be empty (SPEC
  // 見出し: `:` で終わる行は本文が空でも見出し).
  let colon: number | undefined;
  for (let i = body.length - 1; i >= 0; i--) {
    if (body[i] === ":" && /^[\t ]*$/.test(body.slice(i + 1))) {
      colon = contentStart + i;
      break;
    }
  }
  const lineKind: LineKind = colon === undefined ? { kind: "task" } : { kind: "heading", colon };

  // 見出し行は行頭タグ列を持たず、行頭のタグ字面は本文に含める（SPEC 用語
  // 行頭タグ列）。タスク行だけが行頭タグ列を持つ。Heading text excludes
  // the `:`; task text excludes trailing whitespace.
  const leadingTags = colon === undefined ? scannedLeadingTags : [];
  const textStart = colon === undefined ? contentStart : indentLen;
  const textEnd = colon !== undefined ? colon : textStart + trimEndOfSpacesAndTabs(body).length;
  return { lineKind, indentLen, units, level, leadingTags, textRange: [textStart, textEnd], tags };
}

function trimEndOfSpacesAndTabs(body: string): string {
  let end = body.length;
  while (end > 0 && (body[end - 1] === " " || body[end - 1] === "\t")) end--;
  return body.slice(0, end);
}

/** The leading tag column: tags parsed left-to-right from `start`, each
 * followed by whitespace (or the line end for a tag-only line). Returns the
 * tags and the byte offset where the body starts. */
function scanLeadingTagsForward(line: string, start: number): [Tag[], number] {
  const tags: Tag[] = [];
  let pos = start;
  for (;;) {
    const parsed = parseTagStartingAt(line, pos);
    if (!parsed) break;
    const [tag, end] = parsed;
    // A leading tag must be followed by whitespace or the line end;
    // `@x(a)y` is not part of the leading column.
    if (end < line.length && line[end] !== " " && line[end] !== "\t") break;
    tags.push(tag);
    pos = end;
    while (pos < line.length && (line[pos] === " " || line[pos] === "\t")) pos++;
    if (pos === line.length) break;
  }
  return [tags, pos];
}

/** Parse one tag starting exactly at byte `start`: `@name` or `@name(arg)`.
 * `name` is 1+ chars without whitespace or `(`; the argument may contain
 * whitespace but no `)` and must close before the line ends. Returns the
 * tag and the byte offset just past it. */
function parseTagStartingAt(line: string, start: number): [Tag, number] | undefined {
  if (line[start] !== "@") return undefined;
  let i = start + 1;
  while (i < line.length && line[i] !== " " && line[i] !== "\t" && line[i] !== "(") i++;
  if (i === start + 1) return undefined; // empty name
  const name = line.slice(start + 1, i);
  if (line[i] === "(") {
    const close = line.indexOf(")", i + 1);
    if (close === -1) return undefined;
    return [{ name, arg: line.slice(i + 1, close), start, end: close + 1 }, close + 1];
  }
  return [{ name, arg: undefined, start, end: i }, i];
}

/** The trailing tag column: tags parsed right-to-left from the line end,
 * separated by whitespace, returned left-to-right. */
function scanTagColumnBackward(line: string, contentStart: number): Tag[] {
  let pos = line.length;
  const tags: Tag[] = [];
  for (;;) {
    while (pos > contentStart && (line[pos - 1] === " " || line[pos - 1] === "\t")) pos--;
    const tag = parseTagEndingAt(line, pos, contentStart);
    if (!tag) break;
    pos = tag.start;
    tags.push(tag);
    if (pos === contentStart) break;
  }
  tags.reverse();
  return tags;
}

/** Parse one tag that ends exactly at byte `end`. A tag is `@name` or
 * `@name(arg)`: `name` is 1+ chars without whitespace or `(` (a `)` or `@`
 * inside the name is fine), the argument may be empty and contain
 * whitespace but no `)`. The tag must start at `contentStart` or after
 * whitespace — the trailing column begins after a space, so a tag glued to
 * the heading `:` (`Foo:@tag`) is body text. */
function parseTagEndingAt(line: string, end: number, contentStart: number): Tag | undefined {
  if (end <= contentStart) return undefined;
  const boundaryOk = (start: number) =>
    start === contentStart || line[start - 1] === " " || line[start - 1] === "\t";
  if (line[end - 1] === ")") {
    // Arg-carrying tag. `(` candidates are tried right-to-left because an
    // argument may itself contain `(`; the correct opener is the one with a
    // `@name` directly before it and no `)` inside the argument.
    let searchEnd = end - 1;
    for (;;) {
      let open = -1;
      for (let i = searchEnd - 1; i >= contentStart; i--) {
        if (line[i] === "(") {
          open = i;
          break;
        }
      }
      if (open < contentStart) return undefined;
      const arg = line.slice(open + 1, end - 1);
      if (!arg.includes(")")) {
        let j = open;
        while (
          j > contentStart &&
          line[j - 1] !== " " &&
          line[j - 1] !== "\t" &&
          line[j - 1] !== "("
        ) {
          j--;
        }
        if (j < open && line[j] === "@" && boundaryOk(j)) {
          return { name: line.slice(j + 1, open), arg, start: j, end };
        }
      }
      searchEnd = open;
    }
  }
  // No-argument tag `@name`.
  let j = end;
  while (j > contentStart && line[j - 1] !== " " && line[j - 1] !== "\t" && line[j - 1] !== "(") {
    j--;
  }
  if (j + 1 < end && line[j] === "@" && boundaryOk(j)) {
    return { name: line.slice(j + 1, end), arg: undefined, start: j, end };
  }
  return undefined;
}
