use super::*;

#[test]
fn created_by_me_compares_with_the_request_user() {
    assert!(!draft_created_by_current_user("someone@example.org"));
    assert!(draft_created_by_current_user(FALLBACK_PRINCIPAL));
}

fn record() -> DataRecord {
    DataRecord::from_json(&json!({"name": "x"})).unwrap()
}

#[tokio::test]
async fn draft_is_owned_by_the_request_user() {
    CURRENT_PRINCIPAL
        .scope(Some("peter@example.org".into()), async {
            let mut r = record();
            r.edit();
            let admin = r.draft_admin_data().unwrap();
            assert_eq!(admin.in_process_by_user, "peter@example.org");
            assert_eq!(admin.last_changed_by_user, "peter@example.org");
        })
        .await;
}

#[tokio::test]
async fn draft_without_request_user_falls_back() {
    let mut r = record();
    r.edit();
    assert_eq!(
        r.draft_admin_data().unwrap().in_process_by_user,
        FALLBACK_PRINCIPAL
    );
}
