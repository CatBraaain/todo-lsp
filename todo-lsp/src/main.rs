mod analysis;
mod archive;
mod commands;
mod format;
mod line;
mod lsp;
mod parse;
mod repeat;

use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(lsp::Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}
