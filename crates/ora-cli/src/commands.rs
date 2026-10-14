use clap::{Parser, Subcommand};
use ora::{AdminClient, common::LabelFilter};

use crate::{
    commands::{
        executors::Executors, job_types::Types, maintenance::Maintenance, schedules::Schedules,
    },
    output::OutputFormat,
};

pub(crate) mod executors;
pub(crate) mod job_types;
pub(crate) mod jobs;
pub(crate) mod maintenance;
pub(crate) mod schedules;

/// Parse `key` or `key=value` label filters.
///
/// Values of the same key match any of them, a label has one value per key.
pub(crate) fn parse_label_filters<S: AsRef<str>>(filters: &[S]) -> Vec<LabelFilter> {
    let mut values_by_key: Vec<(String, Vec<String>)> = Vec::new();

    for filter in filters {
        let mut parts = filter.as_ref().trim().splitn(2, '=');
        let key = parts.next().unwrap_or_default().trim();

        let values = match values_by_key.iter_mut().find(|(k, _)| k == key) {
            Some((_, values)) => values,
            None => {
                values_by_key.push((key.to_string(), Vec::new()));
                &mut values_by_key.last_mut().unwrap().1
            }
        };

        if let Some(value) = parts.next()
            && !values.iter().any(|v| v == value)
        {
            values.push(value.to_string());
        }
    }

    values_by_key
        .into_iter()
        .map(|(key, mut values)| match values.len() {
            0 => LabelFilter {
                key,
                value: None,
                value_in: None,
            },
            1 => LabelFilter {
                key,
                value: values.pop(),
                value_in: None,
            },
            _ => LabelFilter {
                key,
                value: None,
                value_in: Some(values),
            },
        })
        .collect()
}

/// Command-line client for the ora scheduler.
#[derive(Parser)]
#[clap(name = "ora", version, about)]
pub(crate) struct Cli {
    /// The URL to the ora server.
    #[arg(global = true, long)]
    pub(crate) url: Option<String>,

    /// The output format of list commands.
    #[arg(global = true, long = "output", short = 'o', default_value = "table")]
    pub(crate) output: OutputFormat,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Subcommand, Default)]
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
    // Start a tui application.
    #[default]
    Tui,
}

impl Command {
    pub(crate) async fn execute(
        self,
        client: AdminClient,
        output: OutputFormat,
    ) -> eyre::Result<()> {
        match self {
            Command::Types(job_types) => job_types.execute(client, output).await,
            Command::Jobs(jobs) => jobs.execute(client, output).await,
            Command::Schedules(schedules) => schedules.execute(client, output).await,
            Command::Executors(executors) => executors.execute(client, output).await,
            Command::Maintenance(maintenance) => maintenance.execute(client).await,
            Command::Tui => crate::tui::run(client).await,
        }
    }
}
