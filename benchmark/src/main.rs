//! Benchmark runner.
//!
//! Usage (always with `--release`):
//!
//! * `cargo run --release` or `cargo run --release -- time [FILTER [ROUNDS]]`
//!   times all benchmarks (or those whose name contains `FILTER`).  The
//!   benchmarks are timed in `ROUNDS` interleaved rounds (default 5) and the
//!   best time is reported, which evens out drift (thermals, other load).
//! * `cargo run --release -- loop NAME [N]` runs a single benchmark `N`
//!   times (default 100), for profiling.  `de`, `ser`, `ignore`, `de-serde`
//!   and `ser-serde` are shortcuts for the Twitter benchmarks.
//! * `cargo run --release -- compare BASE NEW` compares two saved outputs of
//!   `time`.
//! * `cargo run --release --features count-allocs -- allocs [FILTER]` counts
//!   the allocations of a single run of every benchmark.
use std::collections::HashMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use deser::{Deserialize, Serialize};

mod datasets;
mod twitter;

use twitter::Twitter;

/// The target duration of a timed batch.
const BATCH: Duration = Duration::from_millis(10);

/// The number of timed batches per round, the best one is reported.
const RUNS: usize = 5;

type BenchFn<'a> = Box<dyn FnMut() + 'a>;

struct Bench<'a> {
    name: String,
    f: BenchFn<'a>,
}

struct Benches<'a>(Vec<Bench<'a>>);

impl<'a> Benches<'a> {
    fn add<F: FnMut() + 'a>(&mut self, name: impl Into<String>, f: F) {
        self.0.push(Bench {
            name: name.into(),
            f: Box::new(f),
        });
    }

    /// Adds serialization and deserialization benchmarks for JSON and CBOR.
    fn add_formats<T>(&mut self, name: &str, value: &'a T, json: &'a str, cbor: &'a [u8])
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        self.add(format!("{}/json/ser", name), move || {
            black_box(deser_json::to_string(value).unwrap());
        });
        self.add(format!("{}/json/de", name), move || {
            black_box(deser_json::from_str::<T>(json).unwrap());
        });
        self.add(format!("{}/cbor/ser", name), move || {
            black_box(deser_cbor::to_vec(value).unwrap());
        });
        self.add(format!("{}/cbor/de", name), move || {
            black_box(deser_cbor::from_slice::<T>(cbor).unwrap());
        });
    }
}

/// A dataset with its serialized forms.
struct Dataset<T> {
    name: &'static str,
    value: T,
    json: String,
    cbor: Vec<u8>,
}

impl<T: Serialize> Dataset<T> {
    fn new(name: &'static str, value: T) -> Dataset<T> {
        let json = deser_json::to_string(&value).unwrap();
        let cbor = deser_cbor::to_vec(&value).unwrap();
        Dataset {
            name,
            value,
            json,
            cbor,
        }
    }
}

/// Holds the data of all benchmarks.
struct Data {
    twitter_json: String,
    twitter: Dataset<Twitter>,
    features: Dataset<datasets::FeatureCollection>,
    point_cloud: Dataset<datasets::PointCloud>,
    blobs: Dataset<datasets::Blobs>,
    registry: Dataset<datasets::Registry>,
    tree: Dataset<datasets::Node>,
}

impl Data {
    fn load() -> Data {
        let twitter_json = std::fs::read_to_string("benches/twitter.json").unwrap();
        let twitter = deser_json::from_str(&twitter_json).unwrap();
        Data {
            twitter: Dataset::new("twitter", twitter),
            twitter_json,
            features: Dataset::new("features", datasets::features()),
            point_cloud: Dataset::new("point-cloud", datasets::point_cloud()),
            blobs: Dataset::new("blobs", datasets::blobs()),
            registry: Dataset::new("registry", datasets::registry()),
            tree: Dataset::new("tree", datasets::tree()),
        }
    }

    fn benches(&self) -> Benches<'_> {
        let mut benches = Benches(Vec::new());

        // the original JSON of the Twitter dump, compared against serde
        let json = &self.twitter_json;
        let value = &self.twitter.value;
        benches.add("twitter/json/de", || {
            black_box(deser_json::from_str::<Twitter>(json).unwrap());
        });
        benches.add("twitter/json/de-serde", || {
            black_box(serde_json::from_str::<Twitter>(json).unwrap());
        });
        benches.add("twitter/json/ignore", || {
            black_box(deser_json::from_str::<Ignore>(json).unwrap());
        });
        benches.add("twitter/json/ignore-serde", || {
            black_box(serde_json::from_str::<serde::de::IgnoredAny>(json).unwrap());
        });
        benches.add("twitter/json/ser", || {
            black_box(deser_json::to_string(value).unwrap());
        });
        benches.add("twitter/json/ser-serde", || {
            black_box(serde_json::to_string(value).unwrap());
        });
        let cbor = &self.twitter.cbor;
        benches.add("twitter/cbor/ser", || {
            black_box(deser_cbor::to_vec(value).unwrap());
        });
        benches.add("twitter/cbor/de", || {
            black_box(deser_cbor::from_slice::<Twitter>(cbor).unwrap());
        });

        macro_rules! add_dataset {
            ($dataset:expr) => {{
                let dataset = &$dataset;
                benches.add_formats(dataset.name, &dataset.value, &dataset.json, &dataset.cbor);
            }};
        }
        add_dataset!(self.features);
        add_dataset!(self.point_cloud);
        add_dataset!(self.blobs);
        add_dataset!(self.registry);
        add_dataset!(self.tree);

        benches
    }
}

/// Times a function and returns the best time per iteration.
fn measure(f: &mut dyn FnMut()) -> Duration {
    // calibrate the batch size, this also warms up
    let start = Instant::now();
    f();
    let once = start.elapsed().max(Duration::from_nanos(1));
    let batch = (BATCH.as_nanos() / once.as_nanos()).clamp(1, 1_000_000) as u32;
    for _ in 0..batch {
        f();
    }

    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..batch {
            f();
        }
        best = best.min(start.elapsed() / batch);
    }
    best
}

fn time(data: &Data, filter: &str, rounds: usize) {
    let mut benches = data.benches().0;
    benches.retain(|bench| bench.name.contains(filter));
    let mut best = vec![Duration::MAX; benches.len()];
    for _ in 0..rounds {
        for (bench, best) in benches.iter_mut().zip(best.iter_mut()) {
            *best = (*best).min(measure(&mut bench.f));
        }
    }
    for (bench, best) in benches.iter().zip(best) {
        println!("{:<28} {:>10.1} us", bench.name, best.as_secs_f64() * 1e6);
    }
}

fn run_loop(data: &Data, name: &str, iterations: usize) {
    let name = match name {
        "de" => "twitter/json/de",
        "ser" => "twitter/json/ser",
        "ignore" => "twitter/json/ignore",
        "de-serde" => "twitter/json/de-serde",
        "ser-serde" => "twitter/json/ser-serde",
        name => name,
    };
    let mut benches = data.benches().0;
    let Some(bench) = benches.iter_mut().find(|bench| bench.name == name) else {
        eprintln!("unknown benchmark {}", name);
        std::process::exit(1);
    };
    for _ in 0..iterations {
        (bench.f)();
    }
}

/// Parses the output of `time`.
fn parse_results(path: &str) -> Vec<(String, f64)> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("cannot read {}: {}", path, err))
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            let value = parts.next()?.parse().ok()?;
            Some((name.to_string(), value))
        })
        .collect()
}

fn compare(base: &str, new: &str) {
    let base = parse_results(base).into_iter().collect::<HashMap<_, _>>();
    for (name, new) in parse_results(new) {
        match base.get(&name) {
            Some(&base) => println!(
                "{:<28} {:>10.1} us {:>10.1} us {:>+7.1}%",
                name,
                base,
                new,
                (new / base - 1.0) * 100.0
            ),
            None => println!("{:<28} {:>13} {:>10.1} us", name, "-", new),
        }
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let arg = |idx: usize| args.get(idx).map(String::as_str);

    match arg(0) {
        None => time(&Data::load(), "", 5),
        Some("time") => time(
            &Data::load(),
            arg(1).unwrap_or(""),
            arg(2).and_then(|x| x.parse().ok()).unwrap_or(5),
        ),
        Some("loop") => {
            let name = arg(1).expect("missing benchmark name");
            let iterations = arg(2).and_then(|x| x.parse().ok()).unwrap_or(100);
            run_loop(&Data::load(), name, iterations);
        }
        Some("compare") => compare(
            arg(1).expect("missing base results"),
            arg(2).expect("missing new results"),
        ),
        #[cfg(feature = "count-allocs")]
        Some("allocs") => {
            let data = Data::load();
            let filter = arg(1).unwrap_or("");
            for bench in data.benches().0 {
                if bench.name.contains(filter) {
                    counting::count(&bench.name, bench.f);
                }
            }
        }
        // shortcuts for the Twitter benchmarks
        Some(name) => {
            let iterations = arg(1).and_then(|x| x.parse().ok()).unwrap_or(100);
            run_loop(&Data::load(), name, iterations);
        }
    }
}

#[cfg(feature = "count-allocs")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub static ALLOCS: AtomicUsize = AtomicUsize::new(0);

    pub struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            // SAFETY: forwarded with the caller's guarantees
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: forwarded with the caller's guarantees
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            // SAFETY: forwarded with the caller's guarantees
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }

    #[global_allocator]
    static GLOBAL: Counting = Counting;

    pub fn count<F: FnOnce()>(name: &str, f: F) {
        let before = ALLOCS.load(Ordering::Relaxed);
        f();
        println!(
            "{:<28} {:>10} allocs",
            name,
            ALLOCS.load(Ordering::Relaxed) - before
        );
    }
}

/// Accepts and ignores any value.
struct Ignore;

impl<'de> deser::Deserialize<'de> for Ignore {
    fn deserialize_into(out: &mut Option<Self>) -> deser::de::SinkHandle<'_, 'de> {
        *out = Some(Ignore);
        deser::de::SinkHandle::null()
    }
}
