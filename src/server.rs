use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dashmap::DashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::analysis::WorldIndex;
use crate::settings::Settings;
use crate::{completion, definition, diagnostics, folding, hover, references, sources, symbols};

pub struct Backend {
    client: Client,
    documents: DashMap<Url, String>,
    index: Mutex<WorldIndex>,
    /// Root path of the workspace, captured during initialization.
    workspace_root: Mutex<Option<PathBuf>>,
    /// Files discovered and indexed from the workspace (not explicitly opened
    /// by the editor).  Tracked so that `did_close` can restore the on-disk
    /// version instead of dropping the file from the index entirely.
    workspace_files: Mutex<HashSet<PathBuf>>,
    settings: Mutex<Settings>,
    /// Warnings from `initializationOptions`, shown in `initialized`.
    settings_warnings: Mutex<Vec<String>>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
            index: Mutex::new(WorldIndex::new()),
            workspace_root: Mutex::new(None),
            workspace_files: Mutex::new(HashSet::new()),
            settings: Mutex::new(Default::default()),
            settings_warnings: Mutex::new(Vec::new()),
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

    /// The diagnostics of a file can depend on other files, such as for
    /// undefined symbols and types. So publish them for all open files
    /// after a change to the index.
    async fn publish_all_diagnostics(&self) {
        let open_uris: Vec<Url> = self.documents.iter().map(|e| e.key().clone()).collect();
        for uri in open_uris {
            self.publish_diagnostics(&uri).await;
        }
    }

    /// Analyzes the text of an open file, and reads the files that its
    /// `source` statements name if they are not in the index. The named
    /// files are workspace files, also if they are open already, so that
    /// closing them reads them from disk again.
    fn update_file(&self, path: PathBuf, text: &str) {
        let sourced = {
            let mut idx = self.index.lock().unwrap();
            idx.reanalyze_file(&path, text);
            sources::read_sourced_files(&mut idx, vec![path])
        };
        self.workspace_files.lock().unwrap().extend(sourced);
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root = params
            .root_uri
            .as_ref()
            .and_then(|u| u.to_file_path().ok())
            .or_else(|| {
                params
                    .workspace_folders
                    .as_ref()
                    .and_then(|wf| wf.first())
                    .and_then(|f| f.uri.to_file_path().ok())
            });
        if let Some(root) = root {
            log::info!("workspace root: {}", root.display());
            *self.workspace_root.lock().unwrap() = Some(root);
        }

        if let Some(ops) = params.initialization_options {
            let (settings, warnings) = Settings::from_json(ops);
            *self.settings.lock().unwrap() = settings;
            *self.settings_warnings.lock().unwrap() = warnings;
        }

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
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: env!("CARGO_PKG_NAME").into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        log::info!("kconfig-lsp initialized");

        // Assign settings while keeping the index lock's lifetime constrained.
        {
            let mut idx = self.index.lock().unwrap();
            idx.settings = self.settings.lock().unwrap().clone();
        }

        let root = self.workspace_root.lock().unwrap().clone();
        if let Some(root) = root {
            let settings = self.settings.lock().unwrap().clone();
            let kconfig_files = discover_kconfig_files(&root, &settings);
            log::info!(
                "discovered {} Kconfig files in workspace",
                kconfig_files.len()
            );

            let mut ws_files = self.workspace_files.lock().unwrap();
            let mut idx = self.index.lock().unwrap();
            idx.root = Some(root);
            for path in kconfig_files {
                match std::fs::read_to_string(&path) {
                    Ok(source) => {
                        idx.analyze_file(&path, &source);
                        ws_files.insert(path);
                    }
                    Err(e) => {
                        log::warn!("failed to read {}: {}", path.display(), e);
                    }
                }
            }
            // The patterns can miss files that the read files source.
            let scanned = ws_files.iter().cloned().collect();
            let before = ws_files.len();
            ws_files.extend(sources::read_sourced_files(&mut idx, scanned));
            log::info!(
                "found {} more files that source statements name",
                ws_files.len() - before
            );
        }

        // Await only after the scan, or a didOpen could run in between and
        // have its unsaved text overwritten by the on-disk copy.
        let warnings = std::mem::take(&mut *self.settings_warnings.lock().unwrap());
        for warning in warnings {
            log::warn!("{warning}");
            self.client
                .show_message(MessageType::WARNING, format!("kconfig-lsp: {warning}"))
                .await;
        }

        // Symbols that the workspace scan found clear their warnings.
        self.publish_all_diagnostics().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.documents.insert(uri.clone(), text.clone());

        if let Some(path) = Self::uri_to_path(&uri) {
            self.update_file(path, &text);
        }
        self.publish_all_diagnostics().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            let text = change.text;
            self.documents.insert(uri.clone(), text.clone());

            if let Some(path) = Self::uri_to_path(&uri) {
                self.update_file(path, &text);
            }
            self.publish_all_diagnostics().await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);

        let Some(path) = Self::uri_to_path(&uri) else {
            return;
        };
        let is_workspace_file = self.workspace_files.lock().unwrap().contains(&path);
        if !is_workspace_file {
            return;
        }
        if let Ok(source) = std::fs::read_to_string(&path) {
            let mut idx = self.index.lock().unwrap();
            idx.reanalyze_file(&path, &source);
        }
        self.publish_all_diagnostics().await;
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

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let idx = self.index.lock().unwrap();
        let path = match Self::uri_to_path(&params.text_document.uri) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(symbols::document_symbols(&idx, &path))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let idx = self.index.lock().unwrap();
        Ok(Some(symbols::workspace_symbols(&idx, &params.query)))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let idx = self.index.lock().unwrap();
        let path = match Self::uri_to_path(&params.text_document.uri) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(folding::folding_ranges(&idx, &path))
    }
}

fn discover_kconfig_files(root: &Path, settings: &Settings) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !is_ignored_dir(&path) {
                    stack.push(path);
                }
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| settings.is_kconfig_file(n))
            {
                result.push(path);
            }
        }
    }

    result
}

fn is_ignored_dir(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return true,
    };
    matches!(name, ".git" | ".hg" | ".svn" | "node_modules" | ".repo")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_reads_only_files_that_match_the_patterns() {
        let root =
            std::env::temp_dir().join(format!("kconfig-lsp-discover-{}", std::process::id()));
        for file in [
            "Config.in",
            "package/foo/Config.in",
            "Kconfig",
            ".git/Config.in",
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
        let settings = Settings {
            kconfig_files: vec!["Config.in".to_string()],
            ..Default::default()
        };
        let mut found: Vec<_> = discover_kconfig_files(&root, &settings)
            .into_iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_path_buf())
            .collect();
        found.sort();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            found,
            [
                PathBuf::from("Config.in"),
                PathBuf::from("package/foo/Config.in")
            ]
        );
    }
}
