//! Isolates primitive and small-container overhead in MessagePack decoding.
//! Run with `cargo run --release --example msgpack` from `benchmark/`.
use std::hint::black_box;
use std::time::{Duration, Instant};

use deser::de::{DeserializeDriver, SinkHandle};
use deser::{ContainerShape, Deserialize, Event, Serialize, State};

const SCALARS: usize = 100_000;

type Bench<'a> = (&'static str, Box<dyn FnMut() + 'a>);

fn measure(mut benches: Vec<Bench<'_>>) {
    let mut best = vec![Duration::MAX; benches.len()];
    for _ in 0..5 {
        for ((_, f), best) in benches.iter_mut().zip(&mut best) {
            let start = Instant::now();
            f();
            let n = (10_000_000 / start.elapsed().as_nanos().max(1)).clamp(1, 10_000);
            let start = Instant::now();
            for _ in 0..n {
                f();
            }
            *best = (*best).min(start.elapsed() / n as u32);
        }
    }
    for ((name, _), best) in benches.iter().zip(best) {
        print!("  {name}: {:.1} us", best.as_secs_f64() * 1e6);
    }
    println!();
}

struct Ignore;

impl<'de> Deserialize<'de> for Ignore {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut State,
    ) -> SinkHandle<'out, 'de> {
        *out = Some(Ignore);
        SinkHandle::null()
    }
}

fn dataset<T>(name: &str, value: T)
where
    T: Serialize
        + for<'de> Deserialize<'de>
        + serde::de::DeserializeOwned
        + PartialEq
        + std::fmt::Debug,
{
    let input = deser_msgpack::to_vec(&value).unwrap();
    assert_eq!(deser_msgpack::from_slice::<T>(&input).unwrap(), value);
    assert_eq!(rmp_serde::from_slice::<T>(&input).unwrap(), value);
    print!("{name}");
    measure(vec![
        (
            "deser",
            Box::new(|| {
                black_box(deser_msgpack::from_slice::<T>(black_box(&input)).unwrap());
            }),
        ),
        (
            "serde",
            Box::new(|| {
                black_box(rmp_serde::from_slice::<T>(black_box(&input)).unwrap());
            }),
        ),
        // Ignoring still runs the parser and driver, but no typed sinks.
        (
            "ignore",
            Box::new(|| {
                black_box(deser_msgpack::from_slice::<Ignore>(black_box(&input)).unwrap());
            }),
        ),
        (
            "ignore-serde",
            Box::new(|| {
                black_box(
                    rmp_serde::from_slice::<serde::de::IgnoredAny>(black_box(&input)).unwrap(),
                );
            }),
        ),
    ]);
}

fn events<T: for<'de> Deserialize<'de>>(values: &[[f32; 2]], pairs: bool) {
    let mut out = None::<T>;
    {
        let mut driver = DeserializeDriver::new(&mut out);
        let shape = |len| Event::SeqStart(ContainerShape::with_len(len));
        driver
            .emit(shape(values.len() * if pairs { 1 } else { 2 }))
            .unwrap();
        for &[x, y] in values {
            if pairs {
                driver.emit(shape(2)).unwrap();
            }
            driver.emit(x).unwrap();
            driver.emit(y).unwrap();
            if pairs {
                driver.emit(Event::SeqEnd).unwrap();
            }
        }
        driver.emit(Event::SeqEnd).unwrap();
    }
    black_box(out.unwrap());
}

fn main() {
    let pairs = (0..SCALARS / 2)
        .map(|i| [i as f32 + 0.25, i as f32 + 0.75])
        .collect::<Vec<_>>();
    dataset(
        "flat-f32",
        pairs.iter().flatten().copied().collect::<Vec<_>>(),
    );
    dataset("pairs-f32", pairs.clone());
    dataset(
        "tuples-f32",
        pairs.iter().map(|p| (p[0], p[1])).collect::<Vec<_>>(),
    );
    dataset(
        "pairs-f64",
        pairs.iter().map(|p| p.map(f64::from)).collect::<Vec<_>>(),
    );
    dataset("flat-u32", (0..SCALARS as u32).collect::<Vec<_>>());
    dataset(
        "pairs-u32",
        (0..SCALARS as u32 / 2)
            .map(|i| [i * 2, i * 2 + 1])
            .collect::<Vec<_>>(),
    );
    print!("events-f32 (no parser)");
    measure(vec![
        (
            "flat",
            Box::new(|| events::<Vec<f32>>(black_box(&pairs), false)),
        ),
        (
            "pairs",
            Box::new(|| events::<Vec<[f32; 2]>>(black_box(&pairs), true)),
        ),
        (
            "tuples",
            Box::new(|| events::<Vec<(f32, f32)>>(black_box(&pairs), true)),
        ),
        (
            "ignore-flat",
            Box::new(|| events::<Ignore>(black_box(&pairs), false)),
        ),
        (
            "ignore-pairs",
            Box::new(|| events::<Ignore>(black_box(&pairs), true)),
        ),
    ]);
}
