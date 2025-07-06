mod app_state;
mod backend;
mod consts;
mod load_config;

use crate::app_state::AppState;
use crate::backend::Backend;
use tower_lsp::{LspService, Server};
use tracing::info;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[tokio::main]
async fn main() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,rshtml_analyzer=debug"));
    let subscriber = FmtSubscriber::builder()
         .with_env_filter(filter)
        .finish();

    tracing::subscriber::set_global_default(subscriber)
           .expect("setting default subscriber failed");

    // let filter_level = if cfg!(debug_assertions) {
    //     tracing::Level::DEBUG
    // } else {
    //     tracing::Level::INFO
    // };
    //
    // tracing_subscriber::fmt()
    //     .with_max_level(filter_level)
    //     .with_writer(std::io::stderr)
    //     .init();

    // let stdin = tokio::io::stdin();
    // let stdout = tokio::io::stdout();

    let addr = "127.0.0.1:9257";

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    info!("LSP sunucusu TCP üzerinden dinliyor: {}", addr);

    loop {
        let (stream, client_addr) = listener.accept().await.unwrap();
        info!("Yeni istemci bağlandı: {}", client_addr);

        //let app_state_clone = app_state.clone();

        let (service, socket) = LspService::new(|client| Backend::new(client, AppState::setup()));

        let (read, write) = tokio::io::split(stream);

        tokio::spawn(async move {
            Server::new(read, write, socket).serve(service).await;
            info!("İstemci oturumu sonlandı: {}", client_addr);
        });
    }
}
