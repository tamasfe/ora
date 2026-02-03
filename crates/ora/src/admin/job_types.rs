//! Interface for querying job type information.

use tonic::Request;

use crate::{AdminClient, JobTypeId, proto::admin::v1::ListJobTypesRequest};

/// Information about a registered job type.
pub struct JobTypeInfo {
    /// The unique identifier of the job type.
    pub id: JobTypeId,
    /// The description of the job type.
    pub description: Option<String>,
    /// The input schema JSON of the job type.
    pub input_schema_json: Option<String>,
    /// The output schema JSON of the job type.
    pub output_schema_json: Option<String>,
}

impl AdminClient {
    /// List known job types.
    pub async fn list_job_types(&self) -> crate::Result<Vec<JobTypeInfo>> {
        self.inner
            .list_job_types(Request::new(ListJobTypesRequest {}))
            .await?
            .into_inner()
            .job_types
            .into_iter()
            .map(|j| {
                Ok(JobTypeInfo {
                    id: JobTypeId::new(j.id)?,
                    description: j.description,
                    input_schema_json: j.input_schema_json,
                    output_schema_json: j.output_schema_json,
                })
            })
            .collect::<Result<Vec<_>, crate::Error>>()
    }
}
