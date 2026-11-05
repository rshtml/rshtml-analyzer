use crossbeam_channel::{Sender, bounded};
use lsp_server::Message;
use lsp_types::{
    CompletionParams, DidChangeTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    PublishDiagnosticsParams, TextDocumentContentChangeEvent, TextDocumentContentChangeWholeDocument,
    TextDocumentIdentifier, TextDocumentItem, Uri, VersionedTextDocumentIdentifier,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::debug;

pub struct RustAnalyzerClient {
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: AtomicU64,
    pending_requests: Arc<Mutex<HashMap<u64, Sender<Value>>>>,
    pub uri_map: Arc<Mutex<HashMap<Uri, Uri>>>,
}

impl RustAnalyzerClient {
    pub fn new(sender: Sender<Message>) -> Option<Self> {
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

        let pending_requests = Arc::new(Mutex::new(HashMap::<u64, Sender<Value>>::new()));
        let pending_requests_clone = pending_requests.clone();

        let uri_map = Arc::new(Mutex::new(HashMap::<Uri, Uri>::new()));
        let uri_map_clone = uri_map.clone();

        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);

            loop {
                let mut line = String::new();

                match reader.read_line(&mut line) {
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
                    if reader.read_line(&mut line).is_err() {
                        break;
                    }

                    let mut body = vec![0; len];
                    if reader.read_exact(&mut body).is_err() {
                        break;
                    }

                    if let Ok(json) = serde_json::from_slice::<Value>(&body) {
                        let id = json.get("id").cloned();
                        let method = json.get("method").and_then(|m| m.as_str()).map(str::to_owned);

                        match (id, method) {
                            (Some(id), Some(method)) => {
                                let result = if method == "workspace/configuration" {
                                    debug!("RA sent a pull request for the configs.");
                                    let n = json["params"]["items"].as_array().map(|a| a.len()).unwrap_or(1);
                                    let cfg = json!({
                                        "rust": { "analyzerTargetDir": true }
                                    });
                                    Value::Array(vec![cfg; n])
                                } else {
                                    debug!("RA request received: {}", method);
                                    Value::Null
                                };

                                let response = json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "result": result
                                });
                                let s = response.to_string();
                                let msg = format!("Content-Length: {}\r\n\r\n{}", s.len(), s);

                                if let Ok(mut w) = stdin_for_reader.lock() {
                                    let _ = w.write_all(msg.as_bytes());
                                    let _ = w.flush();
                                }
                            }
                            (Some(id), None) => {
                                if let Some(id) = id.as_u64()
                                    && let Ok(mut pending) = pending_requests_clone.lock()
                                    && let Some(tx) = pending.remove(&id)
                                {
                                    let _ = tx.send(json);
                                }
                            }
                            (None, Some(method)) => {
                                if method == "textDocument/publishDiagnostics" {
                                    if let Some(params) = json.get("params")
                                        && let Ok(diag_params) =
                                            serde_json::from_value::<PublishDiagnosticsParams>(params.clone())
                                    {
                                        debug!("RA Diagnostics Received:\n{:#?}", params);

                                        let original_uri = uri_map_clone
                                            .lock()
                                            .ok()
                                            .and_then(|map| map.get(&diag_params.uri).cloned());

                                        if let Some(html_uri) = original_uri {
                                            debug!("RA diagnostics will send to editor, Uri: {:#?}", html_uri);

                                            let _ = crate::language_server::publish_diagnostics(
                                                &sender,
                                                &html_uri,
                                                diag_params.diagnostics,
                                                diag_params.version,
                                            );
                                        }
                                    }
                                } else if method == "window/showMessage"
                                    || method == "window/logMessage"
                                    || method == "experimental/serverStatus"
                                {
                                    debug!("RA {}: {:#?}", method, json.get("params"));
                                } else {
                                    debug!("RA notification: {}", method);
                                }
                            }
                            _ => {
                                debug!("RA unknown message:\n{:#?}", json);
                            }
                        }
                    }
                }
            }
        });

        Some(Self { stdin, next_id: AtomicU64::new(1), pending_requests, uri_map })
    }

    fn send_notification(&self, method: &str, params: Value) {
        let message = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params
        });
        self.write_to_stdin(message);
    }

    fn send_request_without_wait(&self, method: &str, params: Value) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        self.write_to_stdin(message);
    }

    fn send_request(&self, method: &str, params: Value) -> Value {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let message = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let (tx, rx) = bounded(1);
        if let Ok(mut pending) = self.pending_requests.lock() {
            pending.insert(id, tx);
        }

        self.write_to_stdin(message);

        rx.recv_timeout(Duration::from_secs(1)).unwrap_or_else(|e| {
            if let Ok(mut pending) = self.pending_requests.lock() {
                pending.remove(&id);
            }
            debug!("No response received from RA for method {}: {}", method, e);
            Value::Null
        })
    }

    fn write_to_stdin(&self, json_msg: Value) {
        let msg_string = json_msg.to_string();
        let lsp_msg = format!("Content-Length: {}\r\n\r\n{}", msg_string.len(), msg_string);

        if let Ok(mut stdin) = self.stdin.lock() {
            stdin.write_all(lsp_msg.as_bytes()).unwrap_or_else(|e| debug!("Failed to write to RA stdin: {}", e));
            stdin.flush().unwrap_or_else(|e| debug!("Failed to flush RA stdin: {}", e));
        }
    }

    pub fn initialize(&self, root_uri: Option<Uri>) -> Value {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "initializationOptions": {
                "rust": { "analyzerTargetDir": true }
            },
            "capabilities": {
                "textDocument": {
                    "synchronization": {
                        "didSave": true,
                        "dynamicRegistration": false,
                        "willSave": false,
                        "willSaveWaitUntil": false
                    },
                    "completion": {
                        "contextSupport": true,
                        "completionItem": {
                            "snippetSupport": true
                        }
                    }
                }
            }
        });

        let response = self.send_request("initialize", params);
        debug!("RA Initialize process completed successfully.");
        response
    }

    pub fn initialized(&self) {
        self.send_notification("initialized", json!({}));
        debug!("RA Handshake done!");
    }

    pub fn did_open(&self, uri: Uri, text: String) {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem { uri, language_id: "rust".into(), version: 1, text },
        };
        self.send_notification("textDocument/didOpen", serde_json::to_value(params).unwrap());
        debug!("RA open file request sent.");

        self.send_request_without_wait("rust-analyzer/reloadWorkspace", json!(null));
        debug!("RA workspace reload request sent");
    }

    pub fn did_change(&self, uri: Uri, version: i32, new_text: String) {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                text_document_identifier: TextDocumentIdentifier { uri },
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument { text: new_text },
            )],
        };

        self.send_notification("textDocument/didChange", serde_json::to_value(params).unwrap());
    }

    pub fn completion(&self, params: CompletionParams) -> Value {
        let response = self.send_request("textDocument/completion", serde_json::to_value(params).unwrap());

        if let Some(err) = response.get("error") {
            debug!("(COMPLETION) RA completion error: {err}");
        }

        response.get("result").cloned().unwrap_or(Value::Null)
    }

    pub fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.send_notification("textDocument/didSave", serde_json::to_value(params).unwrap());
    }

    pub fn did_close(&self, uri: Uri) {
        let params = json!({ "textDocument": { "uri": uri } });
        self.send_notification("textDocument/didClose", params);
    }

    #[allow(dead_code)]
    pub fn analyzer_status(&self, uri: Uri) -> String {
        let params = json!({ "textDocument": { "uri": uri } });
        let response = self.send_request("rust-analyzer/analyzerStatus", params);

        response.get("result").unwrap_or(&response).as_str().unwrap_or_default().to_string()
    }
}
