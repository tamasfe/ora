//! Interface for miscellaneous maintenance operations.

use std::time::SystemTime;

use tonic::Request;

use crate::{AdminClient, proto::admin::v1::DeleteHistoricalDataRequest};

impl AdminClient {
    /// List known job types.
    pub async fn delete_historical_data(&self, before: SystemTime) -> crate::Result<()> {
        self.inner
            .delete_historical_data(Request::new(DeleteHistoricalDataRequest {
                before: Some(before.into()),
            }))
            .await?;

        Ok(())
    }
}
