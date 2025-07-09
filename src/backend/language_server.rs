use crate::app_state::view::View;
use crate::backend::Backend;
use crate::backend::server_capabilities::{semantic_tokens_capabilities, workspace_capabilities};
use crate::backend::tree_extensions::TreeExtensions;
use tower_lsp::jsonrpc::{Error};
use tower_lsp::lsp_types::{CompletionItem, CompletionList, CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, InitializeParams, InitializeResult, InitializedParams, MessageType, SemanticTokens, SemanticTokensParams, SemanticTokensRangeParams, SemanticTokensRangeResult, SemanticTokensResult, ServerCapabilities, ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind};
use tower_lsp::{LanguageServer, jsonrpc};
use tracing::{debug, error};

// TODO: use tree-sitter-rust for rust highlights - compile it with ast

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
                .log_message(MessageType::INFO, format!("Workspace root path: {:?}", path))
                .await;

            let mut workspace = self.state.workspace.write().unwrap();
            workspace.load(&path).unwrap_or_else(|e| {
                debug!("Workspace couldn't load: {}", e);
            });
        }

        debug!("Sending an initialize response.");
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::INCREMENTAL)),
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
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client.log_message(MessageType::INFO, "rshtml LSP initialized!").await;
    }

    async fn shutdown(&self) -> Result<(), Error> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let msg = format!("Opened file: {}", &params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;

        let uri_str = params.text_document.uri.to_string();
        let text = params.text_document.text;

        let tree = if let Ok(mut parser) = self.state.parser.lock()
            && let Some(tree) = parser.parse(&text, None)
        {
            tree
        } else {
            self.client
                .log_message(MessageType::ERROR, "Parser error: Couldn't create tree.")
                .await;
            return;
        };

        let include_paths = tree.find_includes(&self.state.language, &text);
        debug!("Include paths: {:?}", include_paths);

        let use_directives = tree.find_uses(&self.state.language, &text);
        debug!("Use directives: {:?}", use_directives);

        let extends = tree.find_extends(&self.state.language, &text);
        let layout_path = extends.and_then(|extends|
            self.state.find_layout(&params.text_document.uri, extends.as_deref()));
        debug!("Layout path: {:?}", layout_path);

        let section_names = tree.find_sections(&self.state.language, &text);
        debug!("Sections: {:?}", section_names);

        let errors = {
            let mut view = View::new(text, tree, params.text_document.version as usize);
            view.layout_path = layout_path;
            view.include_paths = include_paths;
            view.use_directives = use_directives;
            view.create_use_directive_completion_items();
            view.section_names = section_names;
            view.create_section_completion_items();

            let mut views = self.state.views.write().unwrap();

            let errors = view.tree.find_error(&self.state.language, &view.source);

            views.insert(uri_str, view);

            errors
        };

        self.client
            .publish_diagnostics(params.text_document.uri, errors, Some(params.text_document.version))
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let msg = format!("Changed file: {}", &params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;

        let uri_str = params.text_document.uri.to_string();

        let errors = if let Ok(mut views) = self.state.views.write()
            && let Some(view) = views.get_mut(&uri_str)
        {
            if view.version >= params.text_document.version as usize {
                return;
            }

            self.process_changes(params.content_changes, &mut view.source, &mut view.tree);

            if let Ok(mut parser) = self.state.parser.lock()
                && let Some(tree) = parser.parse(&view.source, Some(&view.tree))
            {
                let extends = tree.find_extends(&self.state.language, &view.source);
                let layout_path = extends.and_then(|extends|
                    self.state.find_layout(&params.text_document.uri, extends.as_deref()));

                let include_paths = tree.find_includes(&self.state.language, &view.source);
                let use_directives = tree.find_uses(&self.state.language, &view.source);
                let section_names = tree.find_sections(&self.state.language, &view.source);

                view.version = params.text_document.version as usize;
                view.tree = tree;
                view.layout_path = layout_path;
                view.include_paths = include_paths;
                view.use_directives = use_directives;
                view.update_use_directive_completion_items();
                view.section_names = section_names;

                let errors = view.tree.find_error(&self.state.language, &view.source);

                errors
            } else {
                error!("Error while parsing tree");
                return;
            }
        } else {
            error!("Error while locked views");
            return;
        };

        self.client
            .publish_diagnostics(params.text_document.uri, errors, Some(params.text_document.version))
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let msg = format!("Closed file: {}", &params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;
        let uri_str = params.text_document.uri.to_string();

        if let Ok(mut views) = self.state.views.write() {
            views.remove(&uri_str);
        }
    }

    async fn semantic_tokens_full(&self, params: SemanticTokensParams) -> Result<Option<SemanticTokensResult>, Error> {
        let uri_str = params.text_document.uri.to_string();

        if let Ok(views) = self.state.views.write()
            && let Some(view) = views.get(&uri_str)
        {
            let highlight = &self.state.highlight;
            let tokens = highlight.highlight(&view.source, None)?;

            debug!("Semantic Tokens: {:?}", tokens.len());

            return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: tokens,
            })));
        }

        Ok(None)
    }

    async fn semantic_tokens_range(&self, params: SemanticTokensRangeParams) -> jsonrpc::Result<Option<SemanticTokensRangeResult>> {
        let uri_str = params.text_document.uri.to_string();
        let range = params.range;

        if let Ok(views) = self.state.views.write()
            && let Some(view) = views.get(&uri_str)
        {
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

    async fn completion(&self, params: CompletionParams) -> jsonrpc::Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let trigger_char = params.context.and_then(|ctx| ctx.trigger_character).and_then(|s| s.chars().next());

        if let Ok(views) = self.state.views.read()
            && let Some(view) = views.get(&uri.to_string())
        {
            let mut completion_items: Vec<CompletionItem> = Vec::new();

            if let Some(tc) = trigger_char {
                for items in view.completion_items.values() {
                    for (item_char, item) in items {
                        if *item_char == tc {
                            completion_items.push(item.clone());
                        }
                    }
                }

                if tc == '@' {
                    completion_items.extend(self.state.completion_items.clone());
                }
            } else {
                for items in view.completion_items.values() {
                    for (_, item) in items {
                        completion_items.push(item.clone());
                    }
                }

                completion_items.extend(self.state.completion_items.clone());
            }

            if Some('@') == trigger_char || None == trigger_char {
                for v in views.values() {
                    let is_layout = uri.to_file_path().map(|x| v.layout_path == Some(x)).unwrap_or(false);
                    if is_layout {
                        let items = v
                            .completion_items
                            .iter()
                            .filter_map(|(name, value)| {
                                if name.starts_with("section_") {
                                    value.first().map(|x| x.1.clone())
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>();

                        completion_items.extend(items);
                    }
                }
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
        let cargo_toml_changed = params.changes.iter().any(|event| event.uri.path().ends_with("/Cargo.toml"));

        if !cargo_toml_changed {
            return;
        }

        debug!("Cargo.toml changed. Re-analyzing...");

        if let Ok(mut workspace) = self.state.workspace.write() {
            let root = workspace.root.clone();
            workspace.load(&root).unwrap_or_else(|e| {
                debug!("Workspace couldn't load: {}", e);
            });
        }

        debug!("Workspace re-analysis complete.");
    }
}
