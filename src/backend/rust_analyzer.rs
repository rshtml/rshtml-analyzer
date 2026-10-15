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

            // let response = ra_client.analyzer_status(module_file_uri.clone()).await;
            // println!("{}", response);
        }

        Ok(())
    }

    pub async fn ra_did_change(
        &self,
        version: i32,
        uri: Url,
        extract_source: String,
    ) -> Result<(), String> {
        if let Some(Some(ra_client)) = self.ra_client.get() {
            // println!(
            //     "(DID CHANGE) Uri: {}, Extracted Source: {}",
            //     uri.clone(),
            //     extract_source
            // );

            ra_client
                .did_change(uri.clone(), version, extract_source)
                .await;

            debug!(
                "(DID CHANGE) RA did change sended, uri: {} version: {}",
                uri, version
            );

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
        // debug!("(COMPLATION) uri: {uri:?}, \n\nparams: {params:?}");

        if let Some(Some(ra_client)) = self.ra_client.get() {
            let t = std::time::Instant::now();
            let completion_value = ra_client.completion(params).await;
            debug!("(COMPLATION) RA completion took {:?}", t.elapsed());
            // debug!("(COMPLATION) RA completion value: {:?}", completion_value);

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
}
