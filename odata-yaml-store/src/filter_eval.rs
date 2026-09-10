use odata::query::{Expr, FilterOperand};
use serde_json::{Number, Value};
use std::cmp::Ordering;
use tracing::trace;

use data_store::DataRecord;

pub fn resolve_property(record: &DataRecord, path: &str) -> Value {
    let mut parts = path.split('/');
    parts
        .nth(0)
        .and_then(|segment| record.get(segment))
        .unwrap_or(Value::Null)
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

pub fn evaluate(record: &DataRecord, expr: &Expr) -> Value {
    let v = match expr {
        Expr::Identifier(id) => {
            let parts = id.split("/").collect::<Vec<_>>();
            match parts.as_slice() {
                [single] => record.get(*single).unwrap_or(Value::Null),
                _ => todo!("Path expressions with multiple segments are not yet implemented"),
            }
        }
        Expr::Path(items) => {
            if let Some(mut v0) = record.get(items[0].as_str()) {
                for i in items.iter().skip(1) {
                    if let Some(v) = v0.get(i) {
                        v0 = v.clone();
                    } else {
                        return Value::Null;
                    }
                }
                v0
            } else {
                Value::Null
            }
        }
        Expr::String(s) => Value::String(s.clone()),
        Expr::Number(n) => Value::Number(Number::from_f64(*n).unwrap()),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Guid(g) => Value::String(g.to_string()),
        Expr::Null => Value::Null,
        Expr::FunctionCall {
            name: _name,
            args: _args,
        } => todo!(),
        Expr::UnaryNot(expr) => {
            let v = evaluate(record, expr);
            if let Value::Bool(b) = v {
                return Value::Bool(!b);
            } else {
                return Value::Null;
            }
        }
        Expr::BinaryOp { op, left, right } => {
            let left = evaluate(record, left);
            let right = evaluate(record, right);
            match op {
                odata::query::BinOp::And => {
                    if let (Value::Bool(left), Value::Bool(right)) = (left, right) {
                        Value::Bool(left && right)
                    } else {
                        Value::Null
                    }
                }

                odata::query::BinOp::Or => {
                    if let (Value::Bool(left), Value::Bool(right)) = (left, right) {
                        Value::Bool(left || right)
                    } else {
                        Value::Null
                    }
                }
                odata::query::BinOp::Eq => Value::Bool(left == right),
                odata::query::BinOp::Ne => Value::Bool(left != right),
                odata::query::BinOp::Gt => todo!(),
                odata::query::BinOp::Ge => todo!(),
                odata::query::BinOp::Lt => todo!(),
                odata::query::BinOp::Le => todo!(),
            }
        }
    };
    trace!("Evaluate: {:?} -> {}", expr, v);
    v
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
