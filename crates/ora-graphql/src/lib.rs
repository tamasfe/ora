use std::sync::Arc;

use async_graphql::{EmptySubscription, Schema};
use mutation::Mutation;
use ora_client::ClientOperations;
use ora_worker::registry::{WorkerRegistry, noop::NoopWorkerRegistry};
use query::Query;

pub(crate) mod common;
pub(crate) mod mutation;
pub(crate) mod query;

pub type OraSchema<W = NoopWorkerRegistry> = Schema<Query<W>, Mutation, EmptySubscription>;

/// Create a GraphQL schema with the given client.
pub fn create_schema<C, W>(client: C, worker_registry: W) -> OraSchema<W>
where
    C: ClientOperations,
    W: WorkerRegistry + Send + Sync + 'static,
{
    let client = Arc::new(client);
    Schema::new(
        Query {
            client: client.clone(),
            worker_registry,
        },
        Mutation { client },
        EmptySubscription,
    )
}
