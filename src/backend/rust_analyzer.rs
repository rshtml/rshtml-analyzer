use crate::{
    backend::{
        Backend,
        extract::{Extract, Snippet},
    },
    rust_analyzer_client::RustAnalyzerClient,
};
use std::path::Path;
use tokio::fs;
use tower_lsp::lsp_types::{CompletionItem, CompletionParams, CompletionResponse, Url};
use tracing::debug;

impl Backend {
    pub async fn ra_initialize(&self, root_uri: Option<Url>) {
        let ra_option = RustAnalyzerClient::new().await;
        if let Some(ra) = &ra_option {
            ra.initialize(root_uri).await;
            ra.initialized().await;
        }
        let _ = self.ra_client.set(ra_option);
    }

    pub async fn ra_did_open(
        &self,
        module_file_uri: &Url,
        module_file_path: &Path,
    ) -> Result<(), String> {
        // TODO: Based on the `use` directives of the component, those that are active will be added to the source as modules.
        // When a component is opened, the system will check if it appears as a `use` directive in any of the currently open components;
        // if so, it will update the source. Additions can be made at the very bottom.

        let module_text = fs::read_to_string(&module_file_path).await.map_err(|e| {
            format!(
                "Failed to read file at '{}': {}",
                module_file_path.display(),
                e
            )
        })?;

        // assert_eq!(extracted_source.as_bytes(), module_text.as_bytes());

        if let Some(Some(ra_client)) = self.ra_client.get() {
            // println!("text: {}", module_text);
            ra_client
                .did_open(module_file_uri.to_owned(), module_text)
                .await;

            let response = ra_client.analyzer_status(module_file_uri.clone()).await;
            println!("{}", response);
        }

        Ok(())
    }

    pub async fn ra_did_change(
        &self,
        version: i32,
        uri: Url,
        cleaned_source: Vec<u8>,
        snippets: Vec<Snippet>,
    ) -> Result<(), String> {
        if let Some(Some(ra_client)) = self.ra_client.get() {
            let fn_params = snippets
                .iter()
                .filter_map(|s| match s {
                    Snippet::Param { name: (_, n), ty } => {
                        let clean_name = n.strip_prefix("self ").unwrap_or(n).trim();
                        let clean_ty = match ty {
                            Some((_, t)) => t.as_str().trim(),
                            None => "impl ::rshtml::View",
                        };
                        Some(format!("{}: {}", clean_name, clean_ty))
                    }
                    _ => None,
                })
                .collect::<Vec<String>>()
                .join(", ");

            let mut extract = Extract::new(cleaned_source);
            for snippet in snippets {
                extract.push(snippet);
            }
            extract.extract();

            let extract_source = format!(
                "fn render(child_content: impl ::rshtml::View, {1}) -> ::std::fmt::Result {{ {2} {0}",
                "Ok(()) } use super::*;use ::rshtml::{Render, View};use std::fmt::Display;",
                fn_params,
                String::from_utf8_lossy(&extract.source)
            );

            println!(
                "(DID CHANGE) Uri: {}, Extracted Source: {}",
                uri.clone(),
                extract_source
            );

            ra_client
                .did_change(uri.clone(), version, extract_source)
                .await;

            debug!("RA did change sended, uri: {} version: {}", uri, version);

            // let response = ra_client.analyzer_status(uri.clone()).await;
            // println!("{}", response);
        }

        Ok(())
    }

    pub async fn ra_completion(
        &self,
        uri: Url,
        mut params: CompletionParams,
    ) -> Result<Vec<CompletionItem>, String> {
        params.text_document_position.text_document.uri = uri.to_owned();
        debug!("COMPLATION uri: {uri:?}, \n\nparams: {params:?}");

        if let Some(Some(ra_client)) = self.ra_client.get() {
            let completion_value = ra_client.completion(params).await;
            debug!("RA completion value: {:?}", completion_value);
            if completion_value.is_null() {
                return Ok(Vec::new());
            }

            let response = serde_json::from_value::<CompletionResponse>(completion_value)
                .map_err(|e| format!("RA Completion JSON parse error: {}", e))?;

            match response {
                CompletionResponse::Array(items) => {
                    return Ok(items);
                }
                CompletionResponse::List(completion_list) => {
                    return Ok(completion_list.items);
                }
            }
        }

        Ok(Vec::new())
    }

    // async fn create_module_file(
    //     &self,
    //     root_path: &Path,
    //     module_name: String,
    // ) -> io::Result<PathBuf> {
    //     let rshtml_dir = root_path.join(".rshtml");
    //     let module_file_path = rshtml_dir.join(format!("{}.rs", module_name));

    //     tokio::fs::create_dir_all(&rshtml_dir).await?;

    //     if !module_file_path.exists() {
    //         tokio::fs::write(&module_file_path, "").await?;
    //         debug!("module file path created: {module_file_path:?}");
    //     }

    //     Ok(module_file_path)
    // }
}
