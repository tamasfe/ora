use std::sync::Arc;

use ora_backend::Backend;
use wgroup::WaitGroupHandle;

use crate::{
    executor_pool::ExecutorPool,
    proto::{
        admin::v1::admin_service_server::AdminService,
        executors::v1::execution_service_server::ExecutionService,
    },
};

mod admin;
mod conv;
mod executors;

/// Types implementing all the services of an ora server.
pub trait GrpcServices: ExecutionService + AdminService {}
impl<B> GrpcServices for GrpcImpl<B> where B: Backend {}

pub(super) struct GrpcImpl<B> {
    pub(crate) backend: Arc<B>,
    pub(crate) executor_pool: ExecutorPool,
    pub(crate) wg: WaitGroupHandle,
}
