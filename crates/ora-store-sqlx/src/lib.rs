//! SQLx-based backend for ora that persists all operations in a supported SQL database.

#![warn(clippy::pedantic, missing_docs)]
#![allow(
    clippy::too_many_lines,
    clippy::needless_raw_string_hashes,
    clippy::enum_variant_names,
    clippy::module_name_repetitions
)]

use std::{
    collections::HashSet,
    sync::{atomic::AtomicUsize, Arc, Mutex},
};

use models::DbEvent;
use ora_common::task::WorkerSelector;
use sqlx::{Database, Pool};
use time::Duration;
use tokio::sync::broadcast;

pub(crate) mod models;
#[cfg(feature = "postgres")]
pub mod postgres;

/// Store options.
#[derive(Debug, Clone)]
pub struct DbStoreOptions {
    /// The interval to use for polling the database.
    ///
    /// In case of postgres, this is only used as a fallback
    /// mechanism so higher values (minutes or hours) are
    /// sufficient.
    pub poll_interval: Duration,
    /// Channel capacity used internally.
    /// Increase this if you run into dropped event errors.
    ///
    /// The default value is on the safer side for medium, or larger
    /// applications.
    pub channel_capacity: usize,
}

impl Default for DbStoreOptions {
    fn default() -> Self {
        Self {
            poll_interval: Duration::seconds(1),
            channel_capacity: 65536,
        }
    }
}

/// A store backed by a database pool.
#[derive(Debug)]
pub struct DbStore<Db>
where
    Db: Database,
{
    options: DbStoreOptions,
    events: broadcast::Sender<DbEvent>,
    /// Union of all wanted worker selectors from all workers.
    /// This is an optimization step to prevent retrieving irrelevant tasks.
    worker_selectors: Arc<Mutex<HashSet<WorkerSelector>>>,
    /// Used to track if the wanted worker selectors have changed.
    worker_selector_version: Arc<AtomicUsize>,
    store_count: Arc<()>,
    db: Pool<Db>,
}

impl<Db> Clone for DbStore<Db>
where
    Db: Database,
{
    fn clone(&self) -> Self {
        Self {
            options: self.options.clone(),
            events: self.events.clone(),
            db: self.db.clone(),
            worker_selectors: self.worker_selectors.clone(),
            worker_selector_version: self.worker_selector_version.clone(),
            store_count: self.store_count.clone(),
        }
    }
}
