#![allow(missing_docs, clippy::cast_possible_truncation)]
use std::{mem, time::Duration};

use minstant::Instant;
use ora_timer::{
    resolution::MillisecondResolution, run_binary_heap_timer, run_hierarchical_timer, Delayed,
    TimerLoopAction, TimerOptions,
};
use rand::{rngs::StdRng, Rng, SeedableRng};

const BASE_DELAY: Duration = Duration::from_secs(1);

fn main() {
    let num_jobs = [1, 10, 100, 1000, 10_000, 100_000, 1_000_000, 10_000_000];

    for num_jobs in num_jobs {
        println!("{num_jobs} jobs:");
        bench_timer::<false, false>(num_jobs);
        bench_timer::<true, false>(num_jobs);
        bench_timer::<false, true>(num_jobs);
        bench_timer::<true, true>(num_jobs);
    }
}

fn bench_timer<const RANDOMIZED: bool, const ARBITRARY_RESOLUTION: bool>(num_jobs: usize) {
    let mut rng = StdRng::seed_from_u64(0);

    let start = Instant::now();
    let mut jobs = (0..num_jobs)
        .map(|_| {
            let delay = if RANDOMIZED {
                BASE_DELAY + Duration::from_millis(rng.gen_range(0..2000))
            } else {
                BASE_DELAY
            };

            Delayed::new(
                Job {
                    created: start,
                    delay,
                },
                delay,
            )
        })
        .collect::<Vec<Delayed<Job>>>();

    let initial_delay = Instant::now() - start;

    let mut completed_jobs = Vec::with_capacity(num_jobs);

    let timer_options = TimerOptions::default();

    let cb = |new: &mut Vec<Delayed<Job>>, ready: &mut Vec<Job>| {
        let completed = Instant::now();
        completed_jobs.extend(ready.drain(..).map(|job| CompletedJob {
            created: job.created,
            delay: job.delay,
            completed,
        }));

        if completed_jobs.len() >= num_jobs {
            TimerLoopAction::StopWhenIdle
        } else {
            new.extend(mem::take(&mut jobs));
            TimerLoopAction::Continue
        }
    };

    if ARBITRARY_RESOLUTION {
        run_binary_heap_timer(timer_options, cb);
    } else {
        run_hierarchical_timer::<Job, MillisecondResolution>(timer_options, cb);
    }

    let max_latency = completed_jobs
        .iter()
        .map(|job| (job.completed - job.created - job.delay).saturating_sub(initial_delay))
        .max()
        .unwrap();

    let avg_latency = completed_jobs
        .iter()
        .map(|job| (job.completed - job.created - job.delay).saturating_sub(initial_delay))
        .sum::<Duration>()
        / completed_jobs.len() as u32;

    let min_latency = completed_jobs
        .iter()
        .map(|job| (job.completed - job.created - job.delay).saturating_sub(initial_delay))
        .min()
        .unwrap();

    print!("\tmean: {avg_latency:?}\tmax: {max_latency:?}\tmin: {min_latency:?}");
    println!("\trandomized: {RANDOMIZED}\tarbitrary resolution: {ARBITRARY_RESOLUTION}");
}

struct Job {
    created: Instant,
    delay: Duration,
}

struct CompletedJob {
    created: Instant,
    delay: Duration,
    completed: Instant,
}

impl PartialOrd for Job {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.delay.cmp(&other.delay))
    }
}

impl Ord for Job {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.delay.cmp(&other.delay)
    }
}

impl PartialEq for Job {
    fn eq(&self, other: &Self) -> bool {
        self.delay == other.delay
    }
}

impl Eq for Job {}
