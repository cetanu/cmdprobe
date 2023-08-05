use serde_json::Value;

pub fn compare(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            if a.len() > b.len() {
                return false;
            }
            a.iter()
                .zip(b.iter())
                .all(|(a_elem, b_elem)| compare(a_elem, b_elem))
        }
        (Value::Object(a), Value::Object(b)) => {
            if a.len() > b.len() {
                return false;
            }

            // Check that all fields in A exist in B and their values are equal
            a.iter().all(|(a_key, a_value)| {
                b.get(a_key)
                    .map(|b_value| compare(a_value, b_value))
                    .unwrap_or(false)
            })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_json_value_can_be_compared() {
        let a = serde_json::json!({"hello": "world"});
        let b = serde_json::json!({"hello": "world", "foo": "bar"});
        assert!(compare(&a, &b))
    }
}
