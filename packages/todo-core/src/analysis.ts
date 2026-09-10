// Document analysis: outline (document symbols), folding ranges,
// diagnostics, semantic tokens and document links. Tree-backed features
// (symbols / folds / diagnostics) walk the tree-sitter parse tree; the rest
// are line-based scanners over the raw document.

import type { Node } from "web-tree-sitter";
import { grayOf, isBlank, isHeading, parseLine, textOf, type LineParts } from "./line.js";
import type { Diagnostic, DocumentSymbol, FoldingRange, Position, Range } from "./lsp-types.js";

export { semanticTokens, semanticTokensAt, semanticTokensLegend } from "./semantic-tokens.js";
export { documentLinks } from "./document-links.js";

// === Outline (document symbols) ===

/** Build the outline: walk `source_file`'s named children, skipping
 * zero-width `indent`/`dedent` tokens, mapping `heading_block` -> `module`
 * and `task_line` -> `string`, recursing through `task_block` for nesting.
 * Ranges are UTF-16. */
export function documentSymbols(root: Node, source: string): DocumentSymbol[] {
  return symbolsFromChildren(root, source);
}

function symbolsFromChildren(node: Node, source: string): DocumentSymbol[] {
  const out: DocumentSymbol[] = [];
  for (const child of namedChildrenOf(node)) {
    const symbol = symbolFromNode(child, source);
    if (symbol) out.push(symbol);
  }
  return out;
}

function symbolFromNode(node: Node, source: string): DocumentSymbol | undefined {
  switch (node.type) {
    case "heading_block":
      return symbolFromHeadingBlock(node, source);
    case "task_line":
    case "tag_only_line":
      return symbolFromTaskLine(node, source);
    // indent, dedent, task_block (handled by its parent) -> skip
    default:
      return undefined;
  }
}

/** The physical line text covered by a line node (trailing newlines and
 * blank runs stripped — `_newline` consumes `\n` runs as one token). The
 * node starts after the indent; `parseLine` accepts that slice directly. */
function lineTextOf(node: Node, source: string): string {
  return source.slice(node.startIndex, node.endIndex).replace(/[\n\r \t]+$/, "");
}

function symbolFromHeadingBlock(node: Node, source: string): DocumentSymbol {
  let headingLine: Node | undefined;
  let taskBlock: Node | undefined;
  for (const child of namedChildrenOf(node)) {
    if (child.type === "heading_line") headingLine = child;
    else if (child.type === "task_block") taskBlock = child;
  }

  let name = "";
  let selectionRange = rangeFromNode(node);
  if (headingLine) {
    // The symbol name is the body text (SPEC アウトライン: `:` の前の本文).
    // The tree's `text` field embeds the leading tag column in the TEXT
    // token, so re-parse the line instead of using the field.
    const line = lineTextOf(headingLine, source);
    name = textOf(parseLine(line), line);
    selectionRange = rangeFromNode(headingLine);
  }

  const children = taskBlock ? symbolsFromChildren(taskBlock, source) : [];
  return {
    name,
    kind: "module",
    range: rangeFromNode(node),
    selectionRange,
    children: children.length > 0 ? children : undefined,
  };
}

function symbolFromTaskLine(node: Node, source: string): DocumentSymbol {
  // The symbol name is the body text (SPEC アウトライン: タグを除いた本文).
  // The tree's `text` field embeds the leading tag column in the TEXT
  // token, so re-parse the line instead of using the field. Tag-only lines
  // (no `text` child) get an empty name.
  const line = lineTextOf(node, source);
  const name = textOf(parseLine(line), line);
  const range = rangeFromNode(node);
  return { name, kind: "string", range, selectionRange: range, children: undefined };
}

// === Folding ranges ===

/** Build heading and gray-block folding ranges. A gray-run fold spans the
 * row owning the run — heading, task line, or the first gray line for the
 * document root — through the run's last gray line; trailing blank lines
 * stay outside the fold. */
export function foldingRanges(root: Node, source: string): FoldingRange[] {
  const parts = linePartsOf(source);
  const tones = parts.map(toneOf);
  const out: FoldingRange[] = [];
  collectFoldingRanges(root, tones, out);
  collectTaskLineGrayFolds(parts, out);
  const seen = new Set<string>();
  return out
    .sort((a, b) => a.startLine - b.startLine || a.endLine - b.endLine)
    .filter((r) => {
      const key = `${r.startLine}:${r.endLine}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
}

type LineTone = "blank" | "gray" | "plain";

/** Parse every physical line (trailing CR stripped) for the line-based
 * folds and the tone classification. */
function linePartsOf(source: string): LineParts[] {
  return source.split("\n").map((raw) => parseLine(raw.endsWith("\r") ? raw.slice(0, -1) : raw));
}

function toneOf(parts: LineParts): LineTone {
  if (isBlank(parts)) return "blank";
  return grayOf(parts) ? "gray" : "plain";
}

function collectFoldingRanges(node: Node, tones: LineTone[], out: FoldingRange[]): void {
  if (node.type === "source_file") {
    const range = leadingGrayChildrenRange(node, tones);
    if (range) out.push(range);
  }

  for (const child of namedChildrenOf(node)) {
    if (child.type === "heading_block") {
      const range = foldingRangeForHeading(child, tones);
      if (range) out.push(range);
      collectFoldingRanges(child, tones, out);
    } else if (child.type === "task_block") {
      // Only headings own a child block in this grammar; recurse to find
      // nested headings. A task_block's own gray run starts at its heading
      // row and is already emitted by foldingRangeForHeading.
      collectFoldingRanges(child, tones, out);
    }
  }
}

function foldingRangeForHeading(node: Node, tones: LineTone[]): FoldingRange | undefined {
  let headingLine: Node | undefined;
  let taskBlock: Node | undefined;
  for (const child of namedChildrenOf(node)) {
    if (child.type === "heading_line") headingLine = child;
    else if (child.type === "task_block") taskBlock = child;
  }
  if (!headingLine || !taskBlock) return undefined;

  // A gray run under the heading folds from the heading row through the
  // run's last gray line, replacing the heading's region fold.
  const grayBounds = leadingGrayChildrenBounds(taskBlock, tones);
  if (grayBounds) {
    return {
      startLine: headingLine.startPosition.row,
      endLine: grayBounds[1],
      kind: "comment",
    };
  }

  const startLine = headingLine.startPosition.row;
  const endLine = lastLineOfBlock(node);
  if (endLine === undefined || endLine <= startLine) return undefined;
  return { startLine, endLine, kind: "region" };
}

function leadingGrayChildrenRange(node: Node, tones: LineTone[]): FoldingRange | undefined {
  const bounds = leadingGrayChildrenBounds(node, tones);
  if (!bounds || bounds[1] <= bounds[0]) return undefined;
  return { startLine: bounds[0], endLine: bounds[1], kind: "comment" };
}

function leadingGrayChildrenBounds(node: Node, tones: LineTone[]): [number, number] | undefined {
  const children = namedChildrenOf(node);
  const first = children[0];
  if (!first || !isGrayBlock(first, tones)) return undefined;

  let last = first;
  for (const child of children.slice(1)) {
    if (!isGrayBlock(child, tones)) break;
    last = child;
  }

  return [first.startPosition.row, lastGrayLineOfBlock(last, tones)];
}

/** The last gray line of a gray block: trailing blank lines swallowed by the
 * final `_newline` run are excluded, so a gray fold never covers the blank
 * run that follows it. A gray block always holds at least one non-blank
 * line, so the search cannot fail. */
function lastGrayLineOfBlock(node: Node, tones: LineTone[]): number {
  const start = node.startPosition.row;
  const end = lastLineOfBlock(node) ?? start;
  for (let line = end; line >= start; line--) {
    if (tones[line] !== "blank") return line;
  }
  return end;
}

function isGrayBlock(node: Node, tones: LineTone[]): boolean {
  const startLine = node.startPosition.row;
  const endLine = lastLineOfBlock(node);
  if (endLine === undefined) return false;
  for (let line = startLine; line <= endLine; line++) {
    if (tones[line] !== "blank" && tones[line] !== "gray") return false;
  }
  return true;
}

/** Return the last physical line of a `task_line` or `heading_block`. Lines
 * consume their trailing newline and any following blank lines. A heading's
 * zero-width `dedent` can point past its child block, so use `task_block`. */
function lastLineOfBlock(node: Node): number | undefined {
  let endRow: number;
  switch (node.type) {
    case "task_line":
    case "tag_only_line":
      endRow = node.endPosition.row;
      break;
    case "heading_block": {
      const taskBlock = namedChildrenOf(node).find((child) => child.type === "task_block");
      endRow = taskBlock ? taskBlock.endPosition.row : node.endPosition.row;
      break;
    }
    default:
      return undefined;
  }
  return Math.max(0, endRow - 1);
}

/** Gray-run folds owned by task lines (SPEC 灰色ブロックの折りたたみ:
 * “直下に子ブロックを持つ各行”). In the parse tree a task line's children
 * are flat siblings — only headings get a nested `task_block` — so task
 * owners are computed from the line model instead. */
function collectTaskLineGrayFolds(parts: LineParts[], out: FoldingRange[]): void {
  for (let row = 0; row < parts.length; row++) {
    if (isBlank(parts[row]) || isHeading(parts[row])) continue;
    const end = grayRunEndBelow(row, parts);
    if (end !== undefined) out.push({ startLine: row, endLine: end, kind: "comment" });
  }
}

/** The last gray row of the leading run of gray child blocks directly
 * below `ownerRow`, or undefined when the owner has no gray child run.
 * A child block starts at the first deeper line and spans its own
 * descendants (any deeper indent); blank lines never end a run. */
function grayRunEndBelow(ownerRow: number, parts: LineParts[]): number | undefined {
  const ownerUnits = parts[ownerRow].units;
  let end: number | undefined;
  let row = nextNonBlank(ownerRow + 1, parts);
  while (row !== undefined && parts[row].units > ownerUnits) {
    const blockUnits = parts[row].units;
    let last = row;
    let gray = toneOf(parts[row]) === "gray";
    let next = nextNonBlank(row + 1, parts);
    while (next !== undefined && parts[next].units > blockUnits) {
      if (toneOf(parts[next]) !== "gray") gray = false;
      last = next;
      next = nextNonBlank(next + 1, parts);
    }
    if (!gray) break;
    end = last;
    row = next;
  }
  return end;
}

function nextNonBlank(from: number, parts: LineParts[]): number | undefined {
  for (let i = from; i < parts.length; i++) {
    if (!isBlank(parts[i])) return i;
  }
  return undefined;
}

// === Diagnostics ===

/** Build diagnostics. SPEC 診断: “診断にエラーを出さない”“Todo 文書は
 * どのような行構成でも構文エラーにならない” — the grammar can still
 * produce ERROR nodes (e.g. a tab-only line between differently indented
 * rows), but no document is a syntax error, so diagnostics are always
 * empty. The parse tree parameter stays for interface compatibility. */
export function diagnostics(_root: Node): Diagnostic[] {
  return [];
}

function namedChildrenOf(node: Node): Node[] {
  const out: Node[] = [];
  for (let i = 0; i < node.namedChildCount; i++) {
    const child = node.namedChild(i);
    if (child) out.push(child);
  }
  return out;
}

/** Map a node's start/end points to an LSP `Range`. web-tree-sitter
 * parses JS strings as UTF-16, so `Point.column` is already a UTF-16
 * code-unit offset (SPEC「LSP の位置」) and passes through unchanged. */
function rangeFromNode(node: Node): Range {
  return { start: pointOf(node.startPosition), end: pointOf(node.endPosition) };
}

function pointOf(point: { row: number; column: number }): Position {
  return { line: point.row, character: point.column };
}
