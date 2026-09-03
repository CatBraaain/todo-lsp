use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use chrono::Local;
use tower_lsp_server::jsonrpc::Error as RpcError;
use tower_lsp_server::ls_types::*;
use tower_lsp_server::{jsonrpc::Result, Client, LanguageServer};

use crate::analysis;
use crate::archive;
use crate::commands::{reindent, toggle, Toggle};
use crate::edits::{line_edits, token_delta_edits};
use crate::format::format_document;
use crate::parse::{parse, Document, DocumentStore};
use crate::repeat::repeat_tasks;

/// Last semantic-token result per document (§表示 色付けの更新).
struct TokenResult {
    result_id: String,
    data: Vec<SemanticToken>,
}

pub struct Backend {
    client: Client,
    documents: Mutex<DocumentStore>,
    /// §表示 色付けの更新: the client advertised
    /// `workspace.semanticTokens.refreshSupport` in `initialize`.
    refresh_supported: AtomicBool,
    /// Set once a command edit was applied; the next `didChange` (the
    /// client-applied edit flowing back) sends exactly one refresh and
    /// clears the flag. Any later `didChange` never refreshes.
    pending_refresh: AtomicBool,
    token_results: Mutex<HashMap<Uri, TokenResult>>,
    next_result_id: AtomicU64,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(DocumentStore::new()),
            refresh_supported: AtomicBool::new(false),
            pending_refresh: AtomicBool::new(false),
            token_results: Mutex::new(HashMap::new()),
            next_result_id: AtomicU64::new(1),
        }
    }

    /// Store a fresh token result for `uri` and return its resultId.
    fn store_token_result(&self, uri: Uri, data: Vec<SemanticToken>) -> String {
        let result_id = format!("todo-{}", self.next_result_id.fetch_add(1, Ordering::SeqCst));
        self.token_results.lock().unwrap().insert(
            uri,
            TokenResult {
                result_id: result_id.clone(),
                data,
            },
        );
        result_id
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let refresh_supported = params
            .capabilities
            .workspace
            .and_then(|w| w.semantic_tokens)
            .and_then(|t| t.refresh_support)
            .unwrap_or(false);
        self.refresh_supported
            .store(refresh_supported, Ordering::SeqCst);

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "todo-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_symbol_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_link_provider: Some(
                    DocumentLinkOptions {
                        resolve_provider: Some(false),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    }
                    .into(),
                ),
                document_formatting_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensOptions {
                        legend: analysis::semantic_tokens_legend(),
                        full: Some(SemanticTokensFullOptions::Delta {
                            delta: Some(true),
                        }),
                        range: None,
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    }
                    .into(),
                ),
                ..Default::default()
            },
            offset_encoding: Some("utf-8".to_string()),
        })
    }

    async fn initialized(&self, _: InitializedParams) {}

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let text = params.text_document.text;

        let tree = parse(&text);
        let diagnostics = analysis::diagnostics(tree.root_node());
        let document = Document {
            version,
            text,
            tree,
        };
        self.documents.lock().unwrap().insert(uri.clone(), document);
        // A reopened document invalidates any token result from its previous
        // open (色付けの更新: 未知の応答識別子には全 token を返す).
        self.token_results.lock().unwrap().remove(&uri);
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        // FULL sync: the client sends the entire document in a single change.
        // Take the last change's full text.
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        let text = change.text;

        let tree = parse(&text);
        let diagnostics = analysis::diagnostics(tree.root_node());
        let document = Document {
            version,
            text,
            tree,
        };
        self.documents.lock().unwrap().insert(uri.clone(), document);
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;

        // 色付けの更新: after a command edit was applied, this `didChange` is
        // the applied edit flowing back — request exactly one recalculation
        // from supporting clients. Any other `didChange` clears the flag
        // without refreshing.
        if self.pending_refresh.swap(false, Ordering::SeqCst)
            && self.refresh_supported.load(Ordering::SeqCst)
        {
            let _ = self.client.semantic_tokens_refresh().await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.lock().unwrap().remove(&uri);
        self.token_results.lock().unwrap().remove(&uri);
        // Clear diagnostics for the closed document.
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let symbols = self.documents.lock().unwrap().get(&uri).map(|document| {
            let root = document.tree.root_node();
            let source = document.text.as_bytes();
            analysis::document_symbols(root, source)
        });
        Ok(symbols.map(DocumentSymbolResponse::Nested))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;
        let ranges = self.documents.lock().unwrap().get(&uri).map(|document| {
            let root = document.tree.root_node();
            let source = document.text.as_bytes();
            analysis::folding_ranges(root, source)
        });
        Ok(ranges)
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let uri = params.text_document.uri;
        let links = self
            .documents
            .lock()
            .unwrap()
            .get(&uri)
            .map(|document| analysis::document_links(document.text.as_bytes()));
        Ok(links)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let tokens = self.documents.lock().unwrap().get(&uri).map(|document| {
            let source = document.text.as_bytes();
            analysis::semantic_tokens(source)
        });
        Ok(tokens
            .map(|data| {
                let result_id = self.store_token_result(uri, data.clone());
                SemanticTokens {
                    result_id: Some(result_id),
                    data,
                }
            })
            .map(SemanticTokensResult::from))
    }

    /// 色付けの更新: a known previous resultId gets a token delta; an
    /// unknown one falls back to the full token data.
    async fn semantic_tokens_full_delta(
        &self,
        params: SemanticTokensDeltaParams,
    ) -> Result<Option<SemanticTokensFullDeltaResult>> {
        let uri = params.text_document.uri;
        let tokens = self.documents.lock().unwrap().get(&uri).map(|document| {
            let source = document.text.as_bytes();
            analysis::semantic_tokens(source)
        });
        let Some(data) = tokens else {
            return Ok(None);
        };
        let previous = self
            .token_results
            .lock()
            .unwrap()
            .get(&uri)
            .filter(|t| t.result_id == params.previous_result_id)
            .map(|t| t.data.clone());
        let result_id = self.store_token_result(uri, data.clone());
        Ok(Some(match previous {
            Some(old) => SemanticTokensDelta {
                result_id: Some(result_id),
                edits: token_delta_edits(&old, &data),
            }
            .into(),
            None => SemanticTokens {
                result_id: Some(result_id),
                data,
            }
            .into(),
        }))
    }

    /// §フォーマット: Format Document replaces the whole text with the
    /// formatted document.
    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let edited = self
            .documents
            .lock()
            .unwrap()
            .get(&uri)
            .and_then(|document| {
                let formatted = format_document(&document.text);
                if formatted == document.text {
                    None
                } else {
                    Some(vec![TextEdit {
                        range: Range::new(Position::new(0, 0), end_position(&document.text)),
                        new_text: formatted,
                    }])
                }
            });
        Ok(edited)
    }

    /// §コマンド: every `todo-language.*` command. Arguments: `[uri, lines?]`
    /// — the document URI and (except Repeat Tasks) the selected 0-based
    /// line numbers. The result is applied as line-limited workspace edits:
    /// each edit covers only lines whose content changes.
    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<LSPAny>> {
        let Some(uri_str) = params.arguments.first().and_then(|v| v.as_str()) else {
            return Ok(None);
        };
        let Ok(uri) = uri_str.parse::<Uri>() else {
            return Ok(None);
        };
        let selection: Vec<usize> = params
            .arguments
            .get(1)
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as usize))
                    .collect()
            })
            .unwrap_or_default();

        let old_text = {
            let documents = self.documents.lock().unwrap();
            let Some(document) = documents.get(&uri) else {
                return Ok(None);
            };
            document.text.clone()
        };
        let new_text = {
            let today = Local::now().date_naive();
            let text = old_text.as_str();
            match params.command.as_str() {
                "todo-language.toggleDone" => Some(toggle(&text, &selection, Toggle::Done, today)),
                "todo-language.toggleCancelled" => {
                    Some(toggle(&text, &selection, Toggle::Cancelled, today))
                }
                "todo-language.toggleStart" => {
                    Some(toggle(&text, &selection, Toggle::Start, today))
                }
                "todo-language.toggleDue" => Some(toggle(&text, &selection, Toggle::Due, today)),
                "todo-language.toggleQueue" => {
                    Some(toggle(&text, &selection, Toggle::Queue, today))
                }
                "todo-language.toggleQueueUnshift" => {
                    Some(toggle(&text, &selection, Toggle::QueueUnshift, today))
                }
                "todo-language.toggleWaiting" => {
                    Some(toggle(&text, &selection, Toggle::Waiting, today))
                }
                "todo-language.togglePending" => {
                    Some(toggle(&text, &selection, Toggle::Pending, today))
                }
                "todo-language.toggleHide" => Some(toggle(&text, &selection, Toggle::Hide, today)),
                "todo-language.toggleRepeat" => {
                    Some(toggle(&text, &selection, Toggle::Repeat, today))
                }
                "todo-language.indent" => Some(reindent(&text, &selection, 1)),
                "todo-language.dedent" => Some(reindent(&text, &selection, -1)),
                "todo-language.repeatTasks" => Some(repeat_tasks(&text, chrono::Utc::now())),
                "todo-language.archive" => Some(archive::archive(&text, &selection)),
                "todo-language.unarchive" => Some(archive::unarchive(&text, &selection)),
                _ => None,
            }
        };

        if let Some(new_text) = new_text {
            if new_text != old_text {
                let edits = line_edits(&old_text, &new_text);
                if !edits.is_empty() {
                    let edit = WorkspaceEdit {
                        changes: Some([(uri, edits)].into_iter().collect()),
                        ..Default::default()
                    };
                    let response = self
                        .client
                        .apply_edit(edit)
                        .await
                        .map_err(|e: RpcError| e)?;
                    // 色付けの更新: the applied edit flows back as didChange,
                    // and that didChange sends exactly one recalculation.
                    if response.applied {
                        self.pending_refresh.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
        Ok(None)
    }
}

/// The position just past the end of `text` (lines are 0-based; characters
/// are UTF-8 byte offsets, matching the advertised offsetEncoding).
fn end_position(text: &str) -> Position {
    if text.is_empty() {
        return Position::new(0, 0);
    }
    let mut line = 0u32;
    let mut start = 0usize;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            line += 1;
            start = i + 1;
        }
    }
    Position::new(line, (text.len() - start) as u32)
}
