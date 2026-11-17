use serde::{Deserialize, Serialize};

use super::{
    Annotation, Collection, PropertyValue, Record, annotations_for_atom_value_list,
    line_item_annotations, object_page_sections, target_key_of, value_list_annotation,
    value_list_parameter_in,
};
use spec::{EntitySpec, FieldSpec, MeasureKind, data_dictionary::DataDictionary};

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
    pub(crate) fn from_entity(
        ddic: &DataDictionary,
        x: &EntitySpec,
        hidden: &[String],
        app_entities: &[String],
    ) -> Vec<Annotations> {
        let mut annotations = Vec::new();
        let mut main_annotations = Annotations {
            target: format!("Service.{}", x.name()),
            annotations: Vec::new(),
        };

        main_annotations.annotations.push(
            Annotation::new("UI.HeaderInfo").record(
                Record::new("UI.HeaderInfoType")
                    .add(PropertyValue::new("TypeName").string(x.type_name()))
                    .add(PropertyValue::new("TypeNamePlural").string(x.type_name_plural()))
                    .add(PropertyValue::new("Title").record(
                        Record::new("UI.DataField").add(PropertyValue::new("Value").path("name")),
                    )),
            ),
        );

        main_annotations
            .annotations
            .extend(line_item_annotations(ddic, x, app_entities));
        // Nur App-Hauptentitaeten sind Sprungziele. Bei Join-Entitaeten (M:N ohne eigenen
        // Inhalt) wuerde der Schluessel-Fallback die ID der Zwischenzeile statt des Ziels senden.
        if app_entities.iter().any(|a| a == x.name()) {
            main_annotations.annotations.push(
                Annotation::new("Common.SemanticKey")
                    .collection(Collection::new().add_property_path(x.primary_key_name())),
            );
        }
        if !x.selection_fields().is_empty() {
            let mut paths = Collection::new();
            for field in x.selection_fields() {
                // Einwertige Navigation filtert ueber den Fremdschluessel.
                let is_to_one = x.fields().any(|f| {
                    matches!(f, FieldSpec::Lookup { name, collection: false, .. } if name == field)
                });
                let path = match x.fields().find_map(|f| match f {
                    FieldSpec::Lookup {
                        name,
                        entity,
                        collection: false,
                        ..
                    } if name == field => Some(entity.as_str()),
                    _ => None,
                }) {
                    Some(target) if is_to_one => ddic.fk_column(target, field),
                    _ => field.clone(),
                };
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
        let (sections, groups) = object_page_sections(ddic, x, hidden, app_entities);
        let mut facets = Collection::new();
        for record in sections {
            facets = facets.add_record(record);
        }
        main_annotations
            .annotations
            .push(Annotation::new("UI.Facets").collection(facets));
        main_annotations.annotations.extend(groups);

        // Side Effects auf Entity-Ebene: je Quellfeld ein Eintrag mit seinen Zielfeldern.
        for f in x.fields() {
            if let FieldSpec::Atom { side_effects, .. } = f {
                if side_effects.is_empty() {
                    continue;
                }
                let mut targets = Collection::new();
                for t in side_effects {
                    targets = targets.add_property_path(t);
                }
                // Je Quellfeld ein eigener Qualifier: ohne ihn ueberschreiben sich die
                // Eintraege gleichen Terms, und FE kennt nur den letzten.
                let mut ann = Annotation::new("Common.SideEffects").record(
                    Record::new("Common.SideEffectsType")
                        .add(
                            PropertyValue::new("SourceProperties")
                                .collection(Collection::new().add_property_path(f.name())),
                        )
                        .add(PropertyValue::new("TargetProperties").collection(targets)),
                );
                ann.qualifier = Some(f.name().to_string());
                main_annotations.annotations.push(ann);
            }
        }

        annotations.push(main_annotations);

        if x.primary_key().is_none() {
            annotations.push(Self::from_field(
                ddic,
                x.name(),
                &FieldSpec::Atom {
                    name: "ID".into(),
                    label: x.name().into(),
                    edm_type: "Edm.Guid".into(),
                    package: None,
                    max_length: None,
                    precision: None,
                    scale: None,
                    computed: true,
                    immutable: false,
                    value_list: None,
                    unit_scale_field: None,
                    code_list: None,
                    link: false,
                    multiline: false,
                    side_effects: Vec::new(),
                    ui: None,
                },
            ));
        }
        for f in x.fields() {
            annotations.push(Self::from_field(ddic, x.name(), f));
            // Begleitfeld einer Measure: SAP kennzeichnet es mit IsUnit bzw. IsCurrency,
            // sonst wird es in der Measure-Darstellung nicht als bearbeitbare Einheit behandelt.
            if let FieldSpec::Measure {
                unit_field, kind, ..
            } = f
            {
                let term = match kind {
                    MeasureKind::Currency => "Common.IsCurrency",
                    MeasureKind::Unit => "Common.IsUnit",
                };
                annotations.push(Annotations {
                    target: format!("Service.{}/{}", x.name(), unit_field),
                    annotations: vec![Annotation::new(term)],
                });
            }
        }
        for (field, value) in x.filter_defaults() {
            let annotation = match value {
                spec::FilterDefault::Text(t) => {
                    Annotation::new("Common.FilterDefaultValue").string(t)
                }
                spec::FilterDefault::Flag(b) => {
                    Annotation::new("Common.FilterDefaultValue").boolean(*b)
                }
            };
            annotations.push(Annotations {
                target: format!("Service.{}/{}", x.name(), field),
                annotations: vec![annotation],
            });
        }
        merge_by_target(annotations)
    }

    pub(crate) fn from_field(
        ddic: &DataDictionary,
        entity_name: &str,
        f: &spec::FieldSpec,
    ) -> Annotations {
        let mut target = format!("Service.{}/{}", entity_name, f.name());
        // let mut annotations = vec![Annotation::new("Common.Label").string(f.label())];
        let mut annotations = vec![Annotation::new("Common.Label").string(format!(
            "{{@i18n>{}.{}}}",
            entity_name,
            f.name()
        ))];
        if matches!(
            f,
            FieldSpec::Atom {
                multiline: true,
                ..
            }
        ) {
            annotations.push(Annotation::new("UI.MultiLineText"));
        }
        match f {
            FieldSpec::Lookup {
                name,
                entity,
                collection,
                owned: _,
                ..
            } => {
                if !collection {
                    if spec::system_user::is_read_only_audit_field(f.name()) {
                        annotations
                            .push(Annotation::new("Org.OData.Core.V1.Computed").boolean(true));
                    }
                    let name = ddic.fk_column(entity, name);
                    target = format!("Service.{}/{}", entity_name, name);
                    annotations.push(
                        Annotation::new("Common.Text")
                            .path(format!("{}/name", f.name()))
                            .annotation(
                                Annotation::new("UI.TextArrangement")
                                    .enum_member("UI.TextArrangementType/TextOnly"),
                            ),
                    );

                    let mut parameters = Collection::new().add_record(
                        Record::new("Common.ValueListParameterOut")
                            .add(PropertyValue::new("LocalDataProperty").property_path(name))
                            .add(
                                PropertyValue::new("ValueListProperty")
                                    .string(target_key_of(ddic, entity).0),
                            ),
                    );
                    if let Some(relationship) = ddic.relationships().find(|relationship| {
                        relationship.many_opt() == Some(entity_name)
                            && relationship.many_property_opt() == Some(f.name())
                    }) {
                        for filter in relationship.value_list_filters() {
                            let local_property = lookup_value_list_property(
                                ddic,
                                entity_name,
                                filter.local_property(),
                            );
                            let target_property =
                                lookup_value_list_property(ddic, entity, filter.target_property());
                            parameters = parameters.add_record(value_list_parameter_in(
                                &local_property,
                                &target_property,
                            ));
                        }
                    }
                    parameters = parameters.add_record(
                        Record::new("Common.ValueListParameterDisplayOnly")
                            .add(PropertyValue::new("ValueListProperty").string("name")),
                    );
                    annotations.push(value_list_annotation(
                        format!("{entity}Set"),
                        false,
                        parameters,
                    ));
                }
            }
            FieldSpec::Atom {
                name,
                computed,
                immutable,
                value_list,
                unit_scale_field,
                ..
            } => {
                if *computed || spec::system_user::is_read_only_audit_field(name) {
                    annotations.push(Annotation::new("Org.OData.Core.V1.Computed").boolean(true));
                }
                if let Some(scale) = unit_scale_field {
                    annotations.push(Annotation::new("Common.UnitSpecificScale").path(scale));
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

fn lookup_value_list_property(ddic: &DataDictionary, entity_name: &str, property: &str) -> String {
    let Some(FieldSpec::Lookup {
        name,
        entity,
        collection: false,
        ..
    }) = ddic
        .active_entity(entity_name)
        .and_then(|entity| entity.field_by_name(property))
    else {
        return property.to_string();
    };
    ddic.fk_column(entity, name)
}

/// Fasst `<Annotations>`-Blöcke mit gleichem Target zusammen (Reihenfolge bleibt erhalten).
/// Sonst entstehen z. B. für ein Begleitfeld einer Measure zwei Blöcke (Feld-Annotationen
/// und `Common.IsCurrency`/`IsUnit`).
fn merge_by_target(blocks: Vec<Annotations>) -> Vec<Annotations> {
    let mut out: Vec<Annotations> = Vec::new();
    for block in blocks {
        match out.iter_mut().find(|b| b.target == block.target) {
            Some(existing) => existing.annotations.extend(block.annotations),
            None => out.push(block),
        }
    }
    out
}
