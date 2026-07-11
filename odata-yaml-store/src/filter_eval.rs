use crate::query::{CompareOp, FilterExpr, FilterOperand};
use serde_json::Value;
use std::cmp::Ordering;

pub fn resolve_property(record: &Value, path: &str) -> Value {
    path.split('/').fold(record.clone(), |acc, segment| {
        acc.get(segment).cloned().unwrap_or(Value::Null)
    })
}

fn evaluate_operand(record: &Value, operand: &FilterOperand) -> Value {
    match operand {
        FilterOperand::Property(path) => resolve_property(record, path),
        FilterOperand::Literal(v) => v.clone(),
        FilterOperand::Function { name, args } => evaluate_value_function(record, name, args),
    }
}

fn evaluate_value_function(record: &Value, name: &str, args: &[FilterOperand]) -> Value {
    let vals: Vec<Value> = args.iter().map(|a| evaluate_operand(record, a)).collect();
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

fn compare_values(left: &Value, right: &Value, op: CompareOp) -> bool {
    let ordering = match (left, right) {
        (Value::Number(a), Value::Number(b)) => a.as_f64().partial_cmp(&b.as_f64()),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        (Value::Null, Value::Null) => Some(Ordering::Equal),
        _ => None,
    };

    match (op, ordering) {
        (CompareOp::Eq, Some(o)) => o == Ordering::Equal,
        (CompareOp::Eq, None) => left == right,
        (CompareOp::Ne, Some(o)) => o != Ordering::Equal,
        (CompareOp::Ne, None) => left != right,
        (CompareOp::Gt, Some(o)) => o == Ordering::Greater,
        (CompareOp::Ge, Some(o)) => o != Ordering::Less,
        (CompareOp::Lt, Some(o)) => o == Ordering::Less,
        (CompareOp::Le, Some(o)) => o != Ordering::Greater,
        _ => false,
    }
}

pub fn evaluate_filter(record: &Value, expr: &FilterExpr) -> bool {
    match expr {
        FilterExpr::Compare { left, op, right } => compare_values(
            &evaluate_operand(record, left),
            &evaluate_operand(record, right),
            *op,
        ),
        FilterExpr::And(a, b) => evaluate_filter(record, a) && evaluate_filter(record, b),
        FilterExpr::Or(a, b) => evaluate_filter(record, a) || evaluate_filter(record, b),
        FilterExpr::Not(e) => !evaluate_filter(record, e),
        FilterExpr::Function { name, args } => evaluate_bool_function(record, name, args),
        FilterExpr::Any {
            collection_property,
            predicate,
            ..
        } => match resolve_property(record, collection_property) {
            Value::Array(items) => items.iter().any(|item| evaluate_filter(item, predicate)),
            _ => false,
        },
        FilterExpr::All {
            collection_property,
            predicate,
            ..
        } => match resolve_property(record, collection_property) {
            Value::Array(items) => items.iter().all(|item| evaluate_filter(item, predicate)),
            _ => false,
        },
    }
}

pub fn compare_json_for_sort(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x.as_f64().partial_cmp(&y.as_f64()).unwrap_or(Ordering::Equal)
        }
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        _ => Ordering::Equal,
    }
}
