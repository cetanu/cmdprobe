use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;
use tracing::debug;

#[derive(Deserialize, Debug)]
pub struct CheckConfig {
    pub test_name: String,
    pub stages: Vec<CheckStage>,
}

#[derive(Deserialize, Debug)]
pub struct CheckStage {
    pub name: String,
    #[serde(default = "default_retries")]
    pub max_retries: u32,
    pub delay_before: Option<u64>,
    pub command: String,
    pub stdout: StdoutMatcher,
}

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
pub enum Backreference {
    Named(String),
    Numbered(usize),
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum StdoutMatcher {
    Exact {
        exact: String,
    },
    Regex {
        regex: String,
    },
    Json {
        json: Option<Value>,
        save: Option<HashMap<String, String>>,
    },
}

fn default_retries() -> u32 {
    1
}

/// Replaces {{ variables }} with values from a map
pub fn format_variables(input: &str, map: &HashMap<Backreference, String>) -> String {
    let mut output = input.to_owned();
    for (key, value) in map {
        let pattern = match key {
            Backreference::Named(name) => format!(r"\{{\{{\s*{}\s*\}}\}}", name),
            Backreference::Numbered(index) => format!(r"\{{\{{\s*{}\s*\}}\}}", index.to_string()),
        };
        let re = Regex::new(&pattern).unwrap();
        output = re.replace_all(&output, value).to_string();
    }
    output
}

/// Walks a json structure and replaces {{ variables }} in strings with values from a map
pub fn format_nested_variables(
    value: &Value,
    replacements: &HashMap<Backreference, String>,
) -> Value {
    match value {
        Value::Object(obj) => {
            let mut new_obj = Map::new();
            for (k, v) in obj {
                new_obj.insert(k.clone(), format_nested_variables(v, replacements));
            }
            Value::Object(new_obj)
        }
        Value::Array(arr) => {
            let new_arr = arr
                .iter()
                .map(|v| format_nested_variables(v, replacements))
                .collect();
            Value::Array(new_arr)
        }
        Value::String(s) => {
            let new_s = format_variables(s, replacements);
            Value::String(new_s)
        }
        _ => value.clone(),
    }
}

pub fn check_stdout(
    stage: &CheckStage,
    test_name: &str,
    output: &str,
    saved: &mut HashMap<Backreference, String>,
) -> bool {
    match &stage.stdout {
        StdoutMatcher::Exact { exact: expected } => {
            debug!(
                test_name,
                stage.name,
                expected = expected,
                response = output
            );
            output.trim() == expected
        }
        StdoutMatcher::Regex { regex: expected } => {
            debug!(
                test_name,
                stage.name,
                expected = expected,
                response = output
            );
            let pattern = Regex::new(expected).expect("Not a valid regex");
            let captures = pattern.captures(output).unwrap();

            // Store capture groups into map
            // so that they can be used in the next stage
            // using format strings in the command
            for (i, value) in captures.iter().enumerate() {
                if let Some(v) = value {
                    saved.insert(Backreference::Numbered(i), v.as_str().to_string());
                }
            }
            for name in pattern.capture_names().flatten() {
                if let Some(v) = captures.name(name) {
                    saved.insert(
                        Backreference::Named(name.to_string()),
                        v.as_str().to_string(),
                    );
                }
            }
            debug!(saved_values = ?saved);

            // Check that value is as expected
            pattern.is_match(output)
        }
        StdoutMatcher::Json {
            json: expected,
            save: save_map,
        } => {
            let mut json_check_is_successful = false;
            if let Ok(response) = serde_json::from_str(output) {
                debug!(test_name, stage.name, expected = ?expected, response = %response);

                // Replace format strings with saved values from previous stage
                json_check_is_successful = if let Some(json) = &expected {
                    let json = format_nested_variables(json, saved);

                    // Check that value is as expected
                    crate::json::compare(&json, &response)
                } else {
                    // expected json was not supplied
                    true
                };

                // Save any values from the response using jmespath expressions
                if let Some(expressions) = &save_map {
                    for (key, expression) in expressions.iter() {
                        let expr = jmespath::compile(expression).unwrap();
                        let data = jmespath::Variable::from_json(output).unwrap();
                        let value = expr.search(data).unwrap();
                        let value = match *value {
                            jmespath::Variable::String(ref s) => s.to_string(),
                            jmespath::Variable::Number(ref n) => n.to_string(),
                            jmespath::Variable::Bool(b) => b.to_string(),
                            jmespath::Variable::Null => "null".to_string(),
                            ref obj => panic!(
                                "Attempt to save non-stringable value using jmespath {expr}: {}",
                                obj.to_string()
                            ),
                        };
                        saved.insert(Backreference::Named(key.to_string()), value);
                    }
                }
                debug!(saved_values = ?saved);
            };
            json_check_is_successful
        }
    }
}
