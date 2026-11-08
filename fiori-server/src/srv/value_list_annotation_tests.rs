use super::*;

/// Konstante Werte muessen als `String="..."` ausgegeben werden; `Constant="..."`
/// ist kein gueltiges CSDL und laesst die Oberflaeche die Wertehilfe verwerfen.
#[test]
fn list_id_constant_is_emitted_as_string() {
    let vl = AtomValueList::FieldValueList {
        list_id: "abc".into(),
        prefer_dialog: false,
    };
    let json = serde_json::to_string(&annotations_for_atom_value_list("status", &vl)).unwrap();
    assert!(!json.contains("@Constant"), "{json}");
    assert!(json.contains("\"@String\":\"abc\""), "{json}");
}
