// Language Server Protocol data types for the analysis functions. Only the
// shapes the todo LSP consumes are modeled; positions and ranges follow
// SPEC.md's「LSP の位置」— UTF-16 code units.

export interface Position {
  /** 0-based line. */
  line: number;
  /** 0-based UTF-16 code-unit offset within the line. */
  character: number;
}

export interface Range {
  start: Position;
  end: Position;
}

export interface TextEdit {
  range: Range;
  newText: string;
}

export interface DocumentLink {
  range: Range;
  /** The URL without its enclosing `<` `>`. */
  target: string;
}

export type SymbolKind = "module" | "string";

export interface DocumentSymbol {
  name: string;
  kind: SymbolKind;
  range: Range;
  selectionRange: Range;
  children: DocumentSymbol[] | undefined;
}

export type FoldingRangeKind = "comment" | "region";

export interface FoldingRange {
  startLine: number;
  endLine: number;
  kind: FoldingRangeKind;
}

export interface Diagnostic {
  range: Range;
  severity: "error";
  source: "todo";
  message: string;
}

export interface SemanticToken {
  /** 0-based line relative to the previous token. */
  deltaLine: number;
  /** UTF-16 start position relative to the previous token's start on the
   * same line; absolute on a new line. */
  deltaStart: number;
  /** Length in UTF-16 code units. */
  length: number;
  tokenType: number;
  tokenModifiers: number;
}

export interface SemanticTokensLegend {
  tokenTypes: string[];
  tokenModifiers: string[];
}
