import * as path from "node:path";
import * as vscode from "vscode";
import {
  ExecuteCommandRequest,
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";
import { setupAutoRepeatTriggers, shouldAutoRepeat } from "./autoRepeatCore.mjs";
import { COMMAND_SPECS } from "./commandSpecs.mjs";
import { platformDirectoryName, serverBinaryName } from "./platform.mjs";

// Resolve the per-platform server binary under bin/<platform>-<arch>/todo-lsp[.exe].
// The layout matches VS Code's process.platform / process.arch naming and must
// stay in sync with the bin_dir / bin_ext logic in the root justfile.
function serverCommand(context: vscode.ExtensionContext): string {
  return context.asAbsolutePath(
    path.join(
      "bin",
      platformDirectoryName(process.platform, process.arch),
      serverBinaryName(process.platform),
    ),
  );
}

// 選択行: every line contained in any selection, sorted and deduplicated.
function selectionLines(editor: vscode.TextEditor): number[] {
  const lines = new Set<number>();
  for (const selection of editor.selections) {
    for (let line = selection.start.line; line <= selection.end.line; line++) {
      lines.add(line);
    }
  }
  return [...lines].sort((a, b) => a - b);
}

// 自動リピート (§リピート): with `todo-language.repeatTask.autoRepeat` enabled
// and a todo document active, Repeat Tasks runs at extension startup, ~0.5s
// after the active editor switches, and every minute at second 0. The
// trigger wiring lives in autoRepeatCore.mjs; this adapter just binds the
// vscode environment onto it.
function setupAutoRepeat(
  context: vscode.ExtensionContext,
  client: LanguageClient,
  started: Promise<void>,
): void {
  const fire = (): unknown => {
    const editor = vscode.window.activeTextEditor;
    const enabled = vscode.workspace
      .getConfiguration("todo-language")
      .get<boolean>("repeatTask.autoRepeat", true);
    if (!editor || !shouldAutoRepeat(enabled, editor.document.languageId)) {
      return undefined;
    }
    return client.sendRequest(ExecuteCommandRequest.type, {
      command: "todo-language.repeatTasks",
      arguments: [editor.document.uri.toString()],
    });
  };

  const triggers = setupAutoRepeatTriggers({
    onStartup: (cb: () => void) => {
      void started.then(cb);
    },
    onEditorSwitch: (cb: () => void) => {
      context.subscriptions.push(
        vscode.window.onDidChangeActiveTextEditor(cb),
      );
    },
    now: () => new Date(),
    setTimeout: (cb: () => void, ms: number) => setTimeout(cb, ms),
    clearTimeout: (timer: NodeJS.Timeout) => clearTimeout(timer),
    fire,
  });
  context.subscriptions.push({ dispose: () => triggers.dispose() });
}

export function activate(context: vscode.ExtensionContext): void {
  const command = serverCommand(context);
  const serverOptions: ServerOptions = {
    run: { command, transport: TransportKind.stdio },
    debug: { command, transport: TransportKind.stdio },
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "todo" }],
  };
  const client = new LanguageClient("todo-lsp", "Todo LSP", serverOptions, clientOptions);
  context.subscriptions.push(client);
  const started = client.start();

  for (const spec of COMMAND_SPECS) {
    context.subscriptions.push(
      vscode.commands.registerCommand(spec.id, () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor || editor.document.languageId !== "todo") {
          return;
        }
        const args: unknown[] = [editor.document.uri.toString()];
        if (spec.needsSelection) {
          args.push(selectionLines(editor));
        }
        return client.sendRequest(ExecuteCommandRequest.type, {
          command: spec.id,
          arguments: args,
        });
      }),
    );
  }

  setupAutoRepeat(context, client, started);
}

export function deactivate(): void {
  // The LanguageClient handles its own disposal via the subscription above.
}
