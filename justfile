_:
  @just --list --unsorted

# Typecheck, then run all tests (workspace packages including vscode-todo,
# VSIX verification, tree-sitter grammar).
# Each workspace package's test script builds itself before running.
test:
  npm run typecheck
  npm test
  npm run test:grammar
  cd vscode-todo && npm run package
  cd vscode-todo && npm run test:vsix

# Assumes dependencies are installed beforehand (npm install at the root
# covers every workspace).
# Build the workspace packages and extension, then take SPEC screenshots
# into screenshots/dist/.
sc:
  npm run build
  bash screenshots/run.sh

# Build everything and package the VSIX.
package:
  npm run generate:grammar
  npm run build
  cd vscode-todo && npm run package
