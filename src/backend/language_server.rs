use crate::backend::Backend;
use crate::backend::process_highlights::process_highlights;
use crate::backend::server_capabilities::server_capabilities;
use tower_lsp::LanguageServer;
use tower_lsp::jsonrpc::{Error, ErrorCode};
use tower_lsp::lsp_types::{
    CompletionOptions, CompletionParams, CompletionResponse, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, InitializeParams, InitializeResult, InitializedParams, MessageType, SemanticTokens,
    SemanticTokensParams, SemanticTokensResult, ServerCapabilities, ServerInfo, TextDocumentSyncCapability,
    TextDocumentSyncKind,
};
use tracing::{error, info};
use tree_sitter::Point;

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult, Error> {
        info!("The Initialize request has been received and is being processed...");
        let workspace_root_path = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|folder| folder.uri.to_file_path().ok());

        if let Some(path) = workspace_root_path {
            info!("Workspace root path: {:?}", path);
            println!("Workspace root path: {:?}", path);
            self.client
                .log_message(MessageType::INFO, format!("Workspace root path: {:?}", path))
                .await;

            let mut workspace = self.state.workspace.write().unwrap();
            workspace.load(&path).unwrap_or_else(|e| {
                info!("Workspace couldn't load: {}", e);
            });
        }

        let semantic_tokens_provider = server_capabilities();

        info!("Sending an initialize response.");
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::INCREMENTAL)),
                semantic_tokens_provider,
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec!["@".to_string()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "rshtml-analyzer".to_string(),
                version: Some("0.1.0".to_string()),
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
            let mut parser = self.state.parser.lock().unwrap();
            parser.parse(&text, None)
        };

        let mut views = self.state.views.write().unwrap();
        views.insert(uri_str, (tree, text, params.text_document.version as usize));
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let msg = format!("Changed file: {}", &params.text_document.uri);
        self.client.log_message(MessageType::INFO, msg).await;

        let uri_str = params.text_document.uri.to_string();

        let mut views = self.state.views.write().unwrap();
        let view = match views.get_mut(&uri_str) {
            Some(v) => v,
            None => {
                error!("Received change for untracked file: {}", uri_str);
                return;
            }
        };

        if view.2 >= params.text_document.version as usize {
            return;
        }

        for change in params.content_changes {
            if let Some(range) = change.range {
                //info!("Change detected in file: {} at range: {:?}", uri_str, range);

                let start_byte = Self::position_to_byte_offset(&view.1, range.start);
                let end_byte = Self::position_to_byte_offset(&view.1, range.end);

                if let Some(tree) = &mut view.0 {
                    let edit = tree_sitter::InputEdit {
                        start_byte,
                        old_end_byte: end_byte,
                        new_end_byte: start_byte + change.text.len(),
                        start_position: Point {
                            row: range.start.line as usize,
                            column: range.start.character as usize,
                        },
                        old_end_position: Point {
                            row: range.end.line as usize,
                            column: range.end.character as usize,
                        },
                        new_end_position: Self::calculate_new_end_point(range.start, &change.text),
                    };
                    tree.edit(&edit);
                }

                view.1.replace_range(start_byte..end_byte, &change.text);
            } else {
                view.1 = change.text;
                view.0 = None;

                break;
            }
        }

        let mut parser = self.state.parser.lock().unwrap();

        view.0 = parser.parse(&view.1, view.0.as_ref());
        view.2 = params.text_document.version as usize;
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

        info!("Highlights source: {}", view.1);
        let highlight = &self.state.highlight;

        let mut highlighter = self.state.highlight.highlighter.lock().unwrap();
        let highlight_events = highlighter
            .highlight(&highlight.highlight_config, view.1.as_bytes(), None, |lang_name| {
                highlight.highlight_injects.get(lang_name)
            })
            .map_err(|e| {
                error!("Error during highlighting: {}", e);
                Error::new(ErrorCode::InternalError)
            })?;

        let highlight_names: Vec<&str> = highlight.highlight_names.iter().map(|s| s.as_str()).collect();

        let tokens = process_highlights(&view.1, highlight_events, &highlight_names, &highlight.token_type_map)?;

        info!("Tokens: {:?}", tokens);

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }

    async fn completion(&self, _: CompletionParams) -> tower_lsp::jsonrpc::Result<Option<CompletionResponse>> {
        Ok(Some(CompletionResponse::Array(self.state.completion_items.clone())))
    }
}
