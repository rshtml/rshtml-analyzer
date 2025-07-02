// use crate::backend::Backend;
// use std::path::PathBuf;
// use std::str::FromStr;
// use tower_lsp::lsp_types::{DidOpenTextDocumentParams, MessageType};
// use crate::config::Config;
//
// pub async fn load_config(backend: &Backend, params: &DidOpenTextDocumentParams) -> Config {
//     fn load(backend: &Backend, params: &DidOpenTextDocumentParams) -> Result<Config, String> {
//         let template_path = params
//             .text_document
//             .uri
//             .to_file_path()
//             .map_err(|_| "Invalid URI")?;
//
//         let root = backend
//             .workspace_root
//             .get()
//             .ok_or("Workspace root not initialized yet.")?;
//
//         let manifest_dir = backend
//             .find_manifest_dir(root, &template_path)
//             .ok_or("Couldnt find Cargo.toml.")?;
//
//         let cargo_toml_path = manifest_dir.join("Cargo.toml");
//
//         Config::load_from_path(cargo_toml_path).ok_or("Could not load config.".to_string())
//     }
//
//     match load(backend, params) {
//         Ok(config) => config,
//         Err(e) => {
//             backend
//                 .client
//                 .log_message(MessageType::WARNING, e.to_string())
//                 .await;
//
//             Config::new((
//                 PathBuf::from_str("layout.rs.html").unwrap(),
//                 "./views".to_string(),
//             ))
//         }
//     }
// }
