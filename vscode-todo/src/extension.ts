import * as path from "node:path";
import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

// v1 ships a Windows x64 server binary only.
function serverCommand(context: vscode.ExtensionContext): string {
  if (process.platform === "win32" && process.arch === "x64") {
    return context.asAbsolutePath(path.join("bin", "win32-x64", "todo-lsp.exe"));
  }
  throw new Error(
    `todo-lsp: unsupported platform ${process.platform}/${process.arch}. v1 ships a Windows x64 binary only.`,
  );
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
  client.start();
}

export function deactivate(): void {
  // The LanguageClient handles its own disposal via the subscription above.
}
