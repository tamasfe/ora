use clap::{Parser, Subcommand};
use ora::AdminClient;

use crate::commands::{
    executors::Executors, job_types::Types, maintenance::Maintenance, schedules::Schedules,
};

pub(crate) mod executors;
pub(crate) mod job_types;
pub(crate) mod jobs;
pub(crate) mod maintenance;
pub(crate) mod schedules;

/// Command-line client for the ora scheduler.
#[derive(Parser)]
#[clap(name = "ora", version, about)]
pub(crate) struct Cli {
    /// The URL to the ora server.
    #[arg(global = true, long)]
    pub(crate) url: Option<String>,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub(crate) enum Command {
    /// Job type commands.
    #[command(subcommand)]
    Types(Types),
    /// Job commands.
    #[command(subcommand)]
    Jobs(jobs::Jobs),
    /// Schedule commands.
    #[command(subcommand)]
    Schedules(Schedules),
    /// Executor commands.
    #[command(subcommand)]
    Executors(Executors),
    // Maintenance commands.
    #[command(subcommand)]
    Maintenance(Maintenance),
}

impl Command {
    pub(crate) async fn execute(self, client: AdminClient) -> eyre::Result<()> {
        match self {
            Command::Types(job_types) => job_types.execute(client).await,
            Command::Jobs(jobs) => jobs.execute(client).await,
            Command::Schedules(schedules) => schedules.execute(client).await,
            Command::Executors(executors) => executors.execute(client).await,
            Command::Maintenance(maintenance) => maintenance.execute(client).await,
        }
    }
}
