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
import { assertNode20OrLater } from "./nodeVersion.mjs";

let client: LanguageClient | undefined;

function serverOptions(context: vscode.ExtensionContext): ServerOptions {
  const server = context.asAbsolutePath(
    path.join("server", "node_modules", "@todo-lsp", "todo-lsp", "dist", "main.js"),
  );
  return {
    run: { command: process.execPath, args: [server], transport: TransportKind.stdio },
    debug: { command: process.execPath, args: [server], transport: TransportKind.stdio },
  };
}

function selectionLines(editor: vscode.TextEditor): number[] {
  const lines = new Set<number>();
  for (const selection of editor.selections) {
    for (let line = selection.start.line; line <= selection.end.line; line++) {
      lines.add(line);
    }
  }
  return [...lines].sort((a, b) => a - b);
}

function setupAutoRepeat(context: vscode.ExtensionContext, languageClient: LanguageClient): void {
  const fire = (): unknown => {
    const editor = vscode.window.activeTextEditor;
    const enabled = vscode.workspace
      .getConfiguration("todo-language")
      .get<boolean>("repeatTask.autoRepeat", true);
    if (!editor || !shouldAutoRepeat(enabled, editor.document.languageId)) {
      return undefined;
    }
    return languageClient.sendRequest(ExecuteCommandRequest.type, {
      command: "todo-language.repeatTasks",
      arguments: [editor.document.uri.toString()],
    });
  };

  const triggers = setupAutoRepeatTriggers({
    onStartup: (callback: () => void) => callback(),
    onEditorSwitch: (callback: () => void) => {
      context.subscriptions.push(vscode.window.onDidChangeActiveTextEditor(callback));
    },
    now: () => new Date(),
    setTimeout: (callback: () => void, ms: number) => setTimeout(callback, ms),
    clearTimeout: (timer: NodeJS.Timeout) => clearTimeout(timer),
    fire,
  });
  context.subscriptions.push({ dispose: () => triggers.dispose() });
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  try {
    assertNode20OrLater(process.versions.node);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    void vscode.window.showErrorMessage(message);
    throw error;
  }

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "todo" }],
  };
  client = new LanguageClient("todo-lsp", "Todo LSP", serverOptions(context), clientOptions);
  await client.start();

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
        return client?.sendRequest(ExecuteCommandRequest.type, {
          command: spec.id,
          arguments: args,
        });
      }),
    );
  }

  setupAutoRepeat(context, client);
}

export function deactivate(): Thenable<void> | undefined {
  const runningClient = client;
  client = undefined;
  return runningClient?.stop();
}
