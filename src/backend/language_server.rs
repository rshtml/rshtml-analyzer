use crate::app_state::view::View;
use crate::backend::Backend;
use crate::backend::server_capabilities::{semantic_tokens_capabilities, workspace_capabilities};
use crate::backend::tree_extensions::TreeExtensions;
use crate::rust_analyzer::RustAnalyzer;
use tower_lsp::jsonrpc::Error;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionList, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, InitializeParams, InitializeResult, InitializedParams, MessageType,
    SemanticTokens, SemanticTokensDelta, SemanticTokensDeltaParams, SemanticTokensFullDeltaResult,
    SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult,
    SemanticTokensResult, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind,
};
use tower_lsp::{LanguageServer, jsonrpc};
use tracing::{debug, error};

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult, Error> {
        debug!("The Initialize request has been received and is being processed...");
        let workspace_root_path = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|folder| folder.uri.to_file_path().ok());

        if let Some(path) = workspace_root_path {
            debug!("Workspace root path: {:?}", path);
            self.client
                .log_message(MessageType::INFO, format!("Workspace root path: {path:?}"))
                .await;

            let mut workspace = self.state.workspace.write().await;
            workspace.load(&path).unwrap_or_else(|e| {
                debug!("Workspace couldn't load: {}", e);
            });
        }

        debug!("Sending an initialize response.");
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                // text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
                //     open_close: Some(true),
                //     change: Some(TextDocumentSyncKind::INCREMENTAL),
                //     will_save: Some(true),
                //     will_save_wait_until: Some(true),
                //     save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions { include_text: Some(false) })),
                // })),
                //document_formatting_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: semantic_tokens_capabilities(),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec!["@".to_string(), "<".to_string()]),
                    ..Default::default()
                }),
                workspace: workspace_capabilities(),
                //position_encoding:Some(PositionEncodingKind::UTF8),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "rshtml-analyzer".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "rshtml LSP initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<(), Error> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let msg = format!("Opened file: {}", &params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;

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

        let struct_info =
            params
                .text_document
                .uri
                .to_file_path()
                .ok()
                .and_then(|rshtml_file_path| {
                    let root_path = self.state.workspace.try_read().ok()?.root.clone();

                    RustAnalyzer::find_struct_for_rshtml(&root_path, &rshtml_file_path).map(
                        |(struct_path, struct_name)| {
                            RustAnalyzer::analyze(
                                &self.state.language,
                                &tree,
                                &text,
                                &struct_path,
                                &struct_name,
                            );

                            (struct_path, struct_name)
                        },
                    )
                });

        debug!("struct info: {struct_info:?}");

        let use_directives = tree.find_uses(&self.state.language, &text);
        debug!("Use directives: {:?}", use_directives);

        let views_path = {
            let file_path = &params.text_document.uri.to_file_path().unwrap_or_default();
            let workspace = self.state.workspace.read().await;

            let member = if let Some(member) = workspace.get_member_by_view(file_path) {
                member
            } else {
                error!("view {file_path:?} not found");
                return;
            };

            member.views_path.clone()
        };

        let mut use_directives_with_params = Vec::new();
        for (use_path, use_name) in &use_directives {
            let use_params = self
                .state
                .find_use_params(&views_path.join(use_path))
                .await
                .unwrap_or(Vec::new());
            debug!("use params: {use_params:?}");
            use_directives_with_params.push((use_path.to_owned(), use_name.to_owned(), use_params))
        }

        let template_params = tree.find_template_params(&self.state.language, &text);
        debug!("Template params: {:?}", template_params);

        let errors = {
            let mut view = View::new(text, tree, params.text_document.version as usize);
            view.use_directives = use_directives_with_params;
            view.create_use_directive_completion_items();
            view.template_params = template_params;
            view.struct_info = struct_info;

            let mut views = self.state.views.write().await;

            let errors = view.tree.find_error(&self.state.language, &view.source);

            views.insert(uri_str, view);

            errors
        };

        self.client
            .publish_diagnostics(
                params.text_document.uri,
                errors,
                Some(params.text_document.version),
            )
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let msg = format!("Changed file: {}", &params.text_document.uri);
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

        let views_path = {
            let file_path = &params.text_document.uri.to_file_path().unwrap_or_default();
            let workspace = self.state.workspace.read().await;
            let member = if let Some(member) = workspace.get_member_by_view(file_path) {
                member
            } else {
                error!("view {file_path:?} not found");
                return;
            };

            member.views_path.clone()
        };

        let mut new_uses_params = Vec::new();
        for (id, use_path) in new_uses {
            let use_params = self
                .state
                .find_use_params(&views_path.join(use_path))
                .await
                .unwrap_or_default();

            new_uses_params.push((id, use_params));
        }

        {
            let mut views = self.state.views.write().await;
            if let Some(view) = views.get_mut(&uri_str) {
                for (id, use_params) in new_uses_params {
                    if let Some(use_directive) = view.use_directives.get_mut(id) {
                        use_directive.2 = use_params;
                    }
                }

                view.update_use_directive_completion_items();
            }
        }

        self.client
            .publish_diagnostics(
                params.text_document.uri,
                errors,
                Some(params.text_document.version),
            )
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let msg = format!("Closed file: {}", &params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;
        let uri_str = params.text_document.uri.to_string();

        let mut views = self.state.views.write().await;
        views.remove(&uri_str);
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
        let uri = params.text_document_position.text_document.uri;
        let trigger_char = params
            .context
            .and_then(|ctx| ctx.trigger_character)
            .and_then(|s| s.chars().next());

        let views = self.state.views.read().await;

        if let Some(view) = views.get(&uri.to_string()) {
            let mut completion_items: Vec<CompletionItem> = Vec::new();

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

            return Ok(Some(CompletionResponse::List(CompletionList {
                is_incomplete: true,
                items: completion_items,
            })));
        }

        debug!("Error while getting completion items");
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
