import * as path from "node:path";
import * as vscode from "vscode";
import {
  ExecuteCommandRequest,
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";
import {
  EDITOR_SWITCH_DELAY_MS,
  msToNextMinute,
  shouldAutoRepeat,
} from "./autoRepeatCore.mjs";

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

// The todo-language.* commands contributed by the server. Every command takes
// the document URI; all but Repeat Tasks also take the selected line numbers
// (the union of the editor's selections).
interface CommandSpec {
  id: string;
  needsSelection: boolean;
}

const COMMAND_SPECS: CommandSpec[] = [
  { id: "todo-language.toggleDone", needsSelection: true },
  { id: "todo-language.toggleCancelled", needsSelection: true },
  { id: "todo-language.toggleStart", needsSelection: true },
  { id: "todo-language.toggleDue", needsSelection: true },
  { id: "todo-language.toggleQueue", needsSelection: true },
  { id: "todo-language.toggleQueueUnshift", needsSelection: true },
  { id: "todo-language.toggleWaiting", needsSelection: true },
  { id: "todo-language.togglePending", needsSelection: true },
  { id: "todo-language.toggleHide", needsSelection: true },
  { id: "todo-language.toggleRepeat", needsSelection: true },
  { id: "todo-language.indent", needsSelection: true },
  { id: "todo-language.dedent", needsSelection: true },
  { id: "todo-language.repeatTasks", needsSelection: false },
  { id: "todo-language.archive", needsSelection: true },
  { id: "todo-language.unarchive", needsSelection: true },
];

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
// after the active editor switches, and every minute at second 0.
function setupAutoRepeat(
  context: vscode.ExtensionContext,
  client: LanguageClient,
  started: Promise<void>,
): void {
  let running = false;

  const trigger = (): void => {
    if (running) {
      return;
    }
    const editor = vscode.window.activeTextEditor;
    const enabled = vscode.workspace
      .getConfiguration("todo-language")
      .get<boolean>("repeatTask.autoRepeat", true);
    if (!editor || !shouldAutoRepeat(enabled, editor.document.languageId)) {
      return;
    }
    running = true;
    void client
      .sendRequest(ExecuteCommandRequest.type, {
        command: "todo-language.repeatTasks",
        arguments: [editor.document.uri.toString()],
      })
      .catch(() => {
        // A failed repeat run (e.g. server restarting) retries on the next
        // trigger; nothing to surface to the user.
      })
      .finally(() => {
        running = false;
      });
  };

  // 拡張の起動時.
  void started.then(trigger);

  // アクティブエディタの切替から約 0.5 秒後.
  let switchTimer: NodeJS.Timeout | undefined;
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(() => {
      clearTimeout(switchTimer);
      switchTimer = setTimeout(trigger, EDITOR_SWITCH_DELAY_MS);
    }),
  );

  // 毎分 0 秒.
  let minuteTimer: NodeJS.Timeout | undefined;
  const scheduleMinute = (): void => {
    minuteTimer = setTimeout(() => {
      trigger();
      scheduleMinute();
    }, msToNextMinute(new Date()));
  };
  scheduleMinute();

  context.subscriptions.push({
    dispose: () => {
      clearTimeout(switchTimer);
      clearTimeout(minuteTimer);
    },
  });
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
