//! A wait-group implementation for named tasks in async contexts.

use std::{
    collections::BTreeMap,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
};

use futures::{future::pending, task::AtomicWaker, Future, Stream};
use tokio_util::sync::CancellationToken;

/// A wait group that keeps track a set of tasks.
#[derive(Debug)]
pub struct WaitGroup {
    cancel_on_drop: bool,
    inner: Arc<WgInner>,
}

impl Default for WaitGroup {
    fn default() -> Self {
        Self {
            cancel_on_drop: true,
            inner: Default::default(),
        }
    }
}

impl WaitGroup {
    /// Create a new wait group.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new wait guard.
    #[allow(clippy::missing_panics_doc)]
    pub fn add(&self) -> WaitGuard {
        self.inner.clone().add("")
    }

    /// Create a new wait guard with a specific name.
    #[allow(clippy::missing_panics_doc)]
    pub fn add_with(&self, name: &str) -> WaitGuard {
        self.inner.clone().add(name)
    }

    /// The number of currently active wait guards.
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn count(&self) -> usize {
        self.inner.guards.lock().unwrap().len()
    }

    /// Whether all wait guards have been dropped.
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn is_done(&self) -> bool {
        self.inner.guards.lock().unwrap().is_empty()
    }

    /// A future that resolves when the wait
    /// group is currently waiting.
    ///
    /// Tasks can interpret it as a form of
    /// cancellation.
    pub async fn waiting(&self) {
        self.inner.cancellation.cancelled().await;
    }

    /// Return whether the wait group is currently waiting.
    #[must_use]
    pub fn is_waiting(&self) -> bool {
        self.inner.cancellation.is_cancelled()
    }

    /// Return a handle to the wait group that allows
    /// adding new wait guards.
    #[must_use]
    pub fn handle(&self) -> WaitGroupHandle {
        WaitGroupHandle {
            inner: Some(self.inner.clone()),
        }
    }

    /// Set whether wait guards should be signaled
    /// when the wait group is dropped.
    ///
    /// Note that regardless of this setting
    /// the group never blocks on drop.
    ///
    /// The default is `true`.
    pub fn waiting_on_drop(&mut self, signal: bool) {
        self.cancel_on_drop = signal;
    }

    /// Wait for all guards to be dropped.
    pub fn all_done(self) -> impl Future<Output = ()> + Unpin {
        AllDone {
            last_count: 0,
            inner: self.inner.clone(),
        }
    }

    /// A stream returning the count of currently active wait guards,
    /// while also returning a list of names where available.
    pub fn all_done_stream(self) -> impl Stream<Item = (usize, Vec<Arc<str>>)> + Unpin {
        AllDone {
            last_count: 0,
            inner: self.inner.clone(),
        }
    }
}

impl Drop for WaitGroup {
    fn drop(&mut self) {
        if self.cancel_on_drop {
            self.inner.cancellation.cancel();
        }
    }
}

/// A handle to a wait group that allows
/// adding new wait guards.
#[derive(Debug, Clone)]
pub struct WaitGroupHandle {
    inner: Option<Arc<WgInner>>,
}

impl WaitGroupHandle {
    /// Create a new wait group handle
    /// that is not associated with any wait group.
    #[must_use]
    pub fn never() -> Self {
        Self { inner: None }
    }

    /// Create a new wait guard.
    #[allow(clippy::missing_panics_doc)]
    pub fn add(&self) -> WaitGuard {
        if let Some(inner) = &self.inner {
            inner.clone().add("")
        } else {
            WaitGuard::never()
        }
    }

    /// Create a new wait guard with a specific name.
    #[allow(clippy::missing_panics_doc)]
    pub fn add_with(&self, name: &str) -> WaitGuard {
        if let Some(inner) = &self.inner {
            inner.clone().add(name)
        } else {
            WaitGuard::never()
        }
    }

    /// Return whether the wait group is currently waiting.
    #[must_use]
    pub fn is_waiting(&self) -> bool {
        if let Some(inner) = &self.inner {
            inner.cancellation.is_cancelled()
        } else {
            false
        }
    }
}

/// A guard that keeps track of a task in a wait group.
///
/// It is considered done when dropped.
#[derive(Debug)]
#[must_use = "A wait guard must be kept until the given task is done"]
pub struct WaitGuard {
    id: usize,
    inner: Arc<WgInner>,
    _waker: DropWaker,
}

impl WaitGuard {
    /// Create a new wait guard
    /// that is not associated with any wait group
    /// and thus will never be considered done.
    pub fn never() -> Self {
        Self {
            id: usize::MAX,
            inner: Arc::new(WgInner::default()),
            _waker: DropWaker(Arc::new(AtomicWaker::new())),
        }
    }

    /// A future that resolves when the wait
    /// group is currently waiting.
    ///
    /// Tasks can interpret it as a form of
    /// cancellation.
    pub async fn waiting(&self) {
        if self.id == usize::MAX {
            return pending().await;
        }

        self.inner.cancellation.cancelled().await;
    }

    /// Return whether the wait group is currently waiting.
    #[must_use]
    pub fn is_waiting(&self) -> bool {
        if self.id == usize::MAX {
            return false;
        }

        self.inner.cancellation.is_cancelled()
    }

    /// Create a new wait guard with a specific name
    /// and add it to the wait group that owns this guard.
    #[allow(clippy::missing_panics_doc)]
    pub fn add(&self) -> Self {
        if self.id == usize::MAX {
            return Self::never();
        }

        self.inner.clone().add("")
    }

    /// Create a new wait guard with a specific name
    /// and add it to the wait group that owns this guard.
    #[allow(clippy::missing_panics_doc)]
    pub fn add_with(&self, name: &str) -> Self {
        if self.id == usize::MAX {
            return Self::never();
        }

        self.inner.clone().add(name)
    }

    /// Complete the wait guard explicitly.
    ///
    /// It is the same as dropping the guard.
    pub fn done(self) {}
}

impl Drop for WaitGuard {
    #[inline]
    fn drop(&mut self) {
        self.inner.guards.lock().unwrap().remove(&self.id);
    }
}

#[derive(Debug, Clone)]
struct DropWaker(Arc<AtomicWaker>);

impl Drop for DropWaker {
    fn drop(&mut self) {
        self.0.wake();
    }
}

struct AllDone {
    last_count: usize,
    inner: Arc<WgInner>,
}

impl Future for AllDone {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.inner.guards.lock().unwrap().is_empty() {
            Poll::Ready(())
        } else {
            self.inner.waker.register(cx.waker());
            Poll::Pending
        }
    }
}

impl Stream for AllDone {
    type Item = (usize, Vec<Arc<str>>);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let guards = self.inner.guards.lock().unwrap();

        if guards.is_empty() {
            Poll::Ready(None)
        } else {
            let count = guards.len();
            self.inner.waker.register(cx.waker());

            // FIXME: This will miss the case where a guard is added
            //        and removed in the same poll cycle.
            let ret = if self.last_count == count {
                Poll::Pending
            } else {
                Poll::Ready(Some((
                    count,
                    guards.values().map(|g| g.name.clone()).collect(),
                )))
            };

            drop(guards);

            self.last_count = count;

            ret
        }
    }
}

#[derive(Debug, Default)]
struct WgInner {
    next_id: AtomicUsize,
    guards: Mutex<BTreeMap<usize, GuardInfo>>,
    cancellation: CancellationToken,
    waker: Arc<AtomicWaker>,
}

impl WgInner {
    fn new_id(&self) -> usize {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Add a new wait guard with a specific name.
    ///
    /// If a guard with the same name already exists,
    /// a unique name will be generated on a best-effort basis.
    #[allow(clippy::missing_panics_doc)]
    pub fn add(self: Arc<Self>, name: &str) -> WaitGuard {
        let mut guards = self.guards.lock().unwrap();

        let id = self.new_id();
        let name = Arc::<str>::from(name);

        guards.insert(id, GuardInfo { name: name.clone() });

        WaitGuard {
            id,
            inner: self.clone(),
            _waker: DropWaker(self.waker.clone()),
        }
    }
}

#[derive(Debug)]
struct GuardInfo {
    name: Arc<str>,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn test_wait_group() {
        let task_count = 1_000;

        let group = WaitGroup::new();

        let done_counter = Arc::new(AtomicUsize::new(0));
        for _ in 0..task_count {
            let guard = group.add();
            let c = done_counter.clone();
            std::thread::spawn(move || {
                let _g = guard;
                std::thread::sleep(std::time::Duration::from_millis(30));
                c.fetch_add(1, Ordering::AcqRel);
            });
        }

        group.all_done().await;

        assert_eq!(done_counter.load(Ordering::Acquire), task_count);
    }

    #[tokio::test]
    async fn test_wait_guard_drop() {
        let task_count = 300;

        let group = WaitGroup::new();

        let done_counter = Arc::new(AtomicUsize::new(0));
        for _ in 0..task_count {
            let guard = group.add();
            let c = done_counter.clone();
            tokio::spawn(async move {
                let g = guard;
                g.waiting().await;
                c.fetch_add(1, Ordering::AcqRel);
            });
        }

        drop(group);

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        assert_eq!(done_counter.load(Ordering::Acquire), task_count);
    }

    #[tokio::test]
    async fn test_wait_guard_drop2() {
        let task_count = 300;

        let group = WaitGroup::new();

        let done_counter = Arc::new(AtomicUsize::new(0));
        for _ in 0..task_count {
            let guard = group.add();
            let c = done_counter.clone();
            tokio::spawn(async move {
                let g = guard;
                g.waiting().await;
                c.fetch_add(1, Ordering::AcqRel);
            });
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        drop(group);

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        assert_eq!(done_counter.load(Ordering::Acquire), task_count);
    }
}
