use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dashmap::DashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::analysis::WorldIndex;
use crate::settings::Settings;
use crate::{
    completion, definition, diagnostics, folding, hover, links, references, rename, sources,
    symbols,
};

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
        // A close can come while a change publishes. Then the closed file
        // keeps the empty list that the close published.
        if !self.documents.contains_key(uri) {
            return;
        }
        self.client
            .publish_diagnostics(uri.clone(), diags, None)
            .await;
    }

    /// Whether `path` is in the workspace and its name matches
    /// `kconfig_files`.
    fn matches_kconfig_files(&self, path: &Path) -> bool {
        let in_root = self
            .workspace_root
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|root| path.starts_with(root));
        let settings = self.settings.lock().unwrap();
        in_root
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| settings.is_kconfig_file(n))
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
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![" ".into(), "\t".into()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_link_provider: Some(DocumentLinkOptions {
                    resolve_provider: Some(false),
                    work_done_progress_options: Default::default(),
                }),
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
        // A file can come into the workspace after the scan.
        let is_workspace_file = self.workspace_files.lock().unwrap().contains(&path)
            || self.matches_kconfig_files(&path);
        // A workspace file goes back to its text on disk. A file that is not
        // in the workspace, or that is not on disk now, leaves the index.
        let source = is_workspace_file
            .then(|| std::fs::read_to_string(&path).ok())
            .flatten();
        {
            let mut idx = self.index.lock().unwrap();
            match &source {
                Some(source) => idx.reanalyze_file(&path, source),
                None => idx.remove_file(&path),
            }
        }
        {
            let mut workspace_files = self.workspace_files.lock().unwrap();
            match source {
                Some(_) => workspace_files.insert(path),
                None => workspace_files.remove(&path),
            };
        }
        // The server publishes diagnostics only for open files. So remove
        // those of the closed file, or the editor keeps them.
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
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

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let idx = self.index.lock().unwrap();
        let path = match Self::uri_to_path(&params.text_document.uri) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(rename::prepare_rename(&idx, &path, params.position).map(PrepareRenameResponse::Range))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let position = params.text_document_position;
        let Some(path) = Self::uri_to_path(&position.text_document.uri) else {
            return Ok(None);
        };
        let mut idx = self.index.lock().unwrap();
        // The edits must fit the text of the files. A file that the editor
        // does not have open can be changed on disk after the server read
        // it, so read all such files again. A file that is gone leaves the
        // index.
        let closed: Vec<PathBuf> = idx
            .files
            .keys()
            .filter(|f| !Url::from_file_path(f).is_ok_and(|u| self.documents.contains_key(&u)))
            .cloned()
            .collect();
        for file in closed {
            match std::fs::read_to_string(&file) {
                Ok(text) if idx.files[&file].source != text => idx.reanalyze_file(&file, &text),
                Ok(_) => {}
                Err(_) => idx.remove_file(&file),
            }
        }
        rename::rename(&idx, &path, position.position, &params.new_name)
            .map_err(tower_lsp::jsonrpc::Error::invalid_params)
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

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        let idx = self.index.lock().unwrap();
        let path = match Self::uri_to_path(&params.text_document.uri) {
            Some(p) => p,
            None => return Ok(None),
        };
        Ok(links::document_links(&idx, &path))
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

    use std::collections::HashMap;

    use futures::StreamExt;
    use tokio::sync::mpsc;
    use tower_lsp::LspService;
    use tower_lsp::jsonrpc::Request;
    use tower_service::Service;

    /// Talks to the server as an editor does, and keeps the diagnostics that
    /// the server publishes.
    struct Editor {
        service: LspService<Backend>,
        messages: mpsc::UnboundedReceiver<Request>,
    }

    impl Editor {
        async fn start(root: &Path) -> Self {
            let (service, socket) = LspService::new(Backend::new);
            // The server waits until the editor reads each message.
            let (tx, messages) = mpsc::unbounded_channel();
            tokio::spawn(socket.for_each(move |m| {
                let _ = tx.send(m);
                async {}
            }));
            let mut editor = Editor { service, messages };
            let folder = WorkspaceFolder {
                uri: Url::from_file_path(root).unwrap(),
                name: "test".to_string(),
            };
            let params = InitializeParams {
                workspace_folders: Some(vec![folder]),
                ..Default::default()
            };
            let params = serde_json::to_value(params).unwrap();
            let request = Request::build("initialize").id(1).params(params).finish();
            editor.send(request).await;
            editor.notify("initialized", InitializedParams {}).await;
            editor
        }

        async fn send(&mut self, request: Request) -> Option<tower_lsp::jsonrpc::Response> {
            std::future::poll_fn(|cx| self.service.poll_ready(cx))
                .await
                .unwrap();
            self.service.call(request).await.unwrap()
        }

        async fn notify(&mut self, method: &'static str, params: impl serde::Serialize) {
            let params = serde_json::to_value(params).unwrap();
            self.send(Request::build(method).params(params).finish())
                .await;
        }

        async fn open(&mut self, path: &Path, text: &str) {
            let uri = Url::from_file_path(path).unwrap();
            let text_document = TextDocumentItem::new(uri, "kconfig".into(), 1, text.into());
            let params = DidOpenTextDocumentParams { text_document };
            self.notify("textDocument/didOpen", params).await;
        }

        async fn close(&mut self, path: &Path) {
            let uri = Url::from_file_path(path).unwrap();
            let text_document = TextDocumentIdentifier::new(uri);
            let params = DidCloseTextDocumentParams { text_document };
            self.notify("textDocument/didClose", params).await;
        }

        /// The diagnostics that the server published since the last call: the
        /// messages of the last list for each file.
        async fn diagnostics(&mut self) -> HashMap<PathBuf, Vec<String>> {
            // The messages arrive in order, so this one comes after all
            // diagnostics that the server published before.
            let client = &self.service.inner().client;
            client.log_message(MessageType::LOG, "end").await;
            let mut found = HashMap::new();
            while let Some(message) = self.messages.recv().await {
                let params = message.params().cloned().unwrap_or_default();
                match message.method() {
                    "window/logMessage" if params["message"] == "end" => break,
                    "textDocument/publishDiagnostics" => {
                        let p: PublishDiagnosticsParams = serde_json::from_value(params).unwrap();
                        let messages = p.diagnostics.into_iter().map(|d| d.message);
                        found.insert(p.uri.to_file_path().unwrap(), messages.collect());
                    }
                    _ => {}
                }
            }
            found
        }

        fn backend(&self) -> &Backend {
            self.service.inner()
        }
    }

    /// An empty directory for one test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kconfig-lsp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn undefined(name: &str) -> Vec<String> {
        vec![format!("symbol `{name}` is not defined in the workspace")]
    }

    #[tokio::test]
    async fn closing_a_file_outside_the_workspace_drops_it() {
        let root = temp_dir("close-outside");
        let kconfig = root.join("Kconfig");
        let text = "config A\n\tbool \"a\"\n\tdepends on B\n";
        std::fs::write(&kconfig, text).unwrap();
        let outside = root.with_extension("Kconfig");
        let mut editor = Editor::start(&root).await;
        editor.open(&kconfig, text).await;
        editor
            .open(&outside, "config B\n\tbool \"b\"\n\tdepends on C\n")
            .await;
        let found = editor.diagnostics().await;
        assert_eq!(found[&kconfig], Vec::<String>::new());
        assert_eq!(found[&outside], undefined("C"));

        editor.close(&outside).await;
        let found = editor.diagnostics().await;
        assert_eq!(found.get(&outside), Some(&Vec::new()));
        assert_eq!(found[&kconfig], undefined("B"));
        let idx = editor.backend().index.lock().unwrap();
        assert!(!idx.files.contains_key(&outside));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn closing_a_workspace_file_reads_it_from_disk() {
        let root = temp_dir("close-workspace");
        let kconfig = root.join("Kconfig");
        let other = root.join("Kconfig.b");
        std::fs::write(&kconfig, "config A\n\tbool \"a\"\n").unwrap();
        let other_text = "config B\n\tbool \"b\"\n\tdepends on NEW\n";
        std::fs::write(&other, other_text).unwrap();
        let mut editor = Editor::start(&root).await;
        editor.open(&other, other_text).await;
        // The editor has text that is not on disk.
        let unsaved = "config A\n\tbool \"a\"\n\nconfig NEW\n\tbool \"new\"\n";
        editor.open(&kconfig, unsaved).await;
        let found = editor.diagnostics().await;
        assert_eq!(found[&other], Vec::<String>::new());

        editor.close(&kconfig).await;
        let found = editor.diagnostics().await;
        assert_eq!(found.get(&kconfig), Some(&Vec::new()));
        assert_eq!(found[&other], undefined("NEW"));
        let idx = editor.backend().index.lock().unwrap();
        assert!(idx.files.contains_key(&kconfig));
        assert!(idx.get_definitions("NEW").is_empty());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn closing_a_workspace_file_that_is_gone_drops_it() {
        let root = temp_dir("close-gone");
        let kconfig = root.join("Kconfig");
        let other = root.join("Kconfig.b");
        let text = "config A\n\tbool \"a\"\n";
        std::fs::write(&kconfig, text).unwrap();
        let other_text = "config B\n\tbool \"b\"\n\tdepends on A\n";
        std::fs::write(&other, other_text).unwrap();
        let mut editor = Editor::start(&root).await;
        editor.open(&other, other_text).await;
        editor.open(&kconfig, text).await;
        std::fs::remove_file(&kconfig).unwrap();

        editor.close(&kconfig).await;
        let found = editor.diagnostics().await;
        assert_eq!(found[&other], undefined("A"));
        let backend = editor.backend();
        assert!(!backend.index.lock().unwrap().files.contains_key(&kconfig));
        assert!(!backend.workspace_files.lock().unwrap().contains(&kconfig));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn closing_a_file_made_after_the_scan_reads_it_from_disk() {
        let root = temp_dir("close-new");
        let kconfig = root.join("Kconfig");
        let text = "config A\n\tbool \"a\"\n\tdepends on B\n";
        std::fs::write(&kconfig, text).unwrap();
        let mut editor = Editor::start(&root).await;
        editor.open(&kconfig, text).await;
        let new = root.join("Kconfig.new");
        let new_text = "config B\n\tbool \"b\"\n";
        std::fs::write(&new, new_text).unwrap();
        editor.open(&new, new_text).await;

        editor.close(&new).await;
        let found = editor.diagnostics().await;
        assert_eq!(found[&kconfig], Vec::<String>::new());
        let backend = editor.backend();
        assert!(backend.index.lock().unwrap().files.contains_key(&new));
        assert!(backend.workspace_files.lock().unwrap().contains(&new));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    async fn rename_reads_files_that_are_not_open_again() {
        let root = temp_dir("rename-disk");
        let kconfig = root.join("Kconfig");
        let other = root.join("Kconfig.b");
        let text = "config FOO\n\tbool \"foo\"\n";
        std::fs::write(&kconfig, text).unwrap();
        std::fs::write(&other, "config B\n\tdepends on FOO\n").unwrap();
        let added = root.join("Kconfig.c");
        std::fs::write(&added, "config C\n\tbool\n").unwrap();
        let gone = root.join("Kconfig.d");
        std::fs::write(&gone, "config D\n\tdepends on FOO\n").unwrap();
        let mut editor = Editor::start(&root).await;
        editor.open(&kconfig, text).await;
        // Another program changes the files after the scan.
        std::fs::write(&other, "# New line.\nconfig B\n\tdepends on FOO\n").unwrap();
        std::fs::write(&added, "config C\n\tdepends on FOO\n").unwrap();
        std::fs::remove_file(&gone).unwrap();

        let uri = Url::from_file_path(&kconfig).unwrap();
        let params = RenameParams {
            text_document_position: TextDocumentPositionParams::new(
                TextDocumentIdentifier::new(uri),
                Position::new(0, 8),
            ),
            new_name: "BAR".to_string(),
            work_done_progress_params: Default::default(),
        };
        let params = serde_json::to_value(params).unwrap();
        let request = Request::build("textDocument/rename")
            .id(2)
            .params(params)
            .finish();
        let (_, result) = editor.send(request).await.unwrap().into_parts();
        let edit: WorkspaceEdit = serde_json::from_value(result.unwrap()).unwrap();
        let edit_at = |line, start| {
            let range = Range::new(Position::new(line, start), Position::new(line, start + 3));
            vec![TextEdit::new(range, "BAR".to_string())]
        };
        let uri = |path: &Path| Url::from_file_path(path).unwrap();
        let changes = edit.changes.unwrap();
        assert_eq!(
            changes,
            HashMap::from([
                (uri(&kconfig), edit_at(0, 7)),
                (uri(&other), edit_at(2, 12)),
                (uri(&added), edit_at(1, 12)),
            ])
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
