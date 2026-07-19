use std::sync::Mutex;

use tower_lsp_server::ls_types::*;
use tower_lsp_server::{Client, LanguageServer, jsonrpc::Result};

use crate::analysis;
use crate::parse::{Document, DocumentStore, parse};

pub struct Backend {
    client: Client,
    documents: Mutex<DocumentStore>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(DocumentStore::new()),
        }
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
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
                semantic_tokens_provider: Some(
                    SemanticTokensOptions {
                        legend: analysis::semantic_tokens_legend(),
                        full: Some(SemanticTokensFullOptions::Bool(true)),
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
        self.documents
            .lock()
            .unwrap()
            .insert(uri.clone(), document);
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
        self.documents
            .lock()
            .unwrap()
            .insert(uri.clone(), document);
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.lock().unwrap().remove(&uri);
        // Clear diagnostics for the closed document.
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let symbols = self
            .documents
            .lock()
            .unwrap()
            .get(&uri)
            .map(|document| {
                let root = document.tree.root_node();
                let source = document.text.as_bytes();
                analysis::document_symbols(root, source)
            });
        Ok(symbols.map(DocumentSymbolResponse::Nested))
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;
        let ranges = self
            .documents
            .lock()
            .unwrap()
            .get(&uri)
            .map(|document| {
                let root = document.tree.root_node();
                let source = document.text.as_bytes();
                analysis::folding_ranges(root, source)
            });
        Ok(ranges)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri;
        let tokens = self
            .documents
            .lock()
            .unwrap()
            .get(&uri)
            .map(|document| {
                let root = document.tree.root_node();
                let source = document.text.as_bytes();
                analysis::semantic_tokens(root, source)
            });
        Ok(tokens
            .map(|data| SemanticTokens { result_id: None, data })
            .map(SemanticTokensResult::from))
    }
}
