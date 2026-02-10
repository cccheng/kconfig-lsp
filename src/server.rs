use std::path::PathBuf;
use std::sync::Mutex;

use dashmap::DashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::analysis::WorldIndex;
use crate::{completion, definition, diagnostics, hover, references};

pub struct Backend {
    client: Client,
    documents: DashMap<Url, String>,
    index: Mutex<WorldIndex>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
            index: Mutex::new(WorldIndex::new()),
        }
    }

    fn uri_to_path(uri: &Url) -> Option<PathBuf> {
        uri.to_file_path().ok()
    }

    async fn publish_diagnostics(&self, uri: &Url) {
        let diags = {
            let idx = self.index.lock().unwrap();
            let path = match Self::uri_to_path(uri) {
                Some(p) => p,
                None => return,
            };
            diagnostics::collect(&idx, &path)
        };
        self.client
            .publish_diagnostics(uri.clone(), diags, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![" ".into(), "\t".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        log::info!("kconfig-lsp initialized");
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.documents.insert(uri.clone(), text.clone());

        if let Some(path) = Self::uri_to_path(&uri) {
            let mut idx = self.index.lock().unwrap();
            idx.reanalyze_file(&path, &text);
        }
        self.publish_diagnostics(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            let text = change.text;
            self.documents.insert(uri.clone(), text.clone());

            if let Some(path) = Self::uri_to_path(&uri) {
                let mut idx = self.index.lock().unwrap();
                idx.reanalyze_file(&path, &text);
            }
            self.publish_diagnostics(&uri).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let idx = self.index.lock().unwrap();
        let path = match Self::uri_to_path(uri) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(hover::hover(&idx, &path, pos))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let idx = self.index.lock().unwrap();
        let path = match Self::uri_to_path(uri) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(definition::goto_definition(&idx, &path, pos))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let idx = self.index.lock().unwrap();
        let path = match Self::uri_to_path(uri) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(references::find_references(&idx, &path, pos))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let idx = self.index.lock().unwrap();
        let path = match Self::uri_to_path(uri) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(completion::complete(&idx, &path, pos))
    }
}
