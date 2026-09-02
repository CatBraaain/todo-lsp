//! End-to-end LSP tests (Layer C).
//!
//! Spawns the `todo-lsp` binary as a subprocess and drives it over JSON-RPC on
//! stdio. This verifies the protocol-level contract that unit tests cannot:
//! capability advertisement in `initialize`, the shape of every feature's
//! response (documentSymbol / foldingRange / publishDiagnostics /
//! semanticTokens), direct command execution, and the `initialize` ->
//! `initialized` ordering constraint.
//!
//! The bin path comes from Cargo's auto-injected `CARGO_BIN_EXE_todo-lsp`.

mod harness {
    use serde_json::{json, Value};
    use std::io::{self, BufRead, BufReader, BufWriter, Write};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    const TIMEOUT: Duration = Duration::from_secs(5);

    pub struct LspSession {
        child: Child,
        stdin: BufWriter<ChildStdin>,
        rx: mpsc::Receiver<Value>,
        next_id: i64,
        /// Params of the last `workspace/applyEdit` request the server sent.
        pub last_apply_edit: Option<Value>,
    }

    impl LspSession {
        pub fn spawn() -> Self {
            let bin =
                std::env::var("CARGO_BIN_EXE_todo-lsp").expect("CARGO_BIN_EXE_todo-lsp not set");
            let mut child = Command::new(&bin)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap_or_else(|e| panic!("failed to spawn {bin}: {e}"));

            let stdin = BufWriter::new(child.stdin.take().unwrap());
            let stdout = BufReader::new(child.stdout.take().unwrap());

            // Dedicated reader thread: continuously drains stdout into a
            // channel. Without this the server can block on a full stdout pipe
            // buffer while the test writes the next request, deadlocking both.
            // recv_timeout drives consumption so a hung server fails the test
            // rather than hanging it forever.
            let (tx, rx) = mpsc::channel::<Value>();
            thread::spawn(move || {
                let mut stdout = stdout;
                loop {
                    match read_msg(&mut stdout) {
                        Ok(v) => {
                            if tx.send(v).is_err() {
                                break; // test dropped the receiver
                            }
                        }
                        Err(_) => break, // EOF or parse error: server closed
                    }
                }
            });

            Self {
                child,
                stdin,
                rx,
                next_id: 1,
                last_apply_edit: None,
            }
        }

        pub fn send_request(&mut self, method: &str, params: Value) -> i64 {
            let id = self.next_id;
            self.next_id += 1;
            let msg = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            });
            write_msg(&mut self.stdin, &msg);
            self.stdin.flush().unwrap();
            id
        }

        pub fn send_notification(&mut self, method: &str, params: Value) {
            let msg = json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            });
            write_msg(&mut self.stdin, &msg);
            self.stdin.flush().unwrap();
        }

        /// Block until the response with the matching id arrives. Out-of-order
        /// notifications (e.g. publishDiagnostics) are skipped. Server-to-
        /// client requests (`workspace/applyEdit`) are answered inline so the
        /// server can finish its own response.
        pub fn await_response(&mut self, id: i64) -> Value {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let msg = self
                    .rx
                    .recv_timeout(remaining)
                    .unwrap_or_else(|e| panic!("timeout/error waiting for id={id}: {e}"));
                if self.answer_server_request(&msg) {
                    continue;
                }
                if msg.get("id") == Some(&json!(id)) && msg.get("result").is_some() {
                    return msg;
                }
            }
        }

        /// Block until a notification with the matching method arrives.
        pub fn await_notification(&mut self, method: &str) -> Value {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let msg = self
                    .rx
                    .recv_timeout(remaining)
                    .unwrap_or_else(|e| panic!("timeout/error waiting for {method}: {e}"));
                if self.answer_server_request(&msg) {
                    continue;
                }
                if msg.get("method").and_then(|v| v.as_str()) == Some(method) {
                    return msg;
                }
            }
        }

        /// Answer a server-to-client request (`workspace/applyEdit`). Returns
        /// whether the message was one (and was answered).
        fn answer_server_request(&mut self, msg: &Value) -> bool {
            if msg.get("method").and_then(|v| v.as_str()) != Some("workspace/applyEdit")
                || msg.get("id").is_none()
            {
                return false;
            }
            self.last_apply_edit = msg.get("params").cloned();
            let response = json!({
                "jsonrpc": "2.0",
                "id": msg["id"],
                "result": { "applied": true },
            });
            write_msg(&mut self.stdin, &response);
            self.stdin.flush().unwrap();
            true
        }

        pub fn shutdown_and_exit(&mut self) {
            let id = self.send_request("shutdown", json!(null));
            let _ = self.await_response(id);
            self.send_notification("exit", json!(null));
            // The server exits from the `exit` notification; reap it. On hang,
            // Drop will kill as a last resort.
            let _ = self.child.wait();
        }
    }

    impl Drop for LspSession {
        fn drop(&mut self) {
            // Best-effort cleanup so a panicking test does not leak a process.
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn write_msg<W: Write>(w: &mut W, msg: &Value) {
        let body = serde_json::to_vec(msg).expect("serialize msg");
        write!(w, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        w.write_all(&body).unwrap();
    }

    fn read_msg<R: BufRead>(r: &mut R) -> io::Result<Value> {
        let mut content_length: Option<usize> = None;
        let mut line = String::new();
        loop {
            line.clear();
            if r.read_line(&mut line)? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stdout closed",
                ));
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break; // blank line ends the header block
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(
                    rest.trim()
                        .parse::<usize>()
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
                );
            }
            // Other headers (Content-Type, etc.) are ignored.
        }
        let len = content_length
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no Content-Length"))?;
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf)?;
        Ok(serde_json::from_slice(&buf)?)
    }
}

use harness::LspSession;
use serde_json::{json, Value};

const SAMPLE_URI: &str = "file:///tmp/sample.todo";

/// Identical to `analysis::tests::SAMPLE` and `vscode-todo/test/fixtures/sample.todo`.
const SAMPLE_TEXT: &str = "\
Inbox:
  buy milk
  call mom @done(2024-01-01)
  Project:
    draft spec @priority(high)
    review @done
  wrap up
Archive:
  old task
";

/// Drives the full handshake against a clean document and asserts every
/// capability's response shape, including `semanticTokens/full`.
#[test]
fn full_handshake_with_clean_sample() {
    let mut s = LspSession::spawn();

    // 1. initialize. CRITICAL: read the response BEFORE sending `initialized` —
    //    tower-lsp-server cancels initialize if a notification precedes its
    //    response (Phase 8 lesson).
    let init_id = s.send_request(
        "initialize",
        json!({ "processId": null, "rootUri": null, "capabilities": {} }),
    );
    let init_resp = s.await_response(init_id);
    let caps: &Value = &init_resp["result"]["capabilities"];

    // Capability assertions: document features must be advertised.
    let dsp = &caps["documentSymbolProvider"];
    assert!(
        dsp.as_bool().unwrap_or(false) || dsp.is_object(),
        "documentSymbolProvider missing: {caps}",
    );
    assert!(
        !caps["foldingRangeProvider"].is_null(),
        "foldingRangeProvider missing: {caps}",
    );
    assert!(
        !caps["documentLinkProvider"].is_null(),
        "documentLinkProvider missing: {caps}",
    );
    let st = caps["semanticTokensProvider"]
        .as_object()
        .expect("semanticTokensProvider missing");
    let token_types = st["legend"]["tokenTypes"]
        .as_array()
        .expect("legend.tokenTypes missing");
    for expected in [
        "todo-line",
        "todo-heading-content",
        "todo-heading-symbol",
        "todo-tag",
        "start-tag",
        "due-tag",
        "repeat-tag",
    ] {
        assert!(
            token_types.iter().any(|t| t.as_str() == Some(expected)),
            "token type {expected:?} not in legend: {token_types:?}",
        );
    }
    let token_mods = st["legend"]["tokenModifiers"]
        .as_array()
        .expect("legend.tokenModifiers missing");
    for expected in ["italic", "queue1", "past", "future", "invalid", "valid"] {
        assert!(
            token_mods.iter().any(|m| m.as_str() == Some(expected)),
            "token modifier {expected:?} not in legend: {token_mods:?}",
        );
    }
    assert_eq!(
        st["full"].as_bool(),
        Some(true),
        "semanticTokens full=true must be advertised",
    );
    // §フォーマット. Commands are registered by the VS Code extension, which
    // supplies the active document URI and selected lines before sending the
    // direct workspace/executeCommand request.
    assert_eq!(caps["documentFormattingProvider"].as_bool(), Some(true));
    assert!(
        caps["executeCommandProvider"].is_null(),
        "executeCommandProvider would duplicate the VS Code command registration: {caps}",
    );

    // 2. initialized (after the initialize response is consumed).
    s.send_notification("initialized", json!({}));

    // 3. didOpen with the clean sample.
    s.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": SAMPLE_URI,
                "languageId": "todo",
                "version": 1,
                "text": SAMPLE_TEXT,
            }
        }),
    );

    // 4. publishDiagnostics: the sample is clean, so zero diagnostics.
    let diag_msg = s.await_notification("textDocument/publishDiagnostics");
    let diags = diag_msg["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diags.is_empty(),
        "clean sample should have 0 diagnostics: {diags:?}"
    );
    assert_eq!(diag_msg["params"]["uri"].as_str().unwrap(), SAMPLE_URI);

    // 5. documentSymbol: top-level Inbox + Archive, both MODULE (kind 2).
    let id = s.send_request(
        "textDocument/documentSymbol",
        json!({ "textDocument": { "uri": SAMPLE_URI } }),
    );
    let resp = s.await_response(id);
    let syms = resp["result"].as_array().expect("symbols array");
    assert_eq!(syms.len(), 2, "top-level: Inbox + Archive");
    assert_eq!(syms[0]["name"].as_str().unwrap(), "Inbox");
    assert_eq!(syms[0]["kind"].as_i64().unwrap(), 2); // MODULE
    assert_eq!(syms[1]["name"].as_str().unwrap(), "Archive");

    // 6. foldingRange: Inbox / Project / Archive = 3 ranges; Inbox spans 0..6.
    let id = s.send_request(
        "textDocument/foldingRange",
        json!({ "textDocument": { "uri": SAMPLE_URI } }),
    );
    let resp = s.await_response(id);
    let folds = resp["result"].as_array().expect("folds array");
    assert_eq!(folds.len(), 3, "Inbox / Project / Archive folds");
    assert_eq!(folds[0]["startLine"].as_i64().unwrap(), 0);
    assert_eq!(folds[0]["endLine"].as_i64().unwrap(), 6);

    // 7. semanticTokens/full: 2 headings x2 + 1 tag + 2 gray lines + archive.
    let id = s.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": SAMPLE_URI } }),
    );
    let resp = s.await_response(id);
    let data = resp["result"]["data"]
        .as_array()
        .expect("semanticTokens data array");
    assert_eq!(data.len() % 5, 0, "data length must be a multiple of 5");
    assert_eq!(
        data.len() / 5,
        8,
        "2 headings x2 + 1 tag + 2 gray + archive"
    );
    let to_i64 =
        |slice: &[Value]| -> Vec<i64> { slice.iter().map(|v| v.as_i64().unwrap()).collect() };
    // Indices: 0=todo-line, 1=todo-heading-content, 2=todo-heading-symbol,
    // 3=todo-tag. Delta-encoded (delta_line, delta_start, len, type, mods).
    // Lines with @done in their tag column are whole-line gray (todo-line).
    let expected: [i64; 40] = [
        0, 0, 5, 1, 0, // L0 "Inbox" (heading-content)
        0, 5, 1, 2, 0, // L0 ":" (heading-symbol)
        2, 0, 28, 0, 0, // L2 "  call mom @done(2024-01-01)" (gray)
        1, 2, 7, 1, 0, // L3 "Project" (heading-content)
        0, 7, 1, 2, 0, // L3 ":" (heading-symbol)
        1, 15, 15, 3, 0, // L4 "@priority(high)" (tag)
        1, 0, 16, 0, 0, // L5 "    review @done" (gray)
        2, 0, 8, 0, 0, // L7 "Archive:" (gray)
    ];
    assert_eq!(to_i64(data), expected.to_vec());

    // 8. documentLink: the sample has no URLs, so an empty link list.
    let id = s.send_request(
        "textDocument/documentLink",
        json!({ "textDocument": { "uri": SAMPLE_URI } }),
    );
    let resp = s.await_response(id);
    assert!(
        resp["result"].as_array().unwrap().is_empty(),
        "sample has no document links: {resp}"
    );

    // 9. graceful shutdown.
    s.shutdown_and_exit();
}

/// An unclosed tag with no preceding text yields an ERROR node, so the server
/// must publish at least one ERROR-severity diagnostic. Both ERROR nodes and
/// non-traversable MISSING descendants are reported as ERROR-severity
/// diagnostics (see `analysis::diagnostics`).
#[test]
fn broken_input_publishes_error_diagnostics() {
    let mut s = LspSession::spawn();

    let init_id = s.send_request(
        "initialize",
        json!({ "processId": null, "rootUri": null, "capabilities": {} }),
    );
    let _ = s.await_response(init_id);
    s.send_notification("initialized", json!({}));

    s.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": SAMPLE_URI,
                "languageId": "todo",
                "version": 1,
                "text": "@done(",
            }
        }),
    );

    let diag_msg = s.await_notification("textDocument/publishDiagnostics");
    let diags = diag_msg["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(!diags.is_empty(), "broken input must produce diagnostics");
    for d in diags {
        assert_eq!(
            d["severity"].as_i64().unwrap(),
            1,
            "must be ERROR severity: {d}"
        );
        assert_eq!(d["source"].as_str().unwrap(), "todo");
    }

    s.shutdown_and_exit();
}

/// §フォーマット: a formatting request returns a single full-document
/// replace TextEdit with the formatted text.
#[test]
fn formatting_returns_full_document_edit() {
    let mut s = LspSession::spawn();
    let init_id = s.send_request(
        "initialize",
        json!({ "processId": null, "rootUri": null, "capabilities": {} }),
    );
    let _ = s.await_response(init_id);
    s.send_notification("initialized", json!({}));

    let messy = "A:\n        deep   child\n\n\nB:\n  b\n";
    s.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": SAMPLE_URI, "languageId": "todo", "version": 1, "text": messy,
            }
        }),
    );
    let _ = s.await_notification("textDocument/publishDiagnostics");

    let id = s.send_request(
        "textDocument/formatting",
        json!({
            "textDocument": { "uri": SAMPLE_URI },
            "options": { "tabSize": 4, "insertSpaces": true },
        }),
    );
    let resp = s.await_response(id);
    let edits = resp["result"].as_array().expect("formatting edits array");
    assert_eq!(edits.len(), 1, "exactly one full-document edit: {edits:?}");
    assert_eq!(edits[0]["range"]["start"]["line"], 0);
    assert_eq!(edits[0]["range"]["start"]["character"], 0);
    // Rule 1: indent normalized; rule 3: blank run collapsed; rule 4: blank
    // between the top-level heading blocks.
    assert_eq!(
        edits[0]["newText"].as_str().unwrap(),
        "A:\n    deep child\n\nB:\n    b\n"
    );

    s.shutdown_and_exit();
}

/// §コマンド: `todo-language.toggleDone` applies a workspace edit that adds
/// `@done(実行日)` to the selected line; the server sends `workspace/applyEdit`
/// and the client-applied result flows back as the command result.
#[test]
fn execute_command_toggle_done_applies_edit() {
    let mut s = LspSession::spawn();
    let init_id = s.send_request(
        "initialize",
        json!({ "processId": null, "rootUri": null, "capabilities": {} }),
    );
    let _ = s.await_response(init_id);
    s.send_notification("initialized", json!({}));

    let text = "task a\ntask b\n";
    s.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": SAMPLE_URI, "languageId": "todo", "version": 1, "text": text,
            }
        }),
    );
    let _ = s.await_notification("textDocument/publishDiagnostics");

    let id = s.send_request(
        "workspace/executeCommand",
        json!({
            "command": "todo-language.toggleDone",
            "arguments": [SAMPLE_URI, [1]],
        }),
    );
    let resp = s.await_response(id);
    assert!(
        resp.get("result").is_some(),
        "command must not error: {resp}"
    );

    let apply = s.last_apply_edit.as_ref().expect("applyEdit was sent");
    let changes = apply["edit"]["changes"][SAMPLE_URI]
        .as_array()
        .expect("changes for the document");
    assert_eq!(changes.len(), 1);
    let new_text = changes[0]["newText"].as_str().unwrap();
    assert!(
        new_text.starts_with("task a\ntask b @done("),
        "got {new_text:?}"
    );
    assert!(
        new_text.ends_with(")\n"),
        "実行日 must be a YYYY-MM-DD argument: {new_text:?}"
    );

    s.shutdown_and_exit();
}

/// §アーカイブ: `todo-language.archive` moves an all-gray top-level block
/// under a newly created root-level `Archive:` heading.
#[test]
fn execute_command_archive_creates_archive_heading() {
    let mut s = LspSession::spawn();
    let init_id = s.send_request(
        "initialize",
        json!({ "processId": null, "rootUri": null, "capabilities": {} }),
    );
    let _ = s.await_response(init_id);
    s.send_notification("initialized", json!({}));

    let text = "keep\nold @done\n";
    s.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": SAMPLE_URI, "languageId": "todo", "version": 1, "text": text,
            }
        }),
    );
    let _ = s.await_notification("textDocument/publishDiagnostics");

    let id = s.send_request(
        "workspace/executeCommand",
        json!({
            "command": "todo-language.archive",
            "arguments": [SAMPLE_URI, [1]],
        }),
    );
    let resp = s.await_response(id);
    assert!(resp.get("result").is_some());

    let apply = s.last_apply_edit.as_ref().expect("applyEdit was sent");
    let new_text = apply["edit"]["changes"][SAMPLE_URI][0]["newText"]
        .as_str()
        .unwrap();
    assert_eq!(new_text, "keep\nArchive:\n    old @done\n");

    s.shutdown_and_exit();
}

/// §インデント: `todo-language.indent` on a selection shifts lines one level
/// (4 spaces) deeper.
#[test]
fn execute_command_indent_writes_four_spaces() {
    let mut s = LspSession::spawn();
    let init_id = s.send_request(
        "initialize",
        json!({ "processId": null, "rootUri": null, "capabilities": {} }),
    );
    let _ = s.await_response(init_id);
    s.send_notification("initialized", json!({}));

    let text = "a\nb\n";
    s.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": SAMPLE_URI, "languageId": "todo", "version": 1, "text": text,
            }
        }),
    );
    let _ = s.await_notification("textDocument/publishDiagnostics");

    let id = s.send_request(
        "workspace/executeCommand",
        json!({
            "command": "todo-language.indent",
            "arguments": [SAMPLE_URI, [0]],
        }),
    );
    let resp = s.await_response(id);
    assert!(resp.get("result").is_some());

    let apply = s.last_apply_edit.as_ref().expect("applyEdit was sent");
    let new_text = apply["edit"]["changes"][SAMPLE_URI][0]["newText"]
        .as_str()
        .unwrap();
    assert_eq!(new_text, "    a\nb\n");

    s.shutdown_and_exit();
}

/// §診断 lifecycle: a full-content change refreshes the diagnostics, and
/// closing the document clears them.
#[test]
fn diagnostics_update_on_change_and_clear_on_close() {
    let mut s = LspSession::spawn();
    let init_id = s.send_request(
        "initialize",
        json!({ "processId": null, "rootUri": null, "capabilities": {} }),
    );
    let _ = s.await_response(init_id);
    s.send_notification("initialized", json!({}));

    let open = |s: &mut LspSession, version, text| {
        s.send_notification(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": SAMPLE_URI, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        );
    };

    s.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": SAMPLE_URI, "languageId": "todo", "version": 1, "text": "@done(",
            }
        }),
    );
    let broken = s.await_notification("textDocument/publishDiagnostics");
    assert!(
        !broken["params"]["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty(),
        "broken input must publish diagnostics"
    );

    // Full change to a clean document -> empty diagnostics.
    open(&mut s, 2, "fixed line\n");
    let fixed = s.await_notification("textDocument/publishDiagnostics");
    assert_eq!(
        fixed["params"]["diagnostics"].as_array().unwrap().len(),
        0,
        "clean document must clear diagnostics"
    );

    // Close -> diagnostics cleared for the document.
    s.send_notification(
        "textDocument/didClose",
        json!({ "textDocument": { "uri": SAMPLE_URI } }),
    );
    let closed = s.await_notification("textDocument/publishDiagnostics");
    assert_eq!(
        closed["params"]["diagnostics"].as_array().unwrap().len(),
        0,
        "closing must clear diagnostics"
    );

    s.shutdown_and_exit();
}

/// §表示 URL: closed http/https/ftp spans are provided as LSP DocumentLinks,
/// which VS Code renders as clickable links.
#[test]
fn document_link_returns_closed_url_target() {
    let mut s = LspSession::spawn();
    let init_id = s.send_request(
        "initialize",
        json!({ "processId": null, "rootUri": null, "capabilities": {} }),
    );
    let _ = s.await_response(init_id);
    s.send_notification("initialized", json!({}));

    s.send_notification(
        "textDocument/didOpen",
        json!({
            "textDocument": {
                "uri": SAMPLE_URI,
                "languageId": "todo",
                "version": 1,
                "text": "see <https://example.com>\n",
            }
        }),
    );
    let _ = s.await_notification("textDocument/publishDiagnostics");

    let id = s.send_request(
        "textDocument/documentLink",
        json!({ "textDocument": { "uri": SAMPLE_URI } }),
    );
    let resp = s.await_response(id);
    let links = resp["result"].as_array().expect("document links array");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["target"].as_str(), Some("https://example.com"));
    assert_eq!(links[0]["range"]["start"]["line"], 0);
    assert_eq!(links[0]["range"]["start"]["character"], 4);

    s.shutdown_and_exit();
}
