use super::*;
use data_store::{json_to_param, DataType};
use serde_json::json;

/// Der Wahrheitswert kommt als `bool` an der Treibergrenze an - nicht als
/// 0/1 wie bei SQLite.
#[test]
fn booleans_stay_booleans() {
    let param = json_to_param(&json!(true), &DataType::Bool, "IsActiveEntity").unwrap();
    assert_eq!(param, SqlParam::Bool(true));
    assert!(<PgParam as ToSql>::accepts(&Type::BOOL));
}
