use core::hint::black_box;

use backwards_compat::backwards_compat;
use criterion::{Criterion, criterion_group, criterion_main};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
struct ConfigV0 {
    legacy: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigV1 {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigV2 {
    name: String,
    host: String,
}

#[backwards_compat(
    tag = "version",
    versions(
        _: ConfigV0 => Self,
        1: ConfigV1,
        2: ConfigV2,
        3: Self,
    )
)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    name: String,
    host: String,
    port: u16,
    enabled: bool,
    tags: Vec<String>,
}

impl From<ConfigV0> for Config {
    fn from(v0: ConfigV0) -> Self {
        Self {
            name: v0.legacy,
            host: "127.0.0.1".to_owned(),
            port: 8080,
            enabled: true,
            tags: Vec::new(),
        }
    }
}

impl From<ConfigV1> for ConfigV2 {
    fn from(v1: ConfigV1) -> Self {
        Self {
            name: v1.name,
            host: "127.0.0.1".to_owned(),
        }
    }
}

impl From<ConfigV2> for Config {
    fn from(v2: ConfigV2) -> Self {
        Self {
            name: v2.name,
            host: v2.host,
            port: 8080,
            enabled: true,
            tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct AdjacentConfigV1 {
    name: String,
}

#[backwards_compat(
    tag = "version",
    content = "data",
    versions(1: AdjacentConfigV1, 2: Self)
)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct AdjacentConfig {
    name: String,
    host: String,
    port: u16,
    enabled: bool,
    tags: Vec<String>,
}

impl From<AdjacentConfigV1> for AdjacentConfig {
    fn from(v1: AdjacentConfigV1) -> Self {
        Self {
            name: v1.name,
            host: "127.0.0.1".to_owned(),
            port: 8080,
            enabled: true,
            tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "version")]
enum ManualConfig {
    #[serde(rename = "1")]
    V1 { name: String },
    #[serde(rename = "2")]
    V2 { name: String, host: String },
    #[serde(rename = "3")]
    V3 {
        name: String,
        host: String,
        port: u16,
        enabled: bool,
        tags: Vec<String>,
    },
}

const JSON_TAG_FIRST: &str = r#"{"version":3,"name":"service-alpha","host":"127.0.0.1","port":8080,"enabled":true,"tags":["api","prod","v3"]}"#;
const JSON_OUT_OF_ORDER: &str = r#"{"name":"service-alpha","host":"127.0.0.1","port":8080,"enabled":true,"tags":["api","prod","v3"],"version":3}"#;
const JSON_UNTAGGED: &str = r#"{"legacy":"service-alpha"}"#;
const JSON_V1_CHAIN: &str = r#"{"version":1,"name":"service-alpha"}"#;
const JSON_MANUAL: &str = r#"{"version":"3","name":"service-alpha","host":"127.0.0.1","port":8080,"enabled":true,"tags":["api","prod","v3"]}"#;

const RON_TAG_FIRST: &str =
    r#"(version:3,name:"service-alpha",host:"127.0.0.1",port:8080,enabled:true,tags:["api","prod","v3"])"#;
const RON_OUT_OF_ORDER: &str =
    r#"(name:"service-alpha",host:"127.0.0.1",port:8080,enabled:true,tags:["api","prod","v3"],version:3)"#;
const RON_UNTAGGED: &str = r#"(legacy:"service-alpha")"#;
const RON_V1_CHAIN: &str = r#"(version:1,name:"service-alpha")"#;
const RON_MANUAL: &str =
    r#"(version:"3",name:"service-alpha",host:"127.0.0.1",port:8080,enabled:true,tags:["api","prod","v3"])"#;

const TOML_TAG_FIRST: &str = "\
version = 3
name = \"service-alpha\"
host = \"127.0.0.1\"
port = 8080
enabled = true
tags = [\"api\", \"prod\", \"v3\"]
";
const TOML_OUT_OF_ORDER: &str = "\
name = \"service-alpha\"
host = \"127.0.0.1\"
port = 8080
enabled = true
tags = [\"api\", \"prod\", \"v3\"]
version = 3
";
const TOML_UNTAGGED: &str = "legacy = \"service-alpha\"\n";
const TOML_V1_CHAIN: &str = "version = 1\nname = \"service-alpha\"\n";
const TOML_MANUAL: &str = "\
version = \"3\"
name = \"service-alpha\"
host = \"127.0.0.1\"
port = 8080
enabled = true
tags = [\"api\", \"prod\", \"v3\"]
";

fn sample_config() -> Config {
    Config {
        name: "service-alpha".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: 8080,
        enabled: true,
        tags: vec!["api".to_owned(), "prod".to_owned(), "v3".to_owned()],
    }
}

fn sample_adjacent_config() -> AdjacentConfig {
    AdjacentConfig {
        name: "service-alpha".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: 8080,
        enabled: true,
        tags: vec!["api".to_owned(), "prod".to_owned(), "v3".to_owned()],
    }
}

fn sample_manual_config() -> ManualConfig {
    ManualConfig::V3 {
        name: "service-alpha".to_owned(),
        host: "127.0.0.1".to_owned(),
        port: 8080,
        enabled: true,
        tags: vec!["api".to_owned(), "prod".to_owned(), "v3".to_owned()],
    }
}

fn bench_deserialize_json(c: &mut Criterion) {
    let mut group = c.benchmark_group("deserialize/json");

    group.bench_function("tag_first_streaming", |b| {
        b.iter(|| serde_json::from_str::<Config>(black_box(JSON_TAG_FIRST)).expect("valid tag-first JSON"));
    });

    group.bench_function("out_of_order_buffering", |b| {
        b.iter(|| {
            serde_json::from_str::<Config>(black_box(JSON_OUT_OF_ORDER)).expect("valid out-of-order JSON")
        });
    });

    group.bench_function("untagged_fallback", |b| {
        b.iter(|| serde_json::from_str::<Config>(black_box(JSON_UNTAGGED)).expect("valid untagged JSON"));
    });

    group.bench_function("migrate_v1_to_v3", |b| {
        b.iter(|| serde_json::from_str::<Config>(black_box(JSON_V1_CHAIN)).expect("valid v1 JSON"));
    });

    group.bench_function("manual_serde_tag_enum", |b| {
        b.iter(|| serde_json::from_str::<ManualConfig>(black_box(JSON_MANUAL)).expect("valid manual JSON"));
    });

    group.finish();
}

fn bench_deserialize_ron(c: &mut Criterion) {
    let mut group = c.benchmark_group("deserialize/ron");

    group.bench_function("tag_first_streaming", |b| {
        b.iter(|| ron::from_str::<Config>(black_box(RON_TAG_FIRST)).expect("valid tag-first RON"));
    });

    group.bench_function("out_of_order_buffering", |b| {
        b.iter(|| ron::from_str::<Config>(black_box(RON_OUT_OF_ORDER)).expect("valid out-of-order RON"));
    });

    group.bench_function("untagged_fallback", |b| {
        b.iter(|| ron::from_str::<Config>(black_box(RON_UNTAGGED)).expect("valid untagged RON"));
    });

    group.bench_function("migrate_v1_to_v3", |b| {
        b.iter(|| ron::from_str::<Config>(black_box(RON_V1_CHAIN)).expect("valid v1 RON"));
    });

    group.bench_function("manual_serde_tag_enum", |b| {
        b.iter(|| ron::from_str::<ManualConfig>(black_box(RON_MANUAL)).expect("valid manual RON"));
    });

    group.finish();
}

fn bench_deserialize_toml(c: &mut Criterion) {
    let mut group = c.benchmark_group("deserialize/toml");

    group.bench_function("tag_first_streaming", |b| {
        b.iter(|| toml::from_str::<Config>(black_box(TOML_TAG_FIRST)).expect("valid tag-first TOML"));
    });

    group.bench_function("out_of_order_buffering", |b| {
        b.iter(|| toml::from_str::<Config>(black_box(TOML_OUT_OF_ORDER)).expect("valid out-of-order TOML"));
    });

    group.bench_function("untagged_fallback", |b| {
        b.iter(|| toml::from_str::<Config>(black_box(TOML_UNTAGGED)).expect("valid untagged TOML"));
    });

    group.bench_function("migrate_v1_to_v3", |b| {
        b.iter(|| toml::from_str::<Config>(black_box(TOML_V1_CHAIN)).expect("valid v1 TOML"));
    });

    group.bench_function("manual_serde_tag_enum", |b| {
        b.iter(|| toml::from_str::<ManualConfig>(black_box(TOML_MANUAL)).expect("valid manual TOML"));
    });

    group.finish();
}

fn bench_serialize(c: &mut Criterion) {
    let config = sample_config();
    let adjacent = sample_adjacent_config();
    let manual = sample_manual_config();

    {
        let mut json_group = c.benchmark_group("serialize/json");
        json_group.bench_function("internally_tagged", |b| {
            b.iter(|| serde_json::to_string(black_box(&config)).expect("serialize JSON"));
        });
        json_group.bench_function("manual_serde_tag_enum", |b| {
            b.iter(|| serde_json::to_string(black_box(&manual)).expect("serialize manual JSON"));
        });
        json_group.bench_function("adjacently_tagged", |b| {
            b.iter(|| serde_json::to_string(black_box(&adjacent)).expect("serialize adjacent JSON"));
        });
        json_group.finish();
    }

    {
        let mut ron_group = c.benchmark_group("serialize/ron");
        ron_group.bench_function("internally_tagged", |b| {
            b.iter(|| ron::to_string(black_box(&config)).expect("serialize RON"));
        });
        ron_group.bench_function("manual_serde_tag_enum", |b| {
            b.iter(|| ron::to_string(black_box(&manual)).expect("serialize manual RON"));
        });
        ron_group.bench_function("adjacently_tagged", |b| {
            b.iter(|| ron::to_string(black_box(&adjacent)).expect("serialize adjacent RON"));
        });
        ron_group.finish();
    }

    {
        let mut toml_group = c.benchmark_group("serialize/toml");
        toml_group.bench_function("internally_tagged", |b| {
            b.iter(|| toml::to_string(black_box(&config)).expect("serialize TOML"));
        });
        toml_group.bench_function("manual_serde_tag_enum", |b| {
            b.iter(|| toml::to_string(black_box(&manual)).expect("serialize manual TOML"));
        });
        toml_group.bench_function("adjacently_tagged", |b| {
            b.iter(|| toml::to_string(black_box(&adjacent)).expect("serialize adjacent TOML"));
        });
        toml_group.finish();
    }
}

criterion_group!(
    benches,
    bench_deserialize_json,
    bench_deserialize_ron,
    bench_deserialize_toml,
    bench_serialize
);
criterion_main!(benches);
