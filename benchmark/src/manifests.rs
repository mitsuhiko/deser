//! Kubernetes manifests (deployments, services and config maps), generated.
//!
//! Configuration as people write it: resources that are internally tagged
//! by `kind`, camel case names, unit enums, a flattened struct, maps of
//! labels and data and optional values.
use std::collections::BTreeMap;

use deser::{Deserialize, Serialize};

/// The number of applications (each has a deployment, a service and a
/// config map).
const APPS: usize = 200;

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
pub struct ManifestList {
    items: Vec<Manifest>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(tag = "kind")]
#[serde(tag = "kind")]
enum Manifest {
    Deployment {
        #[deser(rename = "apiVersion")]
        #[serde(rename = "apiVersion")]
        api_version: String,
        metadata: ObjectMeta,
        spec: DeploymentSpec,
    },
    Service {
        #[deser(rename = "apiVersion")]
        #[serde(rename = "apiVersion")]
        api_version: String,
        metadata: ObjectMeta,
        spec: ServiceSpec,
    },
    ConfigMap {
        #[deser(rename = "apiVersion")]
        #[serde(rename = "apiVersion")]
        api_version: String,
        metadata: ObjectMeta,
        data: BTreeMap<String, String>,
    },
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct ObjectMeta {
    name: String,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
    #[deser(default, skip_serializing_if = BTreeMap::is_empty)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    labels: BTreeMap<String, String>,
    #[deser(default, skip_serializing_if = BTreeMap::is_empty)]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    annotations: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct DeploymentSpec {
    replicas: u32,
    selector: LabelSelector,
    template: PodTemplate,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revision_history_limit: Option<u32>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct LabelSelector {
    match_labels: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct PodTemplate {
    metadata: ObjectMeta,
    spec: PodSpec,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct PodSpec {
    containers: Vec<Container>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    restart_policy: Option<RestartPolicy>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    service_account_name: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
enum RestartPolicy {
    Always,
    OnFailure,
    Never,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Container {
    name: String,
    image: String,
    #[deser(default, skip_serializing_if = Vec::is_empty)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[deser(default, skip_serializing_if = Vec::is_empty)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    ports: Vec<ContainerPort>,
    #[deser(default, skip_serializing_if = Vec::is_empty)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    env: Vec<EnvVar>,
    #[deser(flatten)]
    #[serde(flatten)]
    resources: Resources,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_pull_policy: Option<PullPolicy>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct ContainerPort {
    container_port: u16,
    protocol: Protocol,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
enum Protocol {
    Tcp,
    Udp,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
enum PullPolicy {
    Always,
    IfNotPresent,
    Never,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct EnvVar {
    name: String,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

/// Limits of a container (flattened into it).
#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct Resources {
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cpu_limit: Option<String>,
    #[deser(default, skip_serializing_if = Option::is_none)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    memory_limit: Option<String>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
struct ServiceSpec {
    #[deser(rename = "type")]
    #[serde(rename = "type")]
    kind: ServiceType,
    selector: BTreeMap<String, String>,
    ports: Vec<ServicePort>,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
enum ServiceType {
    ClusterIP,
    NodePort,
    LoadBalancer,
}

#[derive(Serialize, Deserialize, serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[deser(rename_all = "camelCase")]
#[serde(rename_all = "camelCase")]
struct ServicePort {
    port: u16,
    target_port: u16,
    protocol: Protocol,
}

fn labels(app: &str, tier: &str) -> BTreeMap<String, String> {
    [
        ("app.kubernetes.io/name", app),
        ("app.kubernetes.io/part-of", "shop"),
        ("tier", tier),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_string(), value.to_string()))
    .collect()
}

pub fn manifests() -> ManifestList {
    let mut items = Vec::new();
    for index in 0..APPS {
        let app = format!("service-{:03}", index);
        let tier = ["frontend", "backend", "worker"][index % 3];
        let namespace = (index % 4 != 0).then(|| "production".to_string());
        let meta = || ObjectMeta {
            name: app.clone(),
            namespace: namespace.clone(),
            labels: labels(&app, tier),
            annotations: BTreeMap::new(),
        };
        let mut annotations = BTreeMap::new();
        annotations.insert(
            "deployment.kubernetes.io/revision".to_string(),
            (index % 7 + 1).to_string(),
        );
        items.push(Manifest::Deployment {
            api_version: "apps/v1".into(),
            metadata: ObjectMeta {
                annotations,
                ..meta()
            },
            spec: DeploymentSpec {
                replicas: (index % 5 + 1) as u32,
                selector: LabelSelector {
                    match_labels: labels(&app, tier),
                },
                template: PodTemplate {
                    metadata: meta(),
                    spec: PodSpec {
                        containers: (0..1 + index % 2)
                            .map(|container| Container {
                                name: if container == 0 {
                                    app.clone()
                                } else {
                                    "sidecar".into()
                                },
                                image: format!(
                                    "registry.example.com/shop/{}:1.{}.0",
                                    app,
                                    index % 10
                                ),
                                args: if container == 0 {
                                    vec!["--port=8080".into(), "--log-format=json".into()]
                                } else {
                                    Vec::new()
                                },
                                ports: vec![ContainerPort {
                                    container_port: 8080 + container as u16,
                                    protocol: Protocol::Tcp,
                                }],
                                env: vec![
                                    EnvVar {
                                        name: "RUST_LOG".into(),
                                        value: Some("info".into()),
                                    },
                                    EnvVar {
                                        name: "DATABASE_URL".into(),
                                        value: (index % 2 == 0)
                                            .then(|| "postgres://db.production/shop".into()),
                                    },
                                ],
                                resources: Resources {
                                    cpu_limit: Some(format!("{}m", 250 * (index % 4 + 1))),
                                    memory_limit: (index % 3 != 0)
                                        .then(|| format!("{}Mi", 256 * (index % 4 + 1))),
                                },
                                image_pull_policy: (index % 2 == 0)
                                    .then_some(PullPolicy::IfNotPresent),
                            })
                            .collect(),
                        restart_policy: Some(RestartPolicy::Always),
                        service_account_name: (index % 3 == 0).then(|| app.clone()),
                    },
                },
                revision_history_limit: (index % 2 == 0).then_some(10),
            },
        });
        items.push(Manifest::Service {
            api_version: "v1".into(),
            metadata: meta(),
            spec: ServiceSpec {
                kind: if index % 10 == 0 {
                    ServiceType::LoadBalancer
                } else {
                    ServiceType::ClusterIP
                },
                selector: labels(&app, tier),
                ports: vec![ServicePort {
                    port: 80,
                    target_port: 8080,
                    protocol: Protocol::Tcp,
                }],
            },
        });
        let mut data = BTreeMap::new();
        data.insert(
            "config.toml".to_string(),
            format!("[server]\nport = 8080\nname = \"{}\"\n", app),
        );
        data.insert(
            "feature-flags".to_string(),
            "search,checkout-v2".to_string(),
        );
        items.push(Manifest::ConfigMap {
            api_version: "v1".into(),
            metadata: meta(),
            data,
        });
    }
    ManifestList { items }
}
