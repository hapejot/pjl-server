use super::*;

#[test]
fn create_sets_owner_and_created_fields_and_drops_client_changes() {
    let actor = Uuid::new_v4();
    let mut v = json!({"name": "x", "ChangedBy_ID": "evil", "ChangedAt": "x"});
    audit_create(&mut v, actor);
    assert_eq!(v["Owner_ID"], json!(actor.to_string()));
    assert_eq!(v["CreatedBy_ID"], json!(actor.to_string()));
    assert!(v.get("CreatedAt").is_some());
    assert!(v.get("ChangedBy_ID").is_none());
    assert!(v.get("ChangedAt").is_none());
}

#[test]
fn create_keeps_explicit_owner() {
    let actor = Uuid::new_v4();
    let other = Uuid::new_v4();
    let mut v = json!({"Owner_ID": other.to_string()});
    audit_create(&mut v, actor);
    assert_eq!(v["Owner_ID"], json!(other.to_string()));
    assert_eq!(v["CreatedBy_ID"], json!(actor.to_string()));
}

#[test]
fn patch_sets_changed_and_protects_created() {
    let actor = Uuid::new_v4();
    let mut v = json!({"name": "y", "CreatedBy_ID": "evil", "CreatedAt": "x"});
    audit_patch(&mut v, actor);
    assert_eq!(v["ChangedBy_ID"], json!(actor.to_string()));
    assert!(v.get("ChangedAt").is_some());
    assert!(v.get("CreatedBy_ID").is_none());
    assert!(v.get("CreatedAt").is_none());
    assert_eq!(v["name"], json!("y"));
}

#[tokio::test]
async fn current_user_defaults_outside_request() {
    assert_eq!(current_user(), None);
    let seen = CURRENT_USER
        .scope(Some("peter".into()), async { current_user() })
        .await;
    assert_eq!(seen.as_deref(), Some("peter"));
}
