use super::*;

#[test]
fn default_user_id_is_stable() {
    // Festwert: aendert sich nur, wenn Namespace oder Username geaendert werden.
    assert_eq!(
        default_user_id().to_string(),
        Uuid::new_v5(
            &Uuid::new_v5(&Uuid::NAMESPACE_DNS, b"SystemUser"),
            b"none@localhost"
        )
        .to_string()
    );
    assert_eq!(default_user_id(), default_user_id());
}

#[test]
fn fk_column_suffix() {
    assert_eq!(fk_column("Owner"), "Owner_ID");
}
