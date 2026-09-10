//! Auswertung von `$filter`-Ausdruecken auf einem [`DataRecord`].
//!
//! Referenzsemantik fuer alle Backends: Dokumenten-Stores werten hiermit aus,
//! der relationale Store uebersetzt nach SQL und muss zum gleichen Ergebnis
//! kommen (siehe `data_store::conformance`).

use std::cmp::Ordering;

use odata::query::{BinOp, Expr};
use serde_json::{Number, Value};
use tracing::trace;

use crate::datastore::DataRecord;

pub fn resolve_property(record: &DataRecord, path: &str) -> Value {
    resolve_path(record, path.split('/'))
}

fn resolve_path<'a>(record: &DataRecord, mut parts: impl Iterator<Item = &'a str>) -> Value {
    let Some(head) = parts.next() else {
        return Value::Null;
    };
    let mut current = record.get(head).unwrap_or(Value::Null);
    for part in parts {
        match current.get(part) {
            Some(v) => current = v.clone(),
            None => return Value::Null,
        }
    }
    current
}

fn as_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn number(n: f64) -> Value {
    Number::from_f64(n).map(Value::Number).unwrap_or(Value::Null)
}

/// Ordnung fuer Vergleichsoperatoren. `None` = nicht vergleichbar; der
/// Vergleich liefert dann `Null` (OData-Semantik fuer unbekannt).
fn order(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64().partial_cmp(&y.as_f64()),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

fn compare(op: &BinOp, left: &Value, right: &Value) -> Value {
    match order(left, right) {
        Some(ord) => Value::Bool(match op {
            BinOp::Gt => ord == Ordering::Greater,
            BinOp::Ge => ord != Ordering::Less,
            BinOp::Lt => ord == Ordering::Less,
            BinOp::Le => ord != Ordering::Greater,
            _ => unreachable!("nur Vergleichsoperatoren"),
        }),
        None => Value::Null,
    }
}

fn call_function(name: &str, args: &[Value]) -> Value {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
    let text = |i: usize| as_str(&arg(i));

    match name.to_ascii_lowercase().as_str() {
        // -- Praedikate --
        "contains" => Value::Bool(text(0).contains(&text(1))),
        "startswith" => Value::Bool(text(0).starts_with(&text(1))),
        "endswith" => Value::Bool(text(0).ends_with(&text(1))),
        // OData V2: substringof(needle, haystack) - Argumente vertauscht ggue. contains
        "substringof" => Value::Bool(text(1).contains(&text(0))),

        // -- Stringfunktionen --
        "tolower" => Value::String(text(0).to_lowercase()),
        "toupper" => Value::String(text(0).to_uppercase()),
        "trim" => Value::String(text(0).trim().to_string()),
        "length" => number(text(0).chars().count() as f64),
        "concat" => Value::String(args.iter().map(as_str).collect::<Vec<_>>().join("")),
        "indexof" => number(
            text(0)
                .find(&text(1))
                .map(|i| text(0)[..i].chars().count() as f64)
                .unwrap_or(-1.0),
        ),
        "substring" => {
            let s: Vec<char> = text(0).chars().collect();
            let start = arg(1).as_f64().unwrap_or(0.0).max(0.0) as usize;
            let rest = s.len().saturating_sub(start);
            let len = arg(2).as_f64().map(|l| l.max(0.0) as usize).unwrap_or(rest);
            Value::String(s.into_iter().skip(start).take(len).collect())
        }

        // -- Arithmetik auf Zahlen --
        "round" => arg(0).as_f64().map(|v| number(v.round())).unwrap_or(Value::Null),
        "floor" => arg(0).as_f64().map(|v| number(v.floor())).unwrap_or(Value::Null),
        "ceiling" => arg(0).as_f64().map(|v| number(v.ceil())).unwrap_or(Value::Null),

        _ => {
            trace!("unbekannte Filterfunktion '{name}'");
            Value::Null
        }
    }
}

pub fn evaluate(record: &DataRecord, expr: &Expr) -> Value {
    let v = match expr {
        Expr::Identifier(id) => resolve_path(record, id.split('/')),
        Expr::Path(items) => resolve_path(record, items.iter().map(|s| s.as_str())),
        Expr::String(s) => Value::String(s.clone()),
        Expr::Number(n) => number(*n),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Guid(g) => Value::String(g.to_string()),
        Expr::Null => Value::Null,
        Expr::FunctionCall { name, args } => {
            let args: Vec<Value> = args.iter().map(|a| evaluate(record, a)).collect();
            call_function(name, &args)
        }
        Expr::UnaryNot(inner) => match evaluate(record, inner) {
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Null,
        },
        Expr::BinaryOp { op, left, right } => {
            let left = evaluate(record, left);
            let right = evaluate(record, right);
            match op {
                BinOp::And => match (&left, &right) {
                    (Value::Bool(a), Value::Bool(b)) => Value::Bool(*a && *b),
                    // Kurzschluss: false AND unbekannt ist false
                    (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
                    _ => Value::Null,
                },
                BinOp::Or => match (&left, &right) {
                    (Value::Bool(a), Value::Bool(b)) => Value::Bool(*a || *b),
                    (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
                    _ => Value::Null,
                },
                BinOp::Eq => Value::Bool(left == right),
                BinOp::Ne => Value::Bool(left != right),
                op => compare(op, &left, &right),
            }
        }
    };
    trace!("Evaluate: {:?} -> {}", expr, v);
    v
}

pub fn compare_json_for_sort(a: &Value, b: &Value) -> Ordering {
    order(a, b).unwrap_or(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record() -> DataRecord {
        DataRecord::from_json(&json!({
            "ID": "00000000-0000-0000-0000-000000000001",
            "IsActiveEntity": true,
            "Amount": 150,
            "Status": "Released",
        }))
        .unwrap()
    }

    fn ident(name: &str) -> Expr {
        Expr::Identifier(name.into())
    }

    fn bin(op: BinOp, left: Expr, right: Expr) -> Expr {
        Expr::BinaryOp {
            op,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn comparison_operators_work() {
        let r = record();
        for (op, expected) in [
            (BinOp::Gt, true),
            (BinOp::Ge, true),
            (BinOp::Lt, false),
            (BinOp::Le, false),
        ] {
            let e = bin(op, ident("Amount"), Expr::Number(100.0));
            assert_eq!(evaluate(&r, &e), json!(expected), "{op:?}");
        }
    }

    #[test]
    fn comparison_on_incompatible_types_is_null() {
        let e = bin(BinOp::Gt, ident("Status"), Expr::Number(100.0));
        assert_eq!(evaluate(&record(), &e), Value::Null);
    }

    #[test]
    fn string_functions() {
        let r = record();
        let call = |name: &str, args: Vec<Expr>| {
            evaluate(
                &r,
                &Expr::FunctionCall {
                    name: name.into(),
                    args,
                },
            )
        };
        assert_eq!(
            call("contains", vec![ident("Status"), Expr::String("lea".into())]),
            json!(true)
        );
        assert_eq!(
            call("startswith", vec![ident("Status"), Expr::String("Rel".into())]),
            json!(true)
        );
        assert_eq!(
            call("endswith", vec![ident("Status"), Expr::String("sed".into())]),
            json!(true)
        );
        assert_eq!(call("tolower", vec![ident("Status")]), json!("released"));
        assert_eq!(call("length", vec![ident("Status")]), json!(8.0));
        assert_eq!(
            call("substringof", vec![Expr::String("lea".into()), ident("Status")]),
            json!(true)
        );
    }

    #[test]
    fn and_or_short_circuit_on_unknown() {
        let r = record();
        let unknown = bin(BinOp::Gt, ident("Status"), Expr::Number(1.0));
        let f = bin(BinOp::And, Expr::Bool(false), unknown.clone());
        assert_eq!(evaluate(&r, &f), json!(false));
        let t = bin(BinOp::Or, Expr::Bool(true), unknown);
        assert_eq!(evaluate(&r, &t), json!(true));
    }

    #[test]
    fn multi_segment_paths_resolve_without_panicking() {
        let mut r = record();
        r["Customer"] = json!({ "Name": "Mueller" });
        assert_eq!(
            evaluate(&r, &ident("Customer/Name")),
            json!("Mueller")
        );
        assert_eq!(evaluate(&r, &ident("Customer/Nope")), Value::Null);
    }

    #[test]
    fn unknown_function_is_null_not_panic() {
        let e = Expr::FunctionCall {
            name: "geo.distance".into(),
            args: vec![],
        };
        assert_eq!(evaluate(&record(), &e), Value::Null);
    }
}
