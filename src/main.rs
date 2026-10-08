mod analysis;
mod ast;
mod checks;
mod completion;
mod definition;
mod diagnostics;
mod folding;
mod hover;
mod lexer;
mod parser;
mod references;
mod server;
mod settings;
mod sources;
mod symbols;

use tower_lsp::{LspService, Server};

#[tokio::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
