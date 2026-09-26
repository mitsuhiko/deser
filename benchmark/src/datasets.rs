//! Synthetic datasets that stress specific parts of the data model.
//!
//! The Twitter dump is dominated by strings and small integers.  These
//! datasets cover what it barely contains: floats (f64 and f32), bytes,
//! hash maps and deeply nested small containers.  They are generated
//! deterministically so that runs are comparable.
use std::collections::HashMap;

use deser::adapters::bytes::{BytesFallback, Hex};
use deser::{Deserialize, Serialize};

/// A small deterministic pseudo random number generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Returns a float in `0.0..1.0`.
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Float heavy: GeoJSON-like features with f64 coordinates.
#[derive(Serialize, Deserialize)]
pub struct FeatureCollection {
    features: Vec<Feature>,
}

#[derive(Serialize, Deserialize)]
struct Feature {
    id: u64,
    name: String,
    elevation: f64,
    geometry: Geometry,
}

#[derive(Serialize, Deserialize)]
struct Geometry {
    #[deser(rename = "type")]
    kind: String,
    coordinates: Vec<[f64; 2]>,
}

pub fn features() -> FeatureCollection {
    let mut rng = Rng(0x1234_5678);
    FeatureCollection {
        features: (0..200)
            .map(|id| Feature {
                id,
                name: format!("feature-{}", id),
                elevation: rng.unit() * 4000.0,
                geometry: Geometry {
                    kind: "LineString".into(),
                    coordinates: (0..250)
                        .map(|_| [rng.unit() * 360.0 - 180.0, rng.unit() * 180.0 - 90.0])
                        .collect(),
                },
            })
            .collect(),
    }
}

/// f32 heavy: a point cloud.  Exercises the f32 precision path.
#[derive(Serialize, Deserialize)]
pub struct PointCloud {
    points: Vec<[f32; 3]>,
}

pub fn point_cloud() -> PointCloud {
    let mut rng = Rng(0x8765_4321);
    PointCloud {
        points: (0..30_000)
            .map(|_| {
                [
                    (rng.unit() * 100.0) as f32,
                    (rng.unit() * 100.0) as f32,
                    (rng.unit() * 10.0) as f32,
                ]
            })
            .collect(),
    }
}

/// Bytes heavy: many small blobs, plain and with a bytes fallback format.
#[derive(Serialize, Deserialize)]
pub struct Blobs {
    blobs: Vec<Blob>,
}

#[derive(Serialize, Deserialize)]
struct Blob {
    id: u32,
    data: Vec<u8>,
    #[deser(as = BytesFallback<Hex>)]
    digest: Vec<u8>,
}

pub fn blobs() -> Blobs {
    let mut rng = Rng(0xdead_beef);
    let mut bytes = |len: usize| (0..len).map(|_| rng.next() as u8).collect::<Vec<_>>();
    Blobs {
        blobs: (0..5_000)
            .map(|id| Blob {
                id,
                data: bytes(48),
                digest: bytes(32),
            })
            .collect(),
    }
}

/// Hash map heavy: many small maps.
#[derive(Serialize, Deserialize)]
pub struct Registry {
    entries: Vec<HashMap<String, u64>>,
}

pub fn registry() -> Registry {
    let mut rng = Rng(0x0bad_cafe);
    Registry {
        entries: (0..2_000)
            .map(|_| {
                (0..12)
                    .map(|key| (format!("key{}", key), rng.next() % 100_000))
                    .collect()
            })
            .collect(),
    }
}

/// Container heavy: a tree of small nodes.  Every node is a struct with a
/// sequence, so most events are container starts and ends.
#[derive(Serialize, Deserialize)]
pub struct Node {
    id: u32,
    weight: (u8, u8),
    children: Vec<Node>,
}

pub fn tree() -> Node {
    fn build(id: &mut u32, depth: u32) -> Node {
        *id += 1;
        let this = *id;
        Node {
            id: this,
            weight: (this as u8, depth as u8),
            children: if depth == 0 {
                Vec::new()
            } else {
                (0..4).map(|_| build(id, depth - 1)).collect()
            },
        }
    }
    build(&mut 0, 7)
}
