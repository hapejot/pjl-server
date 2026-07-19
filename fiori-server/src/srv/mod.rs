use std::collections::HashMap;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderName, header},
    response::IntoResponse,
    routing::get,
};
use odata::{
    path::parse_odata_path,
    query::{FilterExpr, ODataQuery},
};
use odata_yaml_store::{DataStore, DraftAction, EntityKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spec::{EntitySpec, data_dictionary::DataDictonary};
use tracing::trace;

use crate::AppState;

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
    pub fn from_entity(ddic: &DataDictonary, x: &EntitySpec) -> Self {
        let mut properties = Vec::new();

        properties.push(Property::new("ID", "Edm.Guid").nullable(false));
        properties.push(Property::new("IsActiveEntity", "Edm.Boolean").nullable(false));
        properties.push(Property::new("HasActiveEntity", "Edm.Boolean").nullable(false));
        properties.push(Property::new("HasDraftEntity", "Edm.Boolean").nullable(false));

        for f in x.fields() {
            properties.push(Property::new(f.name(), f.edm_type()));
        }
        let mut navigation_properties = Vec::new();
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
        for r in ddic.relationships().filter(|r| r.many() == x.name()) {
            navigation_properties.push(NavigationProperty::new(
                r.name(),
                format!("Service.{}", r.one()),
                Some(false),
            ));
        }
        for r in ddic.relationships().filter(|r| r.one() == x.name()) {
            navigation_properties.push(NavigationProperty::new(
                r.name(),
                format!("Collection(Service.{})", r.many()),
                Some(true),
            ));
        }
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NavigationProperty {
    #[serde(rename = "@Name")]
    pub name: String,

    #[serde(rename = "@Type")]
    pub type_: String,

    #[serde(rename = "@ContainsTarget", skip_serializing_if = "Option::is_none")]
    pub contains_target: Option<bool>,
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
        }
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
        for r in ddic.relationships().filter(|r| r.many() == x.name()) {
            navigation_property_bindings.push(NavigationPropertyBinding {
                path: r.one().into(),
                target: format!("Service.{}", r.one()),
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

        let mut main_data = Collection::new();
        main_data = main_data
            .add_record(Record::new("UI.DataField").add(PropertyValue::new("Value").path("ID")));
        for f in x.fields() {
            main_data = main_data.add_record(
                Record::new("UI.DataField").add(PropertyValue::new("Value").path(f.name())),
            );
        }
        let mut main_field_group = Annotation::new("UI.FieldGroup");
        main_field_group.qualifier = Some("Main".to_string());
        main_field_group = main_field_group.record(
            Record::new("UI.FieldGroupType").add(PropertyValue::new("Data").collection(main_data)),
        );
        main_annotations.annotations.push(main_field_group);

        annotations.push(main_annotations);

        for f in x.fields() {
            annotations.push(Self::from_field(x.name(), f));
        }
        annotations
    }

    fn from_field(name: &str, f: &spec::FieldSpec) -> Annotations {
        let target = format!("Service.{}/{}", name, f.name());
        let annotations = Vec::new();

        Annotations {
            target,
            annotations,
        }
    }
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
    pub fn record(mut self, record: Record) -> Self {
        self.record = Some(record);
        self
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
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
////////////////////////////////////////////////////////////////////////////////////////
/// router entry point
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/$metadata", get(metadata))
        .route(
            "/{*path}",
            get(get_handler).post(post_handler).patch(patch_handler),
        )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RequestKind {
    Get,
    Post,
    Patch,
}

fn build_query(params: &HashMap<String, String>) -> ODataQuery {
    ODataQuery {
        filter: params.get("$filter").map(|x| FilterExpr::parse(x).unwrap()),
        orderby: vec![],
        top: params.get("$top").map(|x| x.parse().unwrap()),
        skip: params.get("$skip").map(|x| x.parse().unwrap()),
        select: params
            .get("$select")
            .map(|x| x.split(",").map(|y| y.to_string()).collect()),
        expand: vec![],
        count: params.get("$count").is_some(),
        search: params.get("$search").cloned(),
    }
}

fn format_json_response(body: Value) -> ([(HeaderName, String); 2], String) {
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

async fn execute_request(
    kind: RequestKind,
    Path(path): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    mut body: Value,
) -> impl IntoResponse {
    let p = build_query(&params);
    let path_struct = parse_odata_path(&path).unwrap();
    trace!("name: {:?}: {:?}", path_struct, p);

    if kind == RequestKind::Post && path_struct.segments.len() >= 2 {
        let entity_segment = &path_struct.segments[path_struct.segments.len() - 2];
        let action_segment = &path_struct.segments[path_struct.segments.len() - 1];
        if !entity_segment.keys.is_empty() && action_segment.keys.is_empty() {
            let action_name = action_segment.name.rsplit('.').next().unwrap_or_default();
            let action = match action_name {
                "draftEdit" => Some(DraftAction::Edit),
                "draftActivate" => Some(DraftAction::Activate),
                "draftPrepare" => Some(DraftAction::Prepare),
                _ => None,
            };
            if let Some(action) = action {
                let key: EntityKey = entity_segment.into();
                let r = state
                    .store
                    .draft_action(&entity_segment.name, &key, action)
                    .unwrap();
                return format_json_response(r);
            }
        }
    }

    let mut j = json!([]);
    for s in &path_struct.segments {
        if s.keys.is_empty() {
            if kind == RequestKind::Post {
                trace!("creating record with body: {:?}", body);
                body["ID"] = json!(uuid::Uuid::new_v4().to_string());
                let created = state.store.create_entity(&s.name, &body).unwrap();
                j = created;
            } else {
                let r = state.store.get_collection(&s.name, &p).unwrap();
                let mut resp = json!({
                  "@odata.context": format!("/Service/$metadata#{path}"),
                  "value": r
                });
                if p.count {
                    resp["@odata.count"] = json!(state.store.count(&s.name, &p));
                }
                j = resp;
            }
        } else {
            let key: EntityKey = s.into();
            if kind == RequestKind::Patch {
                j = state.store.patch_entity(&s.name, &key, &body).unwrap();
            } else {
                j = state.store.read_entity(&s.name, &key).unwrap();
            }
        }
    }
    format_json_response(j)
}

async fn get_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
) -> impl IntoResponse {
    execute_request(RequestKind::Get, path, state, params, json!({})).await
}

async fn post_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
    payload: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = payload.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    execute_request(RequestKind::Post, path, state, params, body).await
}

async fn patch_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
    payload: Option<Json<Value>>,
) -> impl IntoResponse {
    let body = payload.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    execute_request(RequestKind::Patch, path, state, params, body).await
}

async fn index(State(state): State<AppState>) -> Json<Value> {
    trace!("index");
    let mut value: Vec<Value> = state
        .ddic
        .entities()
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
    let _entities: Vec<EntityType> = state
        .ddic
        .entities()
        .map(|x| EntityType::from_entity(&state.ddic, x))
        .collect();
    let _entity_sets: Vec<EntitySet> = state
        .ddic
        .apps()
        .map(|x| EntitySet::from_entity(&state.ddic, x.main_entity(&state.ddic)))
        .collect();
    let _annotations: Vec<Annotations> = state
        .ddic
        .entities()
        .map(|x| Annotations::from_entity(x))
        .flatten()
        .collect();
    let _sample_entity = sample_entity();
    let _sample_entity_set = sample_entity_set();
    let xml = sample_annotations();
    let sample_annotations: Annotations = quick_xml::de::from_str(&xml).unwrap();
    for a in &sample_annotations.annotations {
        trace!("A: {} {:?}", a.term, a.qualifier);
    }

    let meta = edmx_template();
    let mut type_defs = vec![]; //quick_xml::se::to_string(&sample_entity).unwrap()];
    for x in _entities {
        type_defs.push(quick_xml::se::to_string(&x).unwrap());
    }
    let mut entity_sets = vec![]; //quick_xml::se::to_string(&sample_entity_set).unwrap()];
    for x in _entity_sets {
        entity_sets.push(quick_xml::se::to_string(&x).unwrap());
    }
    let mut annotations = vec![];
    for x in _annotations {
        annotations.push(quick_xml::se::to_string(&x).unwrap());
    }
    for x in state.ddic.entities() {
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

fn sample_annotations() -> String {
    let xml = r##"  
   <Annotations Target="Service.EntityConfig">
    <Annotation Term="UI.SelectionFields">
      <Collection>
        <PropertyPath>SetName</PropertyPath>
      </Collection>
    </Annotation>
    <Annotation Term="UI.LineItem">
      <Collection>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="SetName" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="Title" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="TileDescription" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="TileIcon" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="HeaderTypeName" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="HeaderTypeNamePlural" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="SelectionFields" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="DefaultValues" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="HeaderFacets" />
        </Record>
        <Record Type="UI.DataField">
          <PropertyValue Property="Value" Path="DataPoints" />
        </Record>
      </Collection>
    </Annotation>
    <Annotation Term="UI.HeaderInfo">
      <Record Type="UI.HeaderInfoType">
        <PropertyValue Property="TypeName" String="EntityConfig" />
        <PropertyValue Property="TypeNamePlural" String="EntityConfigs" />
        <PropertyValue Property="Title">
          <Record Type="UI.DataField">
            <PropertyValue Property="Value" Path="SetName" />
          </Record>
        </PropertyValue>
      </Record>
    </Annotation>
    <Annotation Term="UI.HeaderFacets">
      <Collection></Collection>
    </Annotation>
    <Annotation Term="UI.Facets">
      <Collection>
        <Record Type="UI.CollectionFacet">
          <PropertyValue Property="Label" String="Basic" />
          <PropertyValue Property="ID" String="BasicSection" />
          <PropertyValue Property="Facets">
            <Collection>
              <Record Type="UI.ReferenceFacet">
                <PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#Basic" />
                <PropertyValue Property="Label" String="Basic" />
              </Record>
            </Collection>
          </PropertyValue>
        </Record>
        <Record Type="UI.CollectionFacet">
          <PropertyValue Property="Label" String="Tile" />
          <PropertyValue Property="ID" String="TileSection" />
          <PropertyValue Property="Facets">
            <Collection>
              <Record Type="UI.ReferenceFacet">
                <PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#Tile" />
                <PropertyValue Property="Label" String="Tile" />
              </Record>
            </Collection>
          </PropertyValue>
        </Record>
        <Record Type="UI.CollectionFacet">
          <PropertyValue Property="Label" String="Header" />
          <PropertyValue Property="ID" String="HeaderSection" />
          <PropertyValue Property="Facets">
            <Collection>
              <Record Type="UI.ReferenceFacet">
                <PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#Header" />
                <PropertyValue Property="Label" String="Header" />
              </Record>
            </Collection>
          </PropertyValue>
        </Record>
        <Record Type="UI.ReferenceFacet">
          <PropertyValue Property="Label" String="Fields" />
          <PropertyValue Property="ID" String="FieldsSection" />
          <PropertyValue Property="Target" AnnotationPath="Fields/@UI.LineItem" />
        </Record>
        <Record Type="UI.ReferenceFacet">
          <PropertyValue Property="Label" String="Facets" />
          <PropertyValue Property="ID" String="FacetsSection" />
          <PropertyValue Property="Target" AnnotationPath="Facets/@UI.LineItem" />
        </Record>
        <Record Type="UI.ReferenceFacet">
          <PropertyValue Property="Label" String="Navigations" />
          <PropertyValue Property="ID" String="NavigationsSection" />
          <PropertyValue Property="Target" AnnotationPath="Navigations/@UI.LineItem" />
        </Record>
        <Record Type="UI.ReferenceFacet">
          <PropertyValue Property="Label" String="TableFacets" />
          <PropertyValue Property="ID" String="TableFacetsSection" />
          <PropertyValue Property="Target" AnnotationPath="TableFacets/@UI.LineItem" />
        </Record>
      </Collection>
    </Annotation>
    <Annotation Term="UI.FieldGroup" Qualifier="Basic">
      <Record Type="UI.FieldGroupType">
        <PropertyValue Property="Data">
          <Collection>
            <Record Type="UI.DataField">
              <PropertyValue Property="Value" Path="SetName" />
            </Record>
            <Record Type="UI.DataField">
              <PropertyValue Property="Value" Path="TypeName" />
            </Record>
            <Record Type="UI.DataField">
              <PropertyValue Property="Value" Path="SelectionFields" />
            </Record>
          </Collection>
        </PropertyValue>
      </Record>
    </Annotation>
    <Annotation Term="UI.FieldGroup" Qualifier="Tile">
      <Record Type="UI.FieldGroupType">
        <PropertyValue Property="Data">
          <Collection>
            <Record Type="UI.DataField">
              <PropertyValue Property="Value" Path="Title" />
            </Record>
            <Record Type="UI.DataField">
              <PropertyValue Property="Value" Path="TileDescription" />
            </Record>
            <Record Type="UI.DataField">
              <PropertyValue Property="Value" Path="TileIcon" />
            </Record>
          </Collection>
        </PropertyValue>
      </Record>
    </Annotation>
    <Annotation Term="UI.FieldGroup" Qualifier="Header">
      <Record Type="UI.FieldGroupType">
        <PropertyValue Property="Data">
          <Collection>
            <Record Type="UI.DataField">
              <PropertyValue Property="Value" Path="HeaderTypeName" />
            </Record>
            <Record Type="UI.DataField">
              <PropertyValue Property="Value" Path="HeaderTypeNamePlural" />
            </Record>
            <Record Type="UI.DataFieldWithIntentBasedNavigation">
              <PropertyValue Property="Value" Path="HeaderTitlePath" />
              <PropertyValue Property="SemanticObject" String="EntityFields" />
              <PropertyValue Property="Action" String="display" />
              <PropertyValue Property="Mapping">
                <Collection>
                  <Record Type="Common.SemanticObjectMappingType">
                    <PropertyValue Property="LocalProperty" PropertyPath="HeaderTitlePath" />
                    <PropertyValue Property="SemanticObjectProperty" String="ID" />
                  </Record>
                </Collection>
              </PropertyValue>
            </Record>
            <Record Type="UI.DataFieldWithIntentBasedNavigation">
              <PropertyValue Property="Value" Path="HeaderDescriptionPath" />
              <PropertyValue Property="SemanticObject" String="EntityFields" />
              <PropertyValue Property="Action" String="display" />
              <PropertyValue Property="Mapping">
                <Collection>
                  <Record Type="Common.SemanticObjectMappingType">
                    <PropertyValue Property="LocalProperty" PropertyPath="HeaderDescriptionPath" />
                    <PropertyValue Property="SemanticObjectProperty" String="ID" />
                  </Record>
                </Collection>
              </PropertyValue>
            </Record>
          </Collection>
        </PropertyValue>
      </Record>
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderTitlePath">
    <Annotation Term="Common.SemanticObject" String="EntityFields" />
    <Annotation Term="Common.SemanticObjectMapping">
      <Collection>
        <Record>
          <PropertyValue Property="LocalProperty" PropertyPath="HeaderTitlePath" />
          <PropertyValue Property="SemanticObjectProperty" String="ID" />
        </Record>
      </Collection>
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderDescriptionPath">
    <Annotation Term="Common.SemanticObject" String="EntityFields" />
    <Annotation Term="Common.SemanticObjectMapping">
      <Collection>
        <Record>
          <PropertyValue Property="LocalProperty" PropertyPath="HeaderDescriptionPath" />
          <PropertyValue Property="SemanticObjectProperty" String="ID" />
        </Record>
      </Collection>
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityContainer/EntityConfigs">
    <Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions">
      <Record>
        <PropertyValue Property="Updatable" Bool="true" />
      </Record>
    </Annotation>
    <Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions">
      <Record Type="Capabilities.InsertRestrictionsType">
        <PropertyValue Property="NonInsertableProperties">
          <Collection>
            <PropertyPath>ID</PropertyPath>
            <PropertyPath>TypeName</PropertyPath>
            <PropertyPath>IsActiveEntity</PropertyPath>
            <PropertyPath>HasActiveEntity</PropertyPath>
            <PropertyPath>HasDraftEntity</PropertyPath>
          </Collection>
        </PropertyValue>
      </Record>
    </Annotation>
    <Annotation Term="Common.DraftRoot">
      <Record Type="Common.DraftRootType">
        <PropertyValue Property="ActivationAction" String="Service.draftActivate" />
        <PropertyValue Property="EditAction" String="Service.draftEdit" />
        <PropertyValue Property="PreparationAction" String="Service.draftPrepare" />
      </Record>
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityConfig/ID">
    <Annotation Term="Common.Label" String="ID" />
    <Annotation Term="UI.Hidden" Bool="true" />
    <Annotation Term="Org.OData.Core.V1.Computed" Bool="true" />
    <Annotation Term="Common.Text" Path="SetName">
      <Annotation Term="UI.TextArrangement" EnumMember="UI.TextArrangementType/TextOnly" />
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityConfig/SetName">
    <Annotation Term="Common.Label" String="Set Name" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/TypeName">
    <Annotation Term="Common.Label" String="Type Name" />
    <Annotation Term="Org.OData.Core.V1.Computed" Bool="true" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/Title">
    <Annotation Term="Common.Label" String="Title" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/TileDescription">
    <Annotation Term="Common.Label" String="Tile Description" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/TileIcon">
    <Annotation Term="Common.Label" String="Tile Icon" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderTypeName">
    <Annotation Term="Common.Label" String="Header Type Name" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderTypeNamePlural">
    <Annotation Term="Common.Label" String="Header Type Name Plural" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/SelectionFields">
    <Annotation Term="Common.Label" String="Selection Fields" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/DefaultValues">
    <Annotation Term="Common.Label" String="Default Values" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderFacets">
    <Annotation Term="Common.Label" String="Header Facets" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/DataPoints">
    <Annotation Term="Common.Label" String="Data Points" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderTitlePath">
    <Annotation Term="Common.Label" String="Header Title Field" />
    <Annotation Term="UI.Hidden" Bool="true" />
    <Annotation Term="Common.ValueList">
      <Record Type="Common.ValueListType">
        <PropertyValue Property="CollectionPath" String="EntityFields" />
        <PropertyValue Property="Parameters">
          <Collection>
            <Record Type="Common.ValueListParameterOut">
              <PropertyValue Property="LocalDataProperty" PropertyPath="HeaderTitlePath" />
              <PropertyValue Property="ValueListProperty" String="ID" />
            </Record>
            <Record Type="Common.ValueListParameterDisplayOnly">
              <PropertyValue Property="ValueListProperty" String="FieldName" />
            </Record>
            <Record Type="Common.ValueListParameterIn">
              <PropertyValue Property="LocalDataProperty" PropertyPath="ID" />
              <PropertyValue Property="ValueListProperty" String="ConfigID" />
            </Record>
          </Collection>
        </PropertyValue>
      </Record>
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderDescriptionPath">
    <Annotation Term="Common.Label" String="Header Description Field" />
    <Annotation Term="UI.Hidden" Bool="true" />
    <Annotation Term="Common.ValueList">
      <Record Type="Common.ValueListType">
        <PropertyValue Property="CollectionPath" String="EntityFields" />
        <PropertyValue Property="Parameters">
          <Collection>
            <Record Type="Common.ValueListParameterOut">
              <PropertyValue Property="LocalDataProperty" PropertyPath="HeaderDescriptionPath" />
              <PropertyValue Property="ValueListProperty" String="ID" />
            </Record>
            <Record Type="Common.ValueListParameterDisplayOnly">
              <PropertyValue Property="ValueListProperty" String="FieldName" />
            </Record>
            <Record Type="Common.ValueListParameterIn">
              <PropertyValue Property="LocalDataProperty" PropertyPath="ID" />
              <PropertyValue Property="ValueListProperty" String="ConfigID" />
            </Record>
          </Collection>
        </PropertyValue>
      </Record>
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityConfig/IsActiveEntity">
    <Annotation Term="Org.OData.Core.V1.Computed" Bool="true" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/HasActiveEntity">
    <Annotation Term="Org.OData.Core.V1.Computed" Bool="true" />
  </Annotations>
  <Annotations Target="Service.EntityConfig/HasDraftEntity">
    <Annotation Term="Org.OData.Core.V1.Computed" Bool="true" />
  </Annotations>
  <Annotations Target="Service.EntityConfig">
    <Annotation Term="UI.Identification">
      <Collection>
        <Record Type="UI.DataFieldForAction">
          <PropertyValue Property="Action" String="Service.publishConfig" />
          <PropertyValue Property="Label" String="Konfiguration publizieren" />
        </Record>
      </Collection>
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderTitlePath">
    <Annotation Term="Common.ValueList">
      <Record Type="Common.ValueListType">
        <PropertyValue Property="CollectionPath" String="EntityFields" />
        <PropertyValue Property="Parameters">
          <Collection>
            <Record Type="Common.ValueListParameterOut">
              <PropertyValue Property="LocalDataProperty" PropertyPath="HeaderTitlePath" />
              <PropertyValue Property="ValueListProperty" String="ID" />
            </Record>
            <Record Type="Common.ValueListParameterDisplayOnly">
              <PropertyValue Property="ValueListProperty" String="FieldName" />
            </Record>
            <Record Type="Common.ValueListParameterIn">
              <PropertyValue Property="LocalDataProperty" PropertyPath="ID" />
              <PropertyValue Property="ValueListProperty" String="ConfigID" />
            </Record>
          </Collection>
        </PropertyValue>
      </Record>
    </Annotation>
  </Annotations>
  <Annotations Target="Service.EntityConfig/HeaderDescriptionPath">
    <Annotation Term="Common.ValueList">
      <Record Type="Common.ValueListType">
        <PropertyValue Property="CollectionPath" String="EntityFields" />
        <PropertyValue Property="Parameters">
          <Collection>
            <Record Type="Common.ValueListParameterOut">
              <PropertyValue Property="LocalDataProperty" PropertyPath="HeaderDescriptionPath" />
              <PropertyValue Property="ValueListProperty" String="ID" />
            </Record>
            <Record Type="Common.ValueListParameterDisplayOnly">
              <PropertyValue Property="ValueListProperty" String="FieldName" />
            </Record>
            <Record Type="Common.ValueListParameterIn">
              <PropertyValue Property="LocalDataProperty" PropertyPath="ID" />
              <PropertyValue Property="ValueListProperty" String="ConfigID" />
            </Record>
          </Collection>
        </PropertyValue>
      </Record>
    </Annotation>
  </Annotations>"##;
    xml.into()
}

fn sample_entity_set() -> EntitySet {
    let sample_entity_set = EntitySet {
        name: "EntityConfigs".to_string(),
        entity_type: "Service.EntityConfig".to_string(),
        navigation_property_bindings: vec![
            NavigationPropertyBinding {
                path: "Fields".to_string(),
                target: "EntityFields".to_string(),
            },
            NavigationPropertyBinding {
                path: "Facets".to_string(),
                target: "EntityFacets".to_string(),
            },
            NavigationPropertyBinding {
                path: "Navigations".to_string(),
                target: "EntityNavigations".to_string(),
            },
            NavigationPropertyBinding {
                path: "TableFacets".to_string(),
                target: "EntityTableFacets".to_string(),
            },
            NavigationPropertyBinding {
                path: "_HeaderTitlePath".to_string(),
                target: "EntityFields".to_string(),
            },
            NavigationPropertyBinding {
                path: "_HeaderDescPath".to_string(),
                target: "EntityFields".to_string(),
            },
            NavigationPropertyBinding {
                path: "SiblingEntity".to_string(),
                target: "EntityConfigs".to_string(),
            },
            NavigationPropertyBinding {
                path: "DraftAdministrativeData".to_string(),
                target: "DraftAdministrativeData".to_string(),
            },
        ],
    };
    sample_entity_set
}

fn sample_entity() -> EntityType {
    let sample_entity = EntityType {
        name: "EntityConfig".to_string(),
        key: Key::new(&["ID", "IsActiveEntity"]),
        properties: vec![
            Property::new("ID", "Edm.Guid").nullable(false),
            Property::new("Name", "Edm.String")
                .nullable(false)
                .max_length(40),
            Property::new("Title", "Edm.String").max_length(80),
            Property::new("Plural", "Edm.String").max_length(80),
            Property::new("IsActiveEntity", "Edm.Boolean").nullable(false),
            Property::new("HasActiveEntity", "Edm.Boolean").nullable(false),
            Property::new("HasDraftEntity", "Edm.Boolean").nullable(false),
        ],
        navigation_properties: vec![
            NavigationProperty::new("Fields", "Collection(Service.EntityField)", None),
            NavigationProperty::new("Facets", "Collection(Service.EntityFacet)", None),
            NavigationProperty::new("Navigations", "Collection(Service.EntityNavigation)", None),
            NavigationProperty::new("TableFacets", "Collection(Service.EntityTableFacet)", None),
            NavigationProperty::new("_Name", "Service.EntityField", None),
            NavigationProperty::new("_Title", "Service.EntityField", None),
            NavigationProperty::new("SiblingEntity", "Service.EntityConfig", None),
            NavigationProperty::new(
                "DraftAdministrativeData",
                "Service.DraftAdministrativeData",
                Some(true),
            ),
        ],
    };
    sample_entity
}
