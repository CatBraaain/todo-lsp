// §コマンド: the todo-language.* commands this extension registers. Every
// command takes the active document URI; all but Repeat Tasks also take the
// selected line numbers (the union of the editor's selections).
export const COMMAND_SPECS = [
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
