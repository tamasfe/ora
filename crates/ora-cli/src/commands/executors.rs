use std::fmt::Write;

use clap::Subcommand;
use comfy_table::{Table, presets};
use jiff::Timestamp;
use ora::AdminClient;

use crate::output::{OutputFormat, Record, print_records, timestamp};

#[derive(Subcommand)]
pub(crate) enum Executors {
    /// List connected executors.
    #[command(visible_aliases = ["ls"])]
    List,
}

impl Executors {
    pub(crate) async fn execute(
        self,
        client: AdminClient,
        output: OutputFormat,
    ) -> eyre::Result<()> {
        match self {
            Executors::List => {
                let mut executors = client.list_executors().await?;
                executors.sort_by(|a, b| {
                    a.name
                        .clone()
                        .unwrap_or_else(|| a.id.to_string())
                        .cmp(&b.name.clone().unwrap_or_else(|| b.id.to_string()))
                });

                if !output.is_table() {
                    let records = executors
                        .iter()
                        .map(|executor| {
                            let mut record = Record::new();
                            record.insert("id".into(), executor.id.to_string().into());
                            record.insert("name".into(), executor.name.clone().into());
                            record.insert(
                                "last_seen_at".into(),
                                timestamp(Some(executor.last_seen_at)),
                            );
                            record.insert(
                                "queues".into(),
                                executor
                                    .queues
                                    .iter()
                                    .map(|queue| {
                                        (
                                            queue.job_type_id.to_string(),
                                            format!(
                                                "{}/{}",
                                                queue.active_executions,
                                                queue.max_concurrent_executions
                                            )
                                            .into(),
                                        )
                                    })
                                    .collect::<serde_json::Map<_, _>>()
                                    .into(),
                            );
                            record
                        })
                        .collect::<Vec<_>>();

                    return print_records(output, &records);
                }

                let mut table = Table::new();
                table.load_style(presets::UTF8_FULL);

                table.set_header(["Name", "Jobs (active/max)", "Last Seen", "ID"]);

                for executor in executors {
                    let mut job_qs = String::new();

                    for queue in executor.queues {
                        if !job_qs.is_empty() {
                            job_qs.push('\n');
                        }

                        write!(
                            &mut job_qs,
                            "{} ({}/{})",
                            queue.job_type_id,
                            queue.active_executions,
                            queue.max_concurrent_executions
                        )
                        .unwrap();
                    }

                    table.add_row([
                        executor
                            .name
                            .clone()
                            .unwrap_or_else(|| executor.id.to_string()),
                        job_qs,
                        Timestamp::try_from(executor.last_seen_at)
                            .unwrap()
                            .to_string(),
                        executor.id.to_string(),
                    ]);
                }

                println!("{table}");

                Ok(())
            }
        }
    }
}
