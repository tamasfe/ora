use crossterm::event::EventStream;
use futures::{FutureExt, StreamExt};
use ora::{admin::job_types::JobTypeInfo, proto::admin::v1::Job};

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
    JobsUpdated(Vec<Job>),
    Refresh,
}
