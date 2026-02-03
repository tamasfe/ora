use clap::Subcommand;
use clap_complete::ArgValueCompleter;
use comfy_table::{Table, presets};
use eyre::Context;
use ora::AdminClient;

use crate::completions::complete_job_type;

#[derive(Subcommand)]
pub(crate) enum Types {
    /// List available job types.
    #[command(visible_aliases = ["ls"])]
    List,
    /// Get the input schema for a job type.
    ///
    /// Fails if the job type does not exist
    /// or does not have an input schema.
    InputSchema {
        /// The job type to get the input schema for.
        #[arg(
            long = "type",
            aliases = ["type-id", "job-type", "job-type-id"],
            add = ArgValueCompleter::new(complete_job_type),
        )]
        job_type_id: String,
    },
    /// Get the output schema for a job type.
    ///
    /// Fails if the job type does not exist
    /// or does not have an output schema.
    OutputSchema {
        /// The job type to get the output schema for.
        #[arg(
            long = "type",
            aliases = ["type-id", "job-type", "job-type-id"],
            add = ArgValueCompleter::new(complete_job_type),
        )]
        job_type_id: String,
    },
}

impl Types {
    pub(crate) async fn execute(self, client: AdminClient) -> eyre::Result<()> {
        match self {
            Types::List => {
                let mut job_types = client.list_job_types().await?;
                job_types.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

                let mut table = Table::new();
                table.load_preset(presets::NOTHING);
                table.set_style(comfy_table::TableComponent::HeaderLines, '=');

                table.set_header(["Name", "Description"]);

                for job_type in job_types {
                    table.add_row([
                        job_type.id.as_str(),
                        job_type.description.as_deref().unwrap_or(""),
                    ]);
                }

                println!("{table}");

                Ok(())
            }
            Types::InputSchema { job_type_id } => {
                let ty = client
                    .list_job_types()
                    .await?
                    .into_iter()
                    .find(|jt| jt.id.as_str() == job_type_id)
                    .ok_or_else(|| eyre::eyre!("job type '{job_type_id}' not found"))?;

                if let Some(input_schema) = ty.input_schema_json {
                    let input_schema = serde_json::to_string_pretty(
                        &serde_json::from_str::<serde_json::Value>(&input_schema)
                            .wrap_err("invalid input schema")?,
                    )?;

                    println!("{input_schema}");
                } else {
                    eyre::bail!("job type '{job_type_id}' does not have an input schema");
                }

                Ok(())
            }
            Types::OutputSchema { job_type_id } => {
                let ty = client
                    .list_job_types()
                    .await?
                    .into_iter()
                    .find(|jt| jt.id.as_str() == job_type_id)
                    .ok_or_else(|| eyre::eyre!("job type '{job_type_id}' not found"))?;

                if let Some(output_schema) = ty.output_schema_json {
                    let output_schema = serde_json::to_string_pretty(
                        &serde_json::from_str::<serde_json::Value>(&output_schema)
                            .wrap_err("invalid output schema")?,
                    )?;

                    println!("{output_schema}");
                } else {
                    eyre::bail!("job type '{job_type_id}' does not have an output schema");
                }

                Ok(())
            }
        }
    }
}
