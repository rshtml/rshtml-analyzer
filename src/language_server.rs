use crate::{
    BUILD_MODE, Result,
    app_state::AppState,
    handler::{
        completion::completion, did_change, did_change_watched_files, did_close, did_open, did_save, goto_definition,
        hover, initialize, initialized,
    },
};
use crossbeam_channel::Sender;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    ChangeNotifications, CompletionOptions, CompletionRequest, DefinitionProvider, DefinitionRequest, Diagnostic,
    DidChangeTextDocumentNotification, DidChangeWatchedFilesNotification, DidCloseTextDocumentNotification,
    DidOpenTextDocumentNotification, DidSaveTextDocumentNotification, HoverProvider, HoverRequest, InitializeParams,
    InitializeResult, LogMessageNotification, LogMessageParams, MessageType, Notification as _,
    PublishDiagnosticsNotification, PublishDiagnosticsParams, Request as _, Save, ServerCapabilities, ServerInfo,
    ShowMessageNotification, ShowMessageParams, TextDocumentSync, TextDocumentSyncKind, TextDocumentSyncOptions, Uri,
    WorkspaceFoldersServerCapabilities, WorkspaceOptions,
};
use serde_json::{from_value, to_value};
use tracing::debug;

pub fn run(connection: &Connection) -> Result<()> {
    let (id, init_params) = connection.initialize_start()?;
    let init_params: InitializeParams = serde_json::from_value(init_params)?;

    let mut state = AppState::setup();

    initialize(connection, &mut state, &init_params)?;

    debug!("Sending an initialize response.");
    connection.initialize_finish(id, to_value(initialize_result())?)?;

    initialized(connection)?;

    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    break;
                }
                if let Err(err) = handle_request(connection, &req, &mut state) {
                    debug!("[lsp] request {} failed: {err}", req.method);
                }
            }
            Message::Notification(note) => {
                if let Err(err) = handle_notification(connection, &note, &mut state) {
                    debug!("[lsp] notification {} failed: {err}", note.method);
                }
            }
            Message::Response(resp) => {
                debug!("[lsp] response: {resp:?}");
            }
        }
    }
    Ok(())
}

pub fn initialize_result() -> InitializeResult {
    InitializeResult {
        capabilities: ServerCapabilities {
            text_document_sync: Some(TextDocumentSync::Options(TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::Incremental),
                save: Some(Save::Bool(true)),
                will_save: None,
                will_save_wait_until: None,
            })),
            // semantic_tokens_provider: Some(SemanticTokensProvider::SemanticTokensOptions(SemanticTokensOptions {
            //     legend: SemanticTokensLegend {
            //         token_types: SEMANTIC_TOKEN_TYPES.iter().map(|t| t.as_str().to_string()).collect(),
            //         token_modifiers: SEMANTIC_TOKEN_MODIFIERS.iter().map(|m| m.as_str().to_string()).collect(),
            //     },
            //     range: Some(SemanticTokensOptionsRange::Bool(true)),
            //     full: Some(Full::SemanticTokensFullDelta(SemanticTokensFullDelta { delta: Some(true) })),
            //     ..Default::default()
            // })),
            completion_provider: Some(CompletionOptions {
                resolve_provider: Some(false),
                trigger_characters: Some(vec!["@".into(), "<".into(), ":".into(), ".".into(), "'".into(), "(".into()]),
                ..Default::default()
            }),
            definition_provider: Some(DefinitionProvider::Bool(true)),
            hover_provider: Some(HoverProvider::Bool(true)),
            workspace: Some(WorkspaceOptions {
                workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                    supported: Some(false),
                    change_notifications: Some(ChangeNotifications::Bool(true)),
                }),
                file_operations: None,
                ..Default::default()
            }),
            ..Default::default()
        },
        server_info: Some(ServerInfo {
            name: "rshtml-analyzer".to_string(),
            version: Some(format!("{} ({})", env!("CARGO_PKG_VERSION"), BUILD_MODE)),
        }),
    }
}

// =====================================================================
// Dispatchers
// ======================================================================

fn handle_notification(conn: &Connection, note: &Notification, state: &mut AppState) -> Result<()> {
    match note.method.as_str().into() {
        DidOpenTextDocumentNotification::METHOD => did_open(conn, state, from_value(note.params.clone())?)?,
        DidChangeTextDocumentNotification::METHOD => did_change(conn, state, from_value(note.params.clone())?)?,
        DidSaveTextDocumentNotification::METHOD => did_save(conn, state, &from_value(note.params.clone())?),
        DidCloseTextDocumentNotification::METHOD => did_close(conn, state, from_value(note.params.clone())?)?,
        DidChangeWatchedFilesNotification::METHOD => did_change_watched_files(state, &from_value(note.params.clone())?),
        _ => {}
    }

    Ok(())
}

fn handle_request(conn: &Connection, req: &Request, state: &mut AppState) -> Result<()> {
    match req.method.as_str().into() {
        HoverRequest::METHOD => {
            let response = hover(state, &from_value(req.params.clone())?);
            send_ok(&conn.sender, req.id.clone(), &response)?;
        }
        DefinitionRequest::METHOD => {
            let response = goto_definition(state, &from_value(req.params.clone())?);
            send_ok(&conn.sender, req.id.clone(), &response)?;
        }
        CompletionRequest::METHOD => {
            let response = completion(state, &from_value(req.params.clone())?);
            send_ok(&conn.sender, req.id.clone(), &response)?;
        }
        _ => {
            send_err(&conn.sender, req.id.clone(), ErrorCode::MethodNotFound, "unhandled method")?;
        }
    }
    Ok(())
}

// =====================================================================
//  (Diagnostics & Response Sending)
// =====================================================================

pub fn publish_diagnostics(
    sender: &Sender<Message>,
    uri: &Uri,
    diagnostics: Vec<Diagnostic>,
    version: Option<i32>,
) -> Result<()> {
    let params = PublishDiagnosticsParams { uri: uri.clone(), diagnostics, version };
    sender.send(Message::Notification(Notification::new(PublishDiagnosticsNotification::METHOD.into(), params)))?;
    Ok(())
}

fn send_ok<T: serde::Serialize>(sender: &Sender<Message>, id: RequestId, result: &T) -> Result<()> {
    let resp = Response::new_ok(id, serde_json::to_value(result)?);
    sender.send(Message::Response(resp))?;
    Ok(())
}

fn send_err(sender: &Sender<Message>, id: RequestId, code: ErrorCode, msg: &str) -> Result<()> {
    let resp = Response::new_err(id, code as i32, msg.into());
    sender.send(Message::Response(resp))?;
    Ok(())
}

pub fn log_message(sender: &Sender<Message>, kind: MessageType, message: impl Into<String>) -> Result<()> {
    let params = LogMessageParams { kind, message: message.into() };
    sender.send(Message::Notification(Notification::new(LogMessageNotification::METHOD.into(), params)))?;
    Ok(())
}

pub fn show_message(sender: &Sender<Message>, kind: MessageType, message: impl Into<String>) -> Result<()> {
    let params = ShowMessageParams { kind, message: message.into() };
    sender.send(Message::Notification(Notification::new(ShowMessageNotification::METHOD.into(), params)))?;
    Ok(())
}
