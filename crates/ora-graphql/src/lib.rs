use std::sync::Arc;

use async_graphql::{EmptySubscription, Schema};
use mutation::Mutation;
use ora_client::ClientOperations;
use ora_worker::registry::WorkerRegistry;
use query::Query;

pub(crate) mod common;
pub(crate) mod mutation;
pub(crate) mod query;

pub type OraSchema = Schema<Query, Mutation, EmptySubscription>;

/// Create a GraphQL schema with the given client.
pub fn create_schema<C, W>(client: C, worker_registry: W) -> OraSchema
where
    C: ClientOperations,
    W: WorkerRegistry + Send + Sync + 'static,
{
    let client = Arc::new(client);
    let worker_registry = Arc::new(worker_registry);
    Schema::new(
        Query {
            client: client.clone(),
            worker_registry: worker_registry.clone(),
        },
        Mutation {
            client,
            worker_registry,
        },
        EmptySubscription,
    )
}
