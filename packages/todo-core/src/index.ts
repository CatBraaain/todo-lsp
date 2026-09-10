import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { Language, Parser, type Tree } from "web-tree-sitter";

export {
  // Line model
  allTags,
  colonOf,
  grayOf,
  hasTag,
  indentForLevel,
  indentLevel,
  indentUnits,
  isArchiveHeading,
  isBlank,
  isHeading,
  isStructure,
  normalizeBody,
  parseLine,
  render,
  tagArg,
  tagText,
  taskTextOf,
  textOf,
  type Gray,
  type LineKind,
  type LineParts,
  type Tag,
} from "./line.js";
export {
  documentLinks,
  documentSymbols,
  diagnostics,
  foldingRanges,
  semanticTokens,
  semanticTokensAt,
  semanticTokensLegend,
} from "./analysis.js";
export { archive, unarchive } from "./archive.js";
export { toggle, reindent, Toggle, type Toggle as ToggleAction } from "./commands.js";
export { formatDocument, joinLines, splitLines } from "./format.js";
export { repeatTasks } from "./repeat.js";
export { endPosition, lineEdits, matchEol } from "./edits.js";
export type {
  // LSP data types (SPEC「LSP の位置」: UTF-16 code units)
  Diagnostic,
  DocumentLink,
  DocumentSymbol,
  FoldingRange,
  FoldingRangeKind,
  Position,
  Range,
  TextEdit,
  SemanticToken,
  SemanticTokensLegend,
  SymbolKind,
} from "./lsp-types.js";

export type TodoTree = Tree;

export class TodoParser {
  readonly #parser: Parser;

  constructor(parser: Parser) {
    this.#parser = parser;
  }

  parse(document: string): TodoTree {
    const tree = this.#parser.parse(document);

    if (!tree) {
      throw new Error("Todo parser has no language");
    }

    return tree;
  }
}

/** Resolve the grammar wasm: beside this module, or one directory up when
 * this module is the test build's `dist/src/index.js` — `build:wasm` writes
 * the wasm next to the package build's `dist/index.js`. */
function wasmPath(): string {
  const url = new URL("./tree-sitter-todo.wasm", import.meta.url);
  if (existsSync(url)) return fileURLToPath(url);
  return fileURLToPath(new URL("../tree-sitter-todo.wasm", import.meta.url));
}

export async function createTodoParser(): Promise<TodoParser> {
  await Parser.init();

  const language = await Language.load(wasmPath());
  const parser = new Parser();
  parser.setLanguage(language);

  return new TodoParser(parser);
}
