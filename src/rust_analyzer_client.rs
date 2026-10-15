use serde_json::{Value, json};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{Mutex, oneshot};
use tower_lsp::Client;
use tower_lsp::lsp_types::{
    ClientCapabilities, CompletionClientCapabilities, CompletionItemCapability, CompletionParams,
    DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    InitializeParams, PublishDiagnosticsParams, TextDocumentClientCapabilities,
    TextDocumentContentChangeEvent, TextDocumentItem, TextDocumentSyncClientCapabilities, Url,
    VersionedTextDocumentIdentifier,
};
use tracing::{debug, error};

pub struct RustAnalyzerClient {
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicU64,
    pending_requests: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    pub uri_map: Arc<Mutex<HashMap<Url, Url>>>,
}

impl RustAnalyzerClient {
    pub async fn new(client: Client) -> Option<Self> {
        let mut child = Command::new("rust-analyzer")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .inspect_err(|e| debug!("rust-analyzer could not be started ({}).", e))
            .ok()?;

        let stdin = Arc::new(Mutex::new(child.stdin.take()?));
        let stdin_for_reader = stdin.clone();
        let stdout = child.stdout.take()?;

        let pending_requests = Arc::new(Mutex::new(HashMap::<u64, oneshot::Sender<Value>>::new()));
        let pending_requests_clone = pending_requests.clone();

        let uri_map = Arc::new(Mutex::new(HashMap::<Url, Url>::new()));
        let uri_map_clone = uri_map.clone();

        tokio::spawn(async move {
            let mut reader = BufReader::new(stdout);

            loop {
                let mut line = String::new();

                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }

                if line.starts_with("Content-Length:") {
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() < 2 {
                        continue;
                    }

                    let len: usize = match parts[1].trim().parse() {
                        Ok(l) => l,
                        Err(_) => continue,
                    };

                    line.clear();
                    if reader.read_line(&mut line).await.is_err() {
                        break;
                    }

                    let mut body = vec![0; len];
                    if reader.read_exact(&mut body).await.is_err() {
                        break;
                    }

                    if let Ok(json) = serde_json::from_slice::<Value>(&body) {
                        let id = json.get("id").cloned();
                        let method = json
                            .get("method")
                            .and_then(|m| m.as_str())
                            .map(str::to_owned);

                        match (id, method) {
                            (Some(id), Some(method)) => {
                                let result = if method == "workspace/configuration" {
                                    debug!("RA sent a pull request for the configs.");
                                    let n = json["params"]["items"]
                                        .as_array()
                                        .map(|a| a.len())
                                        .unwrap_or(1);
                                    let cfg = json!({
                                        "rust": { "analyzerTargetDir": true }
                                    });
                                    Value::Array(vec![cfg; n])
                                } else {
                                    debug!("RA request received: {}", method);
                                    Value::Null
                                };

                                let response = serde_json::json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": result
                                });
                                let s = response.to_string();
                                let msg = format!("Content-Length: {}\r\n\r\n{}", s.len(), s);

                                let mut w = stdin_for_reader.lock().await;
                                let _ = w.write_all(msg.as_bytes()).await;
                                let _ = w.flush().await;
                            }
                            (Some(id), None) => {
                                if let Some(id) = id.as_u64()
                                    && let Some(tx) =
                                        pending_requests_clone.lock().await.remove(&id)
                                {
                                    let _ = tx.send(json);
                                }
                            }
                            (None, Some(method)) => {
                                if method == "textDocument/publishDiagnostics" {
                                    if let Some(params) = json.get("params")
                                        && let Ok(mut diag_params) =
                                            serde_json::from_value::<PublishDiagnosticsParams>(
                                                params.clone(),
                                            )
                                    {
                                        tracing::debug!("RA Diagnostics Received:\n{:#?}", params);

                                        let original_uri = {
                                            let map = uri_map_clone.lock().await;
                                            map.get(&diag_params.uri).cloned()
                                        };
                                        if let Some(html_uri) = original_uri {
                                            diag_params.uri = html_uri;
                                            tracing::debug!(
                                                "RA Diagnostics Received, Uri: {:#?}",
                                                diag_params.uri
                                            );

                                            client
                                                .publish_diagnostics(
                                                    diag_params.uri,
                                                    diag_params.diagnostics,
                                                    diag_params.version,
                                                )
                                                .await;
                                        }
                                    }
                                } else if method == "window/showMessage"
                                    || method == "window/logMessage"
                                    || method == "experimental/serverStatus"
                                {
                                    tracing::debug!("RA {}: {:#?}", method, json.get("params"));
                                } else {
                                    tracing::debug!("RA notification: {}", method);
                                }
                            }
                            _ => {
                                tracing::debug!("RA unknown message:\n{:#?}", json);
                            }
                        }
                    }
                }
            }
        });

        Some(Self {
            stdin,
            next_id: AtomicU64::new(1),
            pending_requests,
            uri_map,
        })
    }

    async fn send_notification(&self, method: &str, params: Value) {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.write_to_stdin(message).await;
    }

    async fn send_request(&self, method: &str, params: Value) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let (tx, rx) = oneshot::channel();
        self.pending_requests.lock().await.insert(id, tx);

        self.write_to_stdin(message).await;

        rx.await.unwrap_or_else(|_| {
            tracing::error!(
                "The channel was closed; no response was received for method: {}",
                method
            );
            serde_json::Value::Null
        })
    }

    async fn write_to_stdin(&self, json_msg: Value) {
        let msg_string = json_msg.to_string();
        let lsp_msg = format!("Content-Length: {}\r\n\r\n{}", msg_string.len(), msg_string);

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(lsp_msg.as_bytes())
            .await
            .unwrap_or_else(|e| error!("Failed to write to RA stdin: {}", e));
        stdin
            .flush()
            .await
            .unwrap_or_else(|e| error!("Failed to flush RA stdin: {}", e));
    }

    pub async fn initialize(&self, root_uri: Option<Url>) -> Value {
        let initialization_options = json!({ "rust": { "analyzerTargetDir": true }});

        let client_capabilities = ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                synchronization: Some(TextDocumentSyncClientCapabilities {
                    did_save: Some(true),
                    dynamic_registration: Some(false),
                    will_save: Some(false),
                    will_save_wait_until: Some(false),
                }),
                completion: Some(CompletionClientCapabilities {
                    context_support: Some(true),
                    completion_item: Some(CompletionItemCapability {
                        snippet_support: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let params = InitializeParams {
            root_uri,
            capabilities: client_capabilities,
            initialization_options: Some(initialization_options),
            ..Default::default()
        };

        let response = self
            .send_request("initialize", serde_json::to_value(params).unwrap())
            .await;

        debug!("RA Initialize process completed successfully.");

        response
    }

    pub async fn initialized(&self) {
        self.send_notification("initialized", json!({})).await;
        debug!("RA Handshake done!");
    }

    pub async fn did_open(&self, uri: Url, text: String) {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "rust".to_string(),
                version: 1,
                text,
            },
        };
        self.send_notification(
            "textDocument/didOpen",
            serde_json::to_value(params).unwrap(),
        )
        .await;
        debug!("RA open file request sent.");

        self.send_request("rust-analyzer/reloadWorkspace", json!(null))
            .await;
        debug!("RA workspace reload request sent");
    }

    pub async fn did_change(&self, uri: Url, version: i32, new_text: String) {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier { uri, version },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: new_text,
            }],
        };

        self.send_notification(
            "textDocument/didChange",
            serde_json::to_value(params).unwrap(),
        )
        .await;
    }

    pub async fn completion(&self, params: CompletionParams) -> Value {
        let response = self
            .send_request(
                "textDocument/completion",
                serde_json::to_value(params).unwrap(),
            )
            .await;

        if let Some(err) = response.get("error") {
            debug!("(COMPLATION) RA completion error: {err}");
        }

        response.get("result").cloned().unwrap_or(Value::Null)
    }

    pub async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let response = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didSave",
            "params": params
        });

        let s = response.to_string();
        let msg = format!("Content-Length: {}\r\n\r\n{}", s.len(), s);

        let mut w = self.stdin.lock().await;
        let _ = w.write_all(msg.as_bytes()).await;
        let _ = w.flush().await;
    }

    #[allow(dead_code)]
    pub async fn analyzer_status(&self, uri: Url) -> String {
        let params = serde_json::json!({ "textDocument": { "uri": uri } });

        let response = self
            .send_request("rust-analyzer/analyzerStatus", params)
            .await;

        response
            .get("result")
            .unwrap_or(&response)
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}
