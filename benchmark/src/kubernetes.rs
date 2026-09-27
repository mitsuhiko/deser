//! The OpenAPI (Swagger 2.0) description of the Kubernetes API
//! (`api/openapi-spec/swagger.json` of `kubernetes/kubernetes`, vendored
//! with `scripts/update-benchmark-data.sh`).
//!
//! A large configuration-like document: maps with hundreds of entries,
//! recursive schemas, references that are an untagged enum, unit enums and
//! lots of optional fields that are left out when they are not set.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct Swagger {
    swagger: String,
    info: Info,
    paths: BTreeMap<String, PathItem>,
    definitions: BTreeMap<String, Schema>,
    parameters: BTreeMap<String, Parameter>,
    security: Vec<BTreeMap<String, Vec<String>>>,
    #[deser(rename = "securityDefinitions")]
    #[serde(rename = "securityDefinitions")]
    security_definitions: BTreeMap<String, SecurityScheme>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Info {
    title: String,
    version: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct SecurityScheme {
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[deser(rename = "in")]
    #[serde(rename = "in")]
    location: String,
    name: String,
    #[deser(rename = "type")]
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct PathItem {
    #[deser(default, skip_serializing_if = Vec::is_empty)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parameters: Vec<ParameterOrRef>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    get: Option<Operation>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    put: Option<Operation>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    post: Option<Operation>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    delete: Option<Operation>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    patch: Option<Operation>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    head: Option<Operation>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    options: Option<Operation>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Operation {
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[deser(rename = "operationId")]
    #[serde(rename = "operationId")]
    operation_id: String,
    #[deser(default, skip_serializing_if = Vec::is_empty)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    consumes: Vec<String>,
    #[deser(default, skip_serializing_if = Vec::is_empty)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    produces: Vec<String>,
    schemes: Vec<Scheme>,
    tags: Vec<String>,
    #[deser(default, skip_serializing_if = Vec::is_empty)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    parameters: Vec<ParameterOrRef>,
    responses: BTreeMap<String, Response>,
    #[deser(rename = "x-kubernetes-action", default, skip_serializing_if = Option::is_none)]
    #[serde(
        rename = "x-kubernetes-action",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    action: Option<Action>,
    #[deser(
        rename = "x-kubernetes-group-version-kind",
        default,
        skip_serializing_if = Option::is_none
    )]
    #[serde(
        rename = "x-kubernetes-group-version-kind",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    group_version_kind: Option<GroupVersionKind>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum Scheme {
    Http,
    Https,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum Action {
    Get,
    List,
    Watch,
    Watchlist,
    Put,
    Post,
    Patch,
    Delete,
    Deletecollection,
    Connect,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct GroupVersionKind {
    group: String,
    version: String,
    kind: String,
}

/// A parameter or a reference to one (untagged, as in the OpenAPI crates).
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(untagged)]
#[serde(untagged)]
enum ParameterOrRef {
    Ref {
        #[deser(rename = "$ref")]
        #[serde(rename = "$ref")]
        reference: String,
    },
    Parameter(Box<Parameter>),
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Parameter {
    name: String,
    #[deser(rename = "in")]
    #[serde(rename = "in")]
    location: ParameterLocation,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[deser(default)]
    #[serde(default)]
    required: bool,
    #[deser(rename = "type", default, skip_serializing_if = Option::is_none)]
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[deser(rename = "uniqueItems", default)]
    #[serde(rename = "uniqueItems", default)]
    unique_items: bool,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema: Option<Schema>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum ParameterLocation {
    Path,
    Query,
    Body,
    Header,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Response {
    description: String,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    schema: Option<Schema>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(skip_serializing_optionals)]
struct Schema {
    #[deser(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[deser(rename = "type", default)]
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    kind: Option<SchemaType>,
    #[deser(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    format: Option<String>,
    #[deser(rename = "$ref", default)]
    #[serde(rename = "$ref", default, skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    #[deser(default, skip_serializing_if = BTreeMap::is_empty)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    properties: BTreeMap<String, Schema>,
    #[deser(default)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    items: Option<Box<Schema>>,
    #[deser(default, skip_serializing_if = Vec::is_empty)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    required: Vec<String>,
    #[deser(rename = "additionalProperties", default)]
    #[serde(
        rename = "additionalProperties",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    additional_properties: Option<Box<Schema>>,
    #[deser(rename = "x-kubernetes-list-type", default)]
    #[serde(
        rename = "x-kubernetes-list-type",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    list_type: Option<String>,
    #[deser(
        rename = "x-kubernetes-list-map-keys",
        default,
        skip_serializing_if = Vec::is_empty
    )]
    #[serde(
        rename = "x-kubernetes-list-map-keys",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    list_map_keys: Vec<String>,
    #[deser(rename = "x-kubernetes-map-type", default)]
    #[serde(
        rename = "x-kubernetes-map-type",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    map_type: Option<String>,
    #[deser(rename = "x-kubernetes-patch-strategy", default)]
    #[serde(
        rename = "x-kubernetes-patch-strategy",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    patch_strategy: Option<String>,
    #[deser(rename = "x-kubernetes-patch-merge-key", default)]
    #[serde(
        rename = "x-kubernetes-patch-merge-key",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    patch_merge_key: Option<String>,
    #[deser(
        rename = "x-kubernetes-group-version-kind",
        default,
        skip_serializing_if = Vec::is_empty
    )]
    #[serde(
        rename = "x-kubernetes-group-version-kind",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    group_version_kind: Vec<GroupVersionKind>,
    #[deser(rename = "x-kubernetes-unions", default, skip_serializing_if = Vec::is_empty)]
    #[serde(
        rename = "x-kubernetes-unions",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    unions: Vec<Union>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
enum SchemaType {
    String,
    Object,
    Array,
    Integer,
    Boolean,
    Number,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct Union {
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    discriminator: Option<String>,
    #[deser(rename = "fields-to-discriminateBy")]
    #[serde(rename = "fields-to-discriminateBy")]
    fields: BTreeMap<String, String>,
}
