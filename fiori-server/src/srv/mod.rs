use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderName, header},
    response::Html,
    response::IntoResponse,
    routing::get,
};
use odata::{
    path::{PathSegment, parse_odata_path},
    query::{ExpandItem, Expr, ODataQuery},
};
use odata_yaml_store::EntityKey;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spec::{AtomValueList, EntitySpec, FieldSpec, MeasureKind, data_dictionary::DataDictonary};
use tracing::*;

use crate::AppState;
mod sample;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "EntityType")]
pub struct EntityType {
    #[serde(rename = "@Name")]
    pub name: String,

    #[serde(rename = "Key")]
    pub key: Key,

    #[serde(rename = "Property", default)]
    pub properties: Vec<Property>,

    #[serde(rename = "NavigationProperty", default)]
    pub navigation_properties: Vec<NavigationProperty>,
}

impl EntityType {
    pub fn from_entity(x: &EntitySpec) -> Self {
        let mut properties = Vec::new();
        let mut navigation_properties = Vec::new();

        properties.push(Property::new("ID", "Edm.Guid").nullable(false));
        properties.push(Property::new("IsActiveEntity", "Edm.Boolean").nullable(false));
        properties.push(Property::new("HasActiveEntity", "Edm.Boolean").nullable(false));
        properties.push(Property::new("HasDraftEntity", "Edm.Boolean").nullable(false));

        for f in x.fields() {
            match f {
                FieldSpec::Lookup {
                    name,
                    entity,
                    collection,
                } => {
                    let p = if *collection {
                        NavigationProperty::new(name, format!("Service.{}", entity), None)
                    } else {
                        let ref_name = format!("{}_ID", name);
                        properties.push(Property::new(&ref_name, "Edm.Guid"));
                        NavigationProperty::new(name, format!("Service.{}", entity), None)
                            .ref_constraing(ref_name.as_str(), "ID")
                    };
                    navigation_properties.push(p);
                }
                FieldSpec::Atom {
                    name,
                    edm_type,
                    max_length,
                    precision,
                    scale,
                    ..
                } => {
                    let mut property = Property::new(name, edm_type);
                    if let Some(max_length) = max_length {
                        property = property.max_length(*max_length);
                    }
                    if let Some(precision) = precision {
                        property = property.precision(*precision);
                    }
                    if let Some(scale) = scale {
                        property = property.scale(*scale);
                    }
                    properties.push(property);
                }
                FieldSpec::Measure {
                    name,
                    precision,
                    scale,
                    ..
                } => {
                    let mut property = Property::new(name, "Edm.Decimal");
                    if let Some(precision) = precision {
                        property = property.precision(*precision);
                    }
                    if let Some(scale) = scale {
                        property = property.scale(*scale);
                    }
                    properties.push(property);
                }
            }
        }
        navigation_properties.push(NavigationProperty::new(
            "SiblingEntity",
            format!("Service.{}", x.name()),
            None,
        ));
        navigation_properties.push(NavigationProperty::new(
            "DraftAdministrativeData",
            "Service.DraftAdministrativeData",
            Some(true),
        ));
        Self {
            name: x.name().into(),
            key: Key::new(&["ID", "IsActiveEntity"]),
            properties,
            navigation_properties,
        }
    }
}

// impl From<&EntitySpec> for EntityType {
//     fn from(value: &EntitySpec) -> Self {
//         Self {
//             name: value.name().into(),
//             key: Key::new(&["ID"]),
//             properties: Vec::new(),
//             navigation_properties: Vec::new(),
//         }
//     }
// }

// impl From<&EntitySpec> for EntitySet {
//     fn from(value: &EntitySpec) -> Self {
//         Self {
//             name: value.setname().into(),
//             entity_type: format!("Service.{}", value.name()),
//             navigation_property_bindings: Vec::new(),
//         }
//     }
// }

#[derive(Debug, Serialize, Deserialize)]
pub struct Key {
    #[serde(rename = "PropertyRef", default)]
    pub property_refs: Vec<PropertyRef>,
}

impl Key {
    pub fn new(names: &[&str]) -> Self {
        Self {
            property_refs: names
                .iter()
                .map(|x| PropertyRef { name: (*x).into() })
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PropertyRef {
    #[serde(rename = "@Name")]
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Property {
    #[serde(rename = "@Name")]
    pub name: String,

    #[serde(rename = "@Type")]
    pub type_: String,

    #[serde(rename = "@Nullable", skip_serializing_if = "Option::is_none")]
    pub nullable: Option<bool>,

    #[serde(rename = "@MaxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,

    #[serde(rename = "@Precision", skip_serializing_if = "Option::is_none")]
    pub precision: Option<u32>,

    #[serde(rename = "@Scale", skip_serializing_if = "Option::is_none")]
    pub scale: Option<u32>,

    #[serde(rename = "@DefaultValue", skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

impl Property {
    pub fn new(name: &str, datatype: &str) -> Self {
        Self {
            name: name.into(),
            type_: datatype.into(),
            nullable: None,
            max_length: None,
            precision: None,
            scale: None,
            default_value: None,
        }
    }

    pub fn nullable(mut self, nullable: bool) -> Self {
        self.nullable = Some(nullable);
        self
    }

    pub fn max_length(mut self, length: u32) -> Self {
        self.max_length = Some(length);
        self
    }

    pub fn precision(mut self, precision: u32) -> Self {
        self.precision = Some(precision);
        self
    }

    pub fn scale(mut self, scale: u32) -> Self {
        self.scale = Some(scale);
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct NavigationProperty {
    #[serde(rename = "@Name")]
    pub name: String,

    #[serde(rename = "@Type")]
    pub type_: String,

    #[serde(rename = "@ContainsTarget", skip_serializing_if = "Option::is_none")]
    pub contains_target: Option<bool>,

    #[serde(rename = "ReferentialConstraint")]
    pub constraints: Vec<ReferentialConstraint>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ReferentialConstraint {
    #[serde(rename = "@Property")]
    property: String,
    #[serde(rename = "@ReferencedProperty")]
    referenced_property: String,
}

impl NavigationProperty {
    pub fn new(
        name: impl Into<String>,
        type_: impl Into<String>,
        contains_target: Option<bool>,
    ) -> Self {
        Self {
            name: name.into(),
            type_: type_.into(),
            contains_target,
            ..Default::default()
        }
    }

    fn ref_constraing(mut self, property: &str, referenced_property: &str) -> NavigationProperty {
        self.constraints.push(ReferentialConstraint {
            property: property.into(),
            referenced_property: referenced_property.into(),
        });
        self
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "EntitySet")]
pub struct EntitySet {
    #[serde(rename = "@Name")]
    pub name: String,

    #[serde(rename = "@EntityType")]
    pub entity_type: String,

    #[serde(rename = "NavigationPropertyBinding", default)]
    pub navigation_property_bindings: Vec<NavigationPropertyBinding>,
}
impl EntitySet {
    fn from_entity(ddic: &DataDictonary, x: &EntitySpec) -> EntitySet {
        let mut navigation_property_bindings = Vec::new();
        navigation_property_bindings.push(NavigationPropertyBinding {
            path: "SiblingEntity".into(),
            target: x.setname().into(),
        });
        navigation_property_bindings.push(NavigationPropertyBinding {
            path: "DraftAdministrativeData".into(),
            target: "DraftAdministrativeData".into(),
        });
        for navigation in ddic.navigations_for_entity(x) {
            navigation_property_bindings.push(NavigationPropertyBinding {
                path: navigation.name,
                target: navigation.target_set,
            });
        }

        EntitySet {
            name: x.setname().into(),
            entity_type: format!("Service.{}", x.name()),
            navigation_property_bindings,
        }
    }
}

fn draft_annotations_for_entity(entity_name: &str, set_name: &str) -> Vec<String> {
    vec![
        format!(
            r##"<Annotations Target="Service.EntityContainer/{set_name}"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.DraftRoot"><Record Type="Common.DraftRootType"><PropertyValue Property="ActivationAction" String="Service.draftActivate"/><PropertyValue Property="EditAction" String="Service.draftEdit"/><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation></Annotations>"##,
        ),
        format!(
            r##"<Annotations Target="Service.{entity_name}/IsActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>"##,
        ),
        format!(
            r##"<Annotations Target="Service.{entity_name}/HasActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>"##,
        ),
        format!(
            r##"<Annotations Target="Service.{entity_name}/HasDraftEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>"##,
        ),
    ]
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NavigationPropertyBinding {
    #[serde(rename = "@Path")]
    pub path: String,

    #[serde(rename = "@Target")]
    pub target: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename = "Annotations")]
pub struct Annotations {
    #[serde(rename = "@Target")]
    pub target: String,

    #[serde(rename = "Annotation", default)]
    pub annotations: Vec<Annotation>,
}
impl Annotations {
    fn from_entity(x: &EntitySpec) -> Vec<Annotations> {
        let mut annotations = Vec::new();
        let mut main_annotations = Annotations {
            target: format!("Service.{}", x.name()),
            annotations: Vec::new(),
        };

        main_annotations.annotations.push(
            Annotation::new("UI.HeaderInfo").record(
                Record::new("UI.HeaderInfoType")
                    .add(PropertyValue::new("TypeName").string(x.name()))
                    .add(PropertyValue::new("TypeNamePlural").string(x.setname()))
                    .add(PropertyValue::new("Title").record(
                        Record::new("UI.DataField").add(PropertyValue::new("Value").path("name")),
                    )),
            ),
        );

        main_annotations
            .annotations
            .push(
                Annotation::new("UI.LineItem").collection(Collection::new().add_record(
                    Record::new("UI.DataField").add(PropertyValue::new("Value").path("name")),
                )),
            );
        main_annotations.annotations.push(
            Annotation::new("Common.SemanticKey")
                .collection(Collection::new().add_property_path("ID")),
        );
        main_annotations
            .annotations
            .push(Annotation::new("Common.Messages").path("DraftMessages"));

        // Default Object Page section pointing to a main field group.
        main_annotations.annotations.push(
            Annotation::new("UI.Facets").collection(
                Collection::new().add_record(
                    Record::new("UI.CollectionFacet")
                        .add(PropertyValue::new("Label").string("General"))
                        .add(PropertyValue::new("ID").string("GeneralSection"))
                        .add(
                            PropertyValue::new("Facets").collection(
                                Collection::new().add_record(
                                    Record::new("UI.ReferenceFacet")
                                        .add(PropertyValue::new("Label").string("General"))
                                        .add(
                                            PropertyValue::new("Target")
                                                .annotation_path("@UI.FieldGroup#Main"),
                                        ),
                                ),
                            ),
                        ),
                ),
            ),
        );

        let main_field_group = prepare_field_group(x);
        main_annotations.annotations.push(main_field_group);

        annotations.push(main_annotations);

        annotations.push(Self::from_field(
            x.name(),
            &FieldSpec::Atom {
                name: "ID".into(),
                label: x.name().into(),
                edm_type: "Edm.Guid".into(),
                package: None,
                max_length: None,
                precision: None,
                scale: None,
                computed: false,
                immutable: false,
                value_list: None,
                presentation: Default::default(),
            },
        ));
        for f in x.fields() {
            annotations.push(Self::from_field(x.name(), f));
        }
        annotations
    }

    fn from_field(service_name: &str, f: &spec::FieldSpec) -> Annotations {
        let mut target = format!("Service.{}/{}", service_name, f.name());
        let mut annotations = vec![Annotation::new("Common.Label").string(f.label())];

        match f {
            FieldSpec::Lookup {
                name,
                entity,
                collection,
            } => {
                if !collection {
                    let name = format!("{}_ID", name);
                    target = format!("Service.{}/{}", service_name, name);
                    annotations.push(
                        Annotation::new("Common.Text")
                            .path(format!("{}/name", f.name()))
                            .annotation(
                                Annotation::new("UI.TextArrangement")
                                    .enum_member("UI.TextArrangementType/TextOnly"),
                            ),
                    );

                    annotations.push(value_list_annotation(
                        format!("{entity}Set"),
                        false,
                        Collection::new()
                            .add_record(
                                Record::new("Common.ValueListParameterOut")
                                    .add(
                                        PropertyValue::new("LocalDataProperty").property_path(name),
                                    )
                                    .add(PropertyValue::new("ValueListProperty").string("ID")),
                            )
                            .add_record(
                                Record::new("Common.ValueListParameterDisplayOnly")
                                    .add(PropertyValue::new("ValueListProperty").string("name")),
                            ),
                    ));
                }
            }
            FieldSpec::Atom {
                name,
                computed,
                immutable,
                value_list,
                ..
            } => {
                if *computed {
                    annotations.push(Annotation::new("Org.OData.Core.V1.Computed").boolean(true));
                }
                if *immutable {
                    annotations.push(Annotation::new("Org.OData.Core.V1.Immutable").boolean(true));
                }
                if let Some(value_list) = value_list {
                    annotations.extend(annotations_for_atom_value_list(name, value_list));
                }
                if name == "ID" {
                    annotations.push(
                        Annotation::new("Common.Text").path("name").annotation(
                            Annotation::new("UI.TextArrangement")
                                .enum_member("UI.TextArrangementType/TextOnly"),
                        ),
                    );
                }
            }
            FieldSpec::Measure {
                unit_field, kind, ..
            } => {
                let term = match kind {
                    MeasureKind::Currency => "Org.OData.Measures.V1.ISOCurrency",
                    MeasureKind::Unit => "Org.OData.Measures.V1.Unit",
                };
                annotations.push(Annotation::new(term).path(unit_field));
            }
        }

        Annotations {
            target,
            annotations,
        }
    }
}

fn prepare_field_group(x: &EntitySpec) -> Annotation {
    let mut main_data = Collection::new();
    // main_data = main_data
    //     .add_record(Record::new("UI.DataField")
    //     .add(PropertyValue::new("Value")
    //     .path("ID")));
    for f in x.fields() {
        match f {
            FieldSpec::Lookup {
                name, collection, ..
            } => {
                if *collection {
                } else {
                    let name = format!("{name}_ID");
                    main_data = main_data.add_record(
                        Record::new("UI.DataField").add(PropertyValue::new("Value").path(name)),
                    )
                }
            }
            FieldSpec::Atom { name, .. } => {
                main_data = main_data.add_record(
                    Record::new("UI.DataField").add(PropertyValue::new("Value").path(name)),
                )
            }
            FieldSpec::Measure { .. } => todo!(),
        }
    }
    let mut main_field_group = Annotation::new("UI.FieldGroup");
    main_field_group.qualifier = Some("Main".to_string());
    main_field_group = main_field_group.record(
        Record::new("UI.FieldGroupType").add(PropertyValue::new("Data").collection(main_data)),
    );
    main_field_group
}

fn annotations_for_atom_value_list(name: &str, value_list: &AtomValueList) -> Vec<Annotation> {
    match value_list {
        AtomValueList::FieldValueList {
            list_id,
            prefer_dialog,
        } => value_list_annotations(
            "FieldValueListItems",
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
                        .add(PropertyValue::new("Constant").constant(list_id)),
                ),
        ),
        AtomValueList::EntityRef {
            entity_set,
            key_property,
            display_property,
            filters,
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
                parameters = parameters.add_record(
                    Record::new("Common.ValueListParameterIn")
                        .add(
                            PropertyValue::new("LocalDataProperty")
                                .property_path(&filter.local_property),
                        )
                        .add(
                            PropertyValue::new("ValueListProperty").string(&filter.target_property),
                        ),
                );
            }
            value_list_annotations(entity_set, *prefer_dialog, parameters)
        }
    }
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

fn value_list_annotation(
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

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Annotation {
    #[serde(rename = "@Term")]
    pub term: String,

    #[serde(rename = "@Qualifier", skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,

    #[serde(rename = "@String", skip_serializing_if = "Option::is_none")]
    pub string: Option<String>,

    #[serde(rename = "@Bool", skip_serializing_if = "Option::is_none")]
    pub bool_: Option<bool>,

    #[serde(rename = "@Path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(rename = "@EnumMember", skip_serializing_if = "Option::is_none")]
    pub enum_member: Option<String>,

    #[serde(rename = "Collection", skip_serializing_if = "Option::is_none")]
    pub collection: Option<Collection>,

    #[serde(rename = "Record", skip_serializing_if = "Option::is_none")]
    pub record: Option<Record>,

    // z.B. Common.Text hat eine verschachtelte <Annotation> als Kind-Element
    #[serde(rename = "Annotation", default, skip_serializing_if = "Vec::is_empty")]
    pub nested_annotations: Vec<Annotation>,
}

impl Annotation {
    pub fn new(term: impl Into<String>) -> Self {
        Self {
            term: term.into(),
            qualifier: None,
            string: None,
            bool_: None,
            path: None,
            enum_member: None,
            collection: None,
            record: None,
            nested_annotations: vec![],
        }
    }

    pub fn string(mut self, value: impl Into<String>) -> Self {
        self.string = Some(value.into());
        self
    }

    pub fn boolean(mut self, value: bool) -> Self {
        self.bool_ = Some(value);
        self
    }

    pub fn record(mut self, record: Record) -> Self {
        self.record = Some(record);
        self
    }
    pub fn collection(mut self, collection: Collection) -> Self {
        self.collection = Some(collection);
        self
    }
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn enum_member(mut self, arg: impl Into<String>) -> Self {
        self.enum_member = Some(arg.into());
        self
    }

    fn annotation(mut self, enum_member: Annotation) -> Self {
        self.nested_annotations.push(enum_member);
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Collection {
    #[serde(
        rename = "PropertyPath",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub property_paths: Vec<String>,

    #[serde(rename = "Record", default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<Record>,
}

impl Collection {
    pub fn new() -> Self {
        Self {
            property_paths: Vec::new(),
            records: Vec::new(),
        }
    }

    pub fn add_property_path(mut self, path: impl Into<String>) -> Self {
        self.property_paths.push(path.into());
        self
    }

    pub fn add_record(mut self, record: Record) -> Self {
        self.records.push(record);
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Record {
    #[serde(rename = "@Type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    #[serde(rename = "PropertyValue", default)]
    pub property_values: Vec<PropertyValue>,
}

impl Record {
    pub fn new(type_: impl Into<String>) -> Self {
        Self {
            type_: Some(type_.into()),
            property_values: Vec::new(),
        }
    }
    pub fn add(mut self, property_value: PropertyValue) -> Self {
        self.property_values.push(property_value);
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PropertyValue {
    #[serde(rename = "@Property")]
    pub property: String,

    #[serde(rename = "@Path", skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    #[serde(rename = "@String", skip_serializing_if = "Option::is_none")]
    pub string: Option<String>,

    #[serde(rename = "@Bool", skip_serializing_if = "Option::is_none")]
    pub bool_: Option<bool>,

    #[serde(rename = "@Constant", skip_serializing_if = "Option::is_none")]
    pub constant: Option<String>,

    #[serde(rename = "@PropertyPath", skip_serializing_if = "Option::is_none")]
    pub property_path: Option<String>,

    #[serde(rename = "@AnnotationPath", skip_serializing_if = "Option::is_none")]
    pub annotation_path: Option<String>,

    #[serde(rename = "Collection", skip_serializing_if = "Option::is_none")]
    pub collection: Option<Collection>,

    #[serde(rename = "Record", skip_serializing_if = "Option::is_none")]
    pub record: Option<Record>,
}

impl PropertyValue {
    pub fn new(property: impl Into<String>) -> Self {
        Self {
            property: property.into(),
            path: None,
            string: None,
            bool_: None,
            constant: None,
            property_path: None,
            annotation_path: None,
            collection: None,
            record: None,
        }
    }

    pub fn string(mut self, value: impl Into<String>) -> Self {
        self.string = Some(value.into());
        self
    }
    pub fn boolean(mut self, value: impl Into<bool>) -> Self {
        self.bool_ = Some(value.into());
        self
    }

    pub fn constant(mut self, value: impl Into<String>) -> Self {
        self.constant = Some(value.into());
        self
    }

    pub fn record(mut self, record: Record) -> Self {
        self.record = Some(record);
        self
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn property_path(mut self, path: impl Into<String>) -> Self {
        self.property_path = Some(path.into());
        self
    }

    pub fn collection(mut self, collection: Collection) -> Self {
        self.collection = Some(collection);
        self
    }

    pub fn annotation_path(mut self, annotation_path: impl Into<String>) -> Self {
        self.annotation_path = Some(annotation_path.into());
        self
    }
}
// body style alternatives:
//    display: flex;
//    align-items: center;
//    justify-content: center;

const MODERN_STYLE: &str = r#"
<style>
  body {
    font-family: 'Inter', system-ui, -apple-system, sans-serif;
    background: #0f172a;
    color: #e2e8f0;
    min-height: 100vh;
    margin: 0;
  }
  .card {
    background: rgba(255, 255, 255, 0.05);
    backdrop-filter: blur(10px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 16px;
    padding: 2rem 2.5rem;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.3);
    max-width: 480px;
  }
  h1 {
    font-size: 1.5rem;
    font-weight: 600;
    margin: 0 0 0.5rem;
    background: linear-gradient(135deg, #60a5fa, #a78bfa);
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  p {
    font-size: 0.95rem;
    line-height: 1.5;
    color: #94a3b8;
  }
  table {
      width: 100%;
  }
</style>
"#;
/// Build the HTML data dictionary page.
fn render_info_page(state: &AppState) -> String {
    let mut html = String::new();
    html.push_str(MODERN_STYLE);
    // html.push_str("<div class='card'>");
    html.push_str("<h1>Data Dictionary</h1>\n");

    // Packages
    let packages: Vec<_> = state.ddic.packages().collect();
    html.push_str("<h2>Packages</h2>\n<ul>\n");
    for p in packages {
        html.push_str(&format!("<li>{}</li>\n", p.name()));
    }
    html.push_str("</ul>\n");

    // Apps
    let apps: Vec<_> = state.ddic.apps().collect();
    html.push_str(&format!(
        "<h2>Apps ({})</h2>\n<ul class=\"apps\">\n",
        apps.len()
    ));
    for app in apps {
        html.push_str(&format!(
            "<li><span class=\"app-title\">{}</span>\n\
             <span class=\"app-sub\">{}\n\
             <a href=\"{}\">{}</a> · {}\n",
            app.title(),
            app.subtitle().map(|s| s.as_str()).unwrap_or(""),
            app.url(),
            app.name(),
            app.action(),
        ));
    }
    html.push_str("</ul>\n");

    // Entities
    let entities: Vec<_> = state.ddic.active_entities().cloned().collect();
    html.push_str(&format!(
        "<h2>Entities ({})</h2>\n<ul class=\"entities\">\n",
        entities.len()
    ));
    for ent in entities {
        let set = ent.setname();
        let fields: Vec<_> = ent.fields().cloned().collect();
        let navigations = state.ddic.navigations_for_entity(&ent);
        html.push_str(&format!(
            "<h3>{}</h3><p>(set {}, {} fields, {} nav)</p>",
            ent.name(),
            set,
            fields.len(),
            navigations.len()
        ));

        html.push_str(
            "<table class=\"fields\">\n<tr><th>Field</th><th>UI Label</th><th>Edm Type</th></tr>\n",
        );
        for f in fields {
            let display = f.label();
            let ftype = format_field_value(&f);
            html.push_str(&format!(
                "<tr><td>{}</td><td>{display}</td><td>{ftype}</td></tr>\n",
                f
            ));
        }
        html.push_str("</table><ul>\n");

        for nav in navigations {
            let collection = if nav.collection { "collection" } else { "1:1" };
            html.push_str(&format!(
                "<li>{0} → {1} (source set {2}, target set {3}, FK {4})<span class=\"cardinality\">{collection}</span>\n",
                nav.name, nav.target_entity, nav.source_set, nav.target_set,
                nav.foreign_key_field.as_deref().unwrap_or("-")
            ));
        }
        html.push_str("</ul>\n");
    }

    // Relationships
    let relationships: Vec<_> = state.ddic.relationships().cloned().collect();
    html.push_str(&format!(
        "<h2>Relationships ({})</h2>\n<ul class=\"relationships\">\n",
        relationships.len()
    ));
    for rel in relationships {
        html.push_str(&format!(
            "<li>\n\
             <span class=\"rel-name\">{}</span>\n\
             <span class=\"rel-one\">{}</span> → <span class=\"rel-many\">{}</span> \n\
             <span class=\"cardinality\">{}</span> · package <span class=\"rel-pkg\">{}</span>\n",
            rel.name,
            rel.one(),
            rel.many(),
            rel.package,
            rel.package
        ));
    }
    html.push_str("</ul>\n");
    // html.push_str("</div>");
    html
}

fn format_field_value(f: &FieldSpec) -> String {
    match f {
        FieldSpec::Lookup {
            entity, collection, ..
        } => {
            if *collection {
                format!("Lookup ({entity})[]")
            } else {
                format!("Lookup ({entity})")
            }
        }
        FieldSpec::Atom { edm_type, .. } => edm_type.clone(),
        FieldSpec::Measure { label, .. } => label.clone(),
    }
}

/// Serve an HTML page describing the current data dictionary.
async fn info(State(state): State<AppState>) -> impl IntoResponse {
    let html = render_info_page(&state);
    log::info!("serving HTML data dictionary page");
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (HeaderName::from_static("odata-version"), "4.0"),
        ],
        Html(html),
    )
}

////////////////////////////////////////////////////////////////////////////////////////
/// router entry point
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/$metadata", get(metadata))
        .route("/info", get(info))
        .route(
            "/{*path}",
            get(get_handler).post(post_handler).patch(patch_handler),
        )
}

fn build_query(params: &HashMap<String, String>) -> ODataQuery {
    ODataQuery {
        filter: params.get("$filter").map(|x| Expr::parse(x).unwrap()),
        orderby: vec![],
        top: params.get("$top").map(|x| x.parse().unwrap()),
        skip: params.get("$skip").map(|x| x.parse().unwrap()),
        select: params
            .get("$select")
            .and_then(|x| Some(x.split(",").map(|y| y.to_string()).collect::<Vec<_>>()))
            .unwrap_or_else(|| vec![]),
        expand: params
            .get("$expand")
            .map(|x| parse_expand_items(x))
            .unwrap_or_else(|| vec![]),
        count: params.get("$count").is_some(),
        search: params.get("$search").cloned(),
    }
}

fn parse_expand_items(raw: &str) -> Vec<ExpandItem> {
    split_top_level(raw, ',')
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| parse_expand_item(&part))
        .collect()
}

fn parse_expand_item(raw: &str) -> ExpandItem {
    let trimmed = raw.trim();
    if let Some(paren) = find_top_level_options_start(trimmed) {
        let navigation_property = trimmed[..paren].trim().to_string();
        let options = trimmed[paren + 1..trimmed.len() - 1].trim();
        ExpandItem {
            navigation_property,
            nested_query: Box::new(parse_nested_query(options)),
        }
    } else {
        ExpandItem {
            navigation_property: trimmed.to_string(),
            nested_query: Box::new(ODataQuery::default()),
        }
    }
}

fn parse_nested_query(raw: &str) -> ODataQuery {
    let mut query = ODataQuery::default();
    for option in split_top_level(raw, ';') {
        let Some((name, value)) = option.split_once('=') else {
            continue;
        };
        match name.trim() {
            "$filter" => query.filter = Some(Expr::parse(value.trim()).unwrap()),
            "$top" => query.top = Some(value.trim().parse().unwrap()),
            "$skip" => query.skip = Some(value.trim().parse().unwrap()),
            "$select" => query.select = split_top_level(value.trim(), ','),
            "$expand" => query.expand = parse_expand_items(value.trim()),
            "$count" => query.count = value.trim().parse().unwrap_or(true),
            "$search" => query.search = Some(value.trim().to_string()),
            _ => {}
        }
    }
    query
}

fn find_top_level_options_start(raw: &str) -> Option<usize> {
    if !raw.ends_with(')') {
        return None;
    }

    let mut in_string = false;
    for (idx, ch) in raw.char_indices() {
        match ch {
            '\'' => in_string = !in_string,
            '(' if !in_string => return Some(idx),
            _ => {}
        }
    }
    None
}

fn split_top_level(raw: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut in_string = false;

    for ch in raw.chars() {
        match ch {
            '\'' => {
                in_string = !in_string;
                current.push(ch);
            }
            '(' if !in_string => {
                depth += 1;
                current.push(ch);
            }
            ')' if !in_string => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            c if c == separator && depth == 0 && !in_string => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn to_json_response(body: Value) -> ([(HeaderName, String); 2], String) {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/json;odata.metadata=minimal;charset=utf-8".to_string(),
            ),
            (HeaderName::from_static("odata-version"), "4.0".to_string()),
        ],
        body.to_string(),
    )
}

fn entity_spec_for_segment(state: &AppState, s: &PathSegment) -> EntitySpec {
    state
        .ddic
        .active_entity_by_setname(&s.name)
        .unwrap_or_else(|| panic!("No active entity for set '{}'", s.name))
        .clone()
}

async fn get_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let p = build_query(&params);
    let path_struct = parse_odata_path(&path).unwrap();
    let (mut j, context) = resolve_get_path(&state, &p, &path_struct.segments);
    j["@odata.context"] = json!(context.concat());
    to_json_response(j)
}

fn resolve_get_path(
    state: &State<AppState>,
    query: &ODataQuery,
    segments: &[PathSegment],
) -> (Value, Vec<String>) {
    let root = segments.first().expect("OData path has a root segment");
    let entity_spec = entity_spec_for_segment(state, root);
    let value = if root.keys.is_empty() {
        get_collection(state, query, &entity_spec, root)
    } else {
        let mut single = get_single(state, query, &entity_spec, root);
        touch_selected_fields(&mut single, query);
        single
    };
    let is_collection = root.keys.is_empty();

    for segment in segments.iter().skip(1) {
        info!("processing segment {:?}", segment);
        let navigation_query = if segment == segments.last().unwrap() {
            query
        } else {
            &ODataQuery::default()
        };
        // let resolved = resolve_navigation(state, &entity_spec, &value, segment, navigation_query);
        // entity_spec = resolved.entity_spec;
        // value = resolved.value;
        // is_collection = resolved.is_collection;
    }

    // let final_expand = expand_for_spec(state, &entity_spec, &query.expand);
    // apply_expand(state, &entity_spec, &mut value, &final_expand);
    // project_value(&mut value, &query.select, &final_expand);

    let mut context = vec!["/Service/$metadata#".to_string()];
    if is_collection {
        context.push(entity_spec.setname().to_string());
    } else {
        context.push(format!("{}/$entity", entity_spec.setname()));
    }
    (value, context)
}

// fn expand_for_spec(
//     state: &State<AppState>,
//     entity_spec: &EntitySpec,
//     expand_items: &[ExpandItem],
// ) -> Vec<ExpandItem> {
//     expand_items
//         .iter()
//         .cloned()
//         .filter(|expand| has_navigation(state, entity_spec, &expand.navigation_property))
//         .collect()
// }

// fn has_navigation(
//     state: &State<AppState>,
//     entity_spec: &EntitySpec,
//     navigation_property: &str,
// ) -> bool {
//     navigation_property == "DraftAdministrativeData"
//         || state
//             .ddic
//             .navigation_for_entity(entity_spec, navigation_property)
//             .is_some()
// }

// struct ResolvedNavigation {
//     entity_spec: EntitySpec,
//     value: Value,
//     is_collection: bool,
// }

// fn resolve_navigation(
//     state: &State<AppState>,
//     current_spec: &EntitySpec,
//     current_value: &Value,
//     segment: &PathSegment,
//     query: &ODataQuery,
// ) -> ResolvedNavigation {
//     if segment.name == "DraftAdministrativeData" {
//         return ResolvedNavigation {
//             entity_spec: current_spec.clone(),
//             value: current_value["DraftAdministrativeData"].clone(),
//             is_collection: false,
//         };
//     }

//     let navigation = state
//         .ddic
//         .navigation_for_entity(current_spec, &segment.name)
//         .unwrap_or_else(|| {
//             panic!(
//                 "Unknown navigation {} on {}",
//                 segment.name,
//                 current_spec.name()
//             )
//         });

//     let target_spec = entity_spec_by_name(state, &navigation.target_entity);
//     if navigation.collection {
//         resolve_collection_navigation(
//             state,
//             current_spec,
//             current_value,
//             &target_spec,
//             &navigation.name,
//             navigation.foreign_key_field.as_deref(),
//             segment,
//             query,
//         )
//     } else {
//         resolve_single_navigation(
//             state,
//             current_value,
//             &target_spec,
//             &navigation.name,
//             segment,
//         )
//     }
// }

// fn resolve_single_navigation(
//     state: &State<AppState>,
//     current_value: &Value,
//     target_spec: &EntitySpec,
//     lookup_name: &str,
//     segment: &PathSegment,
// ) -> ResolvedNavigation {
//     let target_set = target_spec.setname();
//     let value = if segment.keys.is_empty() {
//         current_value
//             .get(lookup_name)
//             .and_then(value_as_uuid)
//             .map(|id| {
//                 let key = EntityKey::new(id, true);
//                 json!(state.store.read_entity(&target_set, &key).unwrap())
//             })
//             .unwrap_or(Value::Null)
//     } else {
//         let key: EntityKey = segment.into();
//         json!(state.store.read_entity(&target_set, &key).unwrap())
//     };

//     ResolvedNavigation {
//         entity_spec: target_spec.clone(),
//         value,
//         is_collection: false,
//     }
// }

// fn resolve_collection_navigation(
//     state: &State<AppState>,
//     current_spec: &EntitySpec,
//     current_value: &Value,
//     target_spec: &EntitySpec,
//     lookup_name: &str,
//     foreign_key_field: Option<&str>,
//     segment: &PathSegment,
//     query: &ODataQuery,
// ) -> ResolvedNavigation {
//     let target_set = target_spec.setname();
//     let foreign_key = foreign_key_field.unwrap_or_else(|| {
//         panic!(
//             "No foreign key for collection navigation {} from {} to {}",
//             lookup_name,
//             current_spec.name(),
//             target_spec.name()
//         )
//     });
//     let parent_id = current_value
//         .get("ID")
//         .and_then(value_as_uuid)
//         .expect("Current entity has an ID");
//     let mut records = state.store.get_collection(&target_set, query).unwrap();
//     records.retain(|record| {
//         record[foreign_key]
//             .as_str()
//             .and_then(|value| Uuid::parse_str(value).ok())
//             == Some(parent_id)
//     });

//     if segment.keys.is_empty() {
//         ResolvedNavigation {
//             entity_spec: target_spec.clone(),
//             value: json!({ "value": records }),
//             is_collection: true,
//         }
//     } else {
//         let key: EntityKey = segment.into();
//         let record = records
//             .into_iter()
//             .find(|record| {
//                 record.id() == key.id && record.is_active_entity() == key.is_active_entity
//             })
//             .map(|record| json!(record))
//             .unwrap_or(Value::Null);
//         ResolvedNavigation {
//             entity_spec: target_spec.clone(),
//             value: record,
//             is_collection: false,
//         }
//     }
// }

// fn apply_expand(
//     state: &State<AppState>,
//     entity_spec: &EntitySpec,
//     value: &mut Value,
//     expand_items: &[ExpandItem],
// ) {
//     if expand_items.is_empty() {
//         return;
//     }

//     if let Some(items) = value.get_mut("value").and_then(Value::as_array_mut) {
//         for item in items {
//             apply_expand_to_entity(state, entity_spec, item, expand_items);
//         }
//         return;
//     }

//     apply_expand_to_entity(state, entity_spec, value, expand_items);
// }

// fn apply_expand_to_entity(
//     state: &State<AppState>,
//     entity_spec: &EntitySpec,
//     value: &mut Value,
//     expand_items: &[ExpandItem],
// ) {
//     if !value.is_object() {
//         return;
//     }

//     for expand in expand_items {
//         let segment = PathSegment {
//             name: expand.navigation_property.clone(),
//             keys: vec![],
//         };
//         let resolved =
//             resolve_navigation(state, entity_spec, value, &segment, &expand.nested_query);
//         let mut expanded_value = resolved.value;
//         apply_expand(
//             state,
//             &resolved.entity_spec,
//             &mut expanded_value,
//             &expand.nested_query.expand,
//         );
//         project_value(
//             &mut expanded_value,
//             &expand.nested_query.select,
//             &expand.nested_query.expand,
//         );
//         value[&expand.navigation_property] = expanded_value;
//     }
// }

fn touch_selected_fields(value: &mut Value, query: &ODataQuery) {
    for field in query.select.iter() {
        if value[field].is_null() {
            value[field] = json!(format!(""));
        }
    }
}

// fn project_value(value: &mut Value, select: &[String], expand_items: &[ExpandItem]) {
//     if select.is_empty() {
//         return;
//     }

//     if let Some(items) = value.get_mut("value").and_then(Value::as_array_mut) {
//         for item in items {
//             project_entity(item, select, expand_items);
//         }
//         return;
//     }

//     project_entity(value, select, expand_items);
// }

// fn project_entity(value: &mut Value, select: &[String], expand_items: &[ExpandItem]) {
//     let Some(map) = value.as_object_mut() else {
//         return;
//     };
//     map.retain(|key, _| {
//         select.iter().any(|selected| selected == key)
//             || expand_items
//                 .iter()
//                 .any(|expand| expand.navigation_property == *key)
//     });
// }

// fn value_as_uuid(value: &Value) -> Option<Uuid> {
//     value.as_str().and_then(|value| Uuid::parse_str(value).ok())
// }

fn get_single(
    state: &State<AppState>,
    p: &ODataQuery,
    _entity_spec: &EntitySpec,
    s: &odata::path::PathSegment,
) -> Value {
    let key: EntityKey = s.into();
    let j = state.store.read_entity(&s.name, &key, p).unwrap();
    // info!("get single: {j:#?}");
    json!(j)
}

fn get_collection(
    state: &State<AppState>,
    p: &ODataQuery,
    entity_spec: &EntitySpec,
    s: &odata::path::PathSegment,
) -> Value {
    let r = state.store.get_collection(&s.name, p).unwrap();
    let mut resp = json!({
      "@odata.context": format!("/Service/$metadata#{}", entity_spec.setname()),
      "value": r
    });
    if p.count {
        let count = state.store.count(&s.name, p);
        resp["@odata.count"] = json!(count);
    }
    resp
}

async fn post_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
    payload: Option<Json<Value>>,
) -> impl IntoResponse {
    let _p = build_query(&params);
    let path_struct = parse_odata_path(&path).unwrap();

    match path_struct.segments.as_slice() {
        [_entity] => {
            let r = state
                .store
                .create_entity(_entity.name.as_str(), payload.as_ref().unwrap())
                .unwrap();
            return to_json_response(json!(r));
        }
        [entity, action] => {
            if !entity.keys.is_empty() && action.keys.is_empty() {
                let action_name = action.name.rsplit('.').next().unwrap_or_default();
                let key: EntityKey = entity.into();
                match action_name {
                    "draftEdit" => {
                        let r = state.store.draft_edit(&entity.name, &key).unwrap();
                        return to_json_response(r);
                    }
                    "draftActivate" => {
                        let r = state.store.draft_activate(&entity.name, &key).unwrap();
                        return to_json_response(r);
                    }
                    "draftPrepare" => {
                        let r = state.store.draft_prepare(&entity.name, &key).unwrap();
                        return to_json_response(r);
                    }
                    _ => {}
                };
            }
        }
        _ => todo!(),
    }

    to_json_response(
        json!({ "error": {"code": "NotImplemented", "message": "POST not yet implemented"}}),
    )
}

async fn patch_handler(
    path: Path<String>,
    _state: State<AppState>,
    params: Query<HashMap<String, String>>,
    payload: Option<Json<Value>>,
) -> impl IntoResponse {
    let _p = build_query(&params);
    let _path_struct = parse_odata_path(&path).unwrap();
    let _body = payload.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    let key = _path_struct.segments.first().unwrap();
    let ekey = EntityKey::new(key.id().unwrap(), key.is_active_entity());
    _state.store.patch_entity(&key.name, &ekey, &_body).unwrap();
    (
        [content_type_json(), odata_version()],
        json!([]).to_string(),
    )
}
#[allow(dead_code)]
fn not_implemented(msg: impl Into<String>) -> Value {
    json!({"error": {"code": "NotImplemented", "message": format!("{} not yet implemented", msg.into())}})
}

fn odata_version() -> (HeaderName, String) {
    (HeaderName::from_static("odata-version"), "4.0".to_string())
}

fn content_type_json() -> (HeaderName, String) {
    (
        header::CONTENT_TYPE,
        "application/json;odata.metadata=minimal;charset=utf-8".to_string(),
    )
}

async fn index(State(state): State<AppState>) -> Json<Value> {
    trace!("index");
    let mut value: Vec<Value> = state
        .ddic
        .active_entities()
        .map(|e| {
            let set = e.setname();
            json!({"name": set, "url": set})
        })
        .collect();
    value.extend([
        json!({ "name": "EntityConfigs", "url": "EntityConfigs" }),
        json!({ "name": "EntityFields", "url": "EntityFields" }),
        json!({ "name": "EntityFacets", "url": "EntityFacets" }),
        json!({ "name": "EntityNavigations", "url": "EntityNavigations" }),
        json!({ "name": "EntityRelationships", "url": "EntityRelationships" }),
        json!({ "name": "EntityTableFacets", "url": "EntityTableFacets" }),
        json!({ "name": "FieldValueLists", "url": "FieldValueLists" }),
        json!({ "name": "FieldValueListItems", "url": "FieldValueListItems" }),
    ]);
    Json(json!({
      "@odata.context": "/Service/$metadata",
      "value": value
    }))
}
async fn metadata(State(state): State<AppState>) -> impl IntoResponse {
    let entity_lst: Vec<EntityType> = state
        .ddic
        .active_entities()
        .map(|x| EntityType::from_entity(x))
        .collect();
    let entity_set_lst: Vec<EntitySet> = state
        .ddic
        .apps()
        .map(|x| EntitySet::from_entity(&state.ddic, &x.main_entity(&state.ddic)))
        .collect();
    let annotation_lst: Vec<Annotations> = state
        .ddic
        .active_entities()
        .map(|x| Annotations::from_entity(x))
        .flatten()
        .collect();
    // let _sample_entity = sample::sample_entity();
    // let _sample_entity_set = sample::sample_entity_set();
    // let xml = sample::sample_annotations();
    // let sample_annotations: Annotations = quick_xml::de::from_str(&xml).unwrap();
    // for a in &sample_annotations.annotations {
    //     trace!("A: {} {:?}", a.term, a.qualifier);
    // }

    let meta = edmx_template();
    let mut type_defs = vec![]; //quick_xml::se::to_string(&sample_entity).unwrap()];
    for x in entity_lst {
        type_defs.push(quick_xml::se::to_string(&x).unwrap());
    }
    let mut entity_sets = vec![]; //quick_xml::se::to_string(&sample_entity_set).unwrap()];
    for x in entity_set_lst {
        entity_sets.push(quick_xml::se::to_string(&x).unwrap());
    }
    let mut annotations = vec![];
    for x in annotation_lst {
        annotations.push(quick_xml::se::to_string(&x).unwrap());
    }
    for x in state.ddic.active_entities() {
        let set_name = x.setname();
        annotations.extend(draft_annotations_for_entity(x.name(), &set_name));
    }

    let mut actions = vec![];
    for x in state.ddic.apps() {
        let e = x.main_entity(&state.ddic);
        let entity_name = format!("Service.{}", e.name());
        actions.push(format!(
            r##"<Action Name="draftEdit" IsBound="true" EntitySetPath="in">
                    <Parameter Name="in" Type="{entity_name}"/>
                    <Parameter Name="PreserveChanges" Type="Edm.Boolean"/>
                    <ReturnType Type="{entity_name}"/>
                </Action>"##,
        ));
        actions.push(format!(
            r##"<Action Name="draftActivate" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="{entity_name}"/>
                    <ReturnType Type="{entity_name}"/></Action>"##,
        ));
        actions.push(format!(
            r##"<Action Name="draftPrepare" IsBound="true" EntitySetPath="in">
                    <Parameter Name="in" Type="{entity_name}"/>
                    <Parameter Name="SideEffectsQualifier" Type="Edm.String"/>
                    <ReturnType Type="{entity_name}"/></Action>"##,
        ));
        actions.push(format!(
            r##"<Action Name="publishConfig" IsBound="true" EntitySetPath="in">
                    <Parameter Name="in" Type="{entity_name}"/>
                    <ReturnType Type="{entity_name}"/></Action>"##,
        ));
    }

    // let cfg = [cfg1, cfg2, cfg3].concat();
    let xml = meta
        .replace("%TYPE-DEFINITIONS%", &type_defs.concat())
        .replace("%ENTITY-SETS%", &entity_sets.concat())
        .replace("%ANNOTATIONS%", &annotations.concat())
        .replace("%ACTIONS%", &actions.concat())
        .to_string();
    (
        [
            (header::CONTENT_TYPE, "application/xml; charset=utf-8"),
            (HeaderName::from_static("odata-version"), "4.0"),
        ],
        xml,
    )
}

fn edmx_template() -> String {
    let meta = r##"<?xml version="1.0" encoding="utf-8"?>
<edmx:Edmx Version="4.0" xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx">

 <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Aggregation.V1.xml"> <edmx:Include Alias="Aggregation" Namespace="Org.OData.Aggregation.V1"/> </edmx:Reference>
 <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Capabilities.V1.xml"> <edmx:Include Alias="Capabilities" Namespace="Org.OData.Capabilities.V1"/> </edmx:Reference>
 <edmx:Reference Uri="https://sap.github.io/odata-vocabularies/vocabularies/Common.xml"> <edmx:Include Alias="Common" Namespace="com.sap.vocabularies.Common.v1"/> </edmx:Reference>
 <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Core.V1.xml"> <edmx:Include Alias="Core" Namespace="Org.OData.Core.V1"/> </edmx:Reference>
 <edmx:Reference Uri="https://sap.github.io/odata-vocabularies/vocabularies/Hierarchy.xml"> <edmx:Include Alias="Hierarchy" Namespace="com.sap.vocabularies.Hierarchy.v1"/> </edmx:Reference>
 <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Measures.V1.xml"> <edmx:Include Alias="Measures" Namespace="Org.OData.Measures.V1"/> </edmx:Reference>
 <edmx:Reference Uri="https://sap.github.io/odata-vocabularies/vocabularies/UI.xml"> <edmx:Include Alias="UI" Namespace="com.sap.vocabularies.UI.v1"/> </edmx:Reference>

 <edmx:DataServices>
    <Schema Namespace="Service" xmlns="http://docs.oasis-open.org/odata/ns/edm">
    <Annotation term="Core.Links">
       <Collection>
          <Record>
            <PropertyValue Property="rel" String="author"/>
            <PropertyValue Property="href" String="https://cap.cloud.sap"/>
          </Record>
       </Collection>
    </Annotation>
    <EntityContainer Name="EntityContainer">
      %ENTITY-SETS%
    </EntityContainer>
    %TYPE-DEFINITIONS%
    <EntityType Name="DraftAdministrativeData">
        <Key>
            <PropertyRef Name="DraftUUID"/>
        </Key>
        <Property Name="DraftUUID"              Type="Edm.Guid" Nullable="false"/>
        <Property Name="CreationDateTime"       Type="Edm.DateTimeOffset" Precision="7"/>
        <Property Name="CreatedByUser"          Type="Edm.String" MaxLength="256"/>
        <Property Name="DraftIsCreatedByMe"     Type="Edm.Boolean"/>
        <Property Name="LastChangeDateTime"     Type="Edm.DateTimeOffset" Precision="7"/>
        <Property Name="LastChangedByUser"      Type="Edm.String" MaxLength="256"/>
        <Property Name="InProcessByUser"        Type="Edm.String" MaxLength="256"/>
        <Property Name="DraftIsProcessedByMe"   Type="Edm.Boolean"/>
    </EntityType>
%ACTIONS%

%ANNOTATIONS%
   </Schema>
  </edmx:DataServices>
</edmx:Edmx>"##;
    meta.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_type_from_entity_maps_field_variants() {
        let mut ddic = DataDictonary::new();
        ddic.activate();
        let entity = ddic.active_entity("Entity").unwrap();

        let entity_type = EntityType::from_entity(entity);
        dbg!(&entity_type);
        assert!(
            entity_type
                .properties
                .iter()
                .any(|property| property.name == "name" && property.type_ == "Edm.String")
        );
    }

    #[test]
    fn field_annotations_cover_atom_value_list_and_measure() {
        let value_list_field = FieldSpec::string("code", "Code", 20)
            .computed()
            .immutable()
            .with_value_list(AtomValueList::entity_ref("PackageSet", "ID", Some("name")));
        let value_list_annotations = Annotations::from_field("Invoice", &value_list_field);

        assert!(value_list_annotations.annotations.iter().any(|annotation| {
            annotation.term == "Common.Label" && annotation.string.as_deref() == Some("Code")
        }));
        assert!(
            value_list_annotations
                .annotations
                .iter()
                .any(|annotation| annotation.term == "Org.OData.Core.V1.Computed")
        );
        assert!(
            value_list_annotations
                .annotations
                .iter()
                .any(|annotation| annotation.term == "Org.OData.Core.V1.Immutable")
        );
        assert!(
            value_list_annotations
                .annotations
                .iter()
                .any(|annotation| annotation.term == "Common.ValueList")
        );
        assert!(
            value_list_annotations
                .annotations
                .iter()
                .any(|annotation| annotation.term == "Common.ValueListWithFixedValues")
        );

        let measure = FieldSpec::Measure {
            name: "amount".into(),
            label: "Amount".into(),
            package: None,
            precision: Some(15),
            scale: Some(2),
            unit_field: "currency".into(),
            kind: MeasureKind::Currency,
            presentation: Default::default(),
        };
        let measure_annotations = Annotations::from_field("Invoice", &measure);

        assert!(measure_annotations.annotations.iter().any(|annotation| {
            annotation.term == "Org.OData.Measures.V1.ISOCurrency"
                && annotation.path.as_deref() == Some("currency")
        }));
    }

    #[test]
    fn parses_simple_expand_list() {
        let expand = parse_expand_items("DraftAdministrativeData,package");

        assert_eq!(expand.len(), 2);
        assert_eq!(expand[0].navigation_property, "DraftAdministrativeData");
        assert_eq!(expand[1].navigation_property, "package");
    }

    #[test]
    fn parses_expand_with_select_options() {
        let expand =
            parse_expand_items("DraftAdministrativeData($select=DraftUUID,InProcessByUser)");

        assert_eq!(expand.len(), 1);
        assert_eq!(expand[0].navigation_property, "DraftAdministrativeData");
        assert_eq!(
            expand[0].nested_query.select,
            vec!["DraftUUID".to_string(), "InProcessByUser".to_string()]
        );
    }

    #[test]
    fn parses_nested_expand_options() {
        let expand = parse_expand_items("_Entitites($expand=package($select=name))");

        assert_eq!(expand.len(), 1);
        assert_eq!(expand[0].navigation_property, "_Entitites");
        assert_eq!(expand[0].nested_query.expand.len(), 1);
        assert_eq!(
            expand[0].nested_query.expand[0].navigation_property,
            "package"
        );
        assert_eq!(
            expand[0].nested_query.expand[0].nested_query.select,
            vec!["name".to_string()]
        );
    }
}
