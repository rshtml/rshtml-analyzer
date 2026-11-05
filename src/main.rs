mod app_state;
mod handler;
mod language_server;
mod rust_analyzer_client;

use clap::Parser;
use lsp_server::Connection;

#[derive(Parser)]
struct Cli {
    #[arg(long, help = "Run in stdio mode (used by LSP clients like Zed / VSCode)")]
    stdio: bool,

    #[arg(short = 'V', long, help = "Prints version information")]
    version: bool,
}

#[cfg(debug_assertions)]

pub const BUILD_MODE: &str = if cfg!(debug_assertions) { "dev/debug" } else { "release" };
pub type Result<T, E = Box<dyn std::error::Error + Sync + Send>> = std::result::Result<T, E>;

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.version {
        println!("rshtml-analyzer {} ({})", env!("CARGO_PKG_VERSION"), BUILD_MODE);
        return Ok(());
    }

    let filter_level = if cfg!(debug_assertions) { tracing::Level::DEBUG } else { tracing::Level::INFO };

    tracing_subscriber::fmt().with_max_level(filter_level).with_writer(std::io::stderr).init();

    // If --stdio is specified or if built in release mode, default to stdio.
    // In debug builds without --stdio, default to TCP for debugging.
    if cli.stdio || !cfg!(debug_assertions) {
        stdio_connection()?;
    } else {
        #[cfg(debug_assertions)]
        tcp_connection()?;
    }

    Ok(())
}

fn stdio_connection() -> Result<()> {
    let (connection, io_threads) = Connection::stdio();
    language_server::run(connection)?;
    io_threads.join()?;
    Ok(())
}

#[cfg(debug_assertions)]
fn tcp_connection() -> Result<()> {
    use tracing::debug;

    let addr = "127.0.0.1:9257";
    debug!("LSP server listens on TCP server: {}", addr);

    let (connection, io_threads) = Connection::listen(addr)?;
    debug!("New client connected!");

    language_server::run(connection)?;
    io_threads.join()?;
    debug!("Client session ended");

    Ok(())
}
