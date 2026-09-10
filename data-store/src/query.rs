//! Gemeinsame Query-Ausfuehrung fuer alle Backends, die ihre Datensaetze als
//! Dokumente halten (YAML, SQLite). Damit verhalten sich `$filter`, `$orderby`,
//! `$skip` und `$top` in jeder Implementierung identisch.

use std::cmp::Ordering;

use odata::query::{Expr, ODataQuery, SortDirection};
use serde_json::Value;

use crate::datastore::DataRecord;
use crate::filter_eval::evaluate;

/// Ordnungsrelation fuer JSON-Werte. `Null` sortiert vor allen anderen Werten,
/// unvergleichbare Typen gelten als gleich (stabile Sortierung bleibt erhalten).
pub fn compare_json(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// Wendet `$filter` an.
pub fn apply_filter(records: &mut Vec<DataRecord>, query: &ODataQuery) {
    if let Some(filter) = &query.filter {
        records.retain(|r| evaluate(r, filter) == Value::Bool(true));
    }
}

/// Wendet `$orderby` an - die Kriterien werden von hinten nach vorne
/// angewendet, damit das erste Kriterium dominiert.
pub fn apply_orderby(records: &mut [DataRecord], query: &ODataQuery) {
    for item in query.orderby.iter().rev() {
        records.sort_by(|a, b| {
            let expr = Expr::Identifier(item.property.clone());
            let ord = compare_json(&evaluate(a, &expr), &evaluate(b, &expr));
            if item.direction == SortDirection::Desc {
                ord.reverse()
            } else {
                ord
            }
        });
    }
}

/// Wendet `$skip` und `$top` an.
pub fn apply_paging(records: &mut Vec<DataRecord>, query: &ODataQuery) {
    if let Some(skip) = query.skip {
        records.drain(..(skip as usize).min(records.len()));
    }
    if let Some(top) = query.top {
        records.truncate(top as usize);
    }
}

/// `$filter`, `$orderby`, `$skip`, `$top` in dieser Reihenfolge.
pub fn apply_query(records: &mut Vec<DataRecord>, query: &ODataQuery) {
    apply_filter(records, query);
    apply_orderby(records, query);
    apply_paging(records, query);
}

/// Query fuer eine reine Zaehlung: Seitenaufteilung und Projektionen entfallen.
pub fn count_query(query: &ODataQuery) -> ODataQuery {
    let mut q = query.clone();
    q.top = None;
    q.skip = None;
    q.expand.clear();
    q.select = vec![];
    q
}
