
use clap::Subcommand;
use eyre::Context;
use jiff::{Timestamp, civil::Time};
use ora::AdminClient;

#[derive(Subcommand)]
pub(crate) enum Maintenance {
    /// Delete inactive history.
    DeleteHistory {
        /// An optional timestamp.
        ///
        /// All inactive historical data
        /// before this timestamp will be deleted.
        #[arg(long)]
        before: Option<String>,
    },
}

impl Maintenance {
    pub(crate) async fn execute(self, client: AdminClient) -> eyre::Result<()> {
        match self {
            Maintenance::DeleteHistory { before } => {
                let before_ts = if let Some(before) = before {
                    parse_ts_or_date(&before)?
                } else {
                    Timestamp::now()
                };

                tracing::info!("deleting inactive history, this might take a while...");

                client
                    .delete_historical_data(before_ts.into())
                    .await
                    .wrap_err("failed to delete historical data")?;

                Ok(())
            }
        }
    }
}

fn parse_ts_or_date(ts: &str) -> eyre::Result<Timestamp> {
    if let Ok(ts) = ts.parse::<Timestamp>() {
        return Ok(ts);
    }

    Ok(jiff::fmt::strtime::parse("%Y-%m-%d", ts)?
        .to_date()?
        .to_datetime(Time::midnight())
        .in_tz("UTC")?
        .timestamp())
}
