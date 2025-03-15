use std::{
    num::NonZeroU32,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::SystemTime,
};

use ahash::{HashMap, HashSet};
use arc_swap::ArcSwapOption;
use atomic::Atomic;
use parking_lot::RwLock;
use uuid::Uuid;

use crate::time::UnixNanos;

use ora_storage::JobTimeoutPolicy;

use super::ServerMessage;

/// A stateful handle to an executor.
#[derive(Debug, Clone)]
pub(crate) struct ExecutorHandle {
    pub(crate) id: Uuid,
    pub(crate) inner: Arc<ExecutorHandleInner>,
}

impl ExecutorHandle {
    #[must_use]
    pub fn last_seen(&self) -> std::time::SystemTime {
        self.inner.last_seen.load(Ordering::Relaxed).into()
    }

    /// Whether the executor is ready to accept executions.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.is_alive() && self.inner.capabilities.load().is_some()
    }

    #[must_use]
    pub fn is_alive(&self) -> bool {
        let snd_connected = self
            .inner
            .sender
            .load()
            .as_ref()
            .map(|s| s.is_disconnected())
            .unwrap_or(false);

        let recv_connected = self.inner.recv_connected.load(Ordering::Relaxed);
        snd_connected && recv_connected
    }

    // Destroy the connection to the executor, queued
    // messages will still be sent, but no more messages
    // will be accepted.
    pub(crate) fn disconnect(&self) {
        self.inner.sender.swap(None);
        _ = self.inner.close_recv_signal.try_send(());
    }
}

#[derive(Debug)]
pub(crate) struct ExecutorHandleInner {
    pub(crate) last_seen: Atomic<UnixNanos>,
    pub(crate) recv_connected: AtomicBool,
    pub(crate) sender: ArcSwapOption<flume::Sender<ServerMessage>>,
    pub(crate) close_recv_signal: flume::Sender<()>,
    pub(crate) capabilities: ArcSwapOption<ExecutorCapabilities>,
    /// Executions assigned to this executor.
    pub(crate) executions: Arc<RwLock<HashMap<Uuid, ExecutionState>>>,
}

#[derive(Debug)]
pub(crate) struct ExecutionState {
    pub(crate) timeout_policy: JobTimeoutPolicy,
    pub(crate) started_at: Option<SystemTime>,
    pub(crate) timeout_deadline: Option<SystemTime>,
}

#[derive(Debug)]
pub(crate) struct ExecutorCapabilities {
    pub(crate) name: String,
    pub(crate) job_types: HashSet<String>,
    pub(crate) max_concurrent_executions: Option<NonZeroU32>,
}
