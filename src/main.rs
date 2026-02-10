mod analysis;
mod ast;
mod completion;
mod definition;
mod diagnostics;
mod hover;
mod lexer;
mod parser;
mod references;
mod server;

use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
