//! Ora E2E testing, benchmarking and other utilities.
#![allow(clippy::missing_panics_doc, clippy::unused_async, missing_docs)]

pub mod benches;
pub mod jobs;
pub mod tests;

/// Utilities for testing.
pub mod util {
    use std::{sync::OnceLock, thread, time::Duration};

    use futures::StreamExt;
    use ora_server::Storage;
    use tracing::level_filters::LevelFilter;

    /// Initialize common test utilities.
    pub fn init_test() {
        init_tracing();
        monitor_deadlocks();
    }

    /// Initialize tracing with default settings.
    pub fn init_tracing() {
        use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

        _ = tracing_subscriber::registry()
            .with(
                EnvFilter::builder()
                    .with_default_directive(LevelFilter::ERROR.into())
                    .from_env_lossy(),
            )
            .with(tracing_subscriber::fmt::layer())
            .try_init();
    }

    /// Monitor deadlocks in the application and exit if any are detected.
    pub fn monitor_deadlocks() {
        static DEADLOCK_INIT: OnceLock<()> = OnceLock::new();

        DEADLOCK_INIT.get_or_init(|| {
            thread::spawn(move || loop {
                thread::sleep(Duration::from_secs(10));
                let deadlocks = parking_lot::deadlock::check_deadlock();
                if deadlocks.is_empty() {
                    continue;
                }

                println!("{} deadlocks detected", deadlocks.len());
                for (i, threads) in deadlocks.iter().enumerate() {
                    println!("deadlock #{i}");
                    for t in threads {
                        println!("Thread Id {:#?}", t.thread_id());
                        println!("{:#?}", t.backtrace());
                    }
                }

                std::process::exit(1);
            });
        });
    }

    /// Log audit events to the console.
    pub fn log_audit_events<S>(server: &ora_server::Server<S>)
    where
        S: Storage,
    {
        let mut events = server.events();
        tokio::spawn(async move {
            while let Some(event) = events.next().await {
                tracing::trace!("{:?}", event.kind);
            }
        });
    }
}
