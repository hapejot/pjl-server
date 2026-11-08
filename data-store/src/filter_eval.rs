//! Auswertung von `$filter`-Ausdruecken auf einem [`DataRecord`].
//!
//! Referenzsemantik fuer alle Backends: Dokumenten-Stores werten hiermit aus,
//! der relationale Store uebersetzt nach SQL und muss zum gleichen Ergebnis
//! kommen (siehe `data_store::conformance`).

use std::cmp::Ordering;

use odata::query::{BinOp, Expr, LambdaKind};
use serde_json::{Number, Value};
use tracing::trace;

use crate::datastore::DataRecord;

pub fn resolve_property(record: &DataRecord, path: &str) -> Value {
    resolve_in(record, &[], &path.split('/').collect::<Vec<_>>())
}

/// Lambda-Variablen mit ihrem aktuellen Element, innerste zuletzt.
type Scope = [(String, Value)];

/// Loest den Pfad auf; ein Kopf, der einer Lambda-Variable entspricht, bezeichnet das Element.
fn resolve_in(record: &DataRecord, scope: &Scope, parts: &[&str]) -> Value {
    let Some((head, rest)) = parts.split_first() else {
        return Value::Null;
    };
    let start = match scope.iter().rev().find(|(var, _)| var.as_str() == *head) {
        Some((_, item)) => item.clone(),
        None => record.get(head).unwrap_or(Value::Null),
    };
    rest.iter()
        .try_fold(start, |current, part| current.get(*part).cloned())
        .unwrap_or(Value::Null)
}

/// Unbekannt, wenn kein Element entscheidet und mindestens eines unbekannt ist.
fn quantify(kind: LambdaKind, results: impl Iterator<Item = Value>) -> Value {
    let mut unknown = false;
    for result in results {
        match (kind, result) {
            (LambdaKind::Any, Value::Bool(true)) => return Value::Bool(true),
            (LambdaKind::All, Value::Bool(false)) => return Value::Bool(false),
            (_, Value::Bool(_)) => {}
            _ => unknown = true,
        }
    }
    match (unknown, kind) {
        (true, _) => Value::Null,
        (false, LambdaKind::Any) => Value::Bool(false),
        (false, LambdaKind::All) => Value::Bool(true),
    }
}

fn as_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn number(n: f64) -> Value {
    Number::from_f64(n)
        .map(Value::Number)
        .unwrap_or(Value::Null)
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
        "round" => arg(0)
            .as_f64()
            .map(|v| number(v.round()))
            .unwrap_or(Value::Null),
        "floor" => arg(0)
            .as_f64()
            .map(|v| number(v.floor()))
            .unwrap_or(Value::Null),
        "ceiling" => arg(0)
            .as_f64()
            .map(|v| number(v.ceil()))
            .unwrap_or(Value::Null),

        _ => {
            trace!("unbekannte Filterfunktion '{name}'");
            Value::Null
        }
    }
}

pub fn evaluate(record: &DataRecord, expr: &Expr) -> Value {
    eval(record, &[], expr)
}

fn eval(record: &DataRecord, scope: &Scope, expr: &Expr) -> Value {
    let v = match expr {
        Expr::Identifier(id) => resolve_in(record, scope, &id.split('/').collect::<Vec<_>>()),
        Expr::Path(items) => resolve_in(
            record,
            scope,
            &items.iter().map(String::as_str).collect::<Vec<_>>(),
        ),
        Expr::String(s) => Value::String(s.clone()),
        Expr::Number(n) => number(*n),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Guid(g) | Expr::DateTime(g) => Value::String(g.to_string()),
        Expr::Null => Value::Null,
        Expr::FunctionCall { name, args } => {
            let args: Vec<Value> = args.iter().map(|a| eval(record, scope, a)).collect();
            call_function(name, &args)
        }
        Expr::UnaryNot(inner) => match eval(record, scope, inner) {
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Null,
        },
        Expr::Lambda {
            kind,
            collection,
            var,
            predicate,
        } => {
            let parts: Vec<&str> = collection.iter().map(String::as_str).collect();
            match resolve_in(record, scope, &parts) {
                Value::Array(items) => quantify(
                    *kind,
                    items.into_iter().map(|item| {
                        let mut inner = scope.to_vec();
                        inner.push((var.clone(), item));
                        eval(record, &inner, predicate)
                    }),
                ),
                _ => Value::Null,
            }
        }
        Expr::BinaryOp { op, left, right } => {
            let left = eval(record, scope, left);
            let right = eval(record, scope, right);
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
mod tests;
