use odata::query::{FilterExpr, FilterOperand};
use serde_json::Value;
use std::cmp::Ordering;

pub fn resolve_property(record: &Value, path: &str) -> Value {
    path.split('/').fold(record.clone(), |acc, segment| {
        acc.get(segment).cloned().unwrap_or(Value::Null)
    })
}

#[allow(dead_code)]
fn evaluate_value_function(_record: &Value, name: &str, vals: &[Value]) -> Value {
    match name {
        "tolower" => vals
            .first()
            .and_then(|v| v.as_str())
            .map(|s| Value::String(s.to_lowercase()))
            .unwrap_or(Value::Null),
        "toupper" => vals
            .first()
            .and_then(|v| v.as_str())
            .map(|s| Value::String(s.to_uppercase()))
            .unwrap_or(Value::Null),
        "trim" => vals
            .first()
            .and_then(|v| v.as_str())
            .map(|s| Value::String(s.trim().to_string()))
            .unwrap_or(Value::Null),
        "length" => vals
            .first()
            .and_then(|v| v.as_str())
            .map(|s| Value::Number(s.chars().count().into()))
            .unwrap_or(Value::Null),
        "concat" => Value::String(
            vals.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(""),
        ),
        _ => Value::Null,
    }
}

#[allow(dead_code)]
fn evaluate_bool_function(record: &Value, name: &str, args: &[FilterOperand]) -> bool {
    let vals: Vec<Value> = args.iter().map(|a| evaluate_operand(record, a)).collect();
    let as_str = |v: &Value| v.as_str().unwrap_or("").to_string();
    match name {
        "contains" => vals
            .first()
            .map(as_str)
            .unwrap_or_default()
            .contains(&vals.get(1).map(as_str).unwrap_or_default()),
        "startswith" => vals
            .first()
            .map(as_str)
            .unwrap_or_default()
            .starts_with(&vals.get(1).map(as_str).unwrap_or_default()),
        "endswith" => vals
            .first()
            .map(as_str)
            .unwrap_or_default()
            .ends_with(&vals.get(1).map(as_str).unwrap_or_default()),
        // V2-Altlast: substringof(needle, haystack) - Argumentreihenfolge vertauscht ggue. contains
        "substringof" => vals
            .get(1)
            .map(as_str)
            .unwrap_or_default()
            .contains(&vals.first().map(as_str).unwrap_or_default()),
        _ => false,
    }
}

fn evaluate_operand(_record: &Value, _a: &FilterOperand) -> Value {
    todo!()
}

pub fn evaluate_filter(_record: &Value, _expr: &FilterExpr) -> bool {
    false
}

pub fn compare_json_for_sort(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => Ordering::Equal,
    }
}
