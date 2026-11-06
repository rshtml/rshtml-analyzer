pub mod tree_extensions;
use crate::app_state::view::NavigationTarget;
use crate::app_state::view::SyntaxContext;
use crate::{
    BUILD_MODE, Result,
    app_state::{
        AppState,
        view::{FIRST_LINE_SHIFT, PARAMS_SHIFT, View},
    },
    handler::tree_extensions::TreeExtensions,
    language_server::{log_message, publish_diagnostics, show_message},
    rust_analyzer_client::RustAnalyzerClient,
};
use lsp_server::Connection;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemTextEdit, CompletionList, CompletionParams, CompletionResponse,
    Contents, Definition, DefinitionParams, DefinitionResponse, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    Hover, HoverParams, InitializeParams, InsertTextFormat, Location, MarkupContent, MarkupKind, MessageType, Range,
    TextDocumentIdentifier, Uri, WorkspaceFolders,
};
use tracing::debug;

pub fn initialize(conn: &Connection, state: &mut AppState, params: &InitializeParams) -> Result<()> {
    debug!("The Initialize request has been received (build: {})...", BUILD_MODE);

    let workspace_root_uri = params
        .workspace_folders_initialize_params
        .workspace_folders
        .as_ref()
        .and_then(|folders| match folders {
            WorkspaceFolders::WorkspaceFolderList(list) => list.first(),
            WorkspaceFolders::Null => None,
        })
        .map(|folder| &folder.uri);

    debug!("workspace root uri: {:?}", workspace_root_uri);

    if let Some(path) = workspace_root_uri.as_ref().and_then(|uri| uri.to_file_path().ok()) {
        debug!("Workspace root path: {}", path.display());
        log_message(&conn.sender, MessageType::Info, format!("Workspace root path: {}", path.display()))?;

        state.workspace.load(path).unwrap_or_else(|e| {
            debug!("Workspace couldn't load: {}, operating in single-file mode", e);
        });
    }

    let ra_option = RustAnalyzerClient::new(conn.sender.clone());
    if let Some(ra) = &ra_option {
        #[allow(deprecated)]
        ra.initialize(workspace_root_uri);
        ra.initialized();
    }
    state.ra_client = ra_option;

    Ok(())
}

pub fn initialized(conn: &Connection) -> Result<()> {
    let version_msg = format!("rshtml-analyzer v{} ({}) connected!", env!("CARGO_PKG_VERSION"), BUILD_MODE);

    if cfg!(debug_assertions) {
        show_message(&conn.sender, MessageType::Info, &version_msg)?;
    }

    log_message(&conn.sender, MessageType::Info, format!("{version_msg} Handshake completed."))?;

    Ok(())
}

pub fn did_open(conn: &Connection, state: &mut AppState, params: DidOpenTextDocumentParams) -> Result<()> {
    let msg = format!("Opened file: {}", params.text_document.uri);
    log_message(&conn.sender, MessageType::Info, &msg)?;
    debug!(msg);

    let uri_str = params.text_document.uri.to_string();
    let text = params.text_document.text;

    let Some(tree) = state.parser.parse(&text, None) else {
        log_message(&conn.sender, MessageType::Error, "Parser error: Couldn't create tree.")?;
        return Ok(());
    };

    let use_directives = tree.find_uses(&state.language, &text);
    debug!("Use directives: {:?}", use_directives);

    let mut use_directives_with_params = Vec::new();
    for (use_path, use_name) in &use_directives {
        let use_params = if let Ok(uri) = params.text_document.uri.join(use_path) {
            state.find_use_params(&uri).unwrap_or_default()
        } else {
            Vec::new()
        };
        debug!("use params: {use_params:?}");
        use_directives_with_params.push((use_path.to_owned(), use_name.to_owned(), use_params));
    }

    let template_params = tree.find_template_params(&state.language, &text);
    debug!("Template params: {:?}", template_params);

    let extract_file = (|| -> Option<(Uri, std::path::PathBuf)> {
        let path = params
            .text_document
            .uri
            .to_file_path()
            .map_err(|()| {
                debug!("invalid uri: {}", &params.text_document.uri);
            })
            .ok()?;

        let extract_file_path = state
            .workspace
            .generate_extract_file_path(&path)
            .map_err(|e| debug!("generate_module_path error: {}", e))
            .ok()?;

        let extract_file_uri = Uri::from_file_path(&extract_file_path)
            .map_err(|()| debug!("Couldn't convert file to uri: {:?}", extract_file_path))
            .ok()?;

        Some((extract_file_uri, extract_file_path))
    })();

    let errors = {
        let mut view = View::new(
            text.clone(),
            tree.clone(),
            params.text_document.version,
            extract_file.as_ref().map(|x| x.0.clone()),
        );
        view.use_directives = use_directives_with_params;
        view.create_use_directive_completion_items();
        view.template_params = template_params;

        let errors = view.tree.find_error(&state.language, &view.source);

        if cfg!(debug_assertions) {
            view.debug_check_source_mismatch(&state.language, extract_file.as_ref());
        }

        state.views.insert(uri_str, view);

        errors
    };

    let _ = (|| -> Option<()> {
        let (extract_file_uri, extract_file_path) = extract_file?;

        let extract_text = std::fs::read_to_string(&extract_file_path)
            .inspect_err(|e| debug!("Failed to read file at '{}': {}", extract_file_path.display(), e))
            .ok()?;

        let ra_client = state.ra_client.as_ref()?;

        ra_client.uri_map.lock().ok()?.insert(extract_file_uri.clone(), params.text_document.uri.clone());

        ra_client.did_open(extract_file_uri, extract_text);

        Some(())
    })();

    publish_diagnostics(&conn.sender, &params.text_document.uri, errors, Some(params.text_document.version))?;

    Ok(())
}

pub fn did_change(conn: &Connection, state: &mut AppState, params: DidChangeTextDocumentParams) -> Result<()> {
    let uri = params.text_document.text_document_identifier.uri;
    let version = params.text_document.version;

    let msg = format!("Changed file: {uri}");
    log_message(&conn.sender, MessageType::Info, msg)?;

    let uri_str = uri.to_string();

    let (new_uses, errors) = {
        let Some(view) = state.views.get_mut(&uri_str) else {
            debug!("view {uri_str} not found");
            return Ok(());
        };

        if view.version >= version {
            return Ok(());
        }

        view.process_changes(params.content_changes);

        let Some(tree) = state.parser.parse(&view.source, Some(&view.tree)) else {
            debug!("Error while parsing tree");
            return Ok(());
        };

        let use_directives = tree.find_uses(&state.language, &view.source);
        let template_params = tree.find_template_params(&state.language, &view.source);

        view.version = version;
        view.tree = tree;
        let new_uses = view.sync_use_directives(use_directives);

        view.template_params = template_params;

        (new_uses, view.tree.find_error(&state.language, &view.source))
    };

    let mut new_uses_params = Vec::new();
    for (id, use_path) in new_uses {
        let use_params = if let Ok(joined_uri) = uri.join(&use_path) {
            state.find_use_params(&joined_uri).unwrap_or_default()
        } else {
            Vec::new()
        };

        new_uses_params.push((id, use_params));
    }

    let extract_file = if let Some(view) = state.views.get_mut(&uri_str) {
        for (id, use_params) in new_uses_params {
            if let Some(use_directive) = view.use_directives.get_mut(id) {
                use_directive.2 = use_params;
            }
        }

        view.update_use_directive_completion_items();

        let extract_source = view.extract_source(&state.language);

        Some((view.extract_file.clone(), extract_source))
    } else {
        None
    };

    let _ = (|| -> Option<()> {
        let (extract_file_uri, extract_source) = extract_file.and_then(|(u, s)| Some((u?, s)))?;

        let ra_client = state.ra_client.as_ref()?;

        debug!("(DID CHANGE) RA did change will send, uri: {} version: {}", extract_file_uri, version);

        ra_client.did_change(extract_file_uri, version, extract_source);

        debug!("(DID CHANGE) RA did change sent");

        Some(())
    })();

    publish_diagnostics(&conn.sender, &uri, errors, Some(version))?;

    Ok(())
}

pub fn did_close(conn: &Connection, state: &mut AppState, params: DidCloseTextDocumentParams) -> Result<()> {
    let uri = params.text_document.uri;
    log_message(&conn.sender, MessageType::Info, format!("Closed file: {uri}"))?;

    let extract_file = state.views.remove(uri.as_str()).and_then(|view| view.extract_file);

    if let Some(extract_file_uri) = extract_file
        && let Some(ra_client) = state.ra_client.as_ref()
    {
        ra_client.did_close(&extract_file_uri);
    }

    Ok(())
}

pub fn did_save(_conn: &Connection, state: &mut AppState, params: &DidSaveTextDocumentParams) {
    debug!("(DID SAVE) triggered for file: {}", params.text_document.uri);

    let _ = (|| -> Option<()> {
        let Some(view) = state.views.get(params.text_document.uri.as_str()) else {
            debug!("(DID SAVE) error while getting completion items");
            return None;
        };

        let Some(extract_file_uri) = view.extract_file.clone() else {
            debug!("(DID SAVE) error while getting extract file uri");
            return None;
        };

        let save_params =
            DidSaveTextDocumentParams { text_document: TextDocumentIdentifier { uri: extract_file_uri }, text: None };

        let ra_client = state.ra_client.as_ref()?;
        ra_client.did_save(save_params);
        debug!("(DID SAVE) RA save request sent");

        Some(())
    })();
}

pub mod completion {
    #[allow(clippy::wildcard_imports)]
    use super::*;

    pub fn completion(state: &mut AppState, params: &CompletionParams) -> Option<CompletionResponse> {
        let uri = params.text_document_position_params.text_document.uri.as_str();
        let position = params.text_document_position_params.position;
        let trigger_char =
            params.context.as_ref().and_then(|ctx| ctx.trigger_character.as_ref()).and_then(|s| s.chars().next());

        let mut shift = FIRST_LINE_SHIFT;

        let Some(view) = state.views.get(uri) else {
            debug!("Error while getting completion items");
            return None;
        };
        let syntax_context = view.detect_syntax_context(position);
        debug!("(COMPLETION) ctx: {:?}, pos: {:?}", syntax_context, position);

        let mut completion_items: Vec<CompletionItem> = Vec::new();

        let extract_file: Option<Uri> = if syntax_context == SyntaxContext::Html {
            if let Some(tc) = trigger_char {
                for (item_char, item) in view.completion_items.values() {
                    if *item_char == tc {
                        completion_items.push(item.clone());
                    }
                }

                if tc == '@' {
                    completion_items.extend(state.completion_items.clone());
                    view.extract_file.clone()
                } else {
                    None
                }
            } else {
                for (_, item) in view.completion_items.values() {
                    completion_items.push(item.clone());
                }

                completion_items.extend(state.completion_items.clone());

                None
            }
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
                    for (name, params) in view.use_directives_names_and_params() {
                        if name == component_name {
                            for param in params {
                                completion_items.push(CompletionItem {
                                    insert_text: Some(format!("{param}=\"$1\"")),
                                    detail: Some(format!("{component_name} attribute")),
                                    sort_text: Some(format!("0_{param}")),
                                    label: param,
                                    kind: Some(CompletionItemKind::Property),
                                    insert_text_format: Some(InsertTextFormat::Snippet),
                                    ..Default::default()
                                });
                            }
                            break;
                        }
                    }

                    None
                }
                SyntaxContext::RustCode => view.extract_file.clone(),
                SyntaxContext::TemplateParams => {
                    shift = PARAMS_SHIFT;
                    view.extract_file.clone()
                }
                _ => None,
            }
        };

        debug!("Extract File: {:?}", extract_file);

        let ra_items = fetch_ra_items(state, params, extract_file, shift);

        if let Some(tg) = trigger_char
            && tg == '@'
        {
            completion_items.extend(ra_items.into_iter().flatten().filter(|item| match item.kind {
                Some(CompletionItemKind::Variable) => true,
                Some(CompletionItemKind::Function) => {
                    ["json(...)", "json_let(...)", "time(...)"].contains(&item.label.as_str())
                }
                _ => false,
            }));
        } else {
            completion_items.extend(ra_items.into_iter().flatten());
        }

        Some(CompletionResponse::CompletionList(CompletionList {
            is_incomplete: true,
            items: completion_items,
            ..Default::default()
        }))
    }

    fn fetch_ra_items(
        state: &AppState,
        params: &CompletionParams,
        extract_file: Option<Uri>,
        shift: u32,
    ) -> Option<Vec<CompletionItem>> {
        let mut ra_params = params.clone();
        ra_params.text_document_position_params.text_document.uri = extract_file?;

        if ra_params.text_document_position_params.position.line == 0 {
            ra_params.text_document_position_params.position.character += shift;
        }

        let ra_client = state.ra_client.as_ref()?;

        debug!(
            "(COMPLETION) RA completion will send, uri: ({})",
            &ra_params.text_document_position_params.text_document.uri
        );
        let completion_value = ra_client.completion(ra_params);
        debug!("(COMPLETION) RA completion sent");

        let response = Some(completion_value).filter(|v| !v.is_null()).and_then(|v| {
            serde_json::from_value::<CompletionResponse>(v)
                .inspect_err(|e| debug!("RA Completion JSON parse error: {}", e))
                .ok()
        })?;

        let mut items = match response {
            CompletionResponse::CompletionItemList(items) => items,
            CompletionResponse::CompletionList(completion_list) => completion_list.items,
        };

        unshift(&mut items, shift);
        Some(items)
    }

    fn unshift(items: &mut [CompletionItem], shift: u32) {
        for item in items {
            if let Some(CompletionItemTextEdit::TextEdit(e)) = &mut item.text_edit {
                if e.range.start.line == 0 {
                    e.range.start.character = e.range.start.character.saturating_sub(shift);
                }
                if e.range.end.line == 0 {
                    e.range.end.character = e.range.end.character.saturating_sub(shift);
                }
            }
        }
    }
}

pub fn goto_definition(state: &mut AppState, params: &DefinitionParams) -> Option<DefinitionResponse> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    if let Some(view) = state.views.get(uri.as_str())
        && let Some(target) = view.resolve_navigation_target(position, uri)
    {
        let target_uri = match target {
            NavigationTarget::Component { target_uri, .. } | NavigationTarget::UseDirective { target_uri, .. } => {
                target_uri
            }
        };
        return Some(DefinitionResponse::Definition(Definition::Location(Location {
            uri: target_uri,
            range: Range::default(),
        })));
    }

    None
}

pub fn hover(state: &mut AppState, params: &HoverParams) -> Option<Hover> {
    let uri = &params.text_document_position_params.text_document.uri;
    let position = params.text_document_position_params.position;

    if let Some(view) = state.views.get(uri.as_str())
        && let Some(target) = view.resolve_navigation_target(position, uri)
    {
        let (range, markdown) = match target {
            NavigationTarget::Component { name, target_uri, range, params } => {
                let params_doc = if params.is_empty() {
                    "_No parameters_".to_string()
                } else {
                    params.iter().map(|p| format!("- `{p}`")).collect::<Vec<_>>().join("\n")
                };

                let file_name =
                    target_uri.path_segments().and_then(|mut segments| segments.next_back()).unwrap_or("<unknown>");

                let doc =
                    format!("### Component `<{name}>`\n\n**File:** `{file_name}`\n\n**Parameters:**\n{params_doc}");

                (range, doc)
            }
            NavigationTarget::UseDirective { path, target_uri: _, range } => {
                let doc = format!("### Component Import\n\n`{path}`");
                (range, doc)
            }
        };

        return Some(Hover {
            contents: Contents::MarkupContent(MarkupContent { kind: MarkupKind::Markdown, value: markdown }),
            range: Some(range),
        });
    }

    None
}

pub fn did_change_watched_files(state: &mut AppState, params: &DidChangeWatchedFilesParams) {
    let cargo_toml_changed = params.changes.iter().any(|event| event.uri.path().ends_with("/Cargo.toml"));

    if !cargo_toml_changed {
        return;
    }

    debug!("Cargo.toml changed. Re-analyzing...");

    let root = state.workspace.root.clone();
    state.workspace.load(root).unwrap_or_else(|e| {
        debug!("Workspace couldn't load: {}", e);
    });

    debug!("Workspace re-analysis complete.");
}
