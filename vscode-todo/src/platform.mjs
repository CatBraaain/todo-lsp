// Pure platform resolution for §利用環境, free of vscode imports so it can
// be exercised directly by `node --test`. extension.ts wires this into the
// extension-context paths.

const PLATFORM_DIRECTORIES = {
  "win32/x64": "win32-x64",
  "linux/x64": "linux-x64",
};

/**
 * The per-platform server binary directory under bin/, named after
 * process.platform / process.arch. Throws the spec error for unsupported
 * platforms (§利用環境 3rd row).
 */
export function platformDirectoryName(platform, arch) {
  const directory = PLATFORM_DIRECTORIES[`${platform}/${arch}`];
  if (directory === undefined) {
    throw new Error(
      `todo-lsp: unsupported platform ${platform}/${arch}. Ship a matching binary under bin/.`,
    );
  }
  return directory;
}

/**
 * The server binary file name: `.exe` only on Windows.
 */
export function serverBinaryName(platform) {
  return platform === "win32" ? "todo-lsp.exe" : "todo-lsp";
}
