// Pure tab-size resolution (§タブサイズ), free of vscode imports so it can be
// exercised directly by `node --test`. extension.ts injects the vscode
// configuration lookup.

/**
 * The document's effective tab size (§タブサイズ): the editor's resolved
 * option, falling back to the `editor.tabSize` setting and then 4. A value
 * that is not a positive number is treated as unavailable (§タブサイズ).
 */
export function effectiveTabSize(editor, settingTabSize) {
  const option = editor.options.tabSize;
  if (isPositiveNumber(option)) return option;
  const setting = settingTabSize();
  return isPositiveNumber(setting) ? setting : 4;
}

function isPositiveNumber(value) {
  return typeof value === "number" && Number.isFinite(value) && value > 0;
}
