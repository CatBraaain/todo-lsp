import assert from "node:assert/strict";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import test from "node:test";

interface Message {
  id?: number | string;
  method?: string;
  params?: unknown;
  result?: unknown;
}

class JsonRpcClient {
  #buffer = Buffer.alloc(0);
  #messages: Message[] = [];
  #waiters: Array<{ matches: (message: Message) => boolean; resolve: (message: Message) => void }> = [];

  constructor(readonly process: ChildProcessWithoutNullStreams) {
    process.stdout.on("data", (chunk: Buffer) => this.read(chunk));
  }

  send(message: Message): void {
    const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", ...message }));
    this.process.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
    this.process.stdin.write(body);
  }

  request(id: number, method: string, params: unknown): Promise<Message> {
    this.send({ id, method, params });
    return this.next((message) => message.id === id);
  }

  next(matches: (message: Message) => boolean): Promise<Message> {
    const index = this.#messages.findIndex(matches);
    if (index !== -1) return Promise.resolve(this.#messages.splice(index, 1)[0]);

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("timed out waiting for JSON-RPC message")), 5_000);
      this.#waiters.push({
        matches,
        resolve: (message) => {
          clearTimeout(timer);
          resolve(message);
        },
      });
    });
  }

  expectNoMessage(matches: (message: Message) => boolean): Promise<void> {
    if (this.#messages.some(matches)) return Promise.reject(new Error("unexpected JSON-RPC message"));

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        const index = this.#waiters.findIndex((waiter) => waiter.matches === matches);
        if (index !== -1) this.#waiters.splice(index, 1);
        resolve();
      }, 50);
      this.#waiters.push({
        matches,
        resolve: () => {
          clearTimeout(timer);
          reject(new Error("unexpected JSON-RPC message"));
        },
      });
    });
  }

  private read(chunk: Buffer): void {
    this.#buffer = Buffer.concat([this.#buffer, chunk]);
    while (true) {
      const delimiter = this.#buffer.indexOf("\r\n\r\n");
      if (delimiter === -1) return;

      const header = this.#buffer.subarray(0, delimiter).toString("ascii");
      const length = Number(/^Content-Length: (\d+)$/im.exec(header)?.[1]);
      const bodyStart = delimiter + 4;
      if (!Number.isSafeInteger(length) || this.#buffer.length < bodyStart + length) return;

      const message = JSON.parse(this.#buffer.subarray(bodyStart, bodyStart + length).toString()) as Message;
      this.#buffer = this.#buffer.subarray(bodyStart + length);
      this.receive(message);
    }
  }

  private receive(message: Message): void {
    const waiter = this.#waiters.findIndex(({ matches }) => matches(message));
    if (waiter === -1) {
      this.#messages.push(message);
      return;
    }
    this.#waiters.splice(waiter, 1)[0].resolve(message);
  }
}

test("stdio adapter serves the advertised Todo LSP features", async (context) => {
  const serverProcess = spawn(process.execPath, [new URL("../main.js", import.meta.url).pathname]);
  const rpc = new JsonRpcClient(serverProcess);
  context.after(() => {
    if (!serverProcess.killed) serverProcess.kill();
  });

  const initialize = await rpc.request(1, "initialize", {
    processId: null,
    capabilities: { workspace: { semanticTokens: { refreshSupport: true } } },
  });
  const capabilities = (initialize.result as { capabilities: Record<string, unknown> }).capabilities;
  assert.equal(capabilities.positionEncoding, "utf-16");
  assert.deepEqual(capabilities.textDocumentSync, { openClose: true, change: 1 });
  assert.deepEqual(capabilities.documentLinkProvider, { resolveProvider: false });
  assert.deepEqual(capabilities.semanticTokensProvider, {
    legend: {
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
    },
    full: { delta: true },
  });
  assert.deepEqual((capabilities.executeCommandProvider as { commands: string[] }).commands, [
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
  ]);

  rpc.send({ method: "initialized", params: {} });
  const uri = "file:///smoke.todo";
  const text = "😀 task @waiting\nProject:\n    child\n😀 see <https://example.com>\n";
  rpc.send({
    method: "textDocument/didOpen",
    params: { textDocument: { uri, languageId: "todo", version: 1, text } },
  });
  const diagnostics = await rpc.next((message) => message.method === "textDocument/publishDiagnostics");
  assert.deepEqual((diagnostics.params as { diagnostics: unknown[] }).diagnostics, []);

  const symbols = await rpc.request(2, "textDocument/documentSymbol", { textDocument: { uri } });
  assert.deepEqual(symbols.result, [
    {
      name: "😀 task",
      kind: 15,
      range: { start: { line: 0, character: 0 }, end: { line: 1, character: 0 } },
      selectionRange: { start: { line: 0, character: 0 }, end: { line: 1, character: 0 } },
    },
    {
      name: "Project",
      kind: 2,
      range: { start: { line: 1, character: 0 }, end: { line: 3, character: 0 } },
      selectionRange: { start: { line: 1, character: 0 }, end: { line: 2, character: 0 } },
      children: [
        {
          name: "child",
          kind: 15,
          range: { start: { line: 2, character: 4 }, end: { line: 3, character: 0 } },
          selectionRange: { start: { line: 2, character: 4 }, end: { line: 3, character: 0 } },
        },
      ],
    },
    {
      name: "😀 see <https://example.com>",
      kind: 15,
      range: { start: { line: 3, character: 0 }, end: { line: 4, character: 0 } },
      selectionRange: { start: { line: 3, character: 0 }, end: { line: 4, character: 0 } },
    },
  ]);

  const folds = await rpc.request(3, "textDocument/foldingRange", { textDocument: { uri } });
  assert.deepEqual(folds.result, [{ startLine: 1, endLine: 2, kind: "region" }]);

  const links = await rpc.request(4, "textDocument/documentLink", { textDocument: { uri } });
  assert.deepEqual(links.result, [
    {
      range: { start: { line: 3, character: 7 }, end: { line: 3, character: 28 } },
      target: "https://example.com",
    },
  ]);

  const formatted = await rpc.request(5, "textDocument/formatting", {
    textDocument: { uri },
    options: { tabSize: 4, insertSpaces: true },
  });
  assert.deepEqual(formatted.result, [
    {
      range: { start: { line: 0, character: 0 }, end: { line: 4, character: 0 } },
      newText: "😀 task @waiting\n\nProject:\n    child\n\n😀 see <https://example.com>\n",
    },
  ]);

  const full = await rpc.request(6, "textDocument/semanticTokens/full", { textDocument: { uri } });
  const fullResult = full.result as { resultId: string; data: number[] };
  assert.ok(fullResult.resultId);
  assert.ok(fullResult.data.length > 0);

  const changedText = text.replace("@waiting", "@done");
  rpc.send({
    method: "textDocument/didChange",
    params: { textDocument: { uri, version: 2 }, contentChanges: [{ text: changedText }] },
  });
  await rpc.next((message) => message.method === "textDocument/publishDiagnostics");
  const changedDelta = await rpc.request(7, "textDocument/semanticTokens/full/delta", {
    textDocument: { uri },
    previousResultId: fullResult.resultId,
  });
  const changedDeltaResult = changedDelta.result as {
    resultId?: string;
    edits?: SemanticTokensEdit[];
    data?: unknown;
  };
  assert.ok(changedDeltaResult.resultId);
  assert.notEqual(changedDeltaResult.resultId, fullResult.resultId);
  assert.ok(changedDeltaResult.edits && changedDeltaResult.edits.length > 0);
  assert.equal("data" in changedDeltaResult, false);

  const changedFull = await rpc.request(8, "textDocument/semanticTokens/full", { textDocument: { uri } });
  const changedFullData = (changedFull.result as { data: number[] }).data;
  assert.deepEqual(applySemanticTokenEdits(fullResult.data, changedDeltaResult.edits), changedFullData);

  const knownDelta = await rpc.request(9, "textDocument/semanticTokens/full/delta", {
    textDocument: { uri },
    previousResultId: (changedFull.result as { resultId: string }).resultId,
  });
  const knownDeltaResult = knownDelta.result as {
    resultId?: string;
    edits?: unknown[];
    data?: unknown;
  };
  assert.ok(knownDeltaResult.resultId);
  assert.deepEqual(knownDeltaResult.edits, []);
  assert.equal("data" in knownDeltaResult, false);
  const unknownDelta = await rpc.request(10, "textDocument/semanticTokens/full/delta", {
    textDocument: { uri },
    previousResultId: "unknown",
  });
  assert.ok(Array.isArray((unknownDelta.result as { data?: number[] }).data));

  const commands: CommandCase[] = [
    datedToggle("todo-language.toggleDone", "done"),
    datedToggle("todo-language.toggleCancelled", "cancelled"),
    datedToggle("todo-language.toggleStart", "start"),
    datedToggle("todo-language.toggleDue", "due"),
    exactCommand("todo-language.toggleQueue", "task\n", "task @queue(1)\n"),
    exactCommand("todo-language.toggleQueueUnshift", "task\n", "task @queue(1)\n"),
    exactCommand("todo-language.toggleWaiting", "task\n", "task @waiting\n"),
    exactCommand("todo-language.togglePending", "task\n", "task @pending\n"),
    exactCommand("todo-language.toggleHide", "task\n", "task @hide\n"),
    {
      command: "todo-language.toggleRepeat",
      text: "task\n",
      assertText: (actual) => assert.equal(actual, "task @repeat(0 0 * * *)\n"),
    },
    exactCommand("todo-language.indent", "task\n", "    task\n"),
    exactCommand("todo-language.dedent", "    task\n", "task\n"),
    {
      command: "todo-language.repeatTasks",
      text: "task @repeat(* * * * *)\n",
      assertText: (actual) =>
        assert.match(actual, /^task @repeat\(\* \* \* \* \*\)\ntask @start\(\d{4}-\d{2}-\d{2} \d{2}:\d{2}\)\n$/),
    },
    exactCommand("todo-language.archive", "done @done\n", "Archive:\n    done @done\n"),
    exactCommand("todo-language.unarchive", "Archive:\n    done @done\n", "Archive:\n\ndone @done\n", 1),
  ];
  for (const [index, command] of commands.entries()) {
    await executeCommand(rpc, command, index);
  }

  rpc.send({ method: "textDocument/didClose", params: { textDocument: { uri } } });
  const closedDiagnostics = await rpc.next(
    (message) => message.method === "textDocument/publishDiagnostics",
  );
  assert.deepEqual((closedDiagnostics.params as { diagnostics: unknown[] }).diagnostics, []);

  const exited = new Promise<void>((resolve) => serverProcess.once("exit", () => resolve()));
  await rpc.request(11, "shutdown", null);
  rpc.send({ method: "exit", params: null });
  await exited;
});

test("stdio adapter does not refresh semantic tokens for unsupported clients", async (context) => {
  const serverProcess = spawn(process.execPath, [new URL("../main.js", import.meta.url).pathname]);
  const rpc = new JsonRpcClient(serverProcess);
  context.after(() => {
    if (!serverProcess.killed) serverProcess.kill();
  });

  await rpc.request(1, "initialize", { processId: null, capabilities: {} });
  rpc.send({ method: "initialized", params: {} });

  const uri = "file:///unsupported-refresh.todo";
  const text = "task\n";
  rpc.send({
    method: "textDocument/didOpen",
    params: { textDocument: { uri, languageId: "todo", version: 1, text } },
  });
  await rpc.next((message) => message.method === "textDocument/publishDiagnostics");

  rpc.send({
    id: 2,
    method: "workspace/executeCommand",
    params: { command: "todo-language.toggleDone", arguments: [uri, [0]] },
  });
  const apply = await rpc.next((message) => message.method === "workspace/applyEdit");
  const edits = (apply.params as { edit: { changes: Record<string, TextEdit[]> } }).edit.changes[uri];
  assert.ok(edits.length > 0);
  rpc.send({ id: apply.id, result: { applied: true } });
  await rpc.next((message) => message.id === 2);

  rpc.send({
    method: "textDocument/didChange",
    params: {
      textDocument: { uri, version: 2 },
      contentChanges: [{ text: applyEdits(text, edits) }],
    },
  });
  await rpc.next((message) => message.method === "textDocument/publishDiagnostics");
  await rpc.expectNoMessage((message) => message.method === "workspace/semanticTokens/refresh");

  const exited = new Promise<void>((resolve) => serverProcess.once("exit", () => resolve()));
  await rpc.request(3, "shutdown", null);
  rpc.send({ method: "exit", params: null });
  await exited;
});

type CommandCase = {
  command: string;
  text: string;
  selection?: number;
  assertText: (actual: string) => void;
};

type TextEdit = {
  range: { start: { line: number; character: number }; end: { line: number; character: number } };
  newText: string;
};

type SemanticTokensEdit = {
  start: number;
  deleteCount: number;
  data?: number[];
};

function datedToggle(command: string, tag: string): CommandCase {
  return exactCommand(command, "task\n", `task @${tag}(${localDateText()})\n`);
}

function exactCommand(command: string, text: string, expected: string, selection = 0): CommandCase {
  return { command, text, selection, assertText: (actual) => assert.equal(actual, expected) };
}

async function executeCommand(rpc: JsonRpcClient, command: CommandCase, index: number): Promise<void> {
  const uri = `file:///command-${index}.todo`;
  rpc.send({
    method: "textDocument/didOpen",
    params: { textDocument: { uri, languageId: "todo", version: 1, text: command.text } },
  });
  await rpc.next((message) => message.method === "textDocument/publishDiagnostics");

  const id = 100 + index;
  rpc.send({
    id,
    method: "workspace/executeCommand",
    params: { command: command.command, arguments: [uri, [command.selection ?? 0]] },
  });
  const apply = await rpc.next((message) => message.method === "workspace/applyEdit");
  const edits = (apply.params as { edit: { changes: Record<string, TextEdit[]> } }).edit.changes[uri];
  assert.ok(edits.length > 0, `${command.command} should apply edits`);
  const changedText = applyEdits(command.text, edits);
  command.assertText(changedText);
  rpc.send({ id: apply.id, result: { applied: true } });
  await rpc.next((message) => message.id === id);

  rpc.send({
    method: "textDocument/didChange",
    params: { textDocument: { uri, version: 2 }, contentChanges: [{ text: changedText }] },
  });
  const diagnostics = await rpc.next((message) => message.method === "textDocument/publishDiagnostics");
  assert.deepEqual((diagnostics.params as { diagnostics: unknown[] }).diagnostics, []);
  const refresh = await rpc.next((message) => message.method === "workspace/semanticTokens/refresh");
  rpc.send({ id: refresh.id, result: null });
  await rpc.expectNoMessage((message) => message.method === "workspace/semanticTokens/refresh");

  rpc.send({ method: "textDocument/didClose", params: { textDocument: { uri } } });
  const closedDiagnostics = await rpc.next(
    (message) => message.method === "textDocument/publishDiagnostics",
  );
  assert.deepEqual((closedDiagnostics.params as { diagnostics: unknown[] }).diagnostics, []);
}

function localDateText(): string {
  const date = new Date();
  const year = date.getFullYear().toString().padStart(4, "0");
  const month = (date.getMonth() + 1).toString().padStart(2, "0");
  const day = date.getDate().toString().padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function applyEdits(source: string, edits: readonly TextEdit[]): string {
  let result = source;
  for (const edit of [...edits].reverse()) {
    const start = offsetAt(source, edit.range.start);
    const end = offsetAt(source, edit.range.end);
    result = result.slice(0, start) + edit.newText + result.slice(end);
  }
  return result;
}

function applySemanticTokenEdits(data: readonly number[], edits: readonly SemanticTokensEdit[]): number[] {
  const result = [...data];
  for (const edit of [...edits].reverse()) result.splice(edit.start, edit.deleteCount, ...(edit.data ?? []));
  return result;
}

function offsetAt(source: string, position: { line: number; character: number }): number {
  const lines = source.split("\n");
  let offset = 0;
  for (let line = 0; line < position.line; line++) offset += lines[line].length + 1;
  return offset + position.character;
}
