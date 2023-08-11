use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use ora_timer::{resolution::Milliseconds, wheel::TimingWheel};

pub fn bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("timing_wheel_at_once");

    let targets = [
        (0..5)
            .map(|i| (i, Duration::from_millis(1)))
            .collect::<Vec<_>>(),
        (0..100)
            .map(|i| (i, Duration::from_millis(1)))
            .collect::<Vec<_>>(),
        (0..1_000)
            .map(|i| (i, Duration::from_millis(1)))
            .collect::<Vec<_>>(),
        (0..10_000)
            .map(|i| (i, Duration::from_millis(1)))
            .collect::<Vec<_>>(),
        (0..100_000)
            .map(|i| (i, Duration::from_millis(1)))
            .collect::<Vec<_>>(),
        (0..1_000_000)
            .map(|i| (i, Duration::from_millis(1)))
            .collect::<Vec<_>>(),
    ];

    for targets in targets {
        group.throughput(Throughput::Elements(targets.len() as u64));
        group.bench_with_input(
            format!("insert_and_expire_{}", targets.len()),
            &targets,
            |b, elems| {
                b.iter(|| {
                    let mut wheel = TimingWheel::<u64, Milliseconds>::new();
                    for elem in elems {
                        wheel.insert(elem.0, elem.1);
                    }
                    wheel.tick()
                })
            },
        );
    }
    group.finish();

    let mut group = c.benchmark_group("timing_wheel_distributed");

    let targets = [
        (0..5)
            .map(|i| (i, Duration::from_millis(i)))
            .collect::<Vec<_>>(),
        (0..100)
            .map(|i| (i, Duration::from_millis(i)))
            .collect::<Vec<_>>(),
        (0..1_000)
            .map(|i| (i, Duration::from_millis(i)))
            .collect::<Vec<_>>(),
        (0..10_000)
            .map(|i| (i, Duration::from_millis(i)))
            .collect::<Vec<_>>(),
        (0..100_000)
            .map(|i| (i, Duration::from_millis(i)))
            .collect::<Vec<_>>(),
        (0..1_000_000)
            .map(|i| (i, Duration::from_millis(i)))
            .collect::<Vec<_>>(),
    ];

    for targets in targets {
        group.throughput(Throughput::Elements(targets.len() as u64));
        group.bench_with_input(
            format!("insert_and_expire_{}", targets.len()),
            &targets,
            |b, elems| {
                b.iter(|| {
                    let mut wheel = TimingWheel::<u64, Milliseconds>::new();
                    for elem in elems {
                        wheel.insert(elem.0, elem.1);
                    }
                    let mut len = 0;
                    for _ in elems {
                        len = wheel.tick().len();
                    }

                    len
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
