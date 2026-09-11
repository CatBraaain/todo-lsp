_:
  @just --list --unsorted

# Typecheck, then run all tests (vscode-todo, workspace packages, tree-sitter grammar).
# Each workspace package's test script builds itself before running.
test:
  npm run typecheck
  cd vscode-todo && npm run typecheck
  cd vscode-todo && npm test
  npm test
  npm run test:grammar

# Build the extension, then take SPEC screenshots into screenshots/<timestamp>/.
screenshot:
  cd vscode-todo && npm run build
  npm ci --prefix scripts/screenshot
  bash scripts/screenshot/run.sh

# Build everything, package the VSIX, verify it, and take SPEC screenshots.
package:
  npm run generate:grammar
  npm run build
  cd vscode-todo && npm ci && npm run package && npm run test:vsix
  just screenshot
