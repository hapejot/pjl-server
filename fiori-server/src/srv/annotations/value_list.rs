use super::{Annotation, Collection, PropertyValue, Record};
use spec::AtomValueList;

pub(crate) fn annotations_for_atom_value_list(
    name: &str,
    value_list: &AtomValueList,
) -> Vec<Annotation> {
    match value_list {
        AtomValueList::FieldValueList {
            list_id,
            prefer_dialog,
        } => value_list_annotations(
            "FieldValueListItemsSet",
            *prefer_dialog,
            Collection::new()
                .add_record(
                    Record::new("Common.ValueListParameterOut")
                        .add(PropertyValue::new("LocalDataProperty").property_path(name))
                        .add(PropertyValue::new("ValueListProperty").string("value")),
                )
                .add_record(
                    Record::new("Common.ValueListParameterDisplayOnly")
                        .add(PropertyValue::new("ValueListProperty").string("label")),
                )
                .add_record(
                    Record::new("Common.ValueListParameterConstant")
                        .add(PropertyValue::new("ValueListProperty").string("ListID"))
                        .add(PropertyValue::new("Constant").string(list_id)),
                ),
        ),
        AtomValueList::EntityRef {
            entity_set,
            key_property,
            display_property,
            filters,
            constant_filters,
            prefer_dialog,
        } => {
            let mut parameters = Collection::new().add_record(
                Record::new("Common.ValueListParameterOut")
                    .add(PropertyValue::new("LocalDataProperty").property_path(name))
                    .add(PropertyValue::new("ValueListProperty").string(key_property)),
            );
            if let Some(display_property) = display_property {
                parameters = parameters.add_record(
                    Record::new("Common.ValueListParameterDisplayOnly")
                        .add(PropertyValue::new("ValueListProperty").string(display_property)),
                );
            }
            for filter in filters {
                parameters = parameters.add_record(value_list_parameter_in(
                    filter.local_property(),
                    filter.target_property(),
                ));
            }
            for filter in constant_filters {
                parameters = parameters.add_record(
                    Record::new("Common.ValueListParameterConstant")
                        .add(
                            PropertyValue::new("ValueListProperty")
                                .string(filter.target_property()),
                        )
                        .add(PropertyValue::new("Constant").string(filter.value())),
                );
            }
            value_list_annotations(entity_set, *prefer_dialog, parameters)
        }
    }
}

pub(crate) fn value_list_parameter_in(local_property: &str, target_property: &str) -> Record {
    Record::new("Common.ValueListParameterIn")
        .add(PropertyValue::new("LocalDataProperty").property_path(local_property))
        .add(PropertyValue::new("ValueListProperty").string(target_property))
}

fn value_list_annotations(
    collection_path: impl Into<String>,
    prefer_dialog: bool,
    parameters: Collection,
) -> Vec<Annotation> {
    let mut annotations = vec![value_list_annotation(
        collection_path,
        prefer_dialog,
        parameters,
    )];
    if !prefer_dialog {
        annotations.push(Annotation::new("Common.ValueListWithFixedValues").boolean(true));
    }
    annotations
}

pub(crate) fn value_list_annotation(
    collection_path: impl Into<String>,
    prefer_dialog: bool,
    parameters: Collection,
) -> Annotation {
    Annotation::new("Common.ValueList").record(
        Record::new("Common.ValueListType")
            .add(PropertyValue::new("CollectionPath").string(collection_path))
            .add(PropertyValue::new("SearchSupported").boolean(prefer_dialog))
            .add(PropertyValue::new("Parameters").collection(parameters)),
    )
}
