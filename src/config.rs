use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value as JsonValue;

#[derive(Deserialize, Debug)]
pub(crate) struct CheckConfig {
    pub(crate) test_name: String,
    pub(crate) stages: Vec<CheckStage>,
}

#[derive(Deserialize, Debug)]
pub(crate) struct CheckStage {
    pub(crate) name: String,
    #[serde(default = "default_retries")]
    pub(crate) max_retries: u32,
    pub(crate) delay_before: Option<u64>,
    pub(crate) delay_after: Option<u64>,
    pub(crate) check: CheckCommand,
    #[serde(default)]
    pub(crate) matchers: Vec<StdoutMatcher>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub(crate) enum CheckCommand {
    Shell(String),
    HttpRequest {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
        method: String,
    },
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Operation {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub(crate) enum StdoutMatcher {
    Exact {
        exact: String,
    },
    Regex {
        regex: String,
    },
    Json {
        json: Option<JsonValue>,
        save: Option<HashMap<String, String>>,
    },
    JmesPath {
        jmespath: String,
        operation: Operation,
        value: JsonValue,
    },
}

fn default_retries() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_example_config_deserializes() {
        let documents = yaml_serde::Deserializer::from_str(include_str!("../cmdprobe.yaml"))
            .map(CheckConfig::deserialize)
            .collect::<Result<Vec<_>, _>>()
            .expect("example configuration should deserialize");

        assert_eq!(documents.len(), 4);
    }
}
