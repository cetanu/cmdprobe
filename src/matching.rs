use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::{Context, Result, anyhow};
use jmespath::Variable;
use regex::Regex;
use serde_json::{Map, Value as JsonValue};
use tracing::debug;

use crate::config::{CheckStage, Operation, StdoutMatcher};
use crate::json;

#[derive(Hash, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Backreference {
    Named(String),
    Numbered(usize),
}

/// Replaces `{{ variables }}` with values from a map.
pub(crate) fn format_variables(
    input: &str,
    replacements: &HashMap<Backreference, String>,
) -> String {
    let pattern = Regex::new(r"\{\{\s*(?P<key>[A-Za-z_][A-Za-z0-9_]*|[0-9]+)\s*\}\}")
        .expect("variable placeholder pattern is valid");

    pattern
        .replace_all(input, |captures: &regex::Captures<'_>| {
            let key = &captures["key"];
            let reference = match key.parse::<usize>() {
                Ok(index) => Backreference::Numbered(index),
                Err(_) => Backreference::Named(key.to_owned()),
            };
            replacements
                .get(&reference)
                .map(String::as_str)
                .or_else(|| captures.get(0).map(|capture| capture.as_str()))
                .unwrap_or_default()
                .to_owned()
        })
        .into_owned()
}

/// Walks a JSON structure and replaces `{{ variables }}` in strings.
pub(crate) fn format_nested_variables(
    value: &JsonValue,
    replacements: &HashMap<Backreference, String>,
) -> JsonValue {
    match value {
        JsonValue::Object(object) => JsonValue::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), format_nested_variables(value, replacements)))
                .collect(),
        ),
        JsonValue::Array(values) => JsonValue::Array(
            values
                .iter()
                .map(|value| format_nested_variables(value, replacements))
                .collect(),
        ),
        JsonValue::String(value) => JsonValue::String(format_variables(value, replacements)),
        _ => value.clone(),
    }
}

fn var_to_json(var: Rc<Variable>) -> Result<JsonValue> {
    match &*var {
        Variable::Null => Ok(JsonValue::Null),
        Variable::Bool(value) => Ok(JsonValue::Bool(*value)),
        Variable::Number(value) => Ok(JsonValue::Number(value.clone())),
        Variable::String(value) => Ok(JsonValue::String(value.clone())),
        Variable::Array(values) => Ok(JsonValue::Array(
            values
                .iter()
                .map(|value| var_to_json(Rc::clone(value)))
                .collect::<Result<Vec<_>>>()?,
        )),
        Variable::Object(values) => Ok(JsonValue::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), var_to_json(Rc::clone(value))?)))
                .collect::<Result<Map<_, _>>>()?,
        )),
        Variable::Expref(_) => Err(anyhow!("JMESPath expression returned an expref")),
    }
}

fn compare_values(actual: &JsonValue, expected: &JsonValue, operation: &Operation) -> bool {
    match operation {
        Operation::Eq => actual == expected,
        Operation::Ne => actual != expected,
        Operation::Gt | Operation::Ge | Operation::Lt | Operation::Le => {
            let ordering = match (actual, expected) {
                (JsonValue::Number(actual), JsonValue::Number(expected)) => {
                    actual.as_f64().and_then(|actual| {
                        expected
                            .as_f64()
                            .and_then(|expected| actual.partial_cmp(&expected))
                    })
                }
                (JsonValue::String(actual), JsonValue::String(expected)) => {
                    Some(actual.cmp(expected))
                }
                _ => None,
            };

            matches!(
                (operation, ordering),
                (Operation::Gt, Some(Ordering::Greater))
                    | (Operation::Ge, Some(Ordering::Greater | Ordering::Equal))
                    | (Operation::Lt, Some(Ordering::Less))
                    | (Operation::Le, Some(Ordering::Less | Ordering::Equal))
            )
        }
    }
}

fn saved_value(value: JsonValue) -> String {
    match value {
        JsonValue::String(value) => value,
        JsonValue::Number(value) => value.to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Null => "null".to_owned(),
        value => value.to_string(),
    }
}

pub(crate) fn check_stdout(
    stage: &CheckStage,
    test_name: &str,
    output: &str,
    saved: &mut HashMap<Backreference, String>,
) -> Result<bool> {
    // Record the overall result of all matchers, but still execute them all to
    // produce useful logs and metrics.
    let mut combined_result = true;

    for matcher in &stage.matchers {
        let result = match matcher {
            StdoutMatcher::JmesPath {
                jmespath: expression,
                operation,
                value: expected,
            } => {
                let expression = jmespath::compile(expression)
                    .with_context(|| format!("invalid JMESPath expression: {expression}"))?;
                let data = Variable::from_json(output)
                    .map_err(|error| anyhow!(error))
                    .context("command output is not valid JSON")?;
                let actual = expression
                    .search(data)
                    .map_err(|error| anyhow!(error))
                    .with_context(|| format!("JMESPath evaluation failed: {expression}"))?;
                compare_values(&var_to_json(actual)?, expected, operation)
            }
            StdoutMatcher::Exact { exact: expected } => {
                let expected = format_variables(expected, saved);
                debug!(test_name, stage.name, expected = %expected, response = output);
                output.trim() == expected
            }
            StdoutMatcher::Regex { regex: expected } => {
                let expected = format_variables(expected, saved);
                let pattern = Regex::new(&expected)
                    .with_context(|| format!("invalid regular expression: {expected}"))?;
                debug!(test_name, stage.name, expected = %expected, response = output);

                if let Some(captures) = pattern.captures(output) {
                    for (index, value) in captures.iter().enumerate() {
                        if let Some(value) = value {
                            saved.insert(Backreference::Numbered(index), value.as_str().to_owned());
                        }
                    }
                    for name in pattern.capture_names().flatten() {
                        if let Some(value) = captures.name(name) {
                            saved.insert(
                                Backreference::Named(name.to_owned()),
                                value.as_str().to_owned(),
                            );
                        }
                    }
                    debug!(saved_values = ?saved);
                    true
                } else {
                    false
                }
            }
            StdoutMatcher::Json {
                json: expected,
                save: save_map,
            } => {
                let response: JsonValue = match serde_json::from_str(output) {
                    Ok(response) => response,
                    Err(_) => {
                        combined_result = false;
                        continue;
                    }
                };
                debug!(test_name, stage.name, expected = ?expected, response = %response);

                let matches = expected.as_ref().is_none_or(|expected| {
                    let expected = format_nested_variables(expected, saved);
                    json::compare(&expected, &response)
                });

                if let Some(expressions) = save_map {
                    for (key, expression) in expressions {
                        let expression = jmespath::compile(expression).with_context(|| {
                            format!("invalid JMESPath expression: {expression}")
                        })?;
                        let value = expression
                            .search(Variable::from_json(output).map_err(|error| anyhow!(error))?)
                            .map_err(|error| anyhow!(error))
                            .with_context(|| format!("JMESPath evaluation failed: {expression}"))?;
                        saved.insert(
                            Backreference::Named(key.clone()),
                            saved_value(var_to_json(value)?),
                        );
                    }
                }
                debug!(saved_values = ?saved);
                matches
            }
        };
        if !result {
            combined_result = false;
        }
    }
    Ok(combined_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_named_and_numbered_variables_without_replacement_expansion() {
        let replacements = HashMap::from([
            (Backreference::Named("name".to_owned()), "$value".to_owned()),
            (Backreference::Numbered(1), "one".to_owned()),
        ]);

        assert_eq!(
            format_variables("{{ name }} {{1}} {{ missing }}", &replacements),
            "$value one {{ missing }}"
        );
    }

    #[test]
    fn regex_without_a_match_is_a_failed_matcher() {
        let stage = CheckStage {
            name: "regex".to_owned(),
            max_retries: 1,
            delay_before: None,
            delay_after: None,
            check: crate::config::CheckCommand::Shell("true".to_owned()),
            matchers: vec![StdoutMatcher::Regex {
                regex: "missing".to_owned(),
            }],
        };

        assert!(!check_stdout(&stage, "test", "output", &mut HashMap::new()).unwrap());
    }

    #[test]
    fn jmespath_supports_ordering_operations() {
        let stage = CheckStage {
            name: "jmespath".to_owned(),
            max_retries: 1,
            delay_before: None,
            delay_after: None,
            check: crate::config::CheckCommand::Shell("true".to_owned()),
            matchers: vec![StdoutMatcher::JmesPath {
                jmespath: "value".to_owned(),
                operation: Operation::Gt,
                value: JsonValue::from(2),
            }],
        };

        assert!(check_stdout(&stage, "test", r#"{"value": 3}"#, &mut HashMap::new()).unwrap());
    }
}
