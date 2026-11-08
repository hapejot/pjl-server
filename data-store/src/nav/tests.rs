use super::*;

fn nav(single: bool, owning: bool) -> Navigation {
    Navigation {
        single,
        owning,
        from_set: Some("Orders".into()),
        from_field: Some("_Items".into()),
        to_set: Some("OrderItems".into()),
        to_field: Some("Order_ID".into()),
    }
}

fn draft_admin_nav() -> Navigation {
    Navigation {
        single: true,
        owning: true,
        from_set: Some("Orders".into()),
        from_field: Some(DRAFT_ADMIN_SET.into()),
        to_set: Some(DRAFT_ADMIN_SET.into()),
        to_field: Some("ID".into()),
    }
}

#[test]
fn resolves_forward_direction() {
    let links = nav_links(&[nav(false, true)], "Orders");
    let link = links.get("_Items").expect("_Items");
    assert!(!link.single);
    assert_eq!(link.to_set, "OrderItems");
    assert!(link.is_composition_child());
}

#[test]
fn reverse_direction_flips_cardinality() {
    let links = nav_links(&[nav(false, true)], "OrderItems");
    let link = links.get("Order_ID").expect("Order_ID");
    assert!(link.single, "n:1 aus Sicht des Kindes");
    assert_eq!(link.to_set, "Orders");
    assert!(!link.is_composition_child(), "Parent ist kein Kind-Set");
    assert!(link.is_composition_parent());
}

#[test]
fn draft_admin_is_neither_child_nor_parent() {
    let navigations = vec![draft_admin_nav()];
    let links = nav_links(&navigations, "Orders");
    assert!(!links[DRAFT_ADMIN_SET].is_composition_child());
    assert!(!links[DRAFT_ADMIN_SET].is_composition_parent());
}

/// Regression: der Draft-Admin-Link ist single+owning und darf beim Suchen
/// des Parents nicht als solcher durchgehen.
#[test]
fn composition_parent_ignores_draft_admin() {
    let links = nav_links(&[nav(false, true), draft_admin_nav()], "Orders");
    assert_eq!(composition_parent(&links), None);

    let child_links = nav_links(&[nav(false, true), draft_admin_nav()], "OrderItems");
    let (field, link) = composition_parent(&child_links).expect("parent link");
    assert_eq!(field, "Order_ID");
    assert_eq!(link.to_set, "Orders");
}

#[test]
fn incomplete_navigations_are_ignored() {
    let incomplete = Navigation {
        single: true,
        owning: false,
        from_set: Some("Orders".into()),
        from_field: None,
        to_set: Some("Customers".into()),
        to_field: Some("ID".into()),
    };
    assert!(nav_links(&[incomplete], "Orders").is_empty());
}
