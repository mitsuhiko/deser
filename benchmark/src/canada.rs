//! The types of `canada.json` (from json-benchmark): the border of Canada as
//! GeoJSON.  Almost all of it are floats in small sequences.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct FeatureCollection {
    #[deser(rename = "type")]
    #[serde(rename = "type")]
    obj_type: ObjType,
    features: Vec<Feature>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Feature {
    #[deser(rename = "type")]
    #[serde(rename = "type")]
    obj_type: ObjType,
    properties: BTreeMap<String, String>,
    geometry: Geometry,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Geometry {
    #[deser(rename = "type")]
    #[serde(rename = "type")]
    obj_type: ObjType,
    coordinates: Vec<Vec<(Latitude, Longitude)>>,
}

type Latitude = f32;
type Longitude = f32;

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
enum ObjType {
    FeatureCollection,
    Feature,
    Polygon,
}
