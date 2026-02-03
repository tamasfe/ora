//! In-process broadcasted events.
#![allow(dead_code)]

use std::pin::pin;

use futures::Stream;

/// A broadcast sender.
#[derive(Clone)]
pub struct BroadcastSender<T> {
    capacity: usize,
    sender: async_broadcast::Sender<T>,
    keepalive: async_broadcast::InactiveReceiver<T>,
}

impl<T> BroadcastSender<T>
where
    T: Clone + Send + 'static,
{
    /// Create a new broadcast sender with the given capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, recv) = async_broadcast::broadcast(capacity);
        let keepalive = recv.deactivate();
        Self {
            capacity,
            sender,
            keepalive,
        }
    }

    /// Send the given event to all subscribers.
    #[tracing::instrument(skip_all, fields(event_type = std::any::type_name::<T>()))]
    pub async fn send(&self, event: T) {
        let mut fut = pin!(self.sender.broadcast_direct(event));

        tokio::select! {
            res = &mut fut => {
                if res.is_err() {
                    tracing::warn!("broadcast channel full, dropping event");
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                if self.sender.receiver_count() > 0 {
                    tracing::debug!("queuing event took too long");
                }

                _ = fut.await;
            }
        }
    }

    /// Try to send the given event to all subscribers.
    pub fn try_send(&self, event: T) {
        _ = self.sender.try_broadcast(event);
    }

    /// Get a subscription interface for this sender.
    pub fn subscriptions(&self) -> Broadcasted<T> {
        Broadcasted {
            recv: self.keepalive.clone(),
        }
    }
}

/// An interface that allows subscribing to broadcasted events.
#[derive(Clone)]
pub struct Broadcasted<T> {
    recv: async_broadcast::InactiveReceiver<T>,
}

impl<T> Broadcasted<T>
where
    T: Clone,
{
    /// Subscribe to the broadcasted events.
    fn subscribe(&self) -> impl Stream<Item = T> {
        async_stream::stream!({
            while let Ok(item) = self.recv.activate_cloned().recv_direct().await {
                yield item;
            }
        })
    }
}
