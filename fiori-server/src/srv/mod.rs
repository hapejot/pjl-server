use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderName, StatusCode},
    response::Html,
    response::IntoResponse,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use data_store::{DataRecord, EntityKey, StoreError};
use odata::{
    path::{parse_odata_path, PathSegment},
    query::{BinOp, ExpandItem, Expr, ODataQuery, OrderByItem, SortDirection},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use spec::{
    app::App, data_dictionary::DataDictionary, package::Package, AtomValueList, EntitySpec,
    FieldSpec, MeasureKind, Relationship,
};
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
                    owned: _,
                    ..
                } => {
                    let p = if *collection {
                        NavigationProperty::new(
                            name,
                            format!("Collection(Service.{})", entity),
                            None,
                        )
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
                    } else if edm_type == "Edm.DateTimeOffset" {
                        // OData V4: ohne Precision keine Sekundenbruchteile.
                        // UI5 meldet sonst "Illegal DateTimeOffset value" bei
                        // jedem Zeitstempel mit Millisekunden.
                        property = property.precision(7);
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
    fn from_entity(ddic: &DataDictionary, x: &EntitySpec) -> EntitySet {
        let mut navigation_property_bindings = Vec::new();
        navigation_property_bindings.push(NavigationPropertyBinding {
            path: "SiblingEntity".into(),
            target: x.setname().into(),
        });
        navigation_property_bindings.push(NavigationPropertyBinding {
            path: "DraftAdministrativeData".into(),
            target: "DraftAdministrativeData".into(),
        });
        for field in x.fields() {
            debug!("from entity field: {}", field.name());
            if let FieldSpec::Lookup {
                name,
                entity,
                collection,
                owned,
                ..
            } = field
            {
                if !collection && *owned {
                    continue;
                }
                let Some(target) = ddic.active_entity(entity) else {
                    continue;
                };
                navigation_property_bindings.push(NavigationPropertyBinding {
                    path: name.clone(),
                    target: target.setname(),
                });
            }
        }

        EntitySet {
            name: x.setname().into(),
            entity_type: format!("Service.{}", x.name()),
            navigation_property_bindings,
        }
    }
}

fn draft_annotations_for_entity(ddic: &DataDictionary, entity: &EntitySpec) -> Vec<String> {
    let entity_name = entity.name();
    let set_name = entity.setname();
    // Ein Kind einer Composition ist DraftNode, alles andere ein eigenstaendiger DraftRoot.
    let draft_role = match ddic.owning_relationship(entity_name) {
        Some(_) => r##"<Annotation Term="Common.DraftNode"><Record Type="Common.DraftNodeType"><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation>"##.to_string(),
        None => r##"<Annotation Term="Common.DraftRoot"><Record Type="Common.DraftRootType"><PropertyValue Property="ActivationAction" String="Service.draftActivate"/><PropertyValue Property="EditAction" String="Service.draftEdit"/><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation>"##.to_string(),
    };

    let result = vec![
        format!(
            r##"<Annotations Target="Service.EntityContainer/{set_name}"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.DeleteRestrictions"><Record><PropertyValue Property="Deletable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="Insertable" Bool="true"/><PropertyValue Property="NonInsertableProperties"><Collection><PropertyPath>ID</PropertyPath><PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation>{draft_role}{navigation_restrictions}</Annotations>"##,
            navigation_restrictions = navigation_restrictions(ddic, entity),
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
    ];

    result
}

/// Ohne explizite `NavigationRestrictions` bietet Fiori Elements in einer
/// Object-Page-Tabelle kein Anlegen an.
fn navigation_restrictions(ddic: &DataDictionary, entity: &EntitySpec) -> String {
    let restricted: String = ddic
        .owned_children(entity.name())
        .map(|relationship| {
            format!(
                r##"<Record><PropertyValue Property="NavigationProperty" NavigationPropertyPath="{nav}"/><PropertyValue Property="InsertRestrictions"><Record><PropertyValue Property="Insertable" Bool="true"/></Record></PropertyValue><PropertyValue Property="DeleteRestrictions"><Record><PropertyValue Property="Deletable" Bool="true"/></Record></PropertyValue></Record>"##,
                nav = relationship.one_property()
            )
        })
        .collect();
    if restricted.is_empty() {
        return String::new();
    }
    format!(
        r##"<Annotation Term="Org.OData.Capabilities.V1.NavigationRestrictions"><Record><PropertyValue Property="RestrictedProperties"><Collection>{restricted}</Collection></PropertyValue></Record></Annotation>"##
    )
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
    /// `app_entities`: Namen der Hauptentitaeten aller Apps. Ein Verweis auf eine
    /// dieser Entitaeten wird als semantische Verlinkung (Intent) ausgegeben.
    fn from_entity(x: &EntitySpec, hidden: &[String], app_entities: &[String]) -> Vec<Annotations> {
        let mut annotations = Vec::new();
        let mut main_annotations = Annotations {
            target: format!("Service.{}", x.name()),
            annotations: Vec::new(),
        };

        main_annotations.annotations.push(
            Annotation::new("UI.HeaderInfo").record(
                Record::new("UI.HeaderInfoType")
                    .add(PropertyValue::new("TypeName").string(x.type_name()))
                    .add(PropertyValue::new("TypeNamePlural").string(&x.type_name_plural))
                    .add(PropertyValue::new("Title").record(
                        Record::new("UI.DataField").add(PropertyValue::new("Value").path("name")),
                    )),
            ),
        );

        main_annotations.annotations.extend(line_item_annotations(x));
        main_annotations.annotations.push(
            Annotation::new("Common.SemanticKey")
                .collection(Collection::new().add_property_path("ID")),
        );
        if !x.selection_fields().is_empty() {
            let mut paths = Collection::new();
            for field in x.selection_fields() {
                // Einwertige Navigation filtert ueber den Fremdschluessel.
                let is_to_one = x.fields().any(|f| {
                    matches!(f, FieldSpec::Lookup { name, collection: false, .. } if name == field)
                });
                let path = if is_to_one { format!("{field}_ID") } else { field.clone() };
                paths = paths.add_property_path(path);
            }
            main_annotations
                .annotations
                .push(Annotation::new("UI.SelectionFields").collection(paths));
        }
        // Kein Common.Messages: DraftMessages ist im $metadata keine Navigation
        // (nur eine JSON-Spalte im Store). Der Verweis wuerde FE-Fehler
        // "does not point to a property" ausloesen.

        // Object-Page-Abschnitte: Feldgruppen und Listen, siehe `object_page_sections`.
        let (sections, groups) = object_page_sections(x, hidden, app_entities);
        let mut facets = Collection::new();
        for record in sections {
            facets = facets.add_record(record);
        }
        main_annotations
            .annotations
            .push(Annotation::new("UI.Facets").collection(facets));
        main_annotations.annotations.extend(groups);

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
                link: false,
            },
        ));
        for f in x.fields() {
            annotations.push(Self::from_field(x.name(), f));
        }
        for (field, value) in x.filter_defaults() {
            let annotation = match value {
                spec::FilterDefault::Text(t) => Annotation::new("Common.FilterDefaultValue").string(t),
                spec::FilterDefault::Flag(b) => Annotation::new("Common.FilterDefaultValue").boolean(*b),
            };
            annotations.push(Annotations {
                target: format!("Service.{}/{}", x.name(), field),
                annotations: vec![annotation],
            });
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
                owned: _,
                ..
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

/// Listenannotationen einer Entitaet.
///
/// * Ohne `list_views`: unqualifiziertes `UI.LineItem` mit `name` (bisheriges Verhalten).
/// * Mit `list_views`: je Ansicht ein `UI.LineItem` (Qualifier = Ansichtsname, die
///   Ansicht `default` ist unqualifiziert). Zu jeder Ansicht ein
///   `UI.PresentationVariant` mit `Visualizations` auf das LineItem; die Varianten
///   sind die Auswahlpunkte in der ListReport-Ansicht (siehe `flp.rs`).
/// * Eine fehlende `default`-Ansicht wird durch `name` ersetzt, weil Fiori Elements
///   ein unqualifiziertes LineItem fuer die Standardtabelle braucht.
fn line_item_annotations(x: &EntitySpec) -> Vec<Annotation> {
    let views = x.list_views();
    if views.is_empty() {
        return vec![line_item(None, &["name".to_string()], x)];
    }

    let mut out = Vec::new();
    let default_cols: Vec<String> = views
        .iter()
        .find(|v| v.is_default())
        .map(|v| v.columns.clone())
        .unwrap_or_else(|| vec!["name".to_string()]);
    out.push(line_item(None, &default_cols, x));

    for view in views {
        if !view.is_default() {
            out.push(line_item(Some(&view.name), &view.columns, x));
        }
        out.push(presentation_variant(&view.name));
    }
    out
}

/// Ein `UI.LineItem`. Spalten sind Atome (`Value = name`) oder einwertige
/// Navigationen (`Value = nav/name`, Anzeige des Zieltextes).
fn line_item(qualifier: Option<&str>, columns: &[String], x: &EntitySpec) -> Annotation {
    let mut coll = Collection::new();
    for col in columns {
        let is_nav = x.fields().any(|f| {
            matches!(f, spec::FieldSpec::Lookup { name, collection: false, .. } if name == col)
        });
        let path = if is_nav { format!("{col}/name") } else { col.clone() };
        let link = !is_nav && is_link_atom(x, col);
        coll = coll.add_record(data_field(&path, link));
    }
    let mut ann = Annotation::new("UI.LineItem").collection(coll);
    ann.qualifier = qualifier.map(str::to_string);
    ann
}

/// Ist `name` ein Atom mit `link: true` (Wert ist eine URL)?
fn is_link_atom(x: &EntitySpec, name: &str) -> bool {
    x.fields().any(|f| matches!(f, FieldSpec::Atom { name: n, link: true, .. } if n == name))
}

/// `UI.DataField` bzw. bei `link` ein `UI.DataFieldWithUrl`, dessen Ziel der Wert selbst ist.
fn data_field(path: &str, link: bool) -> Record {
    if link {
        Record::new("UI.DataFieldWithUrl")
            .add(PropertyValue::new("Value").path(path))
            .add(PropertyValue::new("Url").path(path))
    } else {
        Record::new("UI.DataField").add(PropertyValue::new("Value").path(path))
    }
}

/// `UI.PresentationVariant` fuer eine Listenansicht. Die Ansicht `default`
/// ist unqualifiziert, alle anderen tragen `#<name>`.
fn presentation_variant(view_name: &str) -> Annotation {
    let (qualifier, line_item_path) = if view_name == spec::ListView::DEFAULT {
        (None, "@UI.LineItem".to_string())
    } else {
        (
            Some(view_name.to_string()),
            format!("@UI.LineItem#{view_name}"),
        )
    };
    let visualizations = Collection::new().add_annotation_path(line_item_path);
    let mut ann = Annotation::new("UI.PresentationVariant").record(
        Record::new("UI.PresentationVariantType")
            .add(PropertyValue::new("Text").string(view_name))
            .add(PropertyValue::new("Visualizations").collection(visualizations)),
    );
    ann.qualifier = qualifier;
    ann
}

/// Identifikator der Allgemein-Facette (Feldabschnitt `Main`).
pub const GENERAL_FACET: &str = "General";
/// Identifikator der Sammel-Facette fuer alle nicht genannten Felder.
pub const DEFAULT_FACET: &str = "Default";

/// Pfad des LineItems fuer eine Navigations-Facette. Ohne Facetten-Einstellung
/// (oder mit Ansicht `default`) das unqualifizierte LineItem, sonst `#<ansicht>`.
fn facet_target_path(x: &EntitySpec, nav: &str) -> String {
    match x.facet(nav).and_then(|f| f.view.as_deref()) {
        Some(view) if view != spec::ListView::DEFAULT => format!("{nav}/@UI.LineItem#{view}"),
        _ => format!("{nav}/@UI.LineItem"),
    }
}

/// Titel einer Facette: der konfigurierte Titel, sonst der Modelltitel der Navigation,
/// sonst der Schluessel.
fn facet_title(x: &EntitySpec, key: &str) -> String {
    if let Some(title) = x.facet(key).and_then(|f| f.title.clone()) {
        return title;
    }
    x.fields()
        .find_map(|f| match f {
            FieldSpec::Lookup { name, label, .. } if name == key => {
                Some(label.clone().unwrap_or_else(|| name.clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| key.to_string())
}

/// Object-Page-Abschnitte einer Entitaet in der Reihenfolge von `EntitySpec.facets`.
///
/// * Ohne Konfiguration: `General` (alle Felder), danach je to-many Navigation ein Listenabschnitt.
/// * `General` ist immer vorhanden. Steht es nicht in `facets`, wird es vorangestellt.
/// * Navigationen, die nicht konfiguriert sind, folgen am Ende.
/// * Eigene Schluessel mit `fields` erzeugen eine eigene `UI.FieldGroup#<schluessel>`.
/// * `Default` sammelt alle Felder, die in keiner Facette genannt sind.
///
/// Liefert die Facetten-Records und die zugehoerigen FieldGroups.
fn object_page_sections(
    x: &EntitySpec,
    hidden: &[String],
    app_entities: &[String],
) -> (Vec<Record>, Vec<Annotation>) {
    let navs: Vec<String> = x
        .fields()
        .filter_map(|f| match f {
            FieldSpec::Lookup { name, collection: true, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mentioned: Vec<&str> = x
        .facets()
        .iter()
        .flat_map(|f| f.fields.iter().map(String::as_str))
        .collect();
    let remainder: Vec<String> = x
        .fields()
        .filter(|f| !matches!(f, FieldSpec::Lookup { collection: true, .. }))
        .filter(|f| !mentioned.contains(&f.name()))
        .map(|f| f.name().to_string())
        .collect();

    let configured: Vec<&str> = x.facets().iter().map(|f| f.facet.as_str()).collect();
    let mut keys: Vec<String> = Vec::new();
    if !configured.contains(&GENERAL_FACET) {
        keys.push(GENERAL_FACET.to_string());
    }
    keys.extend(configured.iter().map(|k| k.to_string()));
    for nav in &navs {
        if !keys.contains(nav) {
            keys.push(nav.clone());
        }
    }

    let mut records = Vec::new();
    let mut groups = Vec::new();
    for key in &keys {
        if navs.contains(key) {
            records.push(list_facet_record(x, key));
            continue;
        }
        let (selected, qualifier): (Option<Vec<String>>, &str) = if key == GENERAL_FACET {
            let fields = x.facet(key).map(|f| f.fields.clone()).filter(|f| !f.is_empty());
            (fields, "Main")
        } else if key == DEFAULT_FACET {
            if remainder.is_empty() {
                continue;
            }
            (Some(remainder.clone()), DEFAULT_FACET)
        } else {
            (x.facet(key).map(|f| f.fields.clone()), key.as_str())
        };
        groups.push(field_group(qualifier, x, selected.as_deref(), hidden, app_entities));

        if key == GENERAL_FACET {
            records.push(
                Record::new("UI.CollectionFacet")
                    .add(PropertyValue::new("Label").string(facet_title(x, key)))
                    .add(PropertyValue::new("ID").string("GeneralSection"))
                    .add(PropertyValue::new("Facets").collection(Collection::new().add_record(
                        Record::new("UI.ReferenceFacet")
                            .add(PropertyValue::new("Label").string(facet_title(x, key)))
                            .add(PropertyValue::new("Target").annotation_path("@UI.FieldGroup#Main")),
                    ))),
            );
        } else {
            records.push(
                Record::new("UI.ReferenceFacet")
                    .add(PropertyValue::new("Label").string(facet_title(x, key)))
                    .add(PropertyValue::new("ID").string(format!("{key}Section")))
                    .add(
                        PropertyValue::new("Target")
                            .annotation_path(format!("@UI.FieldGroup#{qualifier}")),
                    ),
            );
        }
    }
    (records, groups)
}

/// Listenabschnitt (`UI.ReferenceFacet`) fuer eine to-many Navigation.
fn list_facet_record(x: &EntitySpec, nav: &str) -> Record {
    Record::new("UI.ReferenceFacet")
        .add(PropertyValue::new("Label").string(facet_title(x, nav)))
        .add(PropertyValue::new("ID").string(format!("{nav}Section")))
        .add(PropertyValue::new("Target").annotation_path(facet_target_path(x, nav)))
}

/// Einzel-Verweis auf eine App-Entitaet als semantische Verlinkung:
/// `UI.DataFieldWithIntentBasedNavigation` mit Intent `<Entitaet>-display`,
/// Parameter `ID` aus dem Fremdschluessel.
fn lookup_intent_field(name: &str, entity: &str) -> Record {
    let fk = format!("{name}_ID");
    Record::new("UI.DataFieldWithIntentBasedNavigation")
        .add(PropertyValue::new("Value").path(&fk))
        .add(PropertyValue::new("SemanticObject").string(entity))
        .add(PropertyValue::new("Action").string("display"))
        .add(PropertyValue::new("Mapping").collection(Collection::new().add_record(
            Record::new("Common.SemanticObjectMappingType")
                .add(PropertyValue::new("LocalProperty").property_path(&fk))
                .add(PropertyValue::new("SemanticObjectProperty").string("ID")),
        )))
}

/// FieldGroup mit dem Qualifier `qualifier`. `selected`: Felder in Anzeigereihenfolge,
/// `None` = alle Felder der Entitaet.
fn field_group(
    qualifier: &str,
    x: &EntitySpec,
    selected: Option<&[String]>,
    hidden: &[String],
    app_entities: &[String],
) -> Annotation {
    let fields: Vec<&FieldSpec> = match selected {
        Some(names) => names
            .iter()
            .filter_map(|name| x.fields().find(|f| f.name() == name))
            .collect(),
        None => x.fields().collect(),
    };
    let mut data = Collection::new();
    for record in fields
        .into_iter()
        .filter_map(|f| field_record(f, hidden, app_entities))
    {
        data = data.add_record(record);
    }
    let mut ann = Annotation::new("UI.FieldGroup");
    ann.qualifier = Some(qualifier.to_string());
    ann.record(Record::new("UI.FieldGroupType").add(PropertyValue::new("Data").collection(data)))
}

/// Ein `UI.DataField` fuer ein Feld. `None` fuer Felder ohne Zeile in der FieldGroup.
fn field_record(f: &FieldSpec, hidden: &[String], app_entities: &[String]) -> Option<Record> {
    match f {
        FieldSpec::Lookup { name, entity, collection, .. } => {
            if *collection {
                None
            } else if app_entities.iter().any(|a| a == entity) {
                Some(lookup_intent_field(name, entity))
            } else {
                Some(
                    Record::new("UI.DataField")
                        .add(PropertyValue::new("Value").path(format!("{name}_ID"))),
                )
            }
        }
        FieldSpec::Atom { name, link, .. } => {
            if hidden.iter().any(|h| h == name) {
                None
            } else {
                Some(data_field(name, *link))
            }
        }
        FieldSpec::Measure { .. } => todo!(),
    }
}

fn annotations_for_atom_value_list(name: &str, value_list: &AtomValueList) -> Vec<Annotation> {
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

    #[serde(
        rename = "AnnotationPath",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub annotation_paths: Vec<String>,

    #[serde(rename = "Record", default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<Record>,
}

impl Collection {
    pub fn new() -> Self {
        Self {
            property_paths: Vec::new(),
            annotation_paths: Vec::new(),
            records: Vec::new(),
        }
    }

    pub fn add_annotation_path(mut self, path: impl Into<String>) -> Self {
        self.annotation_paths.push(path.into());
        self
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

const INFO_TEMPLATE: &str = include_str!("../../templates/info.hbs");

/// Build the HTML data dictionary page.
//
// Handlebars nimmt jeden `Serialize`-Typ direkt entgegen. Domain-Typen (`Package`,
// `App`, `Relationship`) werden hier unverändert durchgereicht; abgeleitete Werte
// (URL, Kardinalität, FK-Fallback) macht das Template selbst.
// Für `EntitySpec` bauen wir mit `serde_json::json!` ein flaches Ad-hoc-Format,
// weil `setname()` berechnet ist und `FieldSpec` als Enum untauglich fürs Template.
fn render_info_page(state: &AppState) -> String {
    let ddic = state.ddic();
    let packages: Vec<&Package> = ddic.packages().collect();
    let apps: Vec<&App> = ddic.apps().collect();
    let relationships: Vec<&Relationship> = ddic.relationships().collect();

    let entities: Vec<Value> = ddic
        .active_entities()
        .cloned()
        .map(|ent| {
            let fields: Vec<Value> = ent
                .fields()
                .map(|f| {
                    json!({
                        "name": f.name(),
                        "label": f.label(),
                        "edm_type": format_field_value(f),
                    })
                })
                .collect();
            let navigations: Vec<Value> = ent
                .fields()
                .filter_map(|field| match field {
                    FieldSpec::Lookup {
                        name,
                        entity: target_entity,
                        collection,
                        owned: _,
                        ..
                    } => {
                        let target_set = ddic
                            .active_entity(target_entity)
                            .map(|e| e.setname())
                            .unwrap_or_else(|| format!("{target_entity}Set"));
                        Some(json!({
                            "name": name,
                            "source_entity": ent.name(),
                            "source_set": ent.setname(),
                            "target_entity": target_entity,
                            "target_set": target_set,
                            "collection": *collection,
                            "foreign_key_field": ddic.foreign_key_field(&ent, field),
                        }))
                    }
                    _ => None,
                })
                .collect();
            json!({
                "name": ent.name(),
                "set": ent.setname(),
                "field_count": fields.len(),
                "nav_count": navigations.len(),
                "fields": fields,
                "navigations": navigations,
            })
        })
        .collect();

    let ctx = json!({
        "packages": packages,
        "apps_count": apps.len(),
        "apps": apps,
        "entities_count": entities.len(),
        "entities": entities,
        "relationships_count": relationships.len(),
        "relationships": relationships,
    });

    let hbs = handlebars::Handlebars::new();
    match hbs.render_template(INFO_TEMPLATE, &ctx) {
        Ok(html) => html,
        Err(e) => {
            error!("failed to render info page template: {e}");
            format!("<pre>template error: {e}</pre>")
        }
    }
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
    info!("serving HTML data dictionary page");
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
        .route("/reload", post(reload))
        .route(
            "/{*path}",
            get(get_handler)
                .post(post_handler)
                .patch(patch_handler)
                .delete(delete_handler),
        )
}

/// Das Data Dictionary aus dem Storage neu aufbauen.
async fn reload(State(state): State<AppState>) -> impl IntoResponse {
    let ddic = state.reload();
    info!("data dictionary reloaded");
    to_json_response(json!({
        "packages": ddic.packages().count(),
        "entities": ddic.active_entities().count(),
        "relationships": ddic.relationships().count(),
        "apps": ddic.apps().count(),
    }))
}

fn build_query(params: &HashMap<String, String>) -> Result<ODataQuery, ODataError> {
    let filter = match params.get("$filter") {
        Some(raw) => Some(
            Expr::parse(raw)
                .map_err(|e| ODataError::bad_request(format!("Invalid $filter '{raw}': {e}")))?,
        ),
        None => None,
    };
    let top = match params.get("$top") {
        Some(raw) => Some(
            raw.parse::<u32>()
                .map_err(|e| ODataError::bad_request(format!("Invalid $top '{raw}': {e}")))?,
        ),
        None => None,
    };
    let skip = match params.get("$skip") {
        Some(raw) => Some(
            raw.parse::<u32>()
                .map_err(|e| ODataError::bad_request(format!("Invalid $skip '{raw}': {e}")))?,
        ),
        None => None,
    };
    let orderby = match params.get("$orderby") {
        Some(raw) => parse_orderby(raw)?,
        None => vec![],
    };
    let expand = match params.get("$expand") {
        Some(raw) => parse_expand_items(raw)?,
        None => vec![],
    };
    Ok(ODataQuery {
        filter,
        orderby,
        top,
        skip,
        select: params
            .get("$select")
            .and_then(|x| Some(x.split(",").map(|y| y.to_string()).collect::<Vec<_>>()))
            .unwrap_or_else(|| vec![]),
        expand,
        count: params.get("$count").is_some(),
        search: params.get("$search").cloned(),
    })
}

/// `$orderby`: kommagetrennte Liste aus `Property [asc|desc]`.
fn parse_orderby(raw: &str) -> Result<Vec<OrderByItem>, ODataError> {
    split_top_level(raw, ',')
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| parse_orderby_item(&part))
        .collect()
}

fn parse_orderby_item(raw: &str) -> Result<OrderByItem, ODataError> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    let (property, direction) = match parts.as_slice() {
        [p] => (*p, SortDirection::Asc),
        [p, d] if d.eq_ignore_ascii_case("asc") => (*p, SortDirection::Asc),
        [p, d] if d.eq_ignore_ascii_case("desc") => (*p, SortDirection::Desc),
        _ => {
            return Err(ODataError::bad_request(format!(
                "Invalid $orderby item '{raw}'"
            )))
        }
    };
    Ok(OrderByItem {
        property: property.to_string(),
        direction,
    })
}

fn parse_expand_items(raw: &str) -> Result<Vec<ExpandItem>, ODataError> {
    split_top_level(raw, ',')
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| parse_expand_item(&part))
        .collect()
}

fn parse_expand_item(raw: &str) -> Result<ExpandItem, ODataError> {
    let trimmed = raw.trim();
    if let Some(paren) = find_top_level_options_start(trimmed) {
        let navigation_property = trimmed[..paren].trim().to_string();
        let options = trimmed[paren + 1..trimmed.len() - 1].trim();
        Ok(ExpandItem {
            navigation_property,
            nested_query: Box::new(parse_nested_query(options)?),
        })
    } else {
        Ok(ExpandItem {
            navigation_property: trimmed.to_string(),
            nested_query: Box::new(ODataQuery::default()),
        })
    }
}

fn parse_nested_query(raw: &str) -> Result<ODataQuery, ODataError> {
    let mut query = ODataQuery::default();
    for option in split_top_level(raw, ';') {
        let Some((name, value)) = option.split_once('=') else {
            continue;
        };
        match name.trim() {
            "$filter" => query.filter = Some(Expr::parse(value.trim()).unwrap()),
            "$orderby" => query.orderby = parse_orderby(value.trim())?,
            "$top" => query.top = Some(value.trim().parse().unwrap()),
            "$skip" => query.skip = Some(value.trim().parse().unwrap()),
            "$select" => query.select = split_top_level(value.trim(), ','),
            "$expand" => query.expand = parse_expand_items(value.trim())?,
            "$count" => query.count = value.trim().parse().unwrap_or(true),
            "$search" => query.search = Some(value.trim().to_string()),
            _ => {}
        }
    }
    Ok(query)
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

fn entity_spec_for_segment(state: &AppState, s: &PathSegment) -> Result<EntitySpec, ODataError> {
    state
        .ddic()
        .active_entity_by_setname(&s.name)
        .cloned()
        .ok_or_else(|| ODataError::not_found(format!("Unknown entity set '{}'", s.name)))
}

#[derive(Debug)]
struct ODataError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ODataError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "BadRequest",
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "NotFound",
            message: message.into(),
        }
    }

    fn from_store(e: StoreError) -> Self {
        match e {
            StoreError::NotFound { .. } => Self::not_found(e.to_string()),
            StoreError::Conflict(msg) => Self {
                status: StatusCode::CONFLICT,
                code: "Conflict",
                message: msg,
            },
            StoreError::PropertyNotFound(_) => Self::bad_request(e.to_string()),
            _ => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "InternalError",
                message: e.to_string(),
            },
        }
    }
}

impl From<crate::extension::ActionError> for ODataError {
    fn from(e: crate::extension::ActionError) -> Self {
        match e {
            crate::extension::ActionError::Failed(msg) => ODataError::bad_request(msg),
            crate::extension::ActionError::Store(s) => ODataError::from_store(s),
        }
    }
}

impl IntoResponse for ODataError {
    fn into_response(self) -> Response {
        let body = json!({"error": {"code": self.code, "message": self.message}});
        (
            self.status,
            [content_type_json(), odata_version()],
            body.to_string(),
        )
            .into_response()
    }
}

enum GetResult {
    Json(Value),
    Text(String),
    NoContent,
}

impl IntoResponse for GetResult {
    fn into_response(self) -> Response {
        match self {
            GetResult::Json(value) => to_json_response(value).into_response(),
            GetResult::Text(text) => (
                [
                    (header::CONTENT_TYPE, "text/plain;charset=utf-8".to_string()),
                    odata_version(),
                ],
                text,
            )
                .into_response(),
            GetResult::NoContent => (StatusCode::NO_CONTENT, [odata_version()]).into_response(),
        }
    }
}

/// Zwischenstand beim Ablaufen der Pfadsegmente.
enum Cursor {
    Entity {
        spec: EntitySpec,
        set: String,
        record: Box<DataRecord>,
    },
    Collection {
        records: Vec<DataRecord>,
        count: Option<usize>,
    },
    Property {
        value: Value,
    },
    Empty,
}

async fn get_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
) -> Response {
    let query = match build_query(&params) {
        Ok(q) => q,
        Err(e) => return e.into_response(),
    };
    let path_struct = match parse_odata_path(&path) {
        Ok(p) => p,
        Err(e) => {
            return ODataError::bad_request(format!("Invalid path '{}': {}", e.segment, e.message))
                .into_response();
        }
    };
    match resolve_get_path(&state, &query, &path_struct.segments).await {
        Ok(result) => result.into_response(),
        Err(e) => e.into_response(),
    }
}

async fn resolve_get_path(
    state: &AppState,
    query: &ODataQuery,
    segments: &[PathSegment],
) -> Result<GetResult, ODataError> {
    let resolved = resolve_path(state, query, segments).await?;
    materialize(
        resolved.cursor,
        resolved.suffix,
        &resolved.query,
        &resolved.context,
    )
}

struct ResolvedPath<'a> {
    cursor: Cursor,
    suffix: Option<&'a str>,
    /// Query, die auf dem Zielsegment wirkt.
    query: ODataQuery,
    context: String,
}

async fn resolve_path<'a>(
    state: &AppState,
    query: &ODataQuery,
    segments: &'a [PathSegment],
) -> Result<ResolvedPath<'a>, ODataError> {
    // `$count`/`$value` beschreiben die Darstellung des Vorgaengersegments, nicht eine Navigation.
    let suffix = segments
        .last()
        .map(|s| s.name.as_str())
        .filter(|name| *name == "$count" || *name == "$value");
    let nav_segments = match suffix {
        Some(_) => &segments[..segments.len() - 1],
        None => segments,
    };
    let Some(root) = nav_segments.first() else {
        return Err(ODataError::bad_request("Path without entity set"));
    };

    let target_query = match suffix {
        Some("$count") => ODataQuery {
            top: None,
            skip: None,
            count: false,
            select: vec![],
            expand: vec![],
            ..query.clone()
        },
        _ => query.clone(),
    };
    let intermediate_query = ODataQuery::default();
    let last = nav_segments.len() - 1;

    let spec = entity_spec_for_segment(state, root)?;
    let mut context = spec.setname();
    let root_query = if last == 0 {
        &target_query
    } else {
        &intermediate_query
    };
    let mut cursor = if root.keys.is_empty() {
        read_collection(state, &root.name, root_query).await?
    } else {
        let record = read_single(state, &root.name, &entity_key(root)?, root_query).await?;
        Cursor::Entity {
            spec,
            set: root.name.clone(),
            record: Box::new(record),
        }
    };

    for (idx, segment) in nav_segments.iter().enumerate().skip(1) {
        info!("processing segment {:?}", segment);
        let navigation_query = if idx == last {
            &target_query
        } else {
            &intermediate_query
        };
        cursor = follow_segment(state, cursor, segment, navigation_query, &mut context).await?;
    }

    Ok(ResolvedPath {
        cursor,
        suffix,
        query: target_query,
        context,
    })
}

fn materialize(
    cursor: Cursor,
    suffix: Option<&str>,
    query: &ODataQuery,
    context: &str,
) -> Result<GetResult, ODataError> {
    let context = format!("/Service/$metadata#{context}");
    match (cursor, suffix) {
        (Cursor::Collection { records, .. }, Some("$count")) => {
            Ok(GetResult::Text(records.len().to_string()))
        }
        (Cursor::Property { value }, Some("$value")) => Ok(GetResult::Text(match value {
            Value::String(s) => s,
            other => other.to_string(),
        })),
        (_, Some(suffix)) => Err(ODataError::bad_request(format!(
            "'{suffix}' is not applicable here"
        ))),
        (Cursor::Entity { record, .. }, None) => {
            let mut value = json!(record);
            touch_selected_fields(&mut value, query);
            value["@odata.context"] = json!(format!("{context}/$entity"));
            Ok(GetResult::Json(value))
        }
        (Cursor::Collection { records, count }, None) => {
            let mut value = json!({ "@odata.context": context, "value": records });
            if let Some(count) = count {
                value["@odata.count"] = json!(count);
            }
            Ok(GetResult::Json(value))
        }
        (Cursor::Property { value }, None) => Ok(GetResult::Json(
            json!({ "@odata.context": context, "value": value }),
        )),
        (Cursor::Empty, None) => Ok(GetResult::NoContent),
    }
}

/// Ein Navigations-, Property- oder Draft-Segment ausgehend vom aktuellen Cursor aufloesen.
async fn follow_segment(
    state: &AppState,
    cursor: Cursor,
    segment: &PathSegment,
    query: &ODataQuery,
    context: &mut String,
) -> Result<Cursor, ODataError> {
    let Cursor::Entity { spec, set, record } = cursor else {
        return Err(ODataError::bad_request(format!(
            "'{}' cannot be applied to a collection",
            segment.name
        )));
    };
    context.push_str(&format!(
        "(ID={},IsActiveEntity={})/{}",
        record.id(),
        record.is_active_entity(),
        segment.name
    ));

    match segment.name.as_str() {
        // Beide Draft-Navigationen sind nullable: fehlt das Ziel, ist die Antwort leer.
        "SiblingEntity" => {
            let key = EntityKey::new(record.id(), record.is_active_entity()).sibling();
            match state.store.read_entity(&set, &key, query).await {
                Ok(record) => Ok(Cursor::Entity {
                    spec,
                    set,
                    record: Box::new(record),
                }),
                Err(StoreError::NotFound { .. }) => Ok(Cursor::Empty),
                Err(e) => Err(ODataError::from_store(e)),
            }
        }
        "DraftAdministrativeData" => match record.draft_admin_data() {
            Some(data) => Ok(Cursor::Property { value: json!(data) }),
            None => Ok(Cursor::Empty),
        },
        name => match spec.fields().find(|f| f.name() == name).cloned() {
            Some(field) => match &field {
                FieldSpec::Lookup {
                    entity, collection, ..
                } => {
                    let target = state.ddic().active_entity(entity).cloned().ok_or_else(|| {
                        ODataError::not_found(format!("Unknown entity '{entity}'"))
                    })?;
                    if *collection {
                        follow_to_many(state, &spec, &field, target, &record, segment, query).await
                    } else {
                        follow_to_one(state, name, target, &record, query).await
                    }
                }
                _ => Ok(Cursor::Property {
                    value: record.get(name).unwrap_or(Value::Null),
                }),
            },
            None => Err(ODataError::not_found(format!(
                "'{name}' is not a member of '{}'",
                spec.name()
            ))),
        },
    }
}

async fn follow_to_one(
    state: &AppState,
    name: &str,
    target: EntitySpec,
    parent: &DataRecord,
    query: &ODataQuery,
) -> Result<Cursor, ODataError> {
    let foreign_key = format!("{name}_ID");
    let Some(id) = parent.get(&foreign_key).filter(|v| !v.is_null()) else {
        return Ok(Cursor::Empty);
    };
    let set = target.setname();
    let lookup = ODataQuery {
        top: None,
        skip: None,
        count: false,
        filter: Some(and_filter(
            query.filter.clone(),
            Expr::equals_value(&id, "ID"),
        )),
        ..query.clone()
    };
    let record = state
        .store
        .get_collection(&set, &lookup)
        .await
        .map_err(ODataError::from_store)?
        .into_iter()
        .next();
    match record {
        Some(record) => Ok(Cursor::Entity {
            spec: target,
            set,
            record: Box::new(record),
        }),
        None => Ok(Cursor::Empty),
    }
}

async fn follow_to_many(
    state: &AppState,
    spec: &EntitySpec,
    field: &FieldSpec,
    target: EntitySpec,
    parent: &DataRecord,
    segment: &PathSegment,
    query: &ODataQuery,
) -> Result<Cursor, ODataError> {
    let foreign_key = state
        .ddic()
        .foreign_key_field(spec, field)
        .map(|f| format!("{f}_ID"))
        .ok_or_else(|| {
            ODataError::not_found(format!(
                "No foreign key for navigation '{}' of '{}'",
                field.name(),
                spec.name()
            ))
        })?;
    let parent_filter = Expr::equals_guid(parent.id().to_string(), &foreign_key);
    let set = target.setname();

    if segment.keys.is_empty() {
        let mut child_query = query.clone();
        child_query.filter = Some(and_filter(child_query.filter, parent_filter));
        return read_collection(state, &set, &child_query).await;
    }

    let record = read_single(state, &set, &entity_key(segment)?, query).await?;
    if record
        .get(&foreign_key)
        .and_then(|v| v.as_str().map(str::to_string))
        != Some(parent.id().to_string())
    {
        return Err(ODataError::not_found(format!(
            "Entity is not part of '{}'",
            segment.name
        )));
    }
    Ok(Cursor::Entity {
        spec: target,
        set,
        record: Box::new(record),
    })
}

fn and_filter(existing: Option<Expr>, additional: Expr) -> Expr {
    match existing {
        Some(existing) => Expr::BinaryOp {
            op: BinOp::And,
            left: Box::new(existing),
            right: Box::new(additional),
        },
        None => additional,
    }
}

fn entity_key(segment: &PathSegment) -> Result<EntityKey, ODataError> {
    let id = segment.id().ok_or_else(|| {
        ODataError::bad_request(format!("Missing key 'ID' in '{}'", segment.name))
    })?;
    Ok(EntityKey::new(id, segment.is_active_entity()))
}

fn touch_selected_fields(value: &mut Value, query: &ODataQuery) {
    for field in query.select.iter() {
        if field == "*" {
            continue;
        }
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

async fn read_single(
    state: &AppState,
    set: &str,
    key: &EntityKey,
    query: &ODataQuery,
) -> Result<DataRecord, ODataError> {
    state
        .store
        .read_entity(set, key, query)
        .await
        .map_err(ODataError::from_store)
}

async fn read_collection(
    state: &AppState,
    set: &str,
    query: &ODataQuery,
) -> Result<Cursor, ODataError> {
    // Ein Aufruf: Hooks laufen einmal pro Request, Count beruecksichtigt Entfernungen.
    let (records, count) = state
        .store
        .read_collection(set, query, query.count)
        .await
        .map_err(ODataError::from_store)?;
    Ok(Cursor::Collection { records, count })
}

async fn post_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
    payload: Option<Json<Value>>,
) -> Response {
    let _p = match build_query(&params) {
        Ok(q) => q,
        Err(e) => return e.into_response(),
    };
    let path_struct = match parse_odata_path(&path) {
        Ok(p) => p,
        Err(e) => {
            return ODataError::bad_request(format!("Invalid path '{}': {}", e.segment, e.message))
                .into_response();
        }
    };
    let body = payload.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    match path_struct.segments.as_slice() {
        [entity] => match state.store.create_entity(&entity.name, &body).await {
            Ok(r) => created_response(json!(r)),
            Err(e) => ODataError::from_store(e).into_response(),
        },
        [entity, action]
            if !entity.keys.is_empty() && action.keys.is_empty() && action.name.contains('.') =>
        {
            let action_name = action.name.rsplit('.').next().unwrap_or_default();
            let key: EntityKey = entity.into();
            if let Some(a) =
                crate::extension::find_action(&state.extensions, &entity.name, action_name)
            {
                return match a.handler.invoke(&state, &key).await {
                    Ok(Value::Null) => StatusCode::NO_CONTENT.into_response(),
                    Ok(r) => to_json_response(r).into_response(),
                    Err(e) => ODataError::from(e).into_response(),
                };
            }
            match draft_action(&state, &entity.name, &key, action_name).await {
                Ok(r) => to_json_response(r).into_response(),
                Err(e) => e.into_response(),
            }
        }
        segments if segments.len() > 1 => {
            match create_in_navigation(&state, segments, body).await {
                Ok(r) => created_response(r),
                Err(e) => e.into_response(),
            }
        }
        _ => ODataError::bad_request("POST is not supported for this path").into_response(),
    }
}

fn created_response(body: Value) -> Response {
    (
        StatusCode::CREATED,
        [content_type_json(), odata_version()],
        body.to_string(),
    )
        .into_response()
}

async fn draft_action(
    state: &AppState,
    set: &str,
    key: &EntityKey,
    action: &str,
) -> Result<Value, ODataError> {
    match action {
        "draftEdit" => {
            let r = state
                .store
                .draft_edit(set, key)
                .await
                .map_err(ODataError::from_store)?;
            for (child_set, foreign_key) in owned_children_of(state, set) {
                for child in children_of(state, &child_set, &foreign_key, key.id).await? {
                    if child.is_active_entity() {
                        state
                            .store
                            .draft_edit(&child_set, &EntityKey::from(&child))
                            .await
                            .map_err(ODataError::from_store)?;
                    }
                }
            }
            Ok(r)
        }
        "draftActivate" => {
            // Kinderbaum wird vom Store rekursiv aktiviert: Parent zuerst
            // (damit die zusammengesetzten FKs (entity_ID, IsActiveEntity)
            // aufloesen), danach die Kinder. Frueher hat diese Funktion die
            // Kinder vorab aktiviert, was gegen die FK-Reihenfolge lief.
            state
                .store
                .draft_activate(set, key)
                .await
                .map_err(ODataError::from_store)
        }
        "draftPrepare" => state
            .store
            .draft_prepare(set, key)
            .await
            .map_err(ODataError::from_store),
        other => Err(ODataError::not_found(format!("Unknown action '{other}'"))),
    }
}

/// Ein neues Kind in einer To-Many-Navigation anlegen.
async fn create_in_navigation(
    state: &AppState,
    segments: &[PathSegment],
    mut body: Value,
) -> Result<Value, ODataError> {
    let (parent_segments, navigation) = segments.split_at(segments.len() - 1);
    let navigation = &navigation[0];
    if !navigation.keys.is_empty() {
        return Err(ODataError::bad_request(
            "POST addresses a collection, not a single entity",
        ));
    }

    let resolved = resolve_path(state, &ODataQuery::default(), parent_segments).await?;
    let Cursor::Entity { spec, record, .. } = resolved.cursor else {
        return Err(ODataError::bad_request(
            "Navigation requires a single parent entity",
        ));
    };

    let ddic = state.ddic();
    let field = spec
        .fields()
        .find(|f| f.name() == navigation.name)
        .cloned()
        .ok_or_else(|| {
            ODataError::not_found(format!(
                "'{}' is not a member of '{}'",
                navigation.name,
                spec.name()
            ))
        })?;
    let FieldSpec::Lookup {
        entity: target_entity,
        collection: true,
        ..
    } = &field
    else {
        return Err(ODataError::bad_request(format!(
            "'{}' is not a collection navigation",
            navigation.name
        )));
    };
    let target = ddic
        .active_entity(target_entity)
        .ok_or_else(|| ODataError::not_found(format!("Unknown entity '{target_entity}'")))?;
    let foreign_key = ddic
        .foreign_key_field(&spec, &field)
        .map(|f| format!("{f}{}", spec::ID_SUFFIX))
        .ok_or_else(|| {
            ODataError::not_found(format!(
                "No foreign key for navigation '{}'",
                navigation.name
            ))
        })?;

    body[foreign_key] = json!(record.id().to_string());
    // Das Kind teilt den Draft-Zustand seines Parents.
    body["IsActiveEntity"] = json!(record.is_active_entity());
    let created = state
        .store
        .create_entity(&target.setname(), &body)
        .await
        .map_err(ODataError::from_store)?;
    Ok(json!(created))
}

/// Zielset und FK-Feld aller Compositions unterhalb von `set`.
fn owned_children_of(state: &AppState, set: &str) -> Vec<(String, String)> {
    let ddic = state.ddic();
    let Some(parent) = ddic.active_entity_by_setname(set) else {
        return vec![];
    };
    ddic.owned_children(parent.name())
        .filter_map(|relationship| {
            let child = ddic.active_entity(relationship.many())?;
            Some((
                child.setname(),
                format!("{}{}", relationship.many_property(), spec::ID_SUFFIX),
            ))
        })
        .collect()
}

async fn children_of(
    state: &AppState,
    set: &str,
    foreign_key: &str,
    parent: uuid::Uuid,
) -> Result<Vec<DataRecord>, ODataError> {
    let query = ODataQuery::new().filter(Expr::equals_guid(parent.to_string(), foreign_key));
    state
        .store
        .get_collection(set, &query)
        .await
        .map_err(ODataError::from_store)
}

async fn patch_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
    payload: Option<Json<Value>>,
) -> Response {
    let _p = build_query(&params);
    let path_struct = match parse_odata_path(&path) {
        Ok(p) => p,
        Err(e) => {
            return ODataError::bad_request(format!("Invalid path '{}': {}", e.segment, e.message))
                .into_response();
        }
    };
    let body = payload.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    let Some(key) = path_struct.segments.first() else {
        return ODataError::bad_request("Missing entity segment in path").into_response();
    };
    let Some(id) = key.id() else {
        return ODataError::bad_request(format!("Invalid key for '{}'", key.name)).into_response();
    };
    let ekey = EntityKey::new(id, key.is_active_entity());
    match state.store.patch_entity(&key.name, &ekey, &body).await {
        Ok(_) => (
            [content_type_json(), odata_version()],
            json!([]).to_string(),
        )
            .into_response(),
        Err(e) => ODataError::from_store(e).into_response(),
    }
}

async fn delete_handler(path: Path<String>, state: State<AppState>) -> Response {
    let path_struct = match parse_odata_path(&path) {
        Ok(p) => p,
        Err(e) => {
            return ODataError::bad_request(format!("Invalid path '{}': {}", e.segment, e.message))
                .into_response();
        }
    };
    match delete_entity(&state, &path_struct.segments).await {
        Ok(()) => (StatusCode::NO_CONTENT, [odata_version()]).into_response(),
        Err(e) => e.into_response(),
    }
}

async fn delete_entity(state: &AppState, segments: &[PathSegment]) -> Result<(), ODataError> {
    let resolved = resolve_path(state, &ODataQuery::default(), segments).await?;
    if let Some(suffix) = resolved.suffix {
        return Err(ODataError::bad_request(format!(
            "'{suffix}' cannot be deleted"
        )));
    }
    let Cursor::Entity { set, record, .. } = resolved.cursor else {
        return Err(ODataError::bad_request(
            "DELETE requires a single entity addressed by key",
        ));
    };

    let key = EntityKey::from(&*record);
    delete_owned_children(state, &set, &key).await?;

    // If deleting a draft, don't also delete active sibling (discard only the draft)
    // If deleting an active, also delete its draft sibling (cleanup both versions)
    if !key.is_active_entity {
        state
            .store
            .delete_entity_tree_only(&set, &key)
            .await
            .map_err(ODataError::from_store)?;
    } else {
        state
            .store
            .delete_entity(&set, &key)
            .await
            .map_err(ODataError::from_store)?;
        // Mit dem aktiven Satz verschwindet auch ein noch offener Draft.
        // The store doesn't automatically delete the draft when deleting an active,
        // so we need to do it explicitly in fiori-server.
        state
            .store
            .delete_entity_tree_only(&set, &key.sibling())
            .await
            .map_err(ODataError::from_store)?;
    }
    Ok(())
}

/// Kinder einer Composition mitloeschen. Wird nur der Draft verworfen, bleiben
/// die aktiven Kinder erhalten.
///
/// When deleting a draft parent:
/// - Delete draft children using delete_entity_tree_only (which doesn't also delete active siblings)
/// - Keep active children
///
/// When deleting an active parent:
/// - Delete both draft and active children
async fn delete_owned_children(
    state: &AppState,
    set: &str,
    parent: &EntityKey,
) -> Result<(), ODataError> {
    for (child_set, foreign_key) in owned_children_of(state, set) {
        for child in children_of(state, &child_set, &foreign_key, parent.id).await? {
            if !parent.is_active_entity && !child.is_active_entity() {
                // Parent is draft, child is also draft: delete it WITHOUT deleting active sibling
                state
                    .store
                    .delete_entity_tree_only(&child_set, &EntityKey::from(&child))
                    .await
                    .map_err(ODataError::from_store)?;
            } else if parent.is_active_entity {
                // Parent is active: delete all children (will also delete active siblings if draft)
                state
                    .store
                    .delete_entity(&child_set, &EntityKey::from(&child))
                    .await
                    .map_err(ODataError::from_store)?;
            }
            // else: parent is draft and child is active - skip deletion (keep active children)
        }
    }
    Ok(())
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
    // Ausschliesslich das, was auch in `$metadata` steht. Frueher haengte hier
    // eine feste Liste (`EntityConfigs`, `EntityFields`, ...) an, die seit dem
    // Umstieg auf das Data Dictionary keine Entsprechung mehr hat - im
    // Laufzeitserver waeren das Eintraege eines fremden Modells.
    let value: Vec<Value> = state
        .ddic()
        .active_entities()
        .map(|e| {
            let set = e.setname();
            json!({"name": set, "url": set})
        })
        .collect();
    Json(json!({
      "@odata.context": "/Service/$metadata",
      "value": value
    }))
}
/// Namen der Hauptentitaeten aller Apps (Ziele der semantischen Verlinkung).
fn app_entity_names(ddic: &DataDictionary) -> Vec<String> {
    ddic.apps()
        .map(|app| app.main_entity(ddic).name().to_string())
        .collect()
}

async fn metadata(State(state): State<AppState>) -> impl IntoResponse {
    let ddic = state.ddic();
    let entity_lst: Vec<EntityType> = ddic
        .active_entities()
        .map(|x| EntityType::from_entity(x))
        .collect();
    let entity_set_lst: Vec<EntitySet> = ddic
        .active_entities()
        .map(|x| EntitySet::from_entity(&ddic, x))
        .collect();
    let app_entities = app_entity_names(&ddic);
    let annotation_lst: Vec<Annotations> = ddic
        .active_entities()
        .map(|x| Annotations::from_entity(x, &crate::extension::hidden_fields(&state.extensions, x.name()), &app_entities))
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
    for x in ddic.active_entities() {
        annotations.extend(draft_annotations_for_entity(&ddic, x));
    }

    let mut actions = vec![];
    for e in ddic.active_entities() {
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

    // Gebundene Aktionen der Erweiterungen (siehe `extension::Extension::actions`).
    // Header-Buttons landen als DataFieldForAction in UI.Identification der Entitaet;
    // pro Entitaet genau eine Collection.
    let mut header_actions: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for a in crate::extension::bound_actions(&state.extensions) {
        let Some(entity) = ddic.active_entity_by_setname(&a.set) else {
            continue;
        };
        let entity_name = format!("Service.{}", entity.name());
        let returns = a
            .returns
            .as_deref()
            .map(|r| format!(r##"<ReturnType Type="Service.{r}"/>"##))
            .unwrap_or_default();
        actions.push(format!(
            r##"<Action Name="{}" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="{entity_name}"/>{returns}</Action>"##,
            a.name
        ));
        if let Some(avail) = &a.available_if {
            match &avail.equals {
                Value::Bool(b) => annotations.push(format!(
                    r##"<Annotations Target="Service.{}(Service.{})"><Annotation Term="Core.OperationAvailable"><Eq><Path>in/{}</Path><Bool>{b}</Bool></Eq></Annotation></Annotations>"##,
                    a.name,
                    entity.name(),
                    avail.property
                )),
                Value::String(t) => annotations.push(format!(
                    r##"<Annotations Target="Service.{}(Service.{})"><Annotation Term="Core.OperationAvailable"><Eq><Path>in/{}</Path><String>{t}</String></Eq></Annotation></Annotations>"##,
                    a.name,
                    entity.name(),
                    avail.property
                )),
                other => tracing::warn!("availability of {} unsupported: {other}", a.name),
            }
        }
        // Die Eigenschaft der Verfuegbarkeitsbedingung muss vom Client angefordert
        // werden, sonst wertet Fiori Elements sie nicht aus. Ausgeblendetes DataField.
        if let Some(avail) = &a.available_if {
            header_actions
                .entry(entity_name.clone())
                .or_default()
                .push(format!(
                    r##"<Record Type="UI.DataField"><PropertyValue Property="Value" Path="{}"/><Annotation Term="UI.Hidden"/></Record>"##,
                    avail.property
                ));
        }
        if let Some(header) = &a.header {
            // Sichtbarkeit: ausgeblendet, solange die Bedingung nicht erfuellt ist
            // (`UI.Hidden` ist fuer Header-Buttons ausgewertet, `OperationAvailable` nicht).
            let hidden = match &a.available_if {
                Some(avail) => {
                    let literal = match &avail.equals {
                        Value::Bool(b) => format!("<Bool>{b}</Bool>"),
                        Value::String(t) => format!("<String>{t}</String>"),
                        _ => String::new(),
                    };
                    if literal.is_empty() {
                        String::new()
                    } else {
                        format!(
                            r##"<Annotation Term="UI.Hidden"><Ne><Path>{}</Path>{literal}</Ne></Annotation>"##,
                            avail.property
                        )
                    }
                }
                None => String::new(),
            };
            header_actions.entry(entity_name).or_default().push(format!(
                r##"<Record Type="UI.DataFieldForAction"><PropertyValue Property="Action" String="Service.{}"/><PropertyValue Property="Label" String="{}"/>{hidden}</Record>"##,
                a.name, header.text
            ));
        }
    }
    for (entity_name, records) in header_actions {
        annotations.push(format!(
            r##"<Annotations Target="{entity_name}"><Annotation Term="UI.Identification"><Collection>{}</Collection></Annotation></Annotations>"##,
            records.concat()
        ));
    }

    // let cfg = [cfg1, cfg2, cfg3].concat();
    let xml = meta
        .replace("%TYPE-DEFINITIONS%", &type_defs.concat())
        .replace("%ENTITY-SETS%", &entity_sets.concat())
        .replace("%ACTION-IMPORTS%", "")
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
      %ACTION-IMPORTS%
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
        let mut ddic = DataDictionary::new();
        ddic.activate();
        let entity = ddic.active_entity("Entity").unwrap();

        let entity_type = EntityType::from_entity(entity);
        dbg!(&entity_type);
        assert!(entity_type
            .properties
            .iter()
            .any(|property| property.name == "name" && property.type_ == "Edm.String"));
    }

    #[test]
    fn lookup_to_app_entity_gets_intent_based_navigation() {
        let mut ddic = DataDictionary::new();
        ddic.activate();
        let (entity, target, name) = ddic
            .active_entities()
            .find_map(|e| {
                e.fields().find_map(|f| match f {
                    FieldSpec::Lookup {
                        name,
                        entity,
                        collection: false,
                        ..
                    } => Some((e.clone(), entity.clone(), name.clone())),
                    _ => None,
                })
            })
            .expect("ein Einzel-Lookup im Standard-Dictionary");
        let fk = format!("{name}_ID");

        let linked = Annotations::from_entity(&entity, &[], std::slice::from_ref(&target));
        let xml = format!("{linked:?}");
        assert!(xml.contains("UI.DataFieldWithIntentBasedNavigation"));
        assert!(xml.contains(&target));

        let plain = Annotations::from_entity(&entity, &[], &[]);
        let xml = format!("{plain:?}");
        assert!(!xml.contains("UI.DataFieldWithIntentBasedNavigation"));
        assert!(xml.contains(&fk));
    }

    #[test]
    fn facet_target_uses_the_configured_view() {
        let mut x = EntitySpec::new("Actor", "core", "Actors");
        assert_eq!(facet_target_path(&x, "_ActorSceneMap"), "_ActorSceneMap/@UI.LineItem");
        x.facets.push(spec::FacetSpec { facet: "_ActorSceneMap".into(), view: Some("reverse".into()), title: None, fields: vec![] });
        assert_eq!(
            facet_target_path(&x, "_ActorSceneMap"),
            "_ActorSceneMap/@UI.LineItem#reverse"
        );
        x.facets[0].view = Some("default".into());
        assert_eq!(facet_target_path(&x, "_ActorSceneMap"), "_ActorSceneMap/@UI.LineItem");
    }

    #[test]
    fn facet_title_prefers_the_model_label_of_the_navigation() {
        let mut x = EntitySpec::new("Actor", "core", "Actors");
        x.fields.push(FieldSpec::Lookup {
            name: "_ActorSceneMap".into(),
            entity: "ActorSceneMap".into(),
            collection: true,
            owned: false,
            label: Some("Scene assignments".into()),
        });
        assert_eq!(facet_title(&x, "_ActorSceneMap"), "Scene assignments");
        x.facets.push(spec::FacetSpec { facet: "_ActorSceneMap".into(), view: None, title: Some("Szenen".into()), fields: vec![] });
        assert_eq!(facet_title(&x, "_ActorSceneMap"), "Szenen");
    }

    #[test]
    fn facet_title_defaults_to_the_navigation_name() {
        let mut x = EntitySpec::new("Actor", "core", "Actors");
        assert_eq!(facet_title(&x, "_ActorSceneMap"), "_ActorSceneMap");
        x.facets.push(spec::FacetSpec { facet: "_ActorSceneMap".into(), view: None, title: Some("Szenen".into()), fields: vec![] });
        assert_eq!(facet_title(&x, "_ActorSceneMap"), "Szenen");
    }

    #[test]
    fn facets_include_a_list_section_per_to_many_navigation() {
        let mut ddic = DataDictionary::new();
        ddic.activate();
        // "Entity" has to-many lookups _Fields, _Many, _One after activation.
        let entity = ddic.active_entity("Entity").unwrap();

        let annotations = Annotations::from_entity(entity, &[], &[]);
        let main = annotations
            .iter()
            .find(|a| a.target == "Service.Entity")
            .unwrap();
        let facets = main
            .annotations
            .iter()
            .find(|a| a.term == "UI.Facets")
            .unwrap()
            .collection
            .as_ref()
            .unwrap();

        for nav_name in ["_Fields", "_Many", "_One"] {
            let record = facets
                .records
                .iter()
                .find(|r| {
                    r.property_values.iter().any(|pv| {
                        pv.property == "ID"
                            && pv.string.as_deref() == Some(&format!("{nav_name}Section"))
                    })
                })
                .unwrap_or_else(|| panic!("no list facet generated for {nav_name}"));
            let target = record
                .property_values
                .iter()
                .find(|pv| pv.property == "Target")
                .unwrap();
            assert_eq!(
                target.annotation_path.as_deref(),
                Some(format!("{nav_name}/@UI.LineItem").as_str())
            );
        }
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
        assert!(value_list_annotations
            .annotations
            .iter()
            .any(|annotation| annotation.term == "Org.OData.Core.V1.Computed"));
        assert!(value_list_annotations
            .annotations
            .iter()
            .any(|annotation| annotation.term == "Org.OData.Core.V1.Immutable"));
        assert!(value_list_annotations
            .annotations
            .iter()
            .any(|annotation| annotation.term == "Common.ValueList"));
        assert!(value_list_annotations
            .annotations
            .iter()
            .any(|annotation| annotation.term == "Common.ValueListWithFixedValues"));

        let measure = FieldSpec::Measure {
            name: "amount".into(),
            label: "Amount".into(),
            package: None,
            precision: Some(15),
            scale: Some(2),
            unit_field: "currency".into(),
            kind: MeasureKind::Currency,
        };
        let measure_annotations = Annotations::from_field("Invoice", &measure);

        assert!(measure_annotations.annotations.iter().any(|annotation| {
            annotation.term == "Org.OData.Measures.V1.ISOCurrency"
                && annotation.path.as_deref() == Some("currency")
        }));
    }

    #[test]
    fn parses_simple_expand_list() {
        let expand = parse_expand_items("DraftAdministrativeData,package").unwrap();

        assert_eq!(expand.len(), 2);
        assert_eq!(expand[0].navigation_property, "DraftAdministrativeData");
        assert_eq!(expand[1].navigation_property, "package");
    }

    #[test]
    fn parses_expand_with_select_options() {
        let expand =
            parse_expand_items("DraftAdministrativeData($select=DraftUUID,InProcessByUser)")
                .unwrap();

        assert_eq!(expand.len(), 1);
        assert_eq!(expand[0].navigation_property, "DraftAdministrativeData");
        assert_eq!(
            expand[0].nested_query.select,
            vec!["DraftUUID".to_string(), "InProcessByUser".to_string()]
        );
    }

    #[test]
    fn parses_nested_expand_options() {
        let expand = parse_expand_items("_Entities($expand=package($select=name))").unwrap();

        assert_eq!(expand.len(), 1);
        assert_eq!(expand[0].navigation_property, "_Entities");
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

    #[test]
    fn parses_orderby_list_with_directions() {
        let items = parse_orderby("ID,Amount desc, name ASC").unwrap();

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].property, "ID");
        assert_eq!(items[0].direction, SortDirection::Asc);
        assert_eq!(items[1].property, "Amount");
        assert_eq!(items[1].direction, SortDirection::Desc);
        assert_eq!(items[2].property, "name");
        assert_eq!(items[2].direction, SortDirection::Asc);
    }

    #[test]
    fn invalid_orderby_direction_is_bad_request() {
        assert!(parse_orderby("Amount sideways").is_err());
        assert!(parse_orderby("Amount desc extra").is_err());
    }

    #[test]
    fn build_query_passes_orderby_through() {
        let mut params = HashMap::new();
        params.insert("$orderby".to_string(), "ID desc".to_string());
        let query = build_query(&params).unwrap();

        assert_eq!(query.orderby.len(), 1);
        assert_eq!(query.orderby[0].property, "ID");
        assert_eq!(query.orderby[0].direction, SortDirection::Desc);
    }

    #[test]
    fn expand_option_orderby_is_parsed() {
        let expand = parse_expand_items("_Entities($orderby=name desc)").unwrap();

        assert_eq!(expand[0].nested_query.orderby.len(), 1);
        assert_eq!(expand[0].nested_query.orderby[0].property, "name");
        assert_eq!(
            expand[0].nested_query.orderby[0].direction,
            SortDirection::Desc
        );
    }
}

#[cfg(test)]
mod list_view_annotation_tests {
    use super::*;
    use spec::{FieldSpec, ListView, Relationship};

    fn atom(name: &str) -> FieldSpec {
        FieldSpec::Atom {
            name: name.into(),
            label: name.into(),
            edm_type: "Edm.String".into(),
            package: None,
            max_length: None,
            precision: None,
            scale: None,
            computed: false,
            immutable: false,
            value_list: None,
            link: false,
        }
    }

    #[test]
    fn link_atoms_become_clickable_fields() {
        let mut e = active_video(vec![ListView::new("default", &["title"])]);
        for f in e.fields.iter_mut() {
            if let FieldSpec::Atom { name, link, .. } = f {
                if name == "title" {
                    *link = true;
                }
            }
        }
        let fg = serde_json::to_string(&field_group("Main", &e, None, &[], &[])).unwrap();
        assert!(fg.contains("UI.DataFieldWithUrl"), "link-Atom wird DataFieldWithUrl");
        let anns = line_item_annotations(&e);
        let li = serde_json::to_string(&anns[0]).unwrap();
        assert!(li.contains("UI.DataFieldWithUrl"));
        assert!(!serde_json::to_string(&field_group("Main", &active_video(vec![]), None, &[], &[])).unwrap().contains("DataFieldWithUrl"));
    }

    #[test]
    fn header_type_names_are_display_texts_not_technical_names() {
        let mut e = active_video(vec![]);
        let header = |e: &spec::EntitySpec| {
            let anns = Annotations::from_entity(e, &[], &[]);
            let main = anns.iter().find(|a| a.target == "Service.Video").unwrap();
            serde_json::to_string(main.annotations.iter().find(|a| a.term == "UI.HeaderInfo").unwrap()).unwrap()
        };
        let h = header(&e);
        assert!(h.contains("\"Videos\""), "Plural aus type_name_plural");
        assert!(!h.contains("VideoSet"));
        e.type_name = Some("Film".into());
        assert!(header(&e).contains("\"Film\""), "TypeName aus type_name");
    }

    fn active_video(views: Vec<ListView>) -> spec::EntitySpec {
        let mut ddic = spec::data_dictionary::DataDictionary::empty();
        ddic.add_package(spec::package::Package::new("core"));
        ddic.add_entity(spec::EntitySpec::new("Agent", "core", "Agents").field(atom("name")));
        let mut video = spec::EntitySpec::new("Video", "core", "Videos")
            .field(atom("name"))
            .field(atom("title"));
        for v in views {
            video = video.list_view(v);
        }
        ddic.add_entity(video);
        ddic.add_relationship(
            Relationship::new("videos_agent_id", "core")
                .with_one("Agent", "_Videos")
                .with_many("Video", "agent"),
        );
        ddic.validate_list_views().expect("valid list views");
        ddic.activate();
        ddic.active_entity("Video").unwrap().clone()
    }

    fn find<'a>(anns: &'a [Annotation], term: &str, qualifier: Option<&str>) -> &'a Annotation {
        anns.iter()
            .find(|a| a.term == term && a.qualifier.as_deref() == qualifier)
            .unwrap_or_else(|| panic!("{term} qualifier={qualifier:?} fehlt"))
    }

    fn datafield_paths(a: &Annotation) -> Vec<String> {
        a.collection
            .as_ref()
            .unwrap()
            .records
            .iter()
            .map(|r| {
                r.property_values
                    .iter()
                    .find(|p| p.property == "Value")
                    .and_then(|p| p.path.clone())
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn selection_fields_become_ui_selection_fields() {
        let mut e = active_video(vec![]);
        e.selection_fields = vec!["title".into(), "name".into()];
        let anns = Annotations::from_entity(&e, &[], &[]);
        let main = anns.iter().find(|a| a.target == "Service.Video").unwrap();
        let sel = main
            .annotations
            .iter()
            .find(|a| a.term == "UI.SelectionFields")
            .expect("UI.SelectionFields fehlt");
        assert_eq!(datafield_paths_of_property_paths(sel), vec!["title", "name"]);
    }

    fn datafield_paths_of_property_paths(a: &Annotation) -> Vec<String> {
        a.collection.as_ref().unwrap().property_paths.clone()
    }

    #[test]
    fn filter_defaults_become_property_annotations() {
        let mut e = active_video(vec![]);
        e.filter_defaults.insert("name".into(), spec::FilterDefault::Text("Demo".into()));
        e.filter_defaults.insert("title".into(), spec::FilterDefault::Flag(true));
        let anns = Annotations::from_entity(&e, &[], &[]);
        let on = |target: &str| {
            anns.iter()
                .filter(|a| a.target == target)
                .flat_map(|a| a.annotations.iter())
                .find(|a| a.term == "Common.FilterDefaultValue")
                .expect("FilterDefaultValue fehlt")
        };
        assert_eq!(on("Service.Video/name").string.as_deref(), Some("Demo"));
        assert_eq!(on("Service.Video/title").bool_, Some(true));
    }

    #[test]
    fn general_facet_with_fields_restricts_the_main_group() {
        let mut e = active_video(vec![]);
        e.facets.push(spec::FacetSpec { facet: "General".into(), view: None, title: None, fields: vec!["name".into()] });
        let (_, groups) = object_page_sections(&e, &[], &[]);
        let main = serde_json::to_string(&groups[0]).unwrap();
        assert!(main.contains("\"name\""));
        assert!(!main.contains("\"title\""));
    }

    #[test]
    fn default_facet_collects_unmentioned_fields() {
        let mut e = active_video(vec![]);
        e.facets.push(spec::FacetSpec { facet: "General".into(), view: None, title: None, fields: vec!["name".into()] });
        e.facets.push(spec::FacetSpec { facet: "Default".into(), view: None, title: Some("Weitere".into()), fields: vec![] });
        let (records, groups) = object_page_sections(&e, &[], &[]);
        let rest = groups.iter().find(|g| g.qualifier.as_deref() == Some("Default")).unwrap();
        let rest = serde_json::to_string(rest).unwrap();
        assert!(rest.contains("\"title\""));
        assert!(!rest.contains("\"name\""), "genannte Felder stehen nicht in Default");
        let default_ref = serde_json::to_string(&records).unwrap();
        assert!(default_ref.contains("Weitere"));
    }

    #[test]
    fn custom_facet_gets_its_own_field_group() {
        let mut e = active_video(vec![]);
        e.facets.push(spec::FacetSpec { facet: "Meta".into(), view: None, title: Some("Meta".into()), fields: vec!["title".into()] });
        let (_, groups) = object_page_sections(&e, &[], &[]);
        let meta = groups.iter().find(|g| g.qualifier.as_deref() == Some("Meta")).unwrap();
        assert!(serde_json::to_string(meta).unwrap().contains("\"title\""));
        // Ohne General-Konfiguration bleibt der Hauptabschnitt mit allen Feldern erhalten.
        let main = groups.iter().find(|g| g.qualifier.as_deref() == Some("Main")).unwrap();
        assert!(serde_json::to_string(main).unwrap().contains("\"name\""));
    }

    #[test]
    fn without_views_line_item_shows_name_only() {
        let e = active_video(vec![]);
        let anns = line_item_annotations(&e);
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].qualifier, None);
        assert_eq!(datafield_paths(&anns[0]), vec!["name"]);
    }

    #[test]
    fn default_and_named_views_get_qualifiers_and_navigation_paths() {
        let e = active_video(vec![
            ListView::new("default", &["name", "agent", "title"]),
            ListView::new("review", &["title"]),
        ]);
        let anns = line_item_annotations(&e);

        let default = find(&anns, "UI.LineItem", None);
        assert_eq!(datafield_paths(default), vec!["name", "agent/name", "title"]);

        let review = find(&anns, "UI.LineItem", Some("review"));
        assert_eq!(datafield_paths(review), vec!["title"]);

        let pv_default = find(&anns, "UI.PresentationVariant", None);
        let vis = pv_default
            .record
            .as_ref()
            .unwrap()
            .property_values
            .iter()
            .find(|p| p.property == "Visualizations")
            .unwrap()
            .collection
            .as_ref()
            .unwrap();
        assert_eq!(vis.annotation_paths, vec!["@UI.LineItem"]);

        let pv_review = find(&anns, "UI.PresentationVariant", Some("review"));
        let vis = pv_review
            .record
            .as_ref()
            .unwrap()
            .property_values
            .iter()
            .find(|p| p.property == "Visualizations")
            .unwrap()
            .collection
            .as_ref()
            .unwrap();
        assert_eq!(vis.annotation_paths, vec!["@UI.LineItem#review"]);
    }

    #[test]
    fn views_settings_only_with_more_than_one_view() {
        let single = active_video(vec![ListView::new("default", &["name"])]);
        assert!(crate::flp::list_views_settings(&single, &[]).is_empty());

        let multi = active_video(vec![
            ListView::new("review", &["title"]),
            ListView::new("default", &["name"]),
        ]);
        let settings = crate::flp::list_views_settings(&multi, &[]);
        let paths = settings["views"]["paths"].as_array().unwrap();
        assert_eq!(paths[0]["key"], "default");
        assert_eq!(
            paths[1]["annotationPath"],
            "com.sap.vocabularies.UI.v1.PresentationVariant#review"
        );
    }
}

#[cfg(test)]
mod value_list_annotation_tests {
    use super::*;

    /// Konstante Werte muessen als `String="..."` ausgegeben werden; `Constant="..."`
    /// ist kein gueltiges CSDL und laesst die Oberflaeche die Wertehilfe verwerfen.
    #[test]
    fn list_id_constant_is_emitted_as_string() {
        let vl = AtomValueList::FieldValueList {
            list_id: "abc".into(),
            prefer_dialog: false,
        };
        let json = serde_json::to_string(&annotations_for_atom_value_list("status", &vl)).unwrap();
        assert!(!json.contains("@Constant"), "{json}");
        assert!(json.contains("\"@String\":\"abc\""), "{json}");
    }
}
