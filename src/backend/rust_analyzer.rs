use crate::{backend::Backend, rust_analyzer_client::RustAnalyzerClient};
use std::{
    io,
    path::{Path, PathBuf},
};
use tower_lsp::lsp_types::Url;
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

    pub async fn ra_did_open(&self, root_path: &Path, module_name: String) -> Result<(), String> {
        let module_file_path = self
            .create_module_file(root_path, module_name)
            .await
            .map_err(|e| e.to_string())?;

        let module_uri =
            Url::from_file_path(module_file_path).map_err(|_| "Coundn't convert uri")?;

        if let Some(Some(ra_client)) = self.ra_client.get() {
            let text = "#[allow(non_snake_case)]
            pub mod Card_f62e0dcfda318b70;

            use super::*;

            impl HomePage {
                pub fn __home(&self) {
                    if self.is_ok {
                        let x = \"hello home\";
                        println!(\"okk\");

                        self.__Card();
                    }
                }
            }
";
            let module_names: Vec<String> = {
                let views = self.state.views.read().await;
                views
                    .values()
                    .map(|view| view.module_name.clone())
                    .collect()
            };

            println!("module names: {:?}", module_names);
            ra_client.update_rustflags(&module_names).await;
            ra_client.did_open(module_uri, text.to_owned()).await;
        }

        Ok(())
    }

    pub async fn ra_did_change(
        &self,
        version: i32,
        root_path: PathBuf,
        module_name: String,
        new_text: String,
    ) -> Result<(), String> {
        if let Some(Some(ra_client)) = self.ra_client.get() {
            let rshtml_dir = root_path.join(".rshtml");
            let module_file_path = rshtml_dir.join(format!("{}.rs", module_name));

            let uri = Url::from_file_path(&module_file_path).map_err(|_| "Couldn't convert uri")?;

            let new_text = "#[allow(non_snake_case)]
            pub mod Card_f62e0dcfda318b70;

            use super::*;

            impl HomePage {
                pub fn __home(&self) {
                    if self.is_ok {
                        let x = \"hello home\";
                        println!(\"okk\");
                        let y = ;

                        self.__Card();
                    }
                }
            }
"
            .to_string();
            ra_client.did_change(uri.clone(), version, new_text).await;

            debug!(
                "RA did change sended,uri: {} version: {}, module file path: {:?}",
                uri, version, module_file_path
            );

            // let status = ra_client.analyzer_status(uri.clone()).await;
            // if let Some(s) = status.as_str() {
            //     println!("{}", s);
            // }
        }

        Ok(())
    }

    async fn create_module_file(
        &self,
        root_path: &Path,
        module_name: String,
    ) -> io::Result<PathBuf> {
        let rshtml_dir = root_path.join(".rshtml");
        let module_file_path = rshtml_dir.join(format!("{}.rs", module_name));

        tokio::fs::create_dir_all(&rshtml_dir).await?;

        if !module_file_path.exists() {
            tokio::fs::write(&module_file_path, "").await?;
            debug!("module file path created: {module_file_path:?}");
        }

        Ok(module_file_path)
    }
}
