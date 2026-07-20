# モノレポ化: tree-sitter-todo + todo-lsp + vscode-todo

## 進捗状況（再開用）

| Phase | 内容                                 | 状態                                                      |
| ----- | ------------------------------------ | --------------------------------------------------------- |
| 0     | grammar 安定化（メモリ爆発修正）     | ✅ 完了（commit `60f7e85`）                               |
| 1     | レイアウト移行（git mv）             | ✅ 完了（commit `2097e7d`）                               |
| 2     | workspace ルート Cargo.toml          | ✅ 完了（未コミット）                                     |
| 3     | todo-lsp 雛形（FULL sync + did\_\*） | ✅ 完了（initialize 応答確認済み）                        |
| 4     | diagnostics 実装                     | ✅ 完了                                                   |
| 5     | documentSymbol 実装                  | ✅ 完了                                                   |
| 6     | foldingRange 実装                    | ✅ 完了                                                   |
| 7     | vscode-todo 拡張の雛形               | ✅ 完了（npm install + build、tsc クリーン）              |
| 8     | バイナリ起動 E2E                     | ⏳ LSP stdio smoke 成功 / F5 デバッグは Windows 必要      |
| 9     | E2E 検証と vsix パッケージ           | ⏳ Windows 環境で実施（`todo-lsp.exe` が必要）            |

### ここまでの作業ログ

- **2026-07-11 ロールバック**: Phase 1/2 を一度実施したが、その後 `cargo test --workspace` を走らせたところ tree-sitter 側でメモリ爆発が発生。Phase 1/2 の成果物（`git mv` による `tree-sitter-todo/` への移行、workspace ルート `Cargo.toml`/`Cargo.lock`）を全て git で戻し、単一クレート構成に復帰した。
- **Phase 0 完了（commit `60f7e85`）**: メモリ爆発の原因は grammar 側。`source_file` の先頭が `seq(repeat($._newline), ...)` になっており、external scanner トークン `_newline` を先頭で無限に消費できることが parser 生成時の GLR 状態爆発を引き起こしていた。`repeat($._newline)` → `optional($._newline)` に変更し `src/parser.c`/`src/grammar.json` を再生成。`cargo test` が正常終了するようになった。
- **現在の状態（2026-07-18 更新）**: モノレポ完成（workspace = `tree-sitter-todo` + `todo-lsp`、`vscode-todo` 拡張）。`todo-lsp` は diagnostics + documentSymbol + foldingRange を提供し、stdio smoke で応答確認済み（ネストシンボル・3つの REGION 折りたたみ・妥当入力で診断空）。残作業は Windows 環境での F5 デバッグと vsix パッケージ（`todo-lsp.exe` をクロスビルド or Windows ビルドして `vscode-todo/bin/win32-x64/` へ配置）。全成果物は未コミット。

### 2026-07-18 作业ログ（Phase 1〜8 smoke）

- **Phase 1 再完了（commit `2097e7d`）**: ロールバック残骸の空ディレクトリを削除し、grammar 関連ファイルを `tree-sitter-todo/` へ再移行。`cargo test` が通ることを確認（メモリ爆発の再発なし）。
- **Phase 2 完了**: ルート `Cargo.toml`（workspace + `members=["tree-sitter-todo"]` + workspace.package/dependencies）を作成。`Cargo.lock` を `tree-sitter-todo/` からルートへ集約。`cargo test --workspace` green（tree-sitter 0.26.10 に解決）。
- **Phase 3 完了**: `todo-lsp/` クレート雛形（main/parse/lsp/analysis）。`tower-lsp-server 0.23` の API をレジストリソース直読みで確定 — **`ls_types`**（`lsp_types` でなく。`ls-types 0.0.6` の再エクスポート）、**native async fn in trait**（`#[async_trait]` 不要）、**`InitializeResult.offset_encoding` 必須**（`Option<String>`、`"utf-8"` を設定し tree-sitter の byte column を正とする）、`TextDocumentSyncKind::FULL`（newtype struct + 大文字 const）、`Uri` は `Eq+Hash` で HashMap キー化可。initialize 応答確認済み。
- **Phase 4-6 完了**: `analysis.rs` の3関数（diagnostics/document_symbols/folding_ranges）。indent/dedent スキップ、`task_block` 経由のネスト再帰、`task_block.end_position().row - 1` で `task_line` の末尾 `_newline` 分を補正。壊れフィクスチャは `@done(`（text なしなら回復不可で ERROR 生成; `task @broken(` は text が回復して ERROR にならず不採用）。単体テスト4つ green、warning なし。
- **Phase 7 完了**: `vscode-todo/` 拡張雛形。**tmLanguage.yaml が存在しなかった**ため最小の `todo.tmLanguage.json` を新規作成（見出し/コメント/タグ）。`vscode-languageclient 9.x` は `client.start()` が `Promise<void>` を返すため `subscriptions.push(client)` + `client.start()` パターン。npm install + esbuild build 成功、`tsc --noEmit` クリーン。justfile 拡張（build-lsp/copy-binary/build-ext/package/test/dev/generate/test-grammar）。
- **Phase 8（LSP stdio smoke）成功**: todo-lsp バイナリに `initialize→initialized→didOpen→documentSymbol→foldingRange→shutdown` を投入（initialized は initialize 応答後に送る、一気送信だと tower-lsp-server が initialize を Cancel する）。`publishDiagnostics`（妥当なら空）、ネストした `DocumentSymbol`（Inbox/Project/Archive + タスク、MODULE/STRING）、`foldingRange`（3つの REGION）が全て期待通り。**F5 デバッグと vsix 実証は Windows 環境で残作業**。

## Context

現在このリポジトリは、todo 言語の tree-sitter grammar クレート `tree-sitter-todo` 単体で構成されている（Phase 1 で `tree-sitter-todo/` に移動済み）。目標は、これを3層のモノレポに再構成し、todo ファイルを VSCode で IDE 支援（diagnostics / アウトライン / 折りたたみ）できるようにすること:

- `tree-sitter-todo/` — 既存の grammar（Rust lib クレート）。そのまま path 依存として再利用。
- `todo-lsp/` — 新規。`tower-lsp-server`（コミュニティフォーク）で作る LSP server のシングルバイナリ。grammar クレートに依存して AST を得る。
- `vscode-todo/` — 新規。VSCode 拡張。Windows 向け LSP バイナリを同梱し、`vscode-languageclient` で stdio 起動する。TextMate grammar で基本ハイライト、LSP で診断/シンボル/折りたたみを上乗せ。

**確定要件**: MVP の LSP 機能は diagnostics + documentSymbol + folding の3つ。配布ターゲットは v1 では Windows x64 のみ（CI/クロスプラットフォームは将来課題）。

既存資産が活きる: `bindings/rust/lib.rs` が公開する `LANGUAGE` をそのまま LSP 側で使え、`grammar.js` / `src/scanner.c` の external scanner が生成する AST 構造（`heading_block` / `task_line` / `tag`）が3機能の土台になる。

---

## 検証済み API 事実（実装の前提）

**tower-lsp-server（コミュニティフォーク）**: オリジナル `tower-lsp` は2年以上コミットなく実質メンテ停止のため、活動的な `tower-lsp-server` を使う。

- import: `use tower_lsp_server::{Client, LanguageServer, jsonrpc::Result, ls_types::*};` — **`lsp_types` ではなく `ls_types`**（フォーク独自の型モジュール）。
- `params.text_document.uri` は `Uri` 型。`url::Url` ではなく `to_file_path()` は無いので、**HashMap のキー・`publish_diagnostics` の引数としてそのまま使う**（ファイルパス抽出はしない）。
- `did_open` / `did_change` / `did_close`、`document_symbol`（`Result<Option<DocumentSymbolResponse>>`）、`folding_range`（`Result<Option<Vec<FoldingRange>>>`）。
- `publish_diagnostics(uri: Uri, diagnostics: Vec<Diagnostic>, version: Option<i32>)`。
- バージョンは実装時に crates.io で最新 0.23 系を確認して固定（API の形が重要であり、バージョン文字列は実装時確認）。

**tree-sitter 0.26.x Rust API**:

- `parser.set_language(&tree_sitter_todo::LANGUAGE.into())`（grammar クレート自身のテスト `bindings/rust/lib.rs:57` で実証済み）。
- `parser.parse(text, None)` でフル再パース。
- `Node::start_position()/end_position() -> Point{row, column}`（0-based）、`child_by_field_name`、`named_child`、`is_error`/`is_missing`/`has_error`、`utf8_text(source)`。

---

## 1. ディレクトリ構成と移行（Phase 1: 完了）

### 最終ツリー

```
todo-lsp/                                 (リポジトリルート)
├── Cargo.toml                            (新規: workspace ルート)
├── Cargo.lock                            (ルートへ集約・再生成)
├── justfile                              (既存を拡張)
├── README.md  LICENSE                    (新規・最小)
├── tree-sitter-todo/                     (grammar クレート。中身はそのまま)
│   ├── Cargo.toml  grammar.js  GRAMMAR.md  tree-sitter.json
│   └── src/  queries/  bindings/rust/  test/corpus/
├── todo-lsp/                             (新規: LSP server バイナリ)
│   ├── Cargo.toml
│   └── src/  (main.rs, lsp.rs, parse.rs, analysis.rs)
└── vscode-todo/                          (新規: VSCode 拡張)
    ├── package.json  tsconfig.json  esbuild.js  .vscodeignore  language-configuration.json
    ├── syntaxes/todo.tmLanguage.json     (既存 yaml から変換)
    ├── src/extension.ts
    ├── bin/win32-x64/todo-lsp.exe        (ビルド成果物を配置)
    └── test/fixtures/sample.todo
```

### workspace ルート Cargo.toml（Phase 2: 完了）

```toml
[workspace]
resolver = "2"
members = ["tree-sitter-todo"]   # Phase 3 で "todo-lsp" を追加

[workspace.package]
edition = "2021"
version = "0.1.0"
license = "MIT"

[workspace.dependencies]
tree-sitter = "0.26.9"
tree-sitter-todo = { path = "tree-sitter-todo" }
```

（メンバー側は `tree-sitter = { workspace = true }`, `tree-sitter-todo = { workspace = true }` で参照。）

---

## 2. todo-lsp クレート設計（Phase 3〜6）

### Cargo.toml

```toml
[package]
name = "todo-lsp"
version.workspace = true
edition.workspace = true
license.workspace = true
publish = false

[[bin]]
name = "todo-lsp"
path = "src/main.rs"

[dependencies]
tower-lsp-server = "0.23"   # 実装時に crates.io で最新 0.23 系を確認・固定
tokio = { version = "1", features = ["full"] }
tree-sitter = { workspace = true }
tree-sitter-todo = { workspace = true }
serde_json = "1"
anyhow = "1"
```

### テキスト同期は FULL（根拠）

`TextDocumentSyncKind::FULL` を採用。`did_change` では最後の change の全文で置き換え→`parse(text, None)` でフル再パース→診断再計算。理由:

1. 本 grammar はインデント依存の external scanner（`scanner.c` が `indents` スタックを状態保持）。incremental 再パースは `Tree::edit(InputEdit{...})` の正確なバイト/Point 計算が必要で、編集がインデントを変えると scanner のスタック不整合を起こし得る。フル再パース（`old_tree=None`）は scanner をクリーンに再初期化しこのバグ群を回避する。
2. `InputEdit` の byte/UTF-16/Point 計算を不要にする。
3. `.todo` ファイルは小さく（数十〜数百行）、フル再パースはサブミリ秒で IPC コストが支配的。体感遅延なし。

### モジュール構造（データフロー中心・フラット）

- `main.rs` — エントリ + `LspService`/`Server` 配線。
- `parse.rs` — `Document { version, text, tree }` と `parse(text) -> Tree`（パースごとに新規 Parser を生成し、await をまたがない）。`DocumentStore = HashMap<Uri, Document>`。
- `lsp.rs` — `Backend { client: Client, documents: Mutex<DocumentStore> }` + `LanguageServer` 実装。`initialize` で `text_document_sync=FULL`, `document_symbol_provider`, `folding_range_provider` を宣言。`did_open`/`did_change` はパース→保存→診断公開。`did_close` は診断クリア。
- `analysis.rs` — 後述の3関数。

**Mutex 規律**: `std::sync::Mutex` を使うが、ロック内で `.await` しない。ハンドラは「ロック内でデータを取り出し→ロック解放→`.await`（`publish_diagnostics` 等）」の順序を守る。

### 3つの解析アルゴリズム

**document_symbols**: `source_file` の named children を再帰。各ステップで **`kind()=="indent"`/`"dedent"` をスキップ**（最重要。ゼロ幅の named 子ノードのため）。`heading_block` → 最初の named 子 `heading_line`、ラベルは `child_by_field_name("text")` を trim（無ければ "(untitled)"）、`SymbolKind::MODULE`、`task_block` 子へ再帰して `children` に。`task_line` → `SymbolKind::STRING`、ラベルは text フィールド、無ければ最初の `tag` の `name` から（例: "@done"）。ネストは木構造から自然に導出。

**folding_ranges**: `task_block` を持つ各 `heading_block` について1つの `FoldingRange` を生成。`start_line` = `heading_line` の開始行、`end_line` = **`task_block` 子の終了行**（`dedent` の終了行はゼロ幅で1行余分に進むため使わない）。本文なしなら折りたたみなし。`kind=REGION`。

**diagnostics（MVP・控えめ）**: 木全体を走査し `is_error()`/`is_missing()` のノードを `Diagnostic` 化（severity=ERROR, source="todo"）。閉じ `)` のないタグ引数等は既に scanner が ERROR ノードにするため、これだけで構文エラーを実用的に捕捉できる。TextMate と tree-sitter の解釈差による偽陽性を避けるため、文法駆動の ERROR/MISSING のみとする。

### LSP クレートのテスト

`analysis.rs` に `#[cfg(test)]` で、サンプル `.todo` をパース→シンボル数/種別/ネスト、折りたたみ数/行、妥当入力で診断0・壊れ入力で1以上をアサート。

---

## 3. vscode-todo 拡張設計（Phase 7）

### package.json（抜粋）

- `engines.vscode: ^1.85.0`, `main: ./dist/extension.js`, `activationEvents: ["onLanguage:todo"]`。
- `contributes.languages`: id `todo`, extensions `[".todo"]`, configuration `./language-configuration.json`。
- `contributes.grammars`: language `todo`, scopeName `text.todo`, path `./syntaxes/todo.tmLanguage.json`。
- deps: `vscode-languageclient ^9.0.1`。devDeps: `@types/vscode`, `esbuild`, `typescript`, `@vscode/vsce`。

**TextMate 変換**: VSCode は `.tmLanguage.json`（JSON）を期待。既存 `todo.tmLanguage.yaml` を一度 JSON に変換して `syntaxes/todo.tmLanguage.json` として配置（`scopeName: text.todo` は維持）。

### src/extension.ts

`context.asAbsolutePath('bin')` + `platformBin()`（`process.platform==='win32' && process.arch==='x64'` で `win32-x64/todo-lsp.exe`、それ以外は throw）でバイナリパスを解決。`ServerOptions` の `run`/`debug` 共に `TransportKind.stdio` + 実行ファイル。`LanguageClient` を `documentSelector: [{scheme:'file', language:'todo'}]` で生成し `activate` で start、`deactivate` で stop。サーバーが宣言する capability は `vscode-languageclient` が自動接続。

### ビルド/パッケージング

- **esbuild**: `src/extension.ts` → `dist/extension.js`（`platform:'node', format:'cjs', external:['vscode']`）。
- **バイナリ配置**: `vscode-todo/bin/win32-x64/todo-lsp.exe` にコミット（v1 は最も単純で hermetic）。
- **`.vscodeignore`**: `src/**`, `node_modules/**`, `**/*.ts`, `test/**`, 設定類を除外。`dist/**`, `bin/**`, `syntaxes/**` を保持。
- **package**: `cd vscode-todo && vsce package` → `.vsix`。`vsce` が拡張固有の `vscode-todo/README.md` を要求するため最小のものを追加。

### 開発ループ（F5）

`vscode-todo/.vscode/launch.json` の `extensionHost` 構成。`just dev` が debug バイナリを `bin/win32-x64/` にコピー。

---

## 4. ビルド統合（ルート justfile）

既存の `_`/`update-grammar-types`（旧 `prepare`）を残し、以下を追加（プロジェクトは bash 使用のため Git Bash で動作）:

```make
build-lsp:
    cargo build --release -p todo-lsp
copy-binary: build-lsp
    mkdir -p vscode-todo/bin/win32-x64
    cp target/release/todo-lsp.exe vscode-todo/bin/win32-x64/todo-lsp.exe
build-ext:
    cd vscode-todo && npm install && npm run build
package: copy-binary build-ext
    cd vscode-todo && npm run package
test:
    cargo test --workspace
dev:
    cargo build -p todo-lsp
    mkdir -p vscode-todo/bin/win32-x64
    cp target/debug/todo-lsp.exe vscode-todo/bin/win32-x64/todo-lsp.exe
    cd vscode-todo && npm run build
generate:
    cd tree-sitter-todo && tree-sitter generate
test-grammar:
    cd tree-sitter-todo && tree-sitter test
```

tree-sitter CLI は WSL ネイティブで直接実行（Docker 廃止、メモリ爆発は grammar 側 commit `60f7e85` で解決済み）。

---

## 5. 残りの実装フェーズ順（Phase 1〜9）

1. **レイアウト移行（やり直し）** — ロールバック残骸の空 `tree-sitter-todo/`・`vscode-todo/` を削除してから、改めて `git mv` で grammar 関連（`grammar.js`, `grammar.d.ts`, `GRAMMAR.md`, `tree-sitter.json`, `Cargo.toml`, `src/`, `queries/`, `bindings/`, `test/`, `test.txt`）を `tree-sitter-todo/` へ。ルートに `README.md`/`LICENSE` 追加。**完了後すぐ `cargo test` が通ることを確認**（メモリ爆発の再発検知）。
2. **workspace ルート Cargo.toml（やり直し）** — workspace + `members = ["tree-sitter-todo"]` + workspace.package/dependencies。`Cargo.lock` 再生成。`cargo check --workspace` 通過を確認。Phase 3 で `todo-lsp` を `members` に追加。
3. **todo-lsp 雛形** — `Cargo.toml`, `main.rs`, `parse.rs`, `lsp.rs`（FULL sync + symbol/folding provider 宣言、did\_\* 実装、analysis は空スタブ）。workspace の `members` に `todo-lsp` を追加。確認: `cargo build -p todo-lsp`、バイナリに `initialize`/`initialized` を stdin 投入し応答確認。
4. **diagnostics** — ERROR/MISSING 走査。壊れフィクスチャで単体テスト。
5. **documentSymbol** — indent/dedent スキップ付き再帰。ネストをアサート。
6. **foldingRange** — `folding_ranges`。折りたたみ数/行をアサート。（4/5/6 は 3 の雛形に依存しつつ相互独立。）
7. **vscode-todo 雛形** — package.json, language-configuration.json, tmLanguage.json, esbuild, extension.ts。`just build-ext`。
8. **バイナリ起動 E2E** — `just dev` で debug バイナリ配置 → F5 で Extension Development Host → `sample.todo` を開く。
9. **E2E 検証・パッケージ** — アウトライン/折りたたみ/診断を確認。`just package` → `.vsix`。

---

## 6. 検証（Windows で E2E）

1. **grammar 回帰**: `cargo test --workspace` と `tree-sitter test`。corpus 4ファイルが green のまま。
2. **LSP バイナリ stdio スモーク**: `cargo build -p todo-lsp` 後、Content-Length フレームの JSON-RPC を stdin に投入 — `initialize`（`textDocumentSync=FULL`, `documentSymbolProvider`, `foldingRangeProvider` を確認）→ `initialized` → `didOpen`（サンプル）→ `publishDiagnostics`（妥当なら空）→ `documentSymbol`（ネスト）→ `foldingRange`（範囲）→ `shutdown`/`exit`。
3. **拡張開発ホスト**: F5 → `sample.todo` を開き、Outline にヘッダ/タスク階層、折りたたみチエブロン、壊れ行に赤波線を確認。

**サンプル `vscode-todo/test/fixtures/sample.todo`**（LSP 単体テストでも再利用）:

```
# コメント行（extra・構造上無視）
Inbox:
  buy milk
  call mom @done(2024-01-01)
  Project:
    draft spec @priority(high)
    review @done
  wrap up
Archive:
  old task
```

期待: documentSymbol は最上位 `Inbox`(MODULE) に `[buy milk, call mom, Project{draft spec, review}, wrap up]`、最上位 `Archive` に `[old task]`。foldingRange は `Inbox`/`Project`/`Archive` の各 REGION。diagnostics は空。

**壊れフィクスチャ `test/fixtures/broken.todo`**: `task @broken(` → 1以上の ERROR 診断。

---

## 7. リスクと注意

- **external scanner トークンの `repeat` は状態爆発を起こす（発生済み）**: `_newline`/`indent`/`dedent`/`text` はいずれも `src/scanner.c` の external scanner が生成する。これらを `repeat()`/`repeat1()` で直接包むと parser 生成時の GLR 状態が爆発し、`cargo test`（= `src/parser.c` のコンパイル・実行）がメモリ枯渇を起こす。実際に Phase 1/2 実施時に `source_file` の `repeat($._newline)` で発生した（commit `60f7e85` で `optional` に変更して解消）。grammar を編集する際は external トークンの直接繰り返しを避け、`optional()` や別ルールへの押し出しで表現すること。**grammar 変更後は必ず `cargo test` 単体で通ることを先に確認**してから LSP 側（Phase 3〜6）に進むこと。
- **`indent`/`dedent` は named・ゼロ幅の子ノード**。named-children 走査で必ずスキップしないと幽霊シンボル/空範囲が生じる。
- **FULL sync の前提**: クライアントが全文1件の change を送る。`did_change` は `content_changes.last().text` を取る。VSCode は唯一のクライアントなので v1 は問題なし。
- **scanner のエラー回復** (`scanner.c:207` の `error_recovery_mode`): 壊れ入力での診断範囲が近似になり得る。
- **TextMate vs tree-sitter の解釈差**: ヘッダ検出（`time is 12:30` のコロン等）で差異あり。v1 では一致を求めず README に明記。
- **`Uri` 型**: `to_file_path()` なし。不透明なキーとしてのみ使用。
- **`std::sync::Mutex` を `.await` にまたがせない**: 全ハンドラで「ロック内で取り出し→解放→await」を守る。
- **`vsce` の README 要件**: 拡張固有の `vscode-todo/README.md` が無いと package で警告/拒否される。

---

## クリティカルファイル

- `C:\Projects\todo-lsp\Cargo.toml` — workspace ルート（作成済み）
- `C:\Projects\todo-lsp\todo-lsp\src\lsp.rs` — 新規。Backend + LanguageServer 実装、FULL sync、capability 宣言
- `C:\Projects\todo-lsp\todo-lsp\src\analysis.rs` — 新規。3解析と indent/dedent スキップロジック
- `C:\Projects\todo-lsp\todo-lsp\src\parse.rs` — 新規。Document モデル + パース
- `C:\Projects\todo-lsp\vscode-todo\src\extension.ts` — 新規。LanguageClient 配線 + 同梱バイナリ解決
- `C:\Projects\todo-lsp\bindings\rust\lib.rs` — 既存。`LANGUAGE` を公開（編集不要、path 依存で消費）
