use std::sync::Arc;

use async_graphql::{EmptySubscription, Schema};
use mutation::Mutation;
use ora_client::ClientOperations;
use query::Query;

pub(crate) mod common;
pub(crate) mod mutation;
pub(crate) mod query;

pub type OraSchema = Schema<Query, Mutation, EmptySubscription>;

/// Create a GraphQL schema with the given client.
pub fn create_schema<C: ClientOperations>(client: C) -> OraSchema {
    let client = Arc::new(client);
    Schema::new(
        Query {
            client: client.clone(),
        },
        Mutation { client },
        EmptySubscription,
    )
}
