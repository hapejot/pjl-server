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
use odata_yaml_store::DataStore;
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
            "_DraftAdministrativeData",
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
                Some(false),
            ));
        }
        Self {
            name: x.name().into(),
            key: Key::new(&["ID"]),
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
        for f in x.fields() {
            annotations.push(Self::from_field(x.name(), f));
        }
        annotations
    }

    fn from_field(name: &str, f: &spec::FieldSpec) -> Annotations {
        let target = format!("Service.{}{}", name, f.name());
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

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Record {
    #[serde(rename = "@Type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    #[serde(rename = "PropertyValue", default)]
    pub property_values: Vec<PropertyValue>,
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
////////////////////////////////////////////////////////////////////////////////////////
/// router entry point
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index))
        .route("/$metadata", get(metadata))
        .route("/{*path}", get(handler).post(handler).patch(handler))
}

async fn handler(
    Path(path): Path<String>,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let p = ODataQuery {
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
    };
    let path_struct = parse_odata_path(&path).unwrap();
    trace!("name: {:?}: {:?}", path_struct, p);
    let mut j = json!([]);
    for s in path_struct.segments {
        if s.keys.len() == 0 {
            let r = state.store.get_collection(&s.name, &p, None).unwrap();
            j = json!({"@odata.context":format!("/Service/$metadata#{path}"),
                   // "@odata.count": n,
                   "value": r});
        } else {
            let key = odata_yaml_store::EntityKey {
                fields: s.keys_as_map(),
                is_active_entity: s.is_active_entity(),
            };

            j = state.store.read_entity(&s.name, &key).unwrap();
        }
    }
    // let n = r.as_array().map_or_else(|| 0, |x| x.len());

    (
        [
            (
                header::CONTENT_TYPE,
                "application/json;odata.metadata=minimal;charset=utf-8",
            ),
            (HeaderName::from_static("odata-version"), "4.0"),
        ],
        j.to_string(),
    )
}
async fn index() -> Json<Value> {
    trace!("index");
    Json(json!({
      "@odata.context": "/Service/$metadata",
      "value": [
        { "name": "EntityConfigs",       "url": "EntityConfigs" },
        { "name": "EntitySet",           "url": "EntitySet"},
        { "name": "EntityFields",        "url": "EntityFields" },
        { "name": "EntityFacets",        "url": "EntityFacets" },
        { "name": "EntityNavigations",   "url": "EntityNavigations" },
        { "name": "EntityRelationships", "url": "EntityRelationships" },
        { "name": "EntityTableFacets",   "url": "EntityTableFacets" },
        { "name": "FieldValueLists",     "url": "FieldValueLists" },
        { "name": "FieldValueListItems", "url": "FieldValueListItems" }
      ]
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
        .entities()
        .map(|x| EntitySet::from_entity(&state.ddic, x))
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

    let mut actions = vec![];
    for x in state.ddic.apps() {
        let entity_name = x.main_entity(&state.ddic).name();
        actions.push(format!(
            r##"<Action Name="draftEdit" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="Service.{}"/><Parameter Name="PreserveChanges" Type="Edm.Boolean"/><ReturnType Type="Service.{}"/></Action>"##,
            entity_name, entity_name
        ));
        actions.push(format!(
            r##"<Action Name="draftActivate" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="Service.{}"/><ReturnType Type="Service.{}"/></Action>"##,
            entity_name, entity_name
        ));
        actions.push(format!(
            r##"<Action Name="draftPrepare" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="Service.{}"/><Parameter Name="SideEffectsQualifier" Type="Edm.String"/><ReturnType Type="Service.{}"/></Action>"##,
            entity_name, entity_name
        ));
        actions.push(format!(
            r##"<Action Name="publishConfig" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="Service.{}"/><ReturnType Type="Service.{}"/></Action>"##,
            entity_name, entity_name
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

  <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Capabilities.V1.xml">
    <edmx:Include Namespace="Org.OData.Capabilities.V1" Alias="Capabilities"/>
  </edmx:Reference>
  <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Core.V1.xml">
    <edmx:Include Namespace="Org.OData.Core.V1" Alias="Core"/>
  </edmx:Reference>
  <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Measures.V1.xml">
    <edmx:Include Namespace="Org.OData.Measures.V1" Alias="Measures"/>
  </edmx:Reference>
  <edmx:Reference Uri="https://sap.github.io/odata-vocabularies/vocabularies/UI.xml">
    <edmx:Include Namespace="com.sap.vocabularies.UI.v1" Alias="UI"/>
  </edmx:Reference>
  <edmx:Reference Uri="https://sap.github.io/odata-vocabularies/vocabularies/Common.xml">
    <edmx:Include Namespace="com.sap.vocabularies.Common.v1" Alias="Common"/>
  </edmx:Reference>

  <edmx:DataServices>
    <Schema Namespace="Service" xmlns="http://docs.oasis-open.org/odata/ns/edm">
    %TYPE-DEFINITIONS%
<EntityType Name="EntityField"><Key><PropertyRef Name="ID"/><PropertyRef Name="IsActiveEntity"/></Key><Property Name="ID"                Type="Edm.Guid" Nullable="false"/><Property Name="SetName"           Type="Edm.String" MaxLength="40"/><Property Name="FieldName"         Type="Edm.String" MaxLength="40"/><Property Name="Label"             Type="Edm.String" MaxLength="80"/><Property Name="EdmType"           Type="Edm.String" MaxLength="30"/><Property Name="MaxLength"         Type="Edm.Int32"/><Property Name="Precision"         Type="Edm.Int32"/><Property Name="Scale"             Type="Edm.Int32"/><Property Name="IsImmutable"       Type="Edm.Boolean"/><Property Name="IsComputed"        Type="Edm.Boolean"/><Property Name="ReferencesEntity"  Type="Edm.String" MaxLength="40"/><Property Name="PreferDialog"      Type="Edm.Boolean"/><Property Name="TextPath"          Type="Edm.String" MaxLength="80"/><Property Name="DefaultValue"      Type="Edm.String" MaxLength="120"/><Property Name="SortOrder"         Type="Edm.Int32"/><Property Name="ShowInLineItem"    Type="Edm.Boolean"/><Property Name="LineItemImportance" Type="Edm.String" MaxLength="10"/><Property Name="LineItemLabel"     Type="Edm.String" MaxLength="80"/><Property Name="LineItemCriticalityPath" Type="Edm.String" MaxLength="40"/><Property Name="LineItemSemanticObject" Type="Edm.String" MaxLength="40"/><Property Name="Searchable"        Type="Edm.Boolean"/><Property Name="FormGroup"         Type="Edm.String" MaxLength="40"/><Property Name="ConfigID"          Type="Edm.Guid"/><Property Name="ValueSource"       Type="Edm.Guid"/><Property Name="IsActiveEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="true"/><Property Name="HasActiveEntity"  Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><Property Name="HasDraftEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><NavigationProperty Name="_Config" Type="Service.EntityConfig"/><NavigationProperty Name="_ValueList" Type="Service.FieldValueList"/><NavigationProperty Name="SiblingEntity" Type="Service.EntityField"/><NavigationProperty Name="DraftAdministrativeData" Type="Service.DraftAdministrativeData" ContainsTarget="true"/></EntityType>
<EntityType Name="EntityFacet"><Key><PropertyRef Name="ID"/><PropertyRef Name="IsActiveEntity"/></Key><Property Name="ID"                Type="Edm.Guid" Nullable="false"/><Property Name="SectionLabel"      Type="Edm.String" MaxLength="80"/><Property Name="SectionId"         Type="Edm.String" MaxLength="40"/><Property Name="FieldGroupQualifier" Type="Edm.String" MaxLength="40"/><Property Name="FieldGroupLabel"   Type="Edm.String" MaxLength="80"/><Property Name="FieldGroupFields"  Type="Edm.String" MaxLength="500"/><Property Name="SortOrder"         Type="Edm.Int32"/><Property Name="ConfigID"          Type="Edm.Guid"/><Property Name="IsActiveEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="true"/><Property Name="HasActiveEntity"  Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><Property Name="HasDraftEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><NavigationProperty Name="_Config" Type="Service.EntityConfig"/><NavigationProperty Name="SiblingEntity" Type="Service.EntityFacet"/><NavigationProperty Name="DraftAdministrativeData" Type="Service.DraftAdministrativeData" ContainsTarget="true"/></EntityType>
<EntityType Name="EntityNavigation"><Key><PropertyRef Name="ID"/><PropertyRef Name="IsActiveEntity"/></Key><Property Name="ID"                Type="Edm.Guid" Nullable="false"/><Property Name="SetName"           Type="Edm.String" MaxLength="40"/><Property Name="NavName"           Type="Edm.String" MaxLength="40"/><Property Name="TargetType"        Type="Edm.String" MaxLength="40"/><Property Name="TargetSet"         Type="Edm.String" MaxLength="40"/><Property Name="IsCollection"      Type="Edm.Boolean"/><Property Name="ForeignKey"        Type="Edm.String" MaxLength="40"/><Property Name="SortOrder"         Type="Edm.Int32"/><Property Name="ConfigID"          Type="Edm.Guid"/><Property Name="IsActiveEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="true"/><Property Name="HasActiveEntity"  Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><Property Name="HasDraftEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><NavigationProperty Name="_Config" Type="Service.EntityConfig"/><NavigationProperty Name="SiblingEntity" Type="Service.EntityNavigation"/><NavigationProperty Name="DraftAdministrativeData" Type="Service.DraftAdministrativeData" ContainsTarget="true"/></EntityType>
<EntityType Name="EntityRelationship"><Key><PropertyRef Name="ID"/><PropertyRef Name="IsActiveEntity"/></Key><Property Name="ID"                Type="Edm.Guid" Nullable="false"/><Property Name="Name"              Type="Edm.String" MaxLength="80"/><Property Name="OneEntity"         Type="Edm.String" MaxLength="40"/><Property Name="OneNavName"        Type="Edm.String" MaxLength="40"/><Property Name="ManyEntity"        Type="Edm.String" MaxLength="40"/><Property Name="ManyNavName"       Type="Edm.String" MaxLength="40"/><Property Name="IsComposition"     Type="Edm.Boolean"/><Property Name="FkField"           Type="Edm.String" MaxLength="40"/><Property Name="FkLabel"           Type="Edm.String" MaxLength="80"/><Property Name="FkFormGroup"       Type="Edm.String" MaxLength="40"/><Property Name="ConditionType"     Type="Edm.String" MaxLength="20"/><Property Name="ConditionReference" Type="Edm.String" MaxLength="80"/><Property Name="IsActiveEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="true"/><Property Name="HasActiveEntity"  Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><Property Name="HasDraftEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><NavigationProperty Name="SiblingEntity" Type="Service.EntityRelationship"/><NavigationProperty Name="DraftAdministrativeData" Type="Service.DraftAdministrativeData" ContainsTarget="true"/></EntityType>
<EntityType Name="EntityTableFacet"><Key><PropertyRef Name="ID"/><PropertyRef Name="IsActiveEntity"/></Key><Property Name="ID"                Type="Edm.Guid" Nullable="false"/><Property Name="SetName"           Type="Edm.String" MaxLength="40"/><Property Name="FacetLabel"        Type="Edm.String" MaxLength="80"/><Property Name="FacetId"           Type="Edm.String" MaxLength="40"/><Property Name="NavigationProperty" Type="Edm.String" MaxLength="40"/><Property Name="SortOrder"         Type="Edm.Int32"/><Property Name="ConfigID"          Type="Edm.Guid"/><Property Name="IsActiveEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="true"/><Property Name="HasActiveEntity"  Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><Property Name="HasDraftEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><NavigationProperty Name="_Config" Type="Service.EntityConfig"/><NavigationProperty Name="SiblingEntity" Type="Service.EntityTableFacet"/><NavigationProperty Name="DraftAdministrativeData" Type="Service.DraftAdministrativeData" ContainsTarget="true"/></EntityType>
<EntityType Name="FieldValueList"><Key><PropertyRef Name="ID"/><PropertyRef Name="IsActiveEntity"/></Key><Property Name="ID"                Type="Edm.Guid" Nullable="false"/><Property Name="ListName"          Type="Edm.String" MaxLength="40"/><Property Name="Description"       Type="Edm.String" MaxLength="120"/><Property Name="IsActiveEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="true"/><Property Name="HasActiveEntity"  Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><Property Name="HasDraftEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><NavigationProperty Name="Items" Type="Collection(Service.FieldValueListItem)"/><NavigationProperty Name="_ReferencingFields" Type="Collection(Service.EntityField)"/><NavigationProperty Name="SiblingEntity" Type="Service.FieldValueList"/><NavigationProperty Name="DraftAdministrativeData" Type="Service.DraftAdministrativeData" ContainsTarget="true"/></EntityType>
<EntityType Name="FieldValueListItem"><Key><PropertyRef Name="ID"/><PropertyRef Name="IsActiveEntity"/></Key><Property Name="ID"                Type="Edm.Guid" Nullable="false"/><Property Name="Code"              Type="Edm.String" MaxLength="40"/><Property Name="Description"       Type="Edm.String" MaxLength="120"/><Property Name="SortOrder"         Type="Edm.Int32"/><Property Name="ListID"            Type="Edm.Guid"/><Property Name="IsActiveEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="true"/><Property Name="HasActiveEntity"  Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><Property Name="HasDraftEntity"   Type="Edm.Boolean" Nullable="false" DefaultValue="false"/><NavigationProperty Name="_List" Type="Service.FieldValueList"/><NavigationProperty Name="SiblingEntity" Type="Service.FieldValueListItem"/><NavigationProperty Name="DraftAdministrativeData" Type="Service.DraftAdministrativeData" ContainsTarget="true"/></EntityType>
<EntityType Name="DraftAdministrativeData"><Key><PropertyRef Name="DraftUUID"/></Key><Property Name="DraftUUID"              Type="Edm.Guid" Nullable="false"/><Property Name="CreationDateTime"       Type="Edm.DateTimeOffset" Precision="7"/><Property Name="CreatedByUser"          Type="Edm.String" MaxLength="256"/><Property Name="DraftIsCreatedByMe"     Type="Edm.Boolean"/><Property Name="LastChangeDateTime"     Type="Edm.DateTimeOffset" Precision="7"/><Property Name="LastChangedByUser"      Type="Edm.String" MaxLength="256"/><Property Name="InProcessByUser"        Type="Edm.String" MaxLength="256"/><Property Name="DraftIsProcessedByMe"   Type="Edm.Boolean"/></EntityType>
%ACTIONS%
<EntityContainer Name="EntityContainer">
      %ENTITY-SETS%
<EntitySet Name="EntityFields" EntityType="Service.EntityField"><NavigationPropertyBinding Path="_Config" Target="EntityConfigs"/><NavigationPropertyBinding Path="_ValueList" Target="FieldValueLists"/><NavigationPropertyBinding Path="SiblingEntity" Target="EntityFields"/><NavigationPropertyBinding Path="DraftAdministrativeData" Target="DraftAdministrativeData"/></EntitySet>
<EntitySet Name="EntityFacets" EntityType="Service.EntityFacet"><NavigationPropertyBinding Path="_Config" Target="EntityConfigs"/><NavigationPropertyBinding Path="SiblingEntity" Target="EntityFacets"/><NavigationPropertyBinding Path="DraftAdministrativeData" Target="DraftAdministrativeData"/></EntitySet>
<EntitySet Name="EntityNavigations" EntityType="Service.EntityNavigation"><NavigationPropertyBinding Path="_Config" Target="EntityConfigs"/><NavigationPropertyBinding Path="SiblingEntity" Target="EntityNavigations"/><NavigationPropertyBinding Path="DraftAdministrativeData" Target="DraftAdministrativeData"/></EntitySet>
<EntitySet Name="EntityRelationships" EntityType="Service.EntityRelationship"><NavigationPropertyBinding Path="SiblingEntity" Target="EntityRelationships"/><NavigationPropertyBinding Path="DraftAdministrativeData" Target="DraftAdministrativeData"/></EntitySet>
<EntitySet Name="EntityTableFacets" EntityType="Service.EntityTableFacet"><NavigationPropertyBinding Path="_Config" Target="EntityConfigs"/><NavigationPropertyBinding Path="SiblingEntity" Target="EntityTableFacets"/><NavigationPropertyBinding Path="DraftAdministrativeData" Target="DraftAdministrativeData"/></EntitySet>
<EntitySet Name="FieldValueLists" EntityType="Service.FieldValueList"><NavigationPropertyBinding Path="Items" Target="FieldValueListItems"/><NavigationPropertyBinding Path="_ReferencingFields" Target="EntityFields"/><NavigationPropertyBinding Path="SiblingEntity" Target="FieldValueLists"/><NavigationPropertyBinding Path="DraftAdministrativeData" Target="DraftAdministrativeData"/></EntitySet>
<EntitySet Name="FieldValueListItems" EntityType="Service.FieldValueListItem"><NavigationPropertyBinding Path="_List" Target="FieldValueLists"/><NavigationPropertyBinding Path="SiblingEntity" Target="FieldValueListItems"/><NavigationPropertyBinding Path="DraftAdministrativeData" Target="DraftAdministrativeData"/></EntitySet>
        <EntitySet Name="DraftAdministrativeData" EntityType="Service.DraftAdministrativeData"/>
      </EntityContainer>
      %ANNOTATIONS%
<Annotations Target="Service.EntityField"><Annotation Term="UI.SelectionFields"><Collection></Collection></Annotation><Annotation Term="UI.LineItem"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Label"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="EdmType"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="MaxLength"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Precision"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Scale"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="IsImmutable"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="IsComputed"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ReferencesEntity"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="PreferDialog"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="TextPath"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="DefaultValue"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ShowInLineItem"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="LineItemImportance"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="LineItemLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="LineItemCriticalityPath"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="LineItemSemanticObject"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Searchable"/></Record><Record Type="UI.DataFieldWithIntentBasedNavigation"><PropertyValue Property="Value" Path="FormGroup"/><PropertyValue Property="SemanticObject" String="EntityFacets"/><PropertyValue Property="Action" String="display"/><PropertyValue Property="Mapping"><Collection><Record Type="Common.SemanticObjectMappingType"><PropertyValue Property="LocalProperty" PropertyPath="FormGroup"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></PropertyValue></Record></Collection></Annotation><Annotation Term="UI.HeaderInfo"><Record Type="UI.HeaderInfoType"><PropertyValue Property="TypeName" String="EntityField"/><PropertyValue Property="TypeNamePlural" String="EntityFields"/><PropertyValue Property="Title"><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldName"/></Record></PropertyValue></Record></Annotation><Annotation Term="UI.HeaderFacets"><Collection></Collection></Annotation><Annotation Term="UI.Facets"><Collection><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="FieldProps"/><PropertyValue Property="ID" String="FieldPropsSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#FieldProps"/><PropertyValue Property="Label" String="FieldProps"/></Record></Collection></PropertyValue></Record><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="LineItemProps"/><PropertyValue Property="ID" String="LineItemPropsSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#LineItemProps"/><PropertyValue Property="Label" String="LineItemProps"/></Record></Collection></PropertyValue></Record></Collection></Annotation><Annotation Term="UI.FieldGroup" Qualifier="FieldProps"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SetName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Label"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="EdmType"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="MaxLength"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Precision"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Scale"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="IsImmutable"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="IsComputed"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ReferencesEntity"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="PreferDialog"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="TextPath"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="DefaultValue"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Searchable"/></Record><Record Type="UI.DataFieldWithIntentBasedNavigation"><PropertyValue Property="Value" Path="FormGroup"/><PropertyValue Property="SemanticObject" String="EntityFacets"/><PropertyValue Property="Action" String="display"/><PropertyValue Property="Mapping"><Collection><Record Type="Common.SemanticObjectMappingType"><PropertyValue Property="LocalProperty" PropertyPath="FormGroup"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></PropertyValue></Record><Record Type="UI.DataFieldWithIntentBasedNavigation"><PropertyValue Property="Value" Path="ValueSource"/><PropertyValue Property="SemanticObject" String="FieldValueLists"/><PropertyValue Property="Action" String="display"/><PropertyValue Property="Mapping"><Collection><Record Type="Common.SemanticObjectMappingType"><PropertyValue Property="LocalProperty" PropertyPath="ValueSource"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></PropertyValue></Record></Collection></PropertyValue></Record></Annotation><Annotation Term="UI.FieldGroup" Qualifier="LineItemProps"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ShowInLineItem"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="LineItemImportance"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="LineItemLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="LineItemCriticalityPath"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="LineItemSemanticObject"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityField/FormGroup"><Annotation Term="Common.SemanticObject" String="EntityFacets"/><Annotation Term="Common.SemanticObjectMapping"><Collection><Record><PropertyValue Property="LocalProperty" PropertyPath="FormGroup"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></Annotation></Annotations><Annotations Target="Service.EntityField/ValueSource"><Annotation Term="Common.SemanticObject" String="FieldValueLists"/><Annotation Term="Common.SemanticObjectMapping"><Collection><Record><PropertyValue Property="LocalProperty" PropertyPath="ValueSource"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></Annotation></Annotations><Annotations Target="Service.EntityContainer/EntityFields"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>SetName</PropertyPath><PropertyPath>ConfigID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.DraftNode"><Record Type="Common.DraftNodeType"><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation></Annotations><Annotations Target="Service.EntityField/ID"><Annotation Term="Common.Label" String="ID"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/><Annotation Term="Common.Text" Path="FieldName"><Annotation Term="UI.TextArrangement" EnumMember="UI.TextArrangementType/TextOnly"/></Annotation></Annotations><Annotations Target="Service.EntityField/SetName"><Annotation Term="Common.Label" String="Set Name"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityField/FieldName"><Annotation Term="Common.Label" String="Field Name"/></Annotations><Annotations Target="Service.EntityField/Label"><Annotation Term="Common.Label" String="Label"/></Annotations><Annotations Target="Service.EntityField/EdmType"><Annotation Term="Common.Label" String="EDM Type"/><Annotation Term="Common.ValueList"><Record Type="Common.ValueListType"><PropertyValue Property="CollectionPath" String="FieldValueListItems"/><PropertyValue Property="Parameters"><Collection><Record Type="Common.ValueListParameterOut"><PropertyValue Property="LocalDataProperty" PropertyPath="EdmType"/><PropertyValue Property="ValueListProperty" String="Code"/></Record><Record Type="Common.ValueListParameterDisplayOnly"><PropertyValue Property="ValueListProperty" String="Description"/></Record><Record Type="Common.ValueListParameterConstant"><PropertyValue Property="ValueListProperty" String="ListID"/><PropertyValue Property="Constant" String="ea102ff5-5777-5155-b0c3-8dd507435f93"/></Record></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.ValueListWithFixedValues" Bool="true"/></Annotations><Annotations Target="Service.EntityField/MaxLength"><Annotation Term="Common.Label" String="Max Length"/></Annotations><Annotations Target="Service.EntityField/Precision"><Annotation Term="Common.Label" String="Precision"/></Annotations><Annotations Target="Service.EntityField/Scale"><Annotation Term="Common.Label" String="Scale"/></Annotations><Annotations Target="Service.EntityField/IsImmutable"><Annotation Term="Common.Label" String="Immutable"/></Annotations><Annotations Target="Service.EntityField/IsComputed"><Annotation Term="Common.Label" String="Computed"/></Annotations><Annotations Target="Service.EntityField/ReferencesEntity"><Annotation Term="Common.Label" String="References Entity"/></Annotations><Annotations Target="Service.EntityField/PreferDialog"><Annotation Term="Common.Label" String="Prefer Dialog"/></Annotations><Annotations Target="Service.EntityField/TextPath"><Annotation Term="Common.Label" String="Text Path"/></Annotations><Annotations Target="Service.EntityField/DefaultValue"><Annotation Term="Common.Label" String="Default Value"/></Annotations><Annotations Target="Service.EntityField/SortOrder"><Annotation Term="Common.Label" String="Sort Order"/></Annotations><Annotations Target="Service.EntityField/ShowInLineItem"><Annotation Term="Common.Label" String="Show in Line Item"/></Annotations><Annotations Target="Service.EntityField/LineItemImportance"><Annotation Term="Common.Label" String="Line Item Importance"/></Annotations><Annotations Target="Service.EntityField/LineItemLabel"><Annotation Term="Common.Label" String="Line Item Label"/></Annotations><Annotations Target="Service.EntityField/LineItemCriticalityPath"><Annotation Term="Common.Label" String="Criticality Path"/></Annotations><Annotations Target="Service.EntityField/LineItemSemanticObject"><Annotation Term="Common.Label" String="Semantic Object"/></Annotations><Annotations Target="Service.EntityField/Searchable"><Annotation Term="Common.Label" String="Searchable"/></Annotations><Annotations Target="Service.EntityField/FormGroup"><Annotation Term="Common.Label" String="Form Group"/><Annotation Term="Common.ValueList"><Record Type="Common.ValueListType"><PropertyValue Property="CollectionPath" String="EntityFacets"/><PropertyValue Property="Parameters"><Collection><Record Type="Common.ValueListParameterOut"><PropertyValue Property="LocalDataProperty" PropertyPath="FormGroup"/><PropertyValue Property="ValueListProperty" String="FieldGroupQualifier"/></Record><Record Type="Common.ValueListParameterDisplayOnly"><PropertyValue Property="ValueListProperty" String="FieldGroupLabel"/></Record><Record Type="Common.ValueListParameterIn"><PropertyValue Property="LocalDataProperty" PropertyPath="ConfigID"/><PropertyValue Property="ValueListProperty" String="ConfigID"/></Record></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.ValueListWithFixedValues" Bool="true"/></Annotations><Annotations Target="Service.EntityField/ConfigID"><Annotation Term="Common.Label" String="Config"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityField/ValueSource"><Annotation Term="Common.Label" String="Value Source"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Common.ValueList"><Record Type="Common.ValueListType"><PropertyValue Property="CollectionPath" String="FieldValueLists"/><PropertyValue Property="Parameters"><Collection><Record Type="Common.ValueListParameterOut"><PropertyValue Property="LocalDataProperty" PropertyPath="ValueSource"/><PropertyValue Property="ValueListProperty" String="ID"/></Record><Record Type="Common.ValueListParameterDisplayOnly"><PropertyValue Property="ValueListProperty" String="ListName"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityField/IsActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityField/HasActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityField/HasDraftEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityField/FormGroup"><Annotation Term="Common.ValueList"><Record Type="Common.ValueListType"><PropertyValue Property="CollectionPath" String="EntityFacets"/><PropertyValue Property="Parameters"><Collection><Record Type="Common.ValueListParameterOut"><PropertyValue Property="LocalDataProperty" PropertyPath="FormGroup"/><PropertyValue Property="ValueListProperty" String="FieldGroupQualifier"/></Record><Record Type="Common.ValueListParameterDisplayOnly"><PropertyValue Property="ValueListProperty" String="FieldGroupLabel"/></Record><Record Type="Common.ValueListParameterIn"><PropertyValue Property="LocalDataProperty" PropertyPath="ConfigID"/><PropertyValue Property="ValueListProperty" String="ConfigID"/></Record></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.ValueListWithFixedValues" Bool="true"/></Annotations>
<Annotations Target="Service.EntityFacet"><Annotation Term="UI.SelectionFields"><Collection></Collection></Annotation><Annotation Term="UI.LineItem"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SectionLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SectionId"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldGroupQualifier"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldGroupLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldGroupFields"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record></Collection></Annotation><Annotation Term="UI.HeaderInfo"><Record Type="UI.HeaderInfoType"><PropertyValue Property="TypeName" String="EntityFacet"/><PropertyValue Property="TypeNamePlural" String="EntityFacets"/><PropertyValue Property="Title"><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SectionLabel"/></Record></PropertyValue></Record></Annotation><Annotation Term="UI.HeaderFacets"><Collection></Collection></Annotation><Annotation Term="UI.Facets"><Collection><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="FacetInfo"/><PropertyValue Property="ID" String="FacetInfoSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#FacetInfo"/><PropertyValue Property="Label" String="FacetInfo"/></Record></Collection></PropertyValue></Record></Collection></Annotation><Annotation Term="UI.FieldGroup" Qualifier="FacetInfo"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SectionLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SectionId"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldGroupQualifier"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldGroupLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FieldGroupFields"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityContainer/EntityFacets"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>ConfigID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.DraftNode"><Record Type="Common.DraftNodeType"><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation></Annotations><Annotations Target="Service.EntityFacet/ID"><Annotation Term="Common.Label" String="ID"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/><Annotation Term="Common.Text" Path="SectionLabel"><Annotation Term="UI.TextArrangement" EnumMember="UI.TextArrangementType/TextOnly"/></Annotation></Annotations><Annotations Target="Service.EntityFacet/SectionLabel"><Annotation Term="Common.Label" String="Section Label"/></Annotations><Annotations Target="Service.EntityFacet/SectionId"><Annotation Term="Common.Label" String="Section ID"/></Annotations><Annotations Target="Service.EntityFacet/FieldGroupQualifier"><Annotation Term="Common.Label" String="Field Group Qualifier"/></Annotations><Annotations Target="Service.EntityFacet/FieldGroupLabel"><Annotation Term="Common.Label" String="Field Group Label"/></Annotations><Annotations Target="Service.EntityFacet/FieldGroupFields"><Annotation Term="Common.Label" String="Field Group Fields"/></Annotations><Annotations Target="Service.EntityFacet/SortOrder"><Annotation Term="Common.Label" String="Sort Order"/></Annotations><Annotations Target="Service.EntityFacet/ConfigID"><Annotation Term="Common.Label" String="Config"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityFacet/IsActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityFacet/HasActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityFacet/HasDraftEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>
<Annotations Target="Service.EntityNavigation"><Annotation Term="UI.SelectionFields"><Collection></Collection></Annotation><Annotation Term="UI.LineItem"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="NavName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="TargetType"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="TargetSet"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="IsCollection"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ForeignKey"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record></Collection></Annotation><Annotation Term="UI.HeaderInfo"><Record Type="UI.HeaderInfoType"><PropertyValue Property="TypeName" String="EntityNavigation"/><PropertyValue Property="TypeNamePlural" String="EntityNavigations"/><PropertyValue Property="Title"><Record Type="UI.DataField"><PropertyValue Property="Value" Path="NavName"/></Record></PropertyValue></Record></Annotation><Annotation Term="UI.HeaderFacets"><Collection></Collection></Annotation><Annotation Term="UI.Facets"><Collection><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="NavInfo"/><PropertyValue Property="ID" String="NavInfoSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#NavInfo"/><PropertyValue Property="Label" String="NavInfo"/></Record></Collection></PropertyValue></Record></Collection></Annotation><Annotation Term="UI.FieldGroup" Qualifier="NavInfo"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SetName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="NavName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="TargetType"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="TargetSet"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="IsCollection"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ForeignKey"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityContainer/EntityNavigations"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>SetName</PropertyPath><PropertyPath>ConfigID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.DraftNode"><Record Type="Common.DraftNodeType"><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation></Annotations><Annotations Target="Service.EntityNavigation/ID"><Annotation Term="Common.Label" String="ID"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/><Annotation Term="Common.Text" Path="NavName"><Annotation Term="UI.TextArrangement" EnumMember="UI.TextArrangementType/TextOnly"/></Annotation></Annotations><Annotations Target="Service.EntityNavigation/SetName"><Annotation Term="Common.Label" String="Set Name"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityNavigation/NavName"><Annotation Term="Common.Label" String="Navigation Name"/></Annotations><Annotations Target="Service.EntityNavigation/TargetType"><Annotation Term="Common.Label" String="Target Type"/></Annotations><Annotations Target="Service.EntityNavigation/TargetSet"><Annotation Term="Common.Label" String="Target Set"/></Annotations><Annotations Target="Service.EntityNavigation/IsCollection"><Annotation Term="Common.Label" String="Is Collection"/></Annotations><Annotations Target="Service.EntityNavigation/ForeignKey"><Annotation Term="Common.Label" String="Foreign Key"/></Annotations><Annotations Target="Service.EntityNavigation/SortOrder"><Annotation Term="Common.Label" String="Sort Order"/></Annotations><Annotations Target="Service.EntityNavigation/ConfigID"><Annotation Term="Common.Label" String="Config"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityNavigation/IsActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityNavigation/HasActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityNavigation/HasDraftEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>
<Annotations Target="Service.EntityRelationship"><Annotation Term="UI.SelectionFields"><Collection><PropertyPath>Name</PropertyPath></Collection></Annotation><Annotation Term="UI.LineItem"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Name"/></Record><Record Type="UI.DataFieldWithIntentBasedNavigation"><PropertyValue Property="Value" Path="OneEntity"/><PropertyValue Property="SemanticObject" String="EntityConfigs"/><PropertyValue Property="Action" String="display"/><PropertyValue Property="Mapping"><Collection><Record Type="Common.SemanticObjectMappingType"><PropertyValue Property="LocalProperty" PropertyPath="OneEntity"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></PropertyValue></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="OneNavName"/></Record><Record Type="UI.DataFieldWithIntentBasedNavigation"><PropertyValue Property="Value" Path="ManyEntity"/><PropertyValue Property="SemanticObject" String="EntityConfigs"/><PropertyValue Property="Action" String="display"/><PropertyValue Property="Mapping"><Collection><Record Type="Common.SemanticObjectMappingType"><PropertyValue Property="LocalProperty" PropertyPath="ManyEntity"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></PropertyValue></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ManyNavName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="IsComposition"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FkField"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FkLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FkFormGroup"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ConditionType"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ConditionReference"/></Record></Collection></Annotation><Annotation Term="UI.HeaderInfo"><Record Type="UI.HeaderInfoType"><PropertyValue Property="TypeName" String="EntityRelationship"/><PropertyValue Property="TypeNamePlural" String="EntityRelationships"/><PropertyValue Property="Title"><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Name"/></Record></PropertyValue></Record></Annotation><Annotation Term="UI.HeaderFacets"><Collection></Collection></Annotation><Annotation Term="UI.Facets"><Collection><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="Basic"/><PropertyValue Property="ID" String="BasicSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#Basic"/><PropertyValue Property="Label" String="Basic"/></Record></Collection></PropertyValue></Record><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="OneSide"/><PropertyValue Property="ID" String="OneSideSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#OneSide"/><PropertyValue Property="Label" String="OneSide"/></Record></Collection></PropertyValue></Record><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="ManySide"/><PropertyValue Property="ID" String="ManySideSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#ManySide"/><PropertyValue Property="Label" String="ManySide"/></Record></Collection></PropertyValue></Record><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="Condition"/><PropertyValue Property="ID" String="ConditionSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#Condition"/><PropertyValue Property="Label" String="Condition"/></Record></Collection></PropertyValue></Record></Collection></Annotation><Annotation Term="UI.FieldGroup" Qualifier="Basic"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Name"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="IsComposition"/></Record></Collection></PropertyValue></Record></Annotation><Annotation Term="UI.FieldGroup" Qualifier="OneSide"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataFieldWithIntentBasedNavigation"><PropertyValue Property="Value" Path="OneEntity"/><PropertyValue Property="SemanticObject" String="EntityConfigs"/><PropertyValue Property="Action" String="display"/><PropertyValue Property="Mapping"><Collection><Record Type="Common.SemanticObjectMappingType"><PropertyValue Property="LocalProperty" PropertyPath="OneEntity"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></PropertyValue></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="OneNavName"/></Record></Collection></PropertyValue></Record></Annotation><Annotation Term="UI.FieldGroup" Qualifier="ManySide"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataFieldWithIntentBasedNavigation"><PropertyValue Property="Value" Path="ManyEntity"/><PropertyValue Property="SemanticObject" String="EntityConfigs"/><PropertyValue Property="Action" String="display"/><PropertyValue Property="Mapping"><Collection><Record Type="Common.SemanticObjectMappingType"><PropertyValue Property="LocalProperty" PropertyPath="ManyEntity"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></PropertyValue></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ManyNavName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FkField"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FkLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FkFormGroup"/></Record></Collection></PropertyValue></Record></Annotation><Annotation Term="UI.FieldGroup" Qualifier="Condition"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ConditionType"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ConditionReference"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityRelationship/OneEntity"><Annotation Term="Common.SemanticObject" String="EntityConfigs"/><Annotation Term="Common.SemanticObjectMapping"><Collection><Record><PropertyValue Property="LocalProperty" PropertyPath="OneEntity"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></Annotation></Annotations><Annotations Target="Service.EntityRelationship/ManyEntity"><Annotation Term="Common.SemanticObject" String="EntityConfigs"/><Annotation Term="Common.SemanticObjectMapping"><Collection><Record><PropertyValue Property="LocalProperty" PropertyPath="ManyEntity"/><PropertyValue Property="SemanticObjectProperty" String="ID"/></Record></Collection></Annotation></Annotations><Annotations Target="Service.EntityContainer/EntityRelationships"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.DraftRoot"><Record Type="Common.DraftRootType"><PropertyValue Property="ActivationAction" String="Service.draftActivate"/><PropertyValue Property="EditAction" String="Service.draftEdit"/><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation></Annotations><Annotations Target="Service.EntityRelationship/ID"><Annotation Term="Common.Label" String="ID"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/><Annotation Term="Common.Text" Path="Name"><Annotation Term="UI.TextArrangement" EnumMember="UI.TextArrangementType/TextOnly"/></Annotation></Annotations><Annotations Target="Service.EntityRelationship/Name"><Annotation Term="Common.Label" String="Name"/></Annotations><Annotations Target="Service.EntityRelationship/OneEntity"><Annotation Term="Common.Label" String="One-Side Entity"/><Annotation Term="Common.ValueList"><Record Type="Common.ValueListType"><PropertyValue Property="CollectionPath" String="EntityConfigs"/><PropertyValue Property="Parameters"><Collection><Record Type="Common.ValueListParameterOut"><PropertyValue Property="LocalDataProperty" PropertyPath="OneEntity"/><PropertyValue Property="ValueListProperty" String="SetName"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityRelationship/OneNavName"><Annotation Term="Common.Label" String="One-Side Nav Name"/></Annotations><Annotations Target="Service.EntityRelationship/ManyEntity"><Annotation Term="Common.Label" String="Many-Side Entity"/><Annotation Term="Common.ValueList"><Record Type="Common.ValueListType"><PropertyValue Property="CollectionPath" String="EntityConfigs"/><PropertyValue Property="Parameters"><Collection><Record Type="Common.ValueListParameterOut"><PropertyValue Property="LocalDataProperty" PropertyPath="ManyEntity"/><PropertyValue Property="ValueListProperty" String="SetName"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityRelationship/ManyNavName"><Annotation Term="Common.Label" String="Many-Side Nav Name"/></Annotations><Annotations Target="Service.EntityRelationship/IsComposition"><Annotation Term="Common.Label" String="Composition"/></Annotations><Annotations Target="Service.EntityRelationship/FkField"><Annotation Term="Common.Label" String="FK Field"/></Annotations><Annotations Target="Service.EntityRelationship/FkLabel"><Annotation Term="Common.Label" String="FK Label"/></Annotations><Annotations Target="Service.EntityRelationship/FkFormGroup"><Annotation Term="Common.Label" String="FK Form Group"/></Annotations><Annotations Target="Service.EntityRelationship/ConditionType"><Annotation Term="Common.Label" String="Condition Type"/></Annotations><Annotations Target="Service.EntityRelationship/ConditionReference"><Annotation Term="Common.Label" String="Condition Reference"/></Annotations><Annotations Target="Service.EntityRelationship/IsActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityRelationship/HasActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityRelationship/HasDraftEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>
<Annotations Target="Service.EntityTableFacet"><Annotation Term="UI.SelectionFields"><Collection></Collection></Annotation><Annotation Term="UI.LineItem"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FacetLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FacetId"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="NavigationProperty"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record></Collection></Annotation><Annotation Term="UI.HeaderInfo"><Record Type="UI.HeaderInfoType"><PropertyValue Property="TypeName" String="EntityTableFacet"/><PropertyValue Property="TypeNamePlural" String="EntityTableFacets"/><PropertyValue Property="Title"><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FacetLabel"/></Record></PropertyValue></Record></Annotation><Annotation Term="UI.HeaderFacets"><Collection></Collection></Annotation><Annotation Term="UI.Facets"><Collection><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="TFInfo"/><PropertyValue Property="ID" String="TFInfoSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#TFInfo"/><PropertyValue Property="Label" String="TFInfo"/></Record></Collection></PropertyValue></Record></Collection></Annotation><Annotation Term="UI.FieldGroup" Qualifier="TFInfo"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SetName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FacetLabel"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="FacetId"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="NavigationProperty"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityContainer/EntityTableFacets"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>SetName</PropertyPath><PropertyPath>ConfigID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.DraftNode"><Record Type="Common.DraftNodeType"><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation></Annotations><Annotations Target="Service.EntityTableFacet/ID"><Annotation Term="Common.Label" String="ID"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/><Annotation Term="Common.Text" Path="FacetLabel"><Annotation Term="UI.TextArrangement" EnumMember="UI.TextArrangementType/TextOnly"/></Annotation></Annotations><Annotations Target="Service.EntityTableFacet/SetName"><Annotation Term="Common.Label" String="Set Name"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityTableFacet/FacetLabel"><Annotation Term="Common.Label" String="Facet Label"/></Annotations><Annotations Target="Service.EntityTableFacet/FacetId"><Annotation Term="Common.Label" String="Facet ID"/></Annotations><Annotations Target="Service.EntityTableFacet/NavigationProperty"><Annotation Term="Common.Label" String="Navigation Property"/></Annotations><Annotations Target="Service.EntityTableFacet/SortOrder"><Annotation Term="Common.Label" String="Sort Order"/></Annotations><Annotations Target="Service.EntityTableFacet/ConfigID"><Annotation Term="Common.Label" String="Config"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityTableFacet/IsActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityTableFacet/HasActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.EntityTableFacet/HasDraftEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>
<Annotations Target="Service.FieldValueList"><Annotation Term="UI.SelectionFields"><Collection></Collection></Annotation><Annotation Term="UI.LineItem"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ListName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Description"/></Record></Collection></Annotation><Annotation Term="UI.HeaderInfo"><Record Type="UI.HeaderInfoType"><PropertyValue Property="TypeName" String="FieldValueList"/><PropertyValue Property="TypeNamePlural" String="FieldValueLists"/><PropertyValue Property="Title"><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ListName"/></Record></PropertyValue></Record></Annotation><Annotation Term="UI.HeaderFacets"><Collection></Collection></Annotation><Annotation Term="UI.Facets"><Collection><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="ListInfo"/><PropertyValue Property="ID" String="ListInfoSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#ListInfo"/><PropertyValue Property="Label" String="ListInfo"/></Record></Collection></PropertyValue></Record><Record Type="UI.ReferenceFacet"><PropertyValue Property="Label" String="Items"/><PropertyValue Property="ID" String="ItemsSection"/><PropertyValue Property="Target" AnnotationPath="Items/@UI.LineItem"/></Record></Collection></Annotation><Annotation Term="UI.FieldGroup" Qualifier="ListInfo"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="ListName"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Description"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityContainer/FieldValueLists"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.DraftRoot"><Record Type="Common.DraftRootType"><PropertyValue Property="ActivationAction" String="Service.draftActivate"/><PropertyValue Property="EditAction" String="Service.draftEdit"/><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation></Annotations><Annotations Target="Service.FieldValueList/ID"><Annotation Term="Common.Label" String="ID"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/><Annotation Term="Common.Text" Path="ListName"><Annotation Term="UI.TextArrangement" EnumMember="UI.TextArrangementType/TextOnly"/></Annotation></Annotations><Annotations Target="Service.FieldValueList/ListName"><Annotation Term="Common.Label" String="List Name"/></Annotations><Annotations Target="Service.FieldValueList/Description"><Annotation Term="Common.Label" String="Description"/></Annotations><Annotations Target="Service.FieldValueList/IsActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.FieldValueList/HasActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.FieldValueList/HasDraftEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>
<Annotations Target="Service.FieldValueListItem"><Annotation Term="UI.SelectionFields"><Collection></Collection></Annotation><Annotation Term="UI.LineItem"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Code"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Description"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record></Collection></Annotation><Annotation Term="UI.HeaderInfo"><Record Type="UI.HeaderInfoType"><PropertyValue Property="TypeName" String="FieldValueListItem"/><PropertyValue Property="TypeNamePlural" String="FieldValueListItems"/><PropertyValue Property="Title"><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Code"/></Record></PropertyValue></Record></Annotation><Annotation Term="UI.HeaderFacets"><Collection></Collection></Annotation><Annotation Term="UI.Facets"><Collection><Record Type="UI.CollectionFacet"><PropertyValue Property="Label" String="ItemInfo"/><PropertyValue Property="ID" String="ItemInfoSection"/><PropertyValue Property="Facets"><Collection><Record Type="UI.ReferenceFacet"><PropertyValue Property="Target" AnnotationPath="@UI.FieldGroup#ItemInfo"/><PropertyValue Property="Label" String="ItemInfo"/></Record></Collection></PropertyValue></Record></Collection></Annotation><Annotation Term="UI.FieldGroup" Qualifier="ItemInfo"><Record Type="UI.FieldGroupType"><PropertyValue Property="Data"><Collection><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Code"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="Description"/></Record><Record Type="UI.DataField"><PropertyValue Property="Value" Path="SortOrder"/></Record></Collection></PropertyValue></Record></Annotation></Annotations><Annotations Target="Service.EntityContainer/FieldValueListItems"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>ListID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation><Annotation Term="Common.DraftNode"><Record Type="Common.DraftNodeType"><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation></Annotations><Annotations Target="Service.FieldValueListItem/ID"><Annotation Term="Common.Label" String="ID"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/><Annotation Term="Common.Text" Path="Code"><Annotation Term="UI.TextArrangement" EnumMember="UI.TextArrangementType/TextOnly"/></Annotation></Annotations><Annotations Target="Service.FieldValueListItem/Code"><Annotation Term="Common.Label" String="Code"/></Annotations><Annotations Target="Service.FieldValueListItem/Description"><Annotation Term="Common.Label" String="Description"/></Annotations><Annotations Target="Service.FieldValueListItem/SortOrder"><Annotation Term="Common.Label" String="Sort Order"/></Annotations><Annotations Target="Service.FieldValueListItem/ListID"><Annotation Term="Common.Label" String="List"/><Annotation Term="UI.Hidden" Bool="true"/><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.FieldValueListItem/IsActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.FieldValueListItem/HasActiveEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations><Annotations Target="Service.FieldValueListItem/HasDraftEntity"><Annotation Term="Org.OData.Core.V1.Computed" Bool="true"/></Annotations>
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
