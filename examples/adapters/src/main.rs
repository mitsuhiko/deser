//! Adapters: serializing and deserializing types on behalf of others.
//!
//! `#[deser(as = Adapter)]` picks an adapter for a field.  Adapters work for
//! types that do not implement `Serialize` or `Deserialize` themselves or
//! change how a type is represented.  They compose with the standard
//! containers: `Option<DisplayFromStr>` is an adapter for `Option<T>` and
//! `BTreeMap<_, Vec<DisplayFromStr>>` for maps of vectors where `_` stands
//! for the type's own implementation.
//!
//! Some adapters handle errors: `DefaultOnError` falls back to the default
//! and `VecSkipError` skips elements that fail to deserialize, which helps
//! with data from newer versions of a program.
use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};

use deser::adapters::{As, DefaultOnError, DisplayFromStr, FromInto, VecSkipError};
use deser::{Deserialize, Serialize};

/// A type from another crate that knows nothing about deser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(u8, u8, u8);

impl From<(u8, u8, u8)> for Rgb {
    fn from((r, g, b): (u8, u8, u8)) -> Rgb {
        Rgb(r, g, b)
    }
}

impl From<Rgb> for (u8, u8, u8) {
    fn from(Rgb(r, g, b): Rgb) -> (u8, u8, u8) {
        (r, g, b)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[deser(rename_all = "snake_case")]
pub enum Protocol {
    Http,
    Https,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// written as string with `Display`, read with `FromStr`
    #[deser(as = DisplayFromStr)]
    listen: SocketAddr,
    /// optional stays optional
    #[deser(as = Option<DisplayFromStr>)]
    upstream: Option<IpAddr>,
    /// the keys use their own implementation, the values an adapter
    #[deser(as = BTreeMap<_, Vec<DisplayFromStr>>)]
    aliases: BTreeMap<String, Vec<IpAddr>>,
    /// converted from and into a tuple which deser supports
    #[deser(as = FromInto<(u8, u8, u8)>)]
    color: Rgb,
    /// unknown protocols are skipped
    #[deser(as = VecSkipError)]
    protocols: Vec<Protocol>,
    /// an invalid value becomes the default
    #[deser(as = DefaultOnError, default)]
    workers: u32,
}

fn main() {
    let config: Config = deser_json::from_str(
        r#"{
            "listen": "127.0.0.1:8080",
            "aliases": {"local": ["127.0.0.1", "::1"]},
            "color": [255, 128, 0],
            "protocols": ["http", "gopher", "https"],
            "workers": "many"
        }"#,
    )
    .unwrap();
    println!("{:#?}", config);
    assert_eq!(config.upstream, None);
    assert_eq!(config.color, Rgb(255, 128, 0));
    assert_eq!(config.protocols.len(), 2);
    assert_eq!(config.workers, 0);

    let json = deser_json::to_string(&config).unwrap();
    println!("{}", json);
    assert_eq!(
        json,
        r#"{"listen":"127.0.0.1:8080","upstream":null,"aliases":{"local":["127.0.0.1","::1"]},"color":[255,128,0],"protocols":["http","https"],"workers":0}"#
    );

    // outside of the derive the `As` wrapper applies an adapter
    let addrs: Vec<As<IpAddr, DisplayFromStr>> =
        deser_json::from_str(r#"["10.0.0.1", "fe80::1"]"#).unwrap();
    println!("{:?}", addrs.iter().map(|x| **x).collect::<Vec<_>>());
    assert!(addrs[1].is_ipv6());
}
