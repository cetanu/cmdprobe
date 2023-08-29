use jmespath::Variable;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Map, Value as JsonValue};
use std::collections::HashMap;
use std::rc::Rc;
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
    pub delay_after: Option<u64>,
    pub check: CheckCommand,
    pub matchers: Vec<StdoutMatcher>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum CheckCommand {
    Shell(String),
    HttpRequest {
        url: String,
        headers: HashMap<String, String>,
        method: String,
        // TODO: asap auth
    },
}

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
pub enum Backreference {
    Named(String),
    Numbered(usize),
}

#[derive(Deserialize, Debug)]
pub enum Operation {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
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
    value: &JsonValue,
    replacements: &HashMap<Backreference, String>,
) -> JsonValue {
    match value {
        JsonValue::Object(obj) => {
            let mut new_obj = Map::new();
            for (k, v) in obj {
                new_obj.insert(k.clone(), format_nested_variables(v, replacements));
            }
            JsonValue::Object(new_obj)
        }
        JsonValue::Array(arr) => {
            let new_arr = arr
                .iter()
                .map(|v| format_nested_variables(v, replacements))
                .collect();
            JsonValue::Array(new_arr)
        }
        JsonValue::String(s) => {
            let new_s = format_variables(s, replacements);
            JsonValue::String(new_s)
        }
        _ => value.clone(),
    }
}

fn var_to_json(var: Rc<Variable>) -> JsonValue {
    match &*var {
        Variable::Null => JsonValue::Null,
        Variable::Bool(b) => JsonValue::Bool(*b),
        Variable::Number(n) => JsonValue::Number(n.clone()),
        Variable::String(s) => JsonValue::String(s.clone()),
        Variable::Array(arr) => {
            JsonValue::Array(arr.iter().map(|v| var_to_json(Rc::clone(v))).collect())
        }
        Variable::Object(obj) => JsonValue::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), var_to_json(Rc::clone(v))))
                .collect(),
        ),
        _ => panic!(),
    }
}

pub fn check_stdout(
    stage: &CheckStage,
    test_name: &str,
    output: &str,
    saved: &mut HashMap<Backreference, String>,
) -> bool {
    // Record the overall result of all matchers
    // but still execute them all to produce logs/metrics
    let mut combined_result = true;

    for matcher in &stage.matchers {
        let result = match matcher {
            StdoutMatcher::JmesPath {
                jmespath: expression,
                operation: op,
                value: expected,
            } => {
                let expr = jmespath::compile(expression).unwrap();
                let data = jmespath::Variable::from_json(output).unwrap();
                let value = expr.search(data).unwrap();
                match op {
                    Operation::Eq => &var_to_json(value) == expected,
                    _ => panic!(),
                }
            }
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
                let expected = format_variables(expected, saved);
                let pattern = Regex::new(&expected).expect("Not a valid regex");
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
        };
        if !result {
            combined_result = false;
        }
    }
    combined_result
}
