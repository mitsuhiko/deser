//! Benchmark runner.
//!
//! Usage (always with `--release`):
//!
//! * `cargo run --release` or `cargo run --release -- time [FILTER [ROUNDS]]`
//!   times all benchmarks (or those whose name contains `FILTER`).  The
//!   benchmarks are timed in `ROUNDS` interleaved rounds (default 5) and the
//!   best time is reported, which evens out drift (thermals, other load).
//! * `cargo run --release -- versus [FILTER [ROUNDS]]` times like `time`
//!   and prints the times of deser next to the times of serde.
//!   `cargo run --release -- table RESULTS` prints the same table for a
//!   saved output of `time`.
//! * `cargo run --release -- loop NAME [N]` runs a single benchmark `N`
//!   times (default 100), for profiling.  `de`, `ser`, `ignore`, `de-serde`
//!   and `ser-serde` are shortcuts for the Twitter JSON benchmarks.
//! * `cargo run --release -- compare BASE NEW` compares two saved outputs of
//!   `time`.
//! * `cargo run --release -- list` lists the benchmarks, `sizes` prints
//!   the sizes of the inputs and `interop` compares the output of deser and
//!   serde and whether they read each other's output.
//! * `cargo run --release --features count-allocs -- allocs [FILTER]` counts
//!   the allocations of a single run of every benchmark.
use std::collections::HashMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use deser::{Deserialize, Serialize};

mod canada;
mod cargo;
mod citm;
mod compare;
mod datasets;
mod formats;
mod github;
mod kubernetes;
mod logs;
mod manifests;
mod raw;
mod saphyr;
mod sessions;
mod table;
mod twitter;

use compare::Equality;
use formats::{Format, Input};
use twitter::Twitter;

/// The target duration of a timed batch.
const BATCH: Duration = Duration::from_millis(10);

/// The number of timed batches per round, the best one is reported.
const RUNS: usize = 5;

/// The width of the benchmark names in the output.
const NAME_WIDTH: usize = 32;

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

    /// Adds the benchmarks of the deser crates for all formats.
    fn add_deser<T>(&mut self, dataset: &'a Dataset<T>)
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        self.add_events(dataset);
        for (format, input) in dataset.inputs() {
            self.add_deser_format(dataset, format, input);
        }
    }

    /// Adds a benchmark that only produces the events of the value, without
    /// a format.  This is the cost of the serialize driver.
    fn add_events<T: Serialize>(&mut self, dataset: &'a Dataset<T>) {
        let value = &dataset.value;
        self.add(format!("{}/events/ser", dataset.name), move || {
            let mut count = 0usize;
            deser::ser::SerializeDriver::new(value)
                .drive(|event, _| {
                    black_box(&event);
                    count += 1;
                    Ok(())
                })
                .unwrap();
            black_box(count);
        });
    }

    /// Adds the benchmarks of the deser crates and the serde libraries for
    /// all formats.  Both deserialize the same input.
    fn add_deser_and_serde<T>(&mut self, dataset: &'a Dataset<T>)
    where
        T: Serialize + for<'de> Deserialize<'de>,
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        self.add_events(dataset);
        for (format, input) in dataset.inputs() {
            let name = format!("{}/{}", dataset.name, format.name());
            let value = &dataset.value;
            self.add_deser_format(dataset, format, input);
            self.add(format!("{}/de-serde", name), move || {
                black_box(formats::serde_de::<T>(format, input).unwrap());
            });
            self.add(format!("{}/ser-serde", name), move || {
                black_box(formats::serde_ser(format, value).unwrap());
            });
        }
    }

    /// Adds the benchmarks of many small documents for deser and serde.
    fn add_documents<T>(&mut self, documents: &'a Documents<T>)
    where
        T: Serialize + for<'de> Deserialize<'de>,
        T: serde::Serialize + serde::de::DeserializeOwned,
    {
        for (format, inputs) in &documents.inputs {
            let format = *format;
            let name = format!("{}/{}", documents.name, format.name());
            let inputs = inputs
                .iter()
                .map(|input| Input::new(format, input))
                .collect::<Vec<_>>();
            let values = &documents.values;
            let deser_inputs = inputs.clone();
            self.add(format!("{}/de", name), move || {
                for &input in &deser_inputs {
                    black_box(formats::deser_de::<T>(format, input).unwrap());
                }
            });
            self.add(format!("{}/ser", name), move || {
                for value in values {
                    black_box(formats::deser_ser(format, value).unwrap());
                }
            });
            self.add(format!("{}/de-serde", name), move || {
                for &input in &inputs {
                    black_box(formats::serde_de::<T>(format, input).unwrap());
                }
            });
            self.add(format!("{}/ser-serde", name), move || {
                for value in values {
                    black_box(formats::serde_ser(format, value).unwrap());
                }
            });
        }
    }

    fn add_deser_format<T>(&mut self, dataset: &'a Dataset<T>, format: Format, input: Input<'a>)
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let name = format!("{}/{}", dataset.name, format.name());
        let value = &dataset.value;
        self.add(format!("{}/de", name), move || {
            black_box(formats::deser_de::<T>(format, input).unwrap());
        });
        self.add(format!("{}/ser", name), move || {
            black_box(formats::deser_ser(format, value).unwrap());
        });
    }
}

/// A dataset of many small documents with their serialized forms.
///
/// The benchmarks deserialize (and serialize) the documents one by one, which
/// measures what it costs to deserialize a document.
struct Documents<T> {
    name: &'static str,
    values: Vec<T>,
    inputs: Vec<(Format, Vec<Vec<u8>>)>,
}

impl<T> Documents<T>
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    /// Creates the documents from generated values and checks that both
    /// libraries read them.
    fn new(name: &'static str, values: Vec<T>) -> Documents<T> {
        let inputs = Format::ALL
            .iter()
            .map(|&format| {
                let documents = values
                    .iter()
                    .map(|value| formats::deser_ser(format, value).unwrap())
                    .collect();
                (format, documents)
            })
            .collect();
        let documents = Documents {
            name,
            values,
            inputs,
        };
        documents.check("deser", formats::deser_de);
        documents.check("serde", formats::serde_de);
        documents
    }

    /// Loads the documents of a JSONL file in `benchmark/data`, they are
    /// only benchmarked with JSON.  `check` makes sure that a value has
    /// everything of its line (that the types are complete).
    fn load_jsonl(
        name: &'static str,
        path: &str,
        check: impl Fn(&str, &T) -> Result<(), String>,
    ) -> Documents<T> {
        let jsonl = read_data(path);
        let lines = jsonl
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let values = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let value = deser_json::from_str(line).unwrap_or_else(|err| {
                    panic!("{}: deser cannot read line {}: {}", name, index + 1, err)
                });
                if let Err(err) = check(line, &value) {
                    panic!("{}: line {}: {}", name, index + 1, err);
                }
                value
            })
            .collect();
        let inputs = lines.iter().map(|line| line.as_bytes().to_vec()).collect();
        let documents = Documents {
            name,
            values,
            inputs: vec![(Format::Json, inputs)],
        };
        documents.check("serde", formats::serde_de);
        documents
    }

    fn check(&self, library: &str, de: impl Fn(Format, Input) -> Result<T, formats::Error>) {
        for (format, documents) in &self.inputs {
            for (document, expected) in documents.iter().zip(&self.values) {
                let name = format!("{}/{}", self.name, format.name());
                let value = de(*format, Input::new(*format, document))
                    .unwrap_or_else(|err| panic!("{}: {} cannot read: {}", name, library, err));
                if compare::compare(&value, expected) == Equality::Different {
                    panic!("{}: {} reads another value", name, library);
                }
            }
        }
    }

    /// Prints the total size of the documents of every format.
    fn sizes(&self) {
        for (format, documents) in &self.inputs {
            let name = format!("{}/{}", self.name, format.name());
            println!(
                "{:<NAME_WIDTH$} {:>10} in {} documents",
                name,
                format_size(documents.iter().map(Vec::len).sum()),
                documents.len()
            );
        }
    }
}

/// A dataset with its serialized forms.
///
/// Every dataset is benchmarked with all formats.  If the dataset comes
/// from a document, that document is the input of its format, the inputs of
/// the other formats are serialized from the value with deser.
struct Dataset<T> {
    name: &'static str,
    value: T,
    inputs: Vec<(Format, Vec<u8>)>,
}

impl<T> Dataset<T> {
    /// Returns the input of a format.
    fn input(&self, format: Format) -> Input<'_> {
        let (_, input) = self.inputs.iter().find(|(f, _)| *f == format).unwrap();
        Input::new(format, input)
    }

    /// Returns the inputs of all formats.
    fn inputs(&self) -> impl Iterator<Item = (Format, Input<'_>)> {
        self.inputs
            .iter()
            .map(|(format, input)| (*format, Input::new(*format, input)))
    }
}

impl<T> Dataset<T>
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    /// Creates a dataset from a generated value.
    fn new(name: &'static str, value: T) -> Dataset<T> {
        Dataset::build(name, value, None)
    }

    /// Creates a dataset from a document in `benchmark/data`.
    fn load(name: &'static str, format: Format, path: &str) -> Dataset<T> {
        let path = format!("{}/data/{}", env!("CARGO_MANIFEST_DIR"), path);
        let document =
            std::fs::read(&path).unwrap_or_else(|err| panic!("cannot read {}: {}", path, err));
        Dataset::from_document(name, format, document)
    }

    /// Creates a dataset from a document in the given format.
    fn from_document(name: &'static str, format: Format, document: Vec<u8>) -> Dataset<T> {
        let value = formats::deser_de(format, Input::new(format, &document))
            .unwrap_or_else(|err| panic!("cannot read {}: {}", name, err));
        Dataset::build(name, value, Some((format, document)))
    }

    fn build(name: &'static str, value: T, document: Option<(Format, Vec<u8>)>) -> Dataset<T> {
        let inputs = Format::ALL
            .iter()
            .map(|&format| match document {
                Some((document_format, ref document)) if document_format == format => {
                    (format, document.clone())
                }
                _ => (format, formats::deser_ser(format, &value).unwrap()),
            })
            .collect();
        let dataset = Dataset {
            name,
            value,
            inputs,
        };
        dataset.check("deser", formats::deser_de);
        dataset
    }

    /// Makes sure that the inputs deserialize to the value.  Otherwise the
    /// benchmarks would not compare the same work.
    fn check(&self, library: &str, de: impl Fn(Format, Input) -> Result<T, formats::Error>) {
        for (format, input) in self.inputs() {
            let name = format!("{}/{}", self.name, format.name());
            match self.compare(de(format, input)) {
                Ok(Equality::Exact | Equality::Floats) => {}
                Ok(Equality::Different) => {
                    panic!(
                        "{}: {} deserializes the input to another value",
                        name, library
                    )
                }
                Err(err) => panic!(
                    "{}: {} cannot deserialize the input: {}",
                    name, library, err
                ),
            }
        }
    }

    fn compare(&self, value: Result<T, formats::Error>) -> Result<Equality, formats::Error> {
        value.map(|value| compare::compare(&value, &self.value))
    }
}

impl<T> Dataset<T>
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    /// Checks that the serde libraries read the inputs as well.
    fn with_serde(self) -> Dataset<T> {
        self.check("serde", formats::serde_de);
        self
    }

    /// Reports how the output of the libraries differs and whether they
    /// read each other's output.
    fn interop(&self) {
        for format in Format::ALL {
            let deser = formats::deser_ser(format, &self.value).unwrap();
            let serde = formats::serde_ser(format, &self.value);
            let name = format!("{}/{}", self.name, format.name());
            let serde = match serde {
                Ok(serde) => serde,
                Err(err) => {
                    println!("{:<NAME_WIDTH$} serde cannot serialize: {}", name, err);
                    continue;
                }
            };
            let status = |value| match self.compare(value) {
                Ok(Equality::Exact) => "ok".to_string(),
                Ok(Equality::Floats) => "floats differ in the last bits".to_string(),
                Ok(Equality::Different) => "different value".to_string(),
                Err(err) => format!("error: {}", err),
            };
            println!(
                "{:<NAME_WIDTH$} {:>10} {:>10}   deser reads serde: {}, serde reads deser: {}",
                name,
                format_size(deser.len()),
                format_size(serde.len()),
                status(formats::deser_de(format, Input::new(format, &serde))),
                status(formats::serde_de(format, Input::new(format, &deser))),
            );
        }
    }
}

/// The table for CSV (which only holds flat records, see `table.rs`).
struct Table {
    rows: Vec<table::Row>,
    csv: String,
}

impl Table {
    fn new() -> Table {
        let rows = table::rows();
        let csv = table::csv(&rows);
        assert_eq!(table::serde_de(&csv), rows);
        assert_eq!(table::deser_de(&csv), rows);
        Table { rows, csv }
    }

    fn add_benches<'a>(&'a self, benches: &mut Benches<'a>) {
        let (rows, csv) = (&self.rows, self.csv.as_str());
        benches.add("table/csv/de", move || {
            black_box(table::deser_de(csv));
        });
        benches.add("table/csv/ser", move || {
            black_box(table::deser_ser(rows));
        });
        benches.add("table/events/ser", move || {
            let mut count = 0usize;
            deser::ser::SerializeDriver::new(rows)
                .drive(|event, _| {
                    black_box(&event);
                    count += 1;
                    Ok(())
                })
                .unwrap();
            black_box(count);
        });
        benches.add("table/csv/de-serde", move || {
            black_box(table::serde_de(csv));
        });
        benches.add("table/csv/ser-serde", move || {
            black_box(table::serde_ser(rows));
        });
    }
}

/// Holds the data of all benchmarks.
struct Data {
    // real world data
    twitter: Dataset<Twitter>,
    canada: Dataset<canada::FeatureCollection>,
    citm: Dataset<citm::CitmCatalog>,
    cargo_manifest: Dataset<cargo::Manifest>,
    web_sys: Dataset<cargo::Manifest>,
    cargo_lock: Dataset<cargo::Lockfile>,
    saphyr: Dataset<saphyr::Document>,
    github: Dataset<github::Responses>,
    kubernetes: Dataset<kubernetes::Swagger>,
    // synthetic data
    manifests: Dataset<manifests::ManifestList>,
    logs: Documents<logs::LogEvent>,
    session_openai: Documents<sessions::Entry>,
    session_anthropic: Documents<sessions::Entry>,
    table: Table,
    features: Dataset<datasets::FeatureCollection>,
    point_cloud: Dataset<datasets::PointCloud>,
    blobs: Dataset<datasets::Blobs>,
    registry: Dataset<datasets::Registry>,
    tree: Dataset<datasets::Node>,
}

impl Data {
    fn load() -> Data {
        Data {
            twitter: Dataset::load("twitter", Format::Json, "json-benchmark/twitter.json")
                .with_serde(),
            canada: Dataset::load("canada", Format::Json, "json-benchmark/canada.json")
                .with_serde(),
            citm: Dataset::load(
                "citm-catalog",
                Format::Json,
                "json-benchmark/citm_catalog.json",
            )
            .with_serde(),
            cargo_manifest: Dataset::load("cargo-manifest", Format::Toml, "toml/Cargo.cargo.toml")
                .with_serde(),
            web_sys: Dataset::load("web-sys-manifest", Format::Toml, "toml/Cargo.web-sys.toml")
                .with_serde(),
            cargo_lock: Dataset::load("cargo-lock", Format::Toml, "serde-saphyr/Cargo.lock.toml")
                .with_serde(),
            saphyr: Dataset::from_document("saphyr", Format::Yaml, saphyr::yaml().into_bytes())
                .with_serde(),
            github: Dataset::new(
                "github",
                github::responses(&read_data("github/examples.json")),
            )
            .with_serde(),
            kubernetes: Dataset::load("kubernetes", Format::Json, "kubernetes/swagger.json")
                .with_serde(),
            manifests: Dataset::new("manifests", manifests::manifests()).with_serde(),
            logs: Documents::new("logs", logs::events()),
            session_openai: Documents::load_jsonl(
                "session-openai",
                "pi-sessions/openai.jsonl",
                sessions::check,
            ),
            session_anthropic: Documents::load_jsonl(
                "session-anthropic",
                "pi-sessions/anthropic.jsonl",
                sessions::check,
            ),
            table: Table::new(),
            features: Dataset::new("features", datasets::features()).with_serde(),
            point_cloud: Dataset::new("point-cloud", datasets::point_cloud()).with_serde(),
            blobs: Dataset::new("blobs", datasets::blobs()),
            registry: Dataset::new("registry", datasets::registry()).with_serde(),
            tree: Dataset::new("tree", datasets::tree()).with_serde(),
        }
    }

    fn benches(&self) -> Benches<'_> {
        let mut benches = Benches(Vec::new());

        // ignoring the Twitter dump
        let Input::Text(json) = self.twitter.input(Format::Json) else {
            unreachable!()
        };
        benches.add("twitter/json/ignore", || {
            black_box(deser_json::from_str::<Ignore>(json).unwrap());
        });
        benches.add("twitter/json/ignore-serde", || {
            black_box(serde_json::from_str::<serde::de::IgnoredAny>(json).unwrap());
        });

        benches.add_deser_and_serde(&self.twitter);
        benches.add_deser_and_serde(&self.canada);
        benches.add_deser_and_serde(&self.citm);
        benches.add_deser_and_serde(&self.cargo_manifest);
        benches.add_deser_and_serde(&self.web_sys);
        benches.add_deser_and_serde(&self.cargo_lock);
        benches.add_deser_and_serde(&self.saphyr);
        benches.add_deser_and_serde(&self.github);
        benches.add_deser_and_serde(&self.kubernetes);
        benches.add_deser_and_serde(&self.manifests);
        benches.add_documents(&self.logs);
        benches.add_documents(&self.session_openai);
        benches.add_documents(&self.session_anthropic);
        self.table.add_benches(&mut benches);
        benches.add_deser_and_serde(&self.features);
        benches.add_deser_and_serde(&self.point_cloud);
        benches.add_deser(&self.blobs);
        benches.add_deser_and_serde(&self.registry);
        benches.add_deser_and_serde(&self.tree);
        raw::add_benches(&mut benches, &self.twitter, &self.logs);

        benches
    }

    /// Prints the input sizes of all datasets.
    fn sizes(&self) {
        macro_rules! sizes {
            ($($dataset:expr),*) => {$(
                for (format, input) in &$dataset.inputs {
                    let name = format!("{}/{}", $dataset.name, format.name());
                    println!("{:<NAME_WIDTH$} {:>10}", name, format_size(input.len()));
                }
            )*};
        }
        sizes!(
            self.twitter,
            self.canada,
            self.citm,
            self.cargo_manifest,
            self.web_sys,
            self.cargo_lock,
            self.saphyr,
            self.github,
            self.kubernetes,
            self.manifests,
            self.features,
            self.point_cloud,
            self.blobs,
            self.registry,
            self.tree
        );
        self.logs.sizes();
        self.session_openai.sizes();
        self.session_anthropic.sizes();
    }

    /// Prints how the output of deser and serde differs and whether they
    /// read each other's output.
    fn interop(&self) {
        println!(
            "{:<NAME_WIDTH$} {:>10} {:>10}",
            "output size", "deser", "serde"
        );
        self.twitter.interop();
        self.canada.interop();
        self.citm.interop();
        self.cargo_manifest.interop();
        self.web_sys.interop();
        self.cargo_lock.interop();
        self.saphyr.interop();
        self.github.interop();
        self.kubernetes.interop();
        self.manifests.interop();
        self.features.interop();
        self.point_cloud.interop();
        self.registry.interop();
        self.tree.interop();
    }
}

/// Reads a file in `benchmark/data`.
fn read_data(path: &str) -> String {
    let path = format!("{}/data/{}", env!("CARGO_MANIFEST_DIR"), path);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read {}: {}", path, err))
}

/// Extracts the examples of the GitHub benchmark from the OpenAPI
/// description of the GitHub API (used by `scripts/update-benchmark-data.sh`).
fn extract_github(description: &str, output: &str) {
    const EXAMPLES: [&str; 6] = [
        "issue-items",
        "pull-request-simple-items",
        "minimal-repository-items",
        "workflow-run-paginated",
        "commit-items",
        "release-items",
    ];
    let description: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(description).unwrap()).unwrap();
    let mut examples = serde_json::Map::new();
    for name in EXAMPLES {
        let value = description["components"]["examples"][name]["value"].clone();
        assert!(!value.is_null(), "missing example {}", name);
        examples.insert(name.to_string(), value);
    }
    let json = serde_json::to_string_pretty(&serde_json::Value::Object(examples)).unwrap();
    std::fs::write(output, json + "\n").unwrap();
}

fn format_size(size: usize) -> String {
    if size < 1024 {
        format!("{} B", size)
    } else if size < 1024 * 1024 {
        format!("{:.1} KiB", size as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", size as f64 / (1024.0 * 1024.0))
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

/// Times the benchmarks whose name contains `filter` and returns the best
/// time of every benchmark in microseconds.
fn time(data: &Data, filter: &str, rounds: usize) -> Vec<(String, f64)> {
    let mut benches = data.benches().0;
    benches.retain(|bench| bench.name.contains(filter));
    let mut best = vec![Duration::MAX; benches.len()];
    for round in 0..rounds {
        eprint!("\rround {}/{}", round + 1, rounds);
        for (bench, best) in benches.iter_mut().zip(best.iter_mut()) {
            *best = (*best).min(measure(&mut bench.f));
        }
    }
    eprint!("\r{:20}\r", "");
    benches
        .iter()
        .zip(best)
        .map(|(bench, best)| (bench.name.clone(), best.as_secs_f64() * 1e6))
        .collect()
}

fn print_results(results: &[(String, f64)]) {
    for (name, time) in results {
        println!("{:<NAME_WIDTH$} {:>10.1} us", name, time);
    }
}

/// Prints the times of deser next to the times of serde.
///
/// The ratio is the time of deser divided by the time of serde (below 1
/// deser is faster).  The summary has the geometric mean of the ratios by
/// format and operation.
fn print_versus(results: &[(String, f64)]) {
    let times = results.iter().cloned().collect::<HashMap<_, _>>();
    println!(
        "{:<NAME_WIDTH$} {:>12} {:>12} {:>12}",
        "benchmark", "deser", "serde", "deser/serde"
    );
    let mut summary = Vec::<(String, Vec<f64>)>::new();
    for (name, deser) in results {
        let Some(serde) = times.get(&format!("{}-serde", name)) else {
            continue;
        };
        let ratio = deser / serde;
        println!(
            "{:<NAME_WIDTH$} {:>9.1} us {:>9.1} us {:>11.2}x",
            name, deser, serde, ratio
        );
        // group by format and operation (`twitter/json/de` -> `json/de`)
        let group = name.split_once('/').map_or(name.as_str(), |x| x.1);
        match summary.iter_mut().find(|(g, _)| g == group) {
            Some((_, ratios)) => ratios.push(ratio),
            None => summary.push((group.to_string(), vec![ratio])),
        }
    }
    if summary.is_empty() {
        return;
    }
    summary.sort_by(|a, b| a.0.cmp(&b.0));
    println!();
    println!(
        "{:<NAME_WIDTH$} {:>12} {:>12}",
        "geometric mean", "benchmarks", "deser/serde"
    );
    for (group, ratios) in summary {
        let mean = (ratios.iter().map(|x| x.ln()).sum::<f64>() / ratios.len() as f64).exp();
        println!(
            "{:<NAME_WIDTH$} {:>12} {:>11.2}x",
            group,
            ratios.len(),
            mean
        );
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
                "{:<NAME_WIDTH$} {:>10.1} us {:>10.1} us {:>+7.1}%",
                name,
                base,
                new,
                (new / base - 1.0) * 100.0
            ),
            None => println!("{:<NAME_WIDTH$} {:>13} {:>10.1} us", name, "-", new),
        }
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let arg = |idx: usize| args.get(idx).map(String::as_str);
    let filter = || arg(1).unwrap_or("");
    let rounds = || arg(2).and_then(|x| x.parse().ok()).unwrap_or(5);

    match arg(0) {
        None => print_results(&time(&Data::load(), "", 5)),
        Some("time") => print_results(&time(&Data::load(), filter(), rounds())),
        Some("versus") => print_versus(&time(&Data::load(), filter(), rounds())),
        Some("table") => print_versus(&parse_results(arg(1).expect("missing results"))),
        Some("sizes") => Data::load().sizes(),
        Some("interop") => Data::load().interop(),
        Some("list") => {
            for bench in Data::load().benches().0 {
                println!("{}", bench.name);
            }
        }
        Some("loop") => {
            let name = arg(1).expect("missing benchmark name");
            let iterations = arg(2).and_then(|x| x.parse().ok()).unwrap_or(100);
            run_loop(&Data::load(), name, iterations);
        }
        Some("extract-github") => extract_github(
            arg(1).expect("missing description"),
            arg(2).expect("missing output"),
        ),
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
            "{:<32} {:>10} allocs",
            name,
            ALLOCS.load(Ordering::Relaxed) - before
        );
    }
}

/// Accepts and ignores any value.
struct Ignore;

impl<'de> deser::Deserialize<'de> for Ignore {
    fn deserialize_into<'out>(
        out: &'out mut Option<Self>,
        _state: &mut deser::State,
    ) -> deser::de::SinkHandle<'out, 'de> {
        *out = Some(Ignore);
        deser::de::SinkHandle::null()
    }
}
