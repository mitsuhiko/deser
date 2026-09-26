//! The types of `citm_catalog.json` (from json-benchmark): a catalog of
//! events and performances.  Mostly integers and maps keyed by ids.
//!
//! The fields that are always `null` are `Option<String>` (json-benchmark
//! uses `()`) as TOML has no null and leaves them out.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};

type Map<V> = BTreeMap<String, V>;
type Id = u32;

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
pub struct CitmCatalog {
    area_names: Map<String>,
    audience_sub_category_names: Map<String>,
    block_names: Map<String>,
    events: Map<Event>,
    performances: Vec<Performance>,
    seat_category_names: Map<String>,
    sub_topic_names: Map<String>,
    subject_names: Map<String>,
    topic_names: Map<String>,
    topic_sub_topics: Map<Vec<Id>>,
    venue_names: Map<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Event {
    description: Option<String>,
    id: Id,
    logo: Option<String>,
    name: String,
    sub_topic_ids: Vec<Id>,
    subject_code: Option<String>,
    subtitle: Option<String>,
    topic_ids: Vec<Id>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Performance {
    event_id: Id,
    id: Id,
    logo: Option<String>,
    name: Option<String>,
    prices: Vec<Price>,
    seat_categories: Vec<SeatCategory>,
    seat_map_image: Option<String>,
    start: u64,
    venue_code: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Price {
    amount: u32,
    audience_sub_category_id: Id,
    seat_category_id: Id,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct SeatCategory {
    areas: Vec<Area>,
    seat_category_id: Id,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Area {
    area_id: Id,
    /// always empty
    block_ids: Vec<Id>,
}
