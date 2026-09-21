use crossterm::event::EventStream;
use futures::{FutureExt, StreamExt};
use ora::{
    admin::{executors::ExecutorInfo, job_types::JobTypeInfo},
    proto::admin::v1::{Job, Schedule},
};

#[derive(Debug)]
pub(super) struct Events {
    term: EventStream,
    sender: flume::Sender<AppEvent>,
    receiver: flume::Receiver<AppEvent>,
}

impl Default for Events {
    fn default() -> Self {
        let (sender, receiver) = flume::unbounded();
        Self {
            term: Default::default(),
            sender,
            receiver,
        }
    }
}

impl Events {
    pub(super) fn sender(&self) -> flume::Sender<AppEvent> {
        self.sender.clone()
    }

    pub(super) async fn next(&mut self) -> Option<AppEvent> {
        tokio::select! {
            term_event = self.term.next().fuse() => {
                if let Some(Ok(event)) = term_event {
                    Some(AppEvent::Term(event))
                } else {
                    None
                }
            }
            app_event = self.receiver.recv_async() => {
                app_event.ok()
            }
        }
    }
}

pub(super) enum AppEvent {
    Term(crossterm::event::Event),
    JobTypesUpdated(Vec<JobTypeInfo>),
    /// Carries the token of the request it answers, so a superseded
    /// one can be discarded.
    JobsUpdated(u64, Vec<Job>),
    SchedulesUpdated(u64, Vec<Schedule>),
    ExecutorsUpdated(Vec<ExecutorInfo>),
    /// The jobs of one executor, carrying the executor ID they belong to.
    ExecutorJobsUpdated(String, Vec<Job>),
    /// A background request failed, the message is shown in the
    /// footer. Carries the request's token, where it has one.
    Failed(Request, String, Option<u64>),
    Refresh,
    /// Advances the loading indicator and keeps
    /// relative times in the footer current.
    Tick,
}

/// The kind of background request an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Request {
    JobTypes,
    Jobs,
    Schedules,
    Executors,
    ExecutorJobs,
    /// A cancel or stop request, which has no table of its own.
    Action,
}
