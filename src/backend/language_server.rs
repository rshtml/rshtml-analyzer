use crate::app_state::view::View;
use crate::backend::Backend;
use crate::backend::navigation_target::NavigationTarget;
use crate::backend::server_capabilities::{semantic_tokens_capabilities, workspace_capabilities};
use crate::backend::syntax_context::SyntaxContext;
use crate::backend::tree_extensions::TreeExtensions;
use crate::consts;
use crate::rust_analyzer_client::RustAnalyzerClient;
use tower_lsp::jsonrpc::Error;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionOptions, CompletionParams,
    CompletionResponse, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    InsertTextFormat, Location, MarkupContent, MarkupKind, MessageType, OneOf, Range,
    SemanticTokens, SemanticTokensDelta, SemanticTokensDeltaParams, SemanticTokensFullDeltaResult,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, ServerCapabilities, ServerInfo, TextDocumentIdentifier,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use tower_lsp::{LanguageServer, jsonrpc};
use tracing::{debug, error};

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult, Error> {
        debug!(
            "The Initialize request has been received (build: {})...",
            consts::BUILD_MODE
        );

        let workspace_root_path = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|folder| folder.uri.to_file_path().ok());

        debug!("workspace root path: {:?}", workspace_root_path);

        if let Some(path) = workspace_root_path {
            debug!("Workspace root path: {:?}", path);
            self.client
                .log_message(MessageType::INFO, format!("Workspace root path: {path:?}"))
                .await;

            let mut workspace = self.state.workspace.write().await;
            workspace.root = path.to_path_buf();
            workspace.load(&path).unwrap_or_else(|e| {
                debug!(
                    "Workspace couldn't load: {}, operating in single-file mode",
                    e
                );
            });
        }

        let ra_option = RustAnalyzerClient::new(self.client.clone()).await;
        if let Some(ra) = &ra_option {
            ra.initialize(params.root_uri).await;
            ra.initialized().await;
        }
        let _ = self
            .ra_client
            .set(ra_option)
            .inspect_err(|e| debug!("RA client could not be set: {e}"));

        debug!("Sending an initialize response.");
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                semantic_tokens_provider: semantic_tokens_capabilities(),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![
                        "@".into(),
                        "<".into(),
                        ":".into(),
                        ".".into(),
                        "'".into(),
                        "(".into(),
                    ]),
                    ..Default::default()
                }),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                workspace: workspace_capabilities(),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "rshtml-analyzer".to_string(),
                version: Some(format!(
                    "{} ({})",
                    env!("CARGO_PKG_VERSION"),
                    consts::BUILD_MODE
                )),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        let version_msg = format!(
            "rshtml-analyzer v{} ({}) connected!",
            env!("CARGO_PKG_VERSION"),
            consts::BUILD_MODE
        );

        if cfg!(debug_assertions) {
            self.client
                .show_message(MessageType::INFO, &version_msg)
                .await;
        }

        self.client
            .log_message(
                MessageType::INFO,
                format!("{version_msg} Handshake completed."),
            )
            .await;
    }

    async fn shutdown(&self) -> Result<(), Error> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let msg = format!("Opened file: {}", params.text_document.uri);
        self.client.log_message(MessageType::INFO, &msg).await;
        debug!(msg);

        let uri_str = params.text_document.uri.to_string();
        let text = params.text_document.text;

        let tree = {
            let mut parser = self.state.parser.lock().await;

            if let Some(tree) = parser.parse(&text, None) {
                tree
            } else {
                self.client
                    .log_message(MessageType::ERROR, "Parser error: Couldn't create tree.")
                    .await;
                return;
            }
        };

        let use_directives = tree.find_uses(&self.state.language, &text);
        debug!("Use directives: {:?}", use_directives);

        let mut use_directives_with_params = Vec::new();
        for (use_path, use_name) in &use_directives {
            let use_params = if let Ok(uri) = params.text_document.uri.join(use_path) {
                self.state.find_use_params(&uri).await.unwrap_or_default()
            } else {
                Vec::new()
            };
            debug!("use params: {use_params:?}");
            use_directives_with_params.push((use_path.to_owned(), use_name.to_owned(), use_params))
        }

        let template_params = tree.find_template_params(&self.state.language, &text);
        debug!("Template params: {:?}", template_params);

        let extract_file = async {
            let path = params
                .text_document
                .uri
                .to_file_path()
                .map_err(|_| {
                    debug!("invalid uri: {}", &params.text_document.uri);
                })
                .ok()?;

            let workspace = self.state.workspace.read().await;
            let (root_path, relative_path) = workspace.find_root_and_relative_path(&path);

            let extract_file_path = Self::generate_extract_file_path(&root_path, &relative_path)
                .await
                .map_err(|e| debug!("generate_module_path error: {}", e))
                .ok()?;

            let extract_file_uri = Url::from_file_path(&extract_file_path)
                .map_err(|_| debug!("Couldn't convert file to uri: {:?}", extract_file_path))
                .ok()?;

            Some((extract_file_uri, extract_file_path))
        }
        .await;

        let errors = {
            let mut view = View::new(
                text.to_owned(),
                tree.to_owned(),
                params.text_document.version as usize,
                extract_file.as_ref().map(|x| x.0.clone()),
            );
            view.use_directives = use_directives_with_params;
            view.create_use_directive_completion_items();
            view.template_params = template_params;

            let mut views = self.state.views.write().await;

            let errors = view.tree.find_error(&self.state.language, &view.source);

            // Self::debug_check_source_mismatch(
            //     &extract_file,
            //     view.tree
            //         .extract_snippets(&self.state.language, &view.source),
            //     &view.source,
            // )
            // .await;

            views.insert(uri_str, view);

            errors
        };

        let _ = async {
            let (extract_file_uri, extract_file_path) = extract_file?;

            let extract_text = tokio::fs::read_to_string(&extract_file_path)
                .await
                .inspect_err(|e| {
                    debug!(
                        "Failed to read file at '{}': {}",
                        extract_file_path.display(),
                        e
                    )
                })
                .ok()?;

            let ra_client = self.ra_client.get()?.as_ref()?;

            ra_client
                .uri_map
                .lock()
                .await
                .insert(extract_file_uri.clone(), params.text_document.uri.clone());

            ra_client.did_open(extract_file_uri, extract_text).await;

            // let status = ra_client.analyzer_status(extract_file_uri).await;
            // debug!("(DEBUG STATUS) {status}");

            Some(())
        }
        .await;

        self.client
            .publish_diagnostics(
                params.text_document.uri,
                errors,
                Some(params.text_document.version),
            )
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let msg = format!("Changed file: {}", params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;

        let uri_str = params.text_document.uri.to_string();

        let (new_uses, errors) = {
            let mut views = self.state.views.write().await;

            if let Some(view) = views.get_mut(&uri_str) {
                if view.version >= params.text_document.version as usize {
                    return;
                }

                self.process_changes(params.content_changes, &mut view.source, &mut view.tree);

                let tree = {
                    let mut parser = self.state.parser.lock().await;
                    if let Some(tree) = parser.parse(&view.source, Some(&view.tree)) {
                        tree
                    } else {
                        error!("Error while parsing tree");
                        return;
                    }
                };

                let use_directives = tree.find_uses(&self.state.language, &view.source);
                let template_params = tree.find_template_params(&self.state.language, &view.source);

                view.version = params.text_document.version as usize;
                view.tree = tree;
                let new_uses = view.sync_use_directives(use_directives);

                view.template_params = template_params;

                (
                    new_uses,
                    view.tree.find_error(&self.state.language, &view.source),
                )
            } else {
                error!("view {uri_str} not found");
                return;
            }
        };

        let mut new_uses_params = Vec::new();
        for (id, use_path) in new_uses {
            let use_params = if let Ok(uri) = params.text_document.uri.join(&use_path) {
                self.state.find_use_params(&uri).await.unwrap_or_default()
            } else {
                Vec::new()
            };

            new_uses_params.push((id, use_params));
        }

        let extract_file = {
            let mut views = self.state.views.write().await;
            if let Some(view) = views.get_mut(&uri_str) {
                for (id, use_params) in new_uses_params {
                    if let Some(use_directive) = view.use_directives.get_mut(id) {
                        use_directive.2 = use_params;
                    }
                }

                view.update_use_directive_completion_items();

                let snippets = view
                    .tree
                    .extract_snippets(&self.state.language, &view.source);

                let extract_source = Self::extract_source(snippets, &view.source);

                Some((view.extract_file_uri.to_owned(), extract_source))
            } else {
                None
            }
        };

        let _ = async {
            let (extract_file_uri, extract_source) =
                extract_file.and_then(|(u, s)| Some((u?, s)))?;

            let ra_client = self.ra_client.get()?.as_ref()?;

            debug!(
                "(DID CHANGE) RA did change will send, uri: {} version: {}",
                extract_file_uri, params.text_document.version
            );

            ra_client
                .did_change(
                    extract_file_uri,
                    params.text_document.version,
                    extract_source,
                )
                .await;

            debug!("(DID CHANGE) RA did change sent",);

            Some(())
        }
        .await;

        self.client
            .publish_diagnostics(
                params.text_document.uri,
                errors,
                Some(params.text_document.version),
            )
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let msg = format!("Closed file: {}", params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;
        let uri_str = params.text_document.uri.to_string();

        let mut views = self.state.views.write().await;
        views.remove(&uri_str);
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        debug!(
            "(DID SAVE) triggered for file: {}",
            params.text_document.uri
        );

        let _ = async {
            let extract_file_uri = {
                let views = self.state.views.read().await;
                let Some(view) = views.get(&params.text_document.uri.to_string()) else {
                    debug!("(DID SAVE) error while getting completion items");
                    return None;
                };

                let Some(extract_file_uri) = &view.extract_file_uri else {
                    debug!("(DID SAVE) error while getting extract file uri");
                    return None;
                };

                extract_file_uri.to_owned()
            };

            let params = DidSaveTextDocumentParams {
                text_document: TextDocumentIdentifier {
                    uri: extract_file_uri,
                },
                text: None,
            };

            let ra_client = self.ra_client.get()?.as_ref()?;
            ra_client.did_save(params).await;
            debug!("(DID SAVE) RA save request sent");

            Some(())
        }
        .await;
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>, Error> {
        let uri_str = params.text_document.uri.to_string();

        let mut views = self.state.views.write().await;

        if let Some(view) = views.get_mut(&uri_str) {
            let highlight = &self.state.highlight;
            let tokens = highlight.highlight(&view.source, None)?;

            debug!("Semantic Tokens: {:?}", tokens.len());

            view.semantic_tokens_version += 1;

            let semantic_tokens = SemanticTokens {
                result_id: Some(view.semantic_tokens_version.to_string()),
                data: tokens,
            };
            view.semantic_tokens = semantic_tokens.clone();

            return Ok(Some(SemanticTokensResult::Tokens(semantic_tokens)));
        }

        Ok(None)
    }

    async fn semantic_tokens_full_delta(
        &self,
        params: SemanticTokensDeltaParams,
    ) -> jsonrpc::Result<Option<SemanticTokensFullDeltaResult>> {
        let uri_str = params.text_document.uri.to_string();
        let result_id = params.previous_result_id;

        let mut views = self.state.views.write().await;

        if let Some(view) = views.get_mut(&uri_str) {
            let highlight = &self.state.highlight;
            let tokens = highlight.highlight(&view.source, None)?;

            if view.semantic_tokens.result_id.as_ref() != Some(&result_id) {
                debug!("Semantic Tokens Delta | Full: {:?}", tokens.len());
                view.semantic_tokens_version += 1;
                let semantic_tokens = SemanticTokens {
                    result_id: Some(view.semantic_tokens_version.to_string()),
                    data: tokens.clone(),
                };
                view.semantic_tokens = semantic_tokens.clone();
                return Ok(Some(SemanticTokensFullDeltaResult::Tokens(semantic_tokens)));
            }

            let tokens_diff =
                highlight.semantic_tokens_difference(&view.semantic_tokens.data, &tokens);

            debug!(
                "Semantic Tokens Delta: {:?} {:?}",
                tokens_diff.len(),
                tokens_diff
            );

            view.semantic_tokens_version += 1;
            view.semantic_tokens = SemanticTokens {
                result_id: Some(view.semantic_tokens_version.to_string()),
                data: tokens.clone(),
            };

            return Ok(Some(SemanticTokensFullDeltaResult::TokensDelta(
                SemanticTokensDelta {
                    result_id: Some(view.semantic_tokens_version.to_string()),
                    edits: tokens_diff,
                },
            )));
        }

        Ok(None)
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> jsonrpc::Result<Option<SemanticTokensRangeResult>> {
        let uri_str = params.text_document.uri.to_string();
        let range = params.range;

        let views = self.state.views.read().await;

        if let Some(view) = views.get(&uri_str) {
            let highlight = &self.state.highlight;
            let start_byte = Self::position_to_byte_offset(&view.source, range.start);
            let end_byte = Self::position_to_byte_offset(&view.source, range.end);
            let tokens = highlight.highlight(&view.source, Some(start_byte..end_byte))?;

            debug!("Semantic Tokens Range: {:?}", tokens.len());

            return Ok(Some(SemanticTokensRangeResult::Tokens(SemanticTokens {
                result_id: None,
                data: tokens,
            })));
        }
        Ok(None)
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> jsonrpc::Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri.as_ref();
        let position = params.text_document_position.position;
        let trigger_char = params
            .context
            .as_ref()
            .and_then(|ctx| ctx.trigger_character.as_ref())
            .and_then(|s| s.chars().next());

        let (mut completion_items, extract_file_uri) = {
            let views = self.state.views.read().await;
            let Some(view) = views.get(&uri.to_string()) else {
                debug!("Error while getting completion items");
                return Ok(None);
            };

            let syntax_context = SyntaxContext::detect(&view.tree, &view.source, position);
            let mut completion_items: Vec<CompletionItem> = Vec::new();

            debug!(
                "(COMPLATION) ctx: {:?}, pos: {:?}",
                syntax_context, position
            );

            let extract_file_uri: Option<Url> = if syntax_context == SyntaxContext::Html {
                if let Some(tc) = trigger_char {
                    for (item_char, item) in view.completion_items.values() {
                        if *item_char == tc {
                            completion_items.push(item.clone());
                        }
                    }

                    if tc == '@' {
                        completion_items.extend(self.state.completion_items.clone());
                    }
                } else {
                    for (_, item) in view.completion_items.values() {
                        completion_items.push(item.clone());
                    }

                    completion_items.extend(self.state.completion_items.clone());
                }

                None
            } else {
                match syntax_context {
                    SyntaxContext::ComponentTag => {
                        for (item_char, item) in view.completion_items.values() {
                            if *item_char == '<' {
                                completion_items.push(item.clone());
                            }
                        }

                        None
                    }
                    SyntaxContext::ComponentParameter(component_name) => {
                        for (name, params) in view.use_directives_names_and_params().into_iter() {
                            if name == component_name {
                                for param in params {
                                    completion_items.push(CompletionItem {
                                        insert_text: Some(format!("{}=\"$1\"", param)),
                                        detail: Some(format!("{} attribute", component_name)),
                                        sort_text: Some(format!("0_{}", param)),
                                        label: param,
                                        kind: Some(CompletionItemKind::PROPERTY),
                                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                                        ..Default::default()
                                    });
                                }
                                break;
                            }
                        }

                        None
                    }
                    SyntaxContext::RustCode => view.extract_file_uri.to_owned(),
                    _ => None,
                }
            };

            debug!("Extract File Uri: {:?}", extract_file_uri);
            (completion_items, extract_file_uri)
        };

        let ra_items = async {
            let mut ra_params = params.clone();
            ra_params.text_document_position.text_document.uri = extract_file_uri?;

            let ra_client = self.ra_client.get()?.as_ref()?;

            debug!(
                "(COMPLETION) RA completion will send, uri: ({})",
                &ra_params.text_document_position.text_document.uri
            );
            let completion_value = ra_client.completion(ra_params).await;
            debug!("(COMPLETION) RA completion sent");

            let response = Some(completion_value)
                .filter(|v| !v.is_null())
                .and_then(|v| {
                    serde_json::from_value::<CompletionResponse>(v)
                        .inspect_err(|e| debug!("RA Completion JSON parse error: {}", e))
                        .ok()
                })?;

            match response {
                CompletionResponse::Array(items) => Some(items),
                CompletionResponse::List(completion_list) => Some(completion_list.items),
            }
        }
        .await;

        completion_items.extend(ra_items.into_iter().flatten());

        return Ok(Some(CompletionResponse::List(CompletionList {
            is_incomplete: true,
            items: completion_items,
        })));
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> jsonrpc::Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let views = self.state.views.read().await;
        if let Some(view) = views.get(&uri.to_string())
            && let Some(target) = NavigationTarget::resolve_at(
                &view.tree,
                &view.source,
                position,
                &view.use_directives,
                uri,
            )
        {
            let target_uri = match target {
                NavigationTarget::Component { target_uri, .. } => target_uri,
                NavigationTarget::UseDirective { target_uri, .. } => target_uri,
            };
            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: target_uri,
                range: Range::default(),
            })));
        }

        Ok(None)
    }

    async fn hover(&self, params: HoverParams) -> jsonrpc::Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let views = self.state.views.read().await;
        if let Some(view) = views.get(&uri.to_string())
            && let Some(target) = NavigationTarget::resolve_at(
                &view.tree,
                &view.source,
                position,
                &view.use_directives,
                uri,
            )
        {
            let (range, markdown) = match target {
                NavigationTarget::Component {
                    name,
                    target_uri,
                    range,
                    params,
                } => {
                    let params_doc = if params.is_empty() {
                        "_No parameters_".to_string()
                    } else {
                        params
                            .iter()
                            .map(|p| format!("- `{}`", p))
                            .collect::<Vec<_>>()
                            .join("\n")
                    };

                    let file_name = target_uri
                        .path_segments()
                        .and_then(|mut segments| segments.next_back())
                        .unwrap_or("<unknown>");

                    let doc = format!(
                        "### Component `<{name}>`\n\n**File:** `{file_name}`\n\n**Parameters:**\n{params_doc}"
                    );

                    (range, doc)
                }
                NavigationTarget::UseDirective {
                    path,
                    target_uri: _,
                    range,
                } => {
                    let doc = format!("### Component Import\n\n`{path}`");
                    (range, doc)
                }
            };

            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: markdown,
                }),
                range: Some(range),
            }));
        }

        Ok(None)
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let cargo_toml_changed = params
            .changes
            .iter()
            .any(|event| event.uri.path().ends_with("/Cargo.toml"));

        if !cargo_toml_changed {
            return;
        }

        debug!("Cargo.toml changed. Re-analyzing...");

        let mut workspace = self.state.workspace.write().await;

        let root = workspace.root.clone();
        workspace.load(&root).unwrap_or_else(|e| {
            debug!("Workspace couldn't load: {}", e);
        });

        debug!("Workspace re-analysis complete.");
    }
}
