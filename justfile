_:
  @just --list --unsorted

build:
  npm run build
  cd vscode-todo && npm ci && npm run build

test:
  cd vscode-todo && npm test
  npm test
  npm run test:grammar

package:
  just build
  cd vscode-todo && npm run package && npm run test:vsix

typecheck:
  npm run typecheck
  cd vscode-todo && npm run typecheck

generate-grammar:
  npm run generate:grammar

test-grammar:
  npm run test:grammar
