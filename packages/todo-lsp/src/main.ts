#!/usr/bin/env node

import {
  createConnection,
  DiagnosticSeverity,
  FoldingRangeKind,
  PositionEncodingKind,
  SymbolKind,
  TextDocumentSyncKind,
  type Connection,
  type Diagnostic,
  type DocumentLink,
  type DocumentSymbol as LspDocumentSymbol,
  type FoldingRange,
  type InitializeParams,
  type InitializeResult,
  type Position,
  type SemanticTokens,
  type SemanticTokensDelta,
  type TextEdit,
  type WorkspaceEdit,
} from "vscode-languageserver/node";
import { TextDocument } from "vscode-languageserver-textdocument";
import {
  archive,
  createTodoParser,
  diagnostics,
  documentLinks,
  documentSymbols,
  endPosition,
  foldingRanges,
  formatDocument,
  lineEdits,
  matchEol,
  reindent,
  repeatTasks,
  semanticTokens,
  semanticTokensLegend,
  toggle,
  Toggle,
  unarchive,
  type TodoParser,
} from "@todo-lsp/todo-core";

const COMMANDS = [
  "todo-language.toggleDone",
  "todo-language.toggleCancelled",
  "todo-language.toggleStart",
  "todo-language.toggleDue",
  "todo-language.toggleQueue",
  "todo-language.toggleQueueUnshift",
  "todo-language.toggleWaiting",
  "todo-language.togglePending",
  "todo-language.toggleHide",
  "todo-language.toggleRepeat",
  "todo-language.indent",
  "todo-language.dedent",
  "todo-language.repeatTasks",
  "todo-language.archive",
  "todo-language.unarchive",
];

type StoredDocument = {
  document: TextDocument;
  tree: ReturnType<TodoParser["parse"]>;
};

type TokenResult = {
  resultId: string;
  data: number[];
};

class TodoLanguageServer {
  readonly #documents = new Map<string, StoredDocument>();
  readonly #tokenResults = new Map<string, TokenResult>();
  #nextResultId = 1;
  #refreshSupported = false;
  #pendingRefreshes = 0;
  #commandQueue = Promise.resolve();

  constructor(
    private readonly connection: Connection,
    private readonly parser: TodoParser,
  ) {}

  listen(): void {
    this.connection.onInitialize((params) => this.initialize(params));
    this.connection.onShutdown(() => undefined);
    this.connection.onDidOpenTextDocument((params) => {
      const document = TextDocument.create(
        params.textDocument.uri,
        params.textDocument.languageId,
        params.textDocument.version,
        params.textDocument.text,
      );
      this.storeDocument(document);
      this.publishDiagnostics(document);
    });
    this.connection.onDidChangeTextDocument((params) => {
      const current = this.#documents.get(params.textDocument.uri)?.document;
      if (!current) return;

      const document = TextDocument.update(current, params.contentChanges, params.textDocument.version);
      this.storeDocument(document);
      this.publishDiagnostics(document);
      if (this.#pendingRefreshes > 0) {
        this.#pendingRefreshes--;
        if (this.#refreshSupported) void this.connection.languages.semanticTokens.refresh();
      }
    });
    this.connection.onDidCloseTextDocument((params) => {
      this.#documents.delete(params.textDocument.uri);
      this.#tokenResults.delete(params.textDocument.uri);
      void this.connection.sendDiagnostics({ uri: params.textDocument.uri, diagnostics: [] });
    });
    this.connection.onDocumentSymbol((params) => {
      const stored = this.#documents.get(params.textDocument.uri);
      return stored ? documentSymbols(stored.tree.rootNode, stored.document.getText()).map(symbol) : [];
    });
    this.connection.onFoldingRanges((params) => {
      const stored = this.#documents.get(params.textDocument.uri);
      return stored ? foldingRanges(stored.tree.rootNode, stored.document.getText()).map(foldingRange) : [];
    });
    this.connection.onDocumentLinks((params) => {
      const text = this.#documents.get(params.textDocument.uri)?.document.getText();
      return text === undefined ? [] : documentLinks(text).map(documentLink);
    });
    this.connection.languages.semanticTokens.on((params) => this.fullTokens(params.textDocument.uri));
    this.connection.languages.semanticTokens.onDelta((params) =>
      this.deltaTokens(params.textDocument.uri, params.previousResultId),
    );
    this.connection.onDocumentFormatting((params) => {
      const document = this.#documents.get(params.textDocument.uri)?.document;
      if (!document) return [];

      const text = document.getText();
      const formatted = matchEol(text, formatDocument(text));
      return formatted === text ? [] : [{ range: endRange(text), newText: formatted }];
    });
    this.connection.onExecuteCommand((params) => this.enqueueCommand(params.command, params.arguments));
    this.connection.listen();
  }

  private initialize(params: InitializeParams): InitializeResult {
    this.#refreshSupported = params.capabilities.workspace?.semanticTokens?.refreshSupport === true;
    return {
      serverInfo: { name: "todo-lsp", version: "0.1.0" },
      capabilities: {
        positionEncoding: PositionEncodingKind.UTF16,
        textDocumentSync: { openClose: true, change: TextDocumentSyncKind.Full },
        documentSymbolProvider: true,
        foldingRangeProvider: true,
        documentLinkProvider: { resolveProvider: false },
        documentFormattingProvider: true,
        executeCommandProvider: { commands: COMMANDS },
        semanticTokensProvider: {
          legend: semanticTokensLegend(),
          full: { delta: true },
        },
      },
    };
  }

  private storeDocument(document: TextDocument): void {
    this.#documents.set(document.uri, { document, tree: this.parser.parse(document.getText()) });
  }

  private publishDiagnostics(document: TextDocument): void {
    const stored = this.#documents.get(document.uri);
    if (!stored) return;
    void this.connection.sendDiagnostics({
      uri: document.uri,
      version: document.version,
      diagnostics: diagnostics(stored.tree.rootNode).map(diagnostic),
    });
  }

  private fullTokens(uri: string): SemanticTokens | null {
    const text = this.#documents.get(uri)?.document.getText();
    if (text === undefined) return null;

    const data = tokenData(text);
    const resultId = this.storeTokenResult(uri, data);
    return { resultId, data };
  }

  private deltaTokens(uri: string, previousResultId: string): SemanticTokens | SemanticTokensDelta | null {
    const text = this.#documents.get(uri)?.document.getText();
    if (text === undefined) return null;

    const data = tokenData(text);
    const previous = this.#tokenResults.get(uri);
    const resultId = this.storeTokenResult(uri, data);
    if (!previous || previous.resultId !== previousResultId) return { resultId, data };

    return {
      resultId,
      edits: tokenEdits(previous.data, data),
    };
  }

  private storeTokenResult(uri: string, data: number[]): string {
    const resultId = `todo-${this.#nextResultId++}`;
    this.#tokenResults.set(uri, { resultId, data });
    return resultId;
  }

  private enqueueCommand(command: string, arguments_: unknown[] | undefined): Promise<undefined> {
    const run = this.#commandQueue.then(() => this.executeCommand(command, arguments_));
    this.#commandQueue = run.catch(() => undefined);
    return run;
  }

  private async executeCommand(command: string, arguments_: unknown[] | undefined): Promise<undefined> {
    const uri = typeof arguments_?.[0] === "string" ? arguments_[0] : undefined;
    if (!uri) return undefined;

    const stored = this.#documents.get(uri);
    if (!stored) return undefined;

    const selection = lineSelection(arguments_?.[1]);
    const oldText = stored.document.getText();
    const newText = commandResult(command, oldText, selection);
    if (!newText || newText === oldText) return undefined;

    const edits = lineEdits(oldText, newText).map(textEdit);
    if (edits.length === 0) return undefined;

    const edit: WorkspaceEdit = { changes: { [uri]: edits } };
    const response = await this.connection.workspace.applyEdit(edit);
    if (!response.applied) return undefined;

    this.storeDocument(
      TextDocument.create(uri, stored.document.languageId, stored.document.version, newText),
    );
    this.#pendingRefreshes++;
    return undefined;
  }
}

function commandResult(command: string, text: string, selection: readonly number[]): string | undefined {
  const today = new Date();
  switch (command) {
    case "todo-language.toggleDone":
      return toggle(text, selection, Toggle.Done, today);
    case "todo-language.toggleCancelled":
      return toggle(text, selection, Toggle.Cancelled, today);
    case "todo-language.toggleStart":
      return toggle(text, selection, Toggle.Start, today);
    case "todo-language.toggleDue":
      return toggle(text, selection, Toggle.Due, today);
    case "todo-language.toggleQueue":
      return toggle(text, selection, Toggle.Queue, today);
    case "todo-language.toggleQueueUnshift":
      return toggle(text, selection, Toggle.QueueUnshift, today);
    case "todo-language.toggleWaiting":
      return toggle(text, selection, Toggle.Waiting, today);
    case "todo-language.togglePending":
      return toggle(text, selection, Toggle.Pending, today);
    case "todo-language.toggleHide":
      return toggle(text, selection, Toggle.Hide, today);
    case "todo-language.toggleRepeat":
      return toggle(text, selection, Toggle.Repeat, today);
    case "todo-language.indent":
      return reindent(text, selection, 1);
    case "todo-language.dedent":
      return reindent(text, selection, -1);
    case "todo-language.repeatTasks":
      return repeatTasks(text);
    case "todo-language.archive":
      return archive(text, selection);
    case "todo-language.unarchive":
      return unarchive(text, selection);
    default:
      return undefined;
  }
}

function tokenData(text: string): number[] {
  return semanticTokens(text).flatMap((token) => [
    token.deltaLine,
    token.deltaStart,
    token.length,
    token.tokenType,
    token.tokenModifiers,
  ]);
}

function tokenEdits(previous: readonly number[], current: readonly number[]) {
  let start = 0;
  while (sameToken(previous, start, current, start)) start += 5;

  let previousEnd = previous.length;
  let currentEnd = current.length;
  while (previousEnd > start && currentEnd > start && sameToken(previous, previousEnd - 5, current, currentEnd - 5)) {
    previousEnd -= 5;
    currentEnd -= 5;
  }

  if (start === previousEnd && start === currentEnd) return [];
  return [{ start, deleteCount: previousEnd - start, data: current.slice(start, currentEnd) }];
}

function sameToken(left: readonly number[], leftStart: number, right: readonly number[], rightStart: number): boolean {
  return (
    leftStart + 5 <= left.length &&
    rightStart + 5 <= right.length &&
    left.slice(leftStart, leftStart + 5).every((value, index) => value === right[rightStart + index])
  );
}

function lineSelection(value: unknown): number[] {
  if (!Array.isArray(value)) return [];
  return value.filter((line): line is number => Number.isSafeInteger(line) && line >= 0);
}

function diagnostic(value: { range: { start: Position; end: Position }; message: string }): Diagnostic {
  return {
    range: value.range,
    message: value.message,
    severity: DiagnosticSeverity.Error,
    source: "todo",
  };
}

function symbol(value: {
  name: string;
  kind: "module" | "string";
  range: { start: Position; end: Position };
  selectionRange: { start: Position; end: Position };
  children: unknown[] | undefined;
}): LspDocumentSymbol {
  return {
    name: value.name,
    kind: value.kind === "module" ? SymbolKind.Module : SymbolKind.String,
    range: value.range,
    selectionRange: value.selectionRange,
    children: value.children?.map((child) => symbol(child as Parameters<typeof symbol>[0])),
  };
}

function foldingRange(value: { startLine: number; endLine: number; kind: "comment" | "region" }): FoldingRange {
  return {
    startLine: value.startLine,
    endLine: value.endLine,
    kind: value.kind === "comment" ? FoldingRangeKind.Comment : FoldingRangeKind.Region,
  };
}

function documentLink(value: { range: { start: Position; end: Position }; target: string }): DocumentLink {
  return { range: value.range, target: value.target };
}

function textEdit(value: { range: { start: Position; end: Position }; newText: string }): TextEdit {
  return value;
}

function endRange(text: string): { start: Position; end: Position } {
  return { start: { line: 0, character: 0 }, end: endPosition(text) };
}

async function main(): Promise<void> {
  const parser = await createTodoParser();
  const server = new TodoLanguageServer(createConnection(process.stdin, process.stdout), parser);
  server.listen();
}

void main();
