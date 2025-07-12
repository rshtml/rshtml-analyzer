mod app_state;
mod backend;
mod consts;

use crate::app_state::AppState;
use crate::backend::Backend;
use tower_lsp::{LspService, Server};

#[cfg(debug_assertions)]
use tracing::debug;

#[tokio::main]
async fn main() {
    let filter_level = if cfg!(debug_assertions) {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(filter_level)
        .with_writer(std::io::stderr)
        .init();

    #[cfg(debug_assertions)]
    tcp_connection().await;

    #[cfg(not(debug_assertions))]
    stdio_connection().await;
}

#[cfg(debug_assertions)]
async fn tcp_connection() {
    let addr = "127.0.0.1:9257";

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    debug!("LSP server listens on TCP server: {}", addr);

    loop {
        let (stream, client_addr) = listener.accept().await.unwrap();
        debug!("New client connected: {}", client_addr);

        let (service, socket) = LspService::new(|client| Backend::new(client, AppState::setup()));

        let (read, write) = tokio::io::split(stream);

        tokio::spawn(async move {
            Server::new(read, write, socket).serve(service).await;
            debug!("Client session ended: {}", client_addr);
        });
    }
}

#[cfg(not(debug_assertions))]
async fn stdio_connection() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend::new(client, AppState::setup()));
    Server::new(stdin, stdout, socket).serve(service).await;
}
