use tonic::{Request, Response, async_trait};

use crate::proto::admin::v1::{
    AddJobsRequest, AddJobsResponse, AddSchedulesRequest, AddSchedulesResponse, CancelJobsRequest,
    CancelJobsResponse, CountJobsRequest, CountJobsResponse, CountSchedulesRequest,
    CountSchedulesResponse, DeleteHistoricalDataRequest, DeleteHistoricalDataResponse,
    ListExecutorsRequest, ListExecutorsResponse, ListJobTypesRequest, ListJobTypesResponse,
    ListJobsRequest, ListJobsResponse, ListSchedulesRequest, ListSchedulesResponse,
    StopSchedulesRequest, StopSchedulesResponse, admin_service_client::AdminServiceClient,
};

/// A type-erased admin client.
#[async_trait]
pub(super) trait AdminClientInner: Send + Sync + 'static {
    async fn add_jobs(
        &self,
        request: Request<AddJobsRequest>,
    ) -> tonic::Result<Response<AddJobsResponse>>;

    async fn list_jobs(
        &self,
        request: Request<ListJobsRequest>,
    ) -> tonic::Result<Response<ListJobsResponse>>;

    async fn cancel_jobs(
        &self,
        request: Request<CancelJobsRequest>,
    ) -> tonic::Result<Response<CancelJobsResponse>>;

    async fn count_jobs(
        &self,
        request: Request<CountJobsRequest>,
    ) -> tonic::Result<Response<CountJobsResponse>>;

    async fn add_schedules(
        &self,
        request: Request<AddSchedulesRequest>,
    ) -> tonic::Result<Response<AddSchedulesResponse>>;

    async fn list_schedules(
        &self,
        request: Request<ListSchedulesRequest>,
    ) -> tonic::Result<Response<ListSchedulesResponse>>;

    async fn count_schedules(
        &self,
        request: Request<CountSchedulesRequest>,
    ) -> tonic::Result<Response<CountSchedulesResponse>>;

    async fn stop_schedules(
        &self,
        request: Request<StopSchedulesRequest>,
    ) -> tonic::Result<Response<StopSchedulesResponse>>;

    async fn list_job_types(
        &self,
        request: Request<ListJobTypesRequest>,
    ) -> tonic::Result<Response<ListJobTypesResponse>>;

    async fn list_executors(
        &self,
        request: Request<ListExecutorsRequest>,
    ) -> tonic::Result<Response<ListExecutorsResponse>>;

    async fn delete_historical_data(
        &self,
        request: Request<DeleteHistoricalDataRequest>,
    ) -> tonic::Result<Response<DeleteHistoricalDataResponse>>;
}

#[async_trait]
impl<T> AdminClientInner for AdminServiceClient<T>
where
    T: Clone + Send + Sync + 'static,
    T: tonic::client::GrpcService<tonic::body::Body>,
    T::Error: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    T::ResponseBody: http_body::Body<Data = bytes::Bytes> + std::marker::Send + 'static,
    <T::ResponseBody as http_body::Body>::Error:
        Into<Box<dyn std::error::Error + Send + Sync + 'static>> + std::marker::Send,
    <T as tonic::client::GrpcService<tonic::body::Body>>::Future: Send,
{
    async fn add_jobs(
        &self,
        request: Request<AddJobsRequest>,
    ) -> tonic::Result<Response<AddJobsResponse>> {
        AdminServiceClient::<T>::add_jobs(&mut self.clone(), request).await
    }

    async fn list_jobs(
        &self,
        request: Request<ListJobsRequest>,
    ) -> tonic::Result<Response<ListJobsResponse>> {
        AdminServiceClient::<T>::list_jobs(&mut self.clone(), request).await
    }

    async fn cancel_jobs(
        &self,
        request: Request<CancelJobsRequest>,
    ) -> tonic::Result<Response<CancelJobsResponse>> {
        AdminServiceClient::<T>::cancel_jobs(&mut self.clone(), request).await
    }

    async fn count_jobs(
        &self,
        request: Request<CountJobsRequest>,
    ) -> tonic::Result<Response<CountJobsResponse>> {
        AdminServiceClient::<T>::count_jobs(&mut self.clone(), request).await
    }

    async fn add_schedules(
        &self,
        request: Request<AddSchedulesRequest>,
    ) -> tonic::Result<Response<AddSchedulesResponse>> {
        AdminServiceClient::<T>::add_schedules(&mut self.clone(), request).await
    }

    async fn list_schedules(
        &self,
        request: Request<ListSchedulesRequest>,
    ) -> tonic::Result<Response<ListSchedulesResponse>> {
        AdminServiceClient::<T>::list_schedules(&mut self.clone(), request).await
    }

    async fn count_schedules(
        &self,
        request: Request<CountSchedulesRequest>,
    ) -> tonic::Result<Response<CountSchedulesResponse>> {
        AdminServiceClient::<T>::count_schedules(&mut self.clone(), request).await
    }

    async fn stop_schedules(
        &self,
        request: Request<StopSchedulesRequest>,
    ) -> tonic::Result<Response<StopSchedulesResponse>> {
        AdminServiceClient::<T>::stop_schedules(&mut self.clone(), request).await
    }

    async fn list_job_types(
        &self,
        request: Request<ListJobTypesRequest>,
    ) -> tonic::Result<Response<ListJobTypesResponse>> {
        AdminServiceClient::<T>::list_job_types(&mut self.clone(), request).await
    }

    async fn list_executors(
        &self,
        request: Request<ListExecutorsRequest>,
    ) -> tonic::Result<Response<ListExecutorsResponse>> {
        AdminServiceClient::<T>::list_executors(&mut self.clone(), request).await
    }
    
    async fn delete_historical_data(
        &self,
        request: Request<DeleteHistoricalDataRequest>,
    ) -> tonic::Result<Response<DeleteHistoricalDataResponse>> {
        AdminServiceClient::<T>::delete_historical_data(&mut self.clone(), request).await
    }
}
