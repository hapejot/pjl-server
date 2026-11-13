use super::*;
use serde_json::json;

fn document(value: Value) -> (Value, Vec<String>) {
    let generated = generate(&value, "Order", "sales").unwrap();
    (
        serde_json::to_value(generated.ddic).unwrap(),
        generated.warnings,
    )
}

fn atom<'a>(doc: &'a Value, entity: &str, field: &str) -> &'a Value {
    doc["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == entity)
        .unwrap()["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|f| f.get("Atom").filter(|a| a["name"] == field))
        .unwrap()
}

#[test]
fn merges_all_records_and_widens_numbers_without_sample_limits() {
    let (doc, warnings) = document(json!([
        {"quantity": 1, "price": null, "large": 1, "optional": true},
        {"quantity": 2.5, "price": 12.99, "large": 2147483648_i64}
    ]));
    assert!(warnings.is_empty());
    assert_eq!(atom(&doc, "Order", "quantity")["edm_type"], "Edm.Decimal");
    assert_eq!(atom(&doc, "Order", "price")["edm_type"], "Edm.Decimal");
    assert_eq!(atom(&doc, "Order", "large")["edm_type"], "Edm.Int64");
    assert_eq!(atom(&doc, "Order", "optional")["edm_type"], "Edm.Boolean");
}

#[test]
fn decimal_dimensions_cover_fractional_large_and_exponential_samples() {
    let (doc, _) = document(json!([
        {"number": 0.123456789, "large": u64::MAX, "tiny": 1.25e-20},
        {"number": 1234567890123456789_i64, "large": 1.0, "tiny": 2.0}
    ]));
    let number = atom(&doc, "Order", "number");
    assert_eq!(number["scale"], 9);
    assert!(number["precision"].as_u64().unwrap() >= 28);
    let large = atom(&doc, "Order", "large");
    assert!(large["precision"].as_u64().unwrap() - large["scale"].as_u64().unwrap() >= 20);
    assert_eq!(atom(&doc, "Order", "tiny")["scale"], 22);
}

#[test]
fn builds_object_references_and_owned_collections() {
    let (doc, warnings) = document(json!({
        "customer": {"name": "Alice"},
        "items": [{"sku": "A"}, {"quantity": 2}],
        "tags": ["new", "urgent"],
        "matrix": [[1, 2], [3]]
    }));
    assert!(warnings.is_empty());
    assert_eq!(
        atom(&doc, "Order_items", "quantity")["edm_type"],
        "Edm.Int32"
    );
    assert_eq!(atom(&doc, "Order_tags", "value")["edm_type"], "Edm.String");
    assert_eq!(
        atom(&doc, "Order_matrix_value", "value")["edm_type"],
        "Edm.Int32"
    );
    let relations = doc["relationships"].as_array().unwrap();
    assert!(relations.iter().any(|r| r["one"] == "Order_customer"
        && r["many"] == "Order"
        && r["many_property"] == "customer"
        && r.get("owned").is_none()));
    assert!(relations.iter().any(|r| r["one"] == "Order"
        && r["one_field"] == "items"
        && r["many"] == "Order_items"
        && r["owned"] == true));
    assert_eq!(atom(&doc, "Order_items", "_index")["edm_type"], "Edm.Int64");
}

#[test]
fn warns_and_uses_json_text_for_unknown_or_incompatible_shapes() {
    let (doc, warnings) = document(json!({
        "unknown": null, "empty": [], "mixed": [1, {"name": "A"}],
        "records": [{"value": true}, {"value": "yes"}]
    }));
    assert_eq!(warnings.len(), 4);
    for field in ["unknown", "empty", "mixed"] {
        assert_eq!(atom(&doc, "Order", field)["edm_type"], "Edm.String");
    }
    assert_eq!(
        atom(&doc, "Order_records", "value")["edm_type"],
        "Edm.String"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("$[\"records\"][][\"value\"]"))
    );
}

#[test]
fn avoids_source_system_navigation_and_entity_name_collisions() {
    let (doc, warnings) = document(json!({
        "ID": 42, "a-b": 1, "a_b": 2, "": false,
        "customer": {"_parents": 3},
        "customer_ID": "external",
        "items": [{"_parent": "source", "_index": 7}],
        "a": {"b": {"name": "nested"}},
        "a_b_name": {"name": "separate"}
    }));
    assert!(warnings.len() >= 5);
    assert_eq!(atom(&doc, "Order", "ID_2")["edm_type"], "Edm.Int32");
    assert_eq!(atom(&doc, "Order", "a_b_2")["edm_type"], "Edm.Int32");
    assert_eq!(atom(&doc, "Order", "customer_ID")["edm_type"], "Edm.String");
    assert!(
        doc["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["many_property"] == "customer_ref")
    );
    let generated = generate(&json!({"a": {"b": {}}, "a_b": {}}), "Order", "sales").unwrap();
    let entities = generated.ddic.entities();
    let names: HashSet<_> = entities.iter().map(|e| e.name()).collect();
    assert_eq!(names.len(), entities.len());
}

#[test]
fn accepts_scalar_roots_and_rejects_invalid_explicit_names() {
    let (doc, _) = document(json!(true));
    assert_eq!(atom(&doc, "Order", "value")["edm_type"], "Edm.Boolean");
    assert!(generate(&json!({}), "bad-name", "sales").is_err());
    assert!(generate(&json!({}), "Order", "").is_err());
}
