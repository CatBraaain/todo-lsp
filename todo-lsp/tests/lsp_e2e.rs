//! End-to-end LSP tests (Layer C).
//!
//! Spawns the `todo-lsp` binary as a subprocess and drives it over JSON-RPC on
//! stdio. This verifies the protocol-level contract that unit tests cannot:
//! capability advertisement in `initialize`, the shape of every feature's
//! response (documentSymbol / foldingRange / publishDiagnostics /
//! semanticTokens), and the `initialize` -> `initialized` ordering constraint.
//!
//! The bin path comes from Cargo's auto-injected `CARGO_BIN_EXE_todo-lsp`.

mod harness {
    use std::io::{self, BufRead, BufReader, BufWriter, Write};
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};
    use serde_json::{json, Value};

    const TIMEOUT: Duration = Duration::from_secs(5);

    pub struct LspSession {
        child: Child,
        stdin: BufWriter<ChildStdin>,
        rx: mpsc::Receiver<Value>,
        next_id: i64,
    }

    impl LspSession {
        pub fn spawn() -> Self {
            let bin = std::env::var("CARGO_BIN_EXE_todo-lsp")
                .expect("CARGO_BIN_EXE_todo-lsp not set");
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
        /// notifications (e.g. publishDiagnostics) are skipped.
        pub fn await_response(&self, id: i64) -> Value {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let msg = self
                    .rx
                    .recv_timeout(remaining)
                    .unwrap_or_else(|e| panic!("timeout/error waiting for id={id}: {e}"));
                if msg.get("id") == Some(&json!(id)) {
                    return msg;
                }
            }
        }

        /// Block until a notification with the matching method arrives.
        pub fn await_notification(&self, method: &str) -> Value {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let msg = self
                    .rx
                    .recv_timeout(remaining)
                    .unwrap_or_else(|e| panic!("timeout/error waiting for {method}: {e}"));
                if msg.get("method").and_then(|v| v.as_str()) == Some(method) {
                    return msg;
                }
            }
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
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stdout closed"));
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break; // blank line ends the header block
            }
            if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(rest.trim().parse::<usize>().map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidData, e)
                })?);
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

    // Capability assertions: all four features must be advertised.
    let dsp = &caps["documentSymbolProvider"];
    assert!(
        dsp.as_bool().unwrap_or(false) || dsp.is_object(),
        "documentSymbolProvider missing: {caps}",
    );
    assert!(
        !caps["foldingRangeProvider"].is_null(),
        "foldingRangeProvider missing: {caps}",
    );
    let st = caps["semanticTokensProvider"]
        .as_object()
        .expect("semanticTokensProvider missing");
    let token_types = st["legend"]["tokenTypes"]
        .as_array()
        .expect("legend.tokenTypes missing");
    assert!(
        token_types.iter().any(|t| t.as_str() == Some("type")),
        "heading token type 'type' not in legend: {token_types:?}",
    );
    assert!(
        token_types.iter().any(|t| t.as_str() == Some("decorator")),
        "tag token type 'decorator' not in legend: {token_types:?}",
    );
    assert_eq!(
        st["full"].as_bool(),
        Some(true),
        "semanticTokens full=true must be advertised",
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
    assert!(diags.is_empty(), "clean sample should have 0 diagnostics: {diags:?}");
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

    // 7. semanticTokens/full: 3 headings + 3 tags = 6 tokens (30 u32s).
    let id = s.send_request(
        "textDocument/semanticTokens/full",
        json!({ "textDocument": { "uri": SAMPLE_URI } }),
    );
    let resp = s.await_response(id);
    let data = resp["result"]["data"]
        .as_array()
        .expect("semanticTokens data array");
    assert_eq!(data.len() % 5, 0, "data length must be a multiple of 5");
    assert_eq!(data.len() / 5, 6, "3 headings + 3 tags");
    let to_i64 = |slice: &[Value]| -> Vec<i64> {
        slice.iter().map(|v| v.as_i64().unwrap()).collect()
    };
    // Token 0: row 0 col 0 len 6 type 0 (heading "Inbox:")
    assert_eq!(to_i64(&data[0..5]), vec![0, 0, 6, 0, 0], "first token (Inbox:)");
    // Token 1: row 2 col 11 len 17 type 1 (tag "@done(2024-01-01)")
    assert_eq!(
        to_i64(&data[5..10]),
        vec![2, 11, 17, 1, 0],
        "second token (@done(2024-01-01))",
    );

    // 8. graceful shutdown.
    s.shutdown_and_exit();
}

/// An unclosed tag with no preceding text yields an ERROR node, so the server
/// must publish at least one ERROR-severity diagnostic. This also pins the
/// current diagnostics behavior: MISSING nodes are NOT surfaced (the branch is
/// dormant); only ERROR nodes are reported.
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
        assert_eq!(d["severity"].as_i64().unwrap(), 1, "must be ERROR severity: {d}");
        assert_eq!(d["source"].as_str().unwrap(), "todo");
    }

    s.shutdown_and_exit();
}
