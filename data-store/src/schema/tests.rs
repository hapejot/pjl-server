use super::*;

fn orders_schema() -> Result<Schema, SchemaError> {
    Schema::builder()
        .set(
            EntitySet::new("OrderSet", "Order")
                .drafts()
                .column(Column::new(
                    "Status",
                    DataType::String {
                        max_length: Some(20),
                    },
                ))
                .column(Column::foreign_key("Customer_ID", "CustomerSet"))
                .association(Association {
                    name: "_Items".into(),
                    target_set: "ItemSet".into(),
                    cardinality: Cardinality::ToMany,
                    ownership: Ownership::Composition,
                    foreign_key: ForeignKeySide::There {
                        column: "Order_ID".into(),
                    },
                    reverse: Some("Order".into()),
                }),
        )
        .set(
            EntitySet::new("ItemSet", "Item")
                .drafts()
                .column(Column::foreign_key("Order_ID", "OrderSet"))
                .association(Association {
                    name: "Order".into(),
                    target_set: "OrderSet".into(),
                    cardinality: Cardinality::ToOne,
                    ownership: Ownership::Composition,
                    foreign_key: ForeignKeySide::Here {
                        column: "Order_ID".into(),
                    },
                    reverse: Some("_Items".into()),
                }),
        )
        .set(EntitySet::new("CustomerSet", "Customer").drafts())
        .build()
}

#[test]
fn drafts_add_the_activation_key() {
    let schema = orders_schema().unwrap();
    let keys: Vec<_> = schema
        .set("OrderSet")
        .unwrap()
        .key_columns()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(keys, vec![ID_COLUMN, IS_ACTIVE_COLUMN]);
}

#[test]
fn creation_order_puts_parents_first() {
    let schema = orders_schema().unwrap();
    let order: Vec<_> = schema
        .creation_order()
        .unwrap()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let parent = order.iter().position(|n| n == "OrderSet").unwrap();
    let child = order.iter().position(|n| n == "ItemSet").unwrap();
    assert!(parent < child, "{order:?}");
}

#[test]
fn composition_helpers_agree_on_direction() {
    let schema = orders_schema().unwrap();
    let orders = schema.set("OrderSet").unwrap();
    let items = schema.set("ItemSet").unwrap();
    assert_eq!(orders.composition_children().count(), 1);
    assert!(orders.composition_parent().is_none());
    assert_eq!(items.composition_children().count(), 0);
    assert_eq!(items.composition_parent().unwrap().target_set, "OrderSet");
}

#[test]
fn unknown_set_is_an_error_not_none() {
    let schema = orders_schema().unwrap();
    assert!(matches!(
        schema.set("Nope"),
        Err(SchemaError::UnknownSet(_))
    ));
}

#[test]
fn dangling_association_target_is_rejected() {
    let result = Schema::builder()
        .set(
            EntitySet::new("OrderSet", "Order").association(Association {
                name: "_Items".into(),
                target_set: "Missing".into(),
                cardinality: Cardinality::ToMany,
                ownership: Ownership::Composition,
                foreign_key: ForeignKeySide::There {
                    column: "Order_ID".into(),
                },
                reverse: None,
            }),
        )
        .build();
    assert!(matches!(result, Err(SchemaError::DanglingTarget { .. })));
}

#[test]
fn missing_foreign_key_column_is_rejected() {
    let result = Schema::builder()
        .set(
            EntitySet::new("OrderSet", "Order").association(Association {
                name: "_Items".into(),
                target_set: "ItemSet".into(),
                cardinality: Cardinality::ToMany,
                ownership: Ownership::Composition,
                foreign_key: ForeignKeySide::There {
                    column: "Order_ID".into(),
                },
                reverse: None,
            }),
        )
        .set(EntitySet::new("ItemSet", "Item")) // ohne Order_ID
        .build();
    assert!(matches!(result, Err(SchemaError::MissingForeignKey { .. })));
}

#[test]
fn composition_cycles_are_rejected() {
    let assoc = |target: &str, name: &str| Association {
        name: name.into(),
        target_set: target.into(),
        cardinality: Cardinality::ToOne,
        ownership: Ownership::Composition,
        foreign_key: ForeignKeySide::Here {
            column: format!("{name}_ID"),
        },
        reverse: None,
    };
    let result = Schema::builder()
        .set(
            EntitySet::new("A", "A")
                .column(Column::foreign_key("b_ID", "B"))
                .association(assoc("B", "b")),
        )
        .set(
            EntitySet::new("B", "B")
                .column(Column::foreign_key("a_ID", "A"))
                .association(assoc("A", "a")),
        )
        .build();
    assert!(matches!(result, Err(SchemaError::CompositionCycle(_))));
}

/// Der Shim muss die Ownership tragen - genau die ging im alten
/// `navigations_from_ddic` verloren (immer `false`).
#[test]
fn navigation_shim_preserves_ownership() {
    let schema = orders_schema().unwrap();
    let navs = schema.to_navigations();
    let items = navs
        .iter()
        .find(|n| n.from_field.as_deref() == Some("_Items"))
        .expect("_Items");
    assert!(items.owning, "Komposition muss als owning ankommen");
    assert!(!items.single);
    assert_eq!(items.to_set.as_deref(), Some("ItemSet"));
    assert_eq!(items.to_field.as_deref(), Some("Order_ID"));
}

#[test]
fn navigation_shim_emits_draft_admin_link() {
    let schema = orders_schema().unwrap();
    let navs = schema.to_navigations();
    assert!(navs.iter().any(|n| {
        n.from_set.as_deref() == Some("OrderSet")
            && n.from_field.as_deref() == Some(crate::DRAFT_ADMIN_SET)
            && n.single
    }));
}

#[test]
fn static_source_round_trips() {
    let source = StaticSchema::new(orders_schema().unwrap());
    assert_eq!(source.revision(), SchemaRevision(1));
    assert!(source.schema().unwrap().contains("OrderSet"));
}
