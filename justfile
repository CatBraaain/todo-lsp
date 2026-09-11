_:
  @just --list --unsorted

# Typecheck, then run all tests (vscode-todo, workspace packages,
# VSIX verification, tree-sitter grammar).
# Each workspace package's test script builds itself before running.
test:
  npm run typecheck
  npm test
  npm run test:grammar
  cd vscode-todo && npm run typecheck
  cd vscode-todo && npm test
  cd vscode-todo && npm run package
  cd vscode-todo && npm run test:vsix

# Install dependencies, build the workspace packages and extension, then take
# SPEC screenshots into screenshots/dist/.
sc:
  npm ci
  npm run build
  cd vscode-todo && npm ci && npm run build
  npm ci --prefix screenshots
  bash screenshots/run.sh

# Build everything and package the VSIX.
package:
  npm run generate:grammar
  npm run build
  cd vscode-todo && npm ci && npm run package
