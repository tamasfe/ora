//! Command-line client for the ora scheduler.

use clap::{CommandFactory, Parser};
use ora::proto::admin::v1::admin_service_client::AdminServiceClient;
use tonic::transport::Endpoint;
use tracing::Level;

use crate::commands::Cli;

mod commands;
mod completions;
mod tui;

fn main() {
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();

    cmd_main();
}

#[tokio::main(flavor = "current_thread")]
async fn cmd_main() {
    tracing_subscriber::fmt()
        .with_target(false)
        .without_time()
        .with_env_filter(
            tracing_subscriber::filter::EnvFilter::builder()
                .with_default_directive(Level::INFO.into())
                .from_env_lossy(),
        )
        .init();

    let cli = Cli::parse();

    let Some(url) = cli.url.or_else(|| std::env::var("ORA_URL").ok()) else {
        tracing::error!(
            "Error: No server URL provided. Use --url or set ORA_URL environment variable."
        );
        std::process::exit(1);
    };

    let client = ora::AdminClient::new(AdminServiceClient::new(
        Endpoint::from_shared(url).unwrap().connect_lazy(),
    ));

    if let Err(error) = cli.command.unwrap_or_default().execute(client).await {
        tracing::error!(%error, "fatal error");
        std::process::exit(1);
    }
}
