use std::time::Duration;

use ora_timer::{resolution::Milliseconds, Timer};
use time::OffsetDateTime;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let (timer, mut ready): (Timer<OffsetDateTime, Milliseconds>, _) = Timer::new();
    let handle: ora_timer::TimerHandle<OffsetDateTime> = timer.handle();

    tokio::spawn(timer.run());

    let events = (0..10)
        .map(|i| {
            let now = OffsetDateTime::now_utc();
            let target = now + Duration::from_secs(i);
            (target, Duration::from_secs(i))
        })
        .collect::<Vec<_>>();

    for event in &events {
        handle.schedule(event.0, event.1);
    }

    let mut i = 0;
    while let Some(target) = ready.recv().await {
        let now = OffsetDateTime::now_utc();
        println!("target: {target}, latency: {}", now - target);

        i += 1;

        if i == events.len() {
            handle.stop();
        }
    }
}
