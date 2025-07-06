use crate::app_state::view::View;
use crate::backend::Backend;
use crate::backend::process_highlights::process_highlights;
use crate::backend::server_capabilities::{semantic_tokens_capabilities, workspace_capabilities};
use crate::backend::tree_extensions::TreeExtensions;
use tower_lsp::LanguageServer;
use tower_lsp::jsonrpc::{Error, ErrorCode};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidChangeWatchedFilesParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, InitializeParams, InitializeResult, InitializedParams, MessageType,
    SemanticTokens, SemanticTokensParams, SemanticTokensResult, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind,
};
use tracing::{debug, error};

// TODO: layout içerisinde olunup olunmadığın layout içerisindeyse section ların önerilmesini sağla.

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
                semantic_tokens_provider: semantic_tokens_capabilities(),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec!["@".to_string(), "<".to_string()]),
                    ..Default::default()
                }),
                workspace: workspace_capabilities(),
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

        let tree = {
            let mut parser = self.state.parser.lock().unwrap();
            parser.parse(&text, None)
        };

        let tree = if let Some(tree) = tree {
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
        let layout_path = extends.and_then(|extends| self.find_layout(&params.text_document.uri, extends.as_deref()));
        debug!("Layout path: {:?}", layout_path);

        let section_names = tree.find_sections(&self.state.language, &text);
        debug!("Sections: {:?}", section_names);

        {
            let mut view = View::new(text, tree, params.text_document.version as usize);
            view.layout_path = layout_path;
            view.include_paths = include_paths;
            view.use_directives = use_directives;
            view.create_use_directive_completion_items();
            view.section_names = section_names;

            let mut views = self.state.views.write().unwrap();
            views.insert(uri_str, view);
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let msg = format!("Changed file: {}", &params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;

        let uri_str = params.text_document.uri.to_string();
        let tree = {
            let mut views = self.state.views.write().unwrap();
            let view = match views.get_mut(&uri_str) {
                Some(v) => v,
                None => {
                    error!("Received change for untracked file: {}", uri_str);
                    return;
                }
            };

            if view.version >= params.text_document.version as usize {
                return;
            }

            self.process_changes(params.content_changes, &mut view.source, &mut view.tree);

            {
                let mut parser = self.state.parser.lock().unwrap();
                parser.parse(&view.source, Some(&view.tree))
            }
        };

        let tree = if let Some(tree) = tree {
            tree
        } else {
            self.client
                .log_message(MessageType::ERROR, "Parser error: Couldn't create tree.")
                .await;
            return;
        };

        let mut views = self.state.views.write().unwrap();
        let view = views.get_mut(&uri_str).unwrap();

        let extends = tree.find_extends(&self.state.language, &view.source);
        let layout_path = extends.and_then(|extends| self.find_layout(&params.text_document.uri, extends.as_deref()));

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
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let msg = format!("Closed file: {}", &params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;

        let mut views = self.state.views.write().unwrap();
        let uri_str = params.text_document.uri.to_string();
        views.remove(&uri_str);
    }

    // TODO: implement semantic_token_full_delta and range
    async fn semantic_tokens_full(&self, params: SemanticTokensParams) -> Result<Option<SemanticTokensResult>, Error> {
        let uri_str = params.text_document.uri.to_string();
        let views = self.state.views.write().unwrap();

        let view = views.get(&uri_str).ok_or(Error::new(ErrorCode::InvalidParams))?;

        //debug!("Highlights source: {}", view.source);
        let highlight = &self.state.highlight;

        let mut highlighter = self.state.highlight.highlighter.lock().unwrap();
        let highlight_events = highlighter
            .highlight(&highlight.highlight_config, view.source.as_bytes(), None, |lang_name| {
                highlight.highlight_injects.get(lang_name)
            })
            .map_err(|e| {
                error!("Error during highlighting: {}", e);
                Error::new(ErrorCode::InternalError)
            })?;

        let highlight_names: Vec<&str> = highlight.highlight_names.iter().map(|s| s.as_str()).collect();

        let tokens = process_highlights(&view.source, highlight_events, &highlight_names, &highlight.token_type_map)?;

        //debug!("Tokens: {:?}", tokens);

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn completion(&self, params: CompletionParams) -> tower_lsp::jsonrpc::Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let trigger_char = params.context.and_then(|ctx| ctx.trigger_character).and_then(|s| s.chars().next());

        let views = self.state.views.read().unwrap();
        let view = views.get(&uri.to_string()).ok_or(Error::new(ErrorCode::InvalidParams))?;

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

        Ok(Some(CompletionResponse::Array(completion_items)))
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let cargo_toml_changed = params.changes.iter().any(|event| event.uri.path().ends_with("/Cargo.toml"));

        if !cargo_toml_changed {
            return;
        }

        debug!("Cargo.toml changed. Re-analyzing...");

        {
            let mut workspace = self.state.workspace.write().unwrap();
            let root = workspace.root.clone();
            workspace.load(&root).unwrap_or_else(|e| {
                debug!("Workspace couldn't load: {}", e);
            });
        }

        debug!("Workspace re-analysis complete.");
    }
}
