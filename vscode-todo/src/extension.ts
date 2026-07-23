import * as path from "node:path";
import * as vscode from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

// Resolve the per-platform server binary under bin/<platform>-<arch>/todo-lsp[.exe].
// The layout matches VS Code's process.platform / process.arch naming and must
// stay in sync with the bin_dir / bin_ext logic in the root justfile.
function serverCommand(context: vscode.ExtensionContext): string {
  const ext = process.platform === "win32" ? ".exe" : "";
  return context.asAbsolutePath(
    path.join("bin", platformDirectoryName(), `todo-lsp${ext}`),
  );
}

function platformDirectoryName(): string {
  switch (`${process.platform}/${process.arch}`) {
    case "win32/x64":
      return "win32-x64";
    case "linux/x64":
      return "linux-x64";
    default:
      throw new Error(
        `todo-lsp: unsupported platform ${process.platform}/${process.arch}. Ship a matching binary under bin/.`,
      );
  }
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
