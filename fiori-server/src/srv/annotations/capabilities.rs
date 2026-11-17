use spec::FieldSpec;
use spec::{EntitySpec, data_dictionary::DataDictionary};

/// Code-Listen-Annotationen fuer Einheiten/Waehrungen (SAP CodeList.v1).
///
/// Die Runtime (UI5 `ODataMetaModel.requestCodeList`) laedt die Nachkommastellen nur,
/// wenn der Container `CodeList.v1.<term>` auf ein EntitySet zeigt und der Schluessel
/// ueber einen Alternativschluessel (`Core.AlternateKeys`) mit `Common.UnitSpecificScale`
/// und `Common.Text` erreichbar ist. Ohne diese Annotationen greift fuer `Scale="variable"`
/// der Standardwert, und alle Einheiten erscheinen mit gleicher Stellenzahl.
pub(crate) fn code_list_annotations(entity: &EntitySpec) -> Vec<String> {
    let Some((code, scale, term)) = entity.fields().find_map(|f| match f {
        FieldSpec::Atom {
            name,
            unit_scale_field: Some(scale),
            code_list: Some(term),
            ..
        } => Some((name.clone(), scale.clone(), term.clone())),
        _ => None,
    }) else {
        return vec![];
    };
    let entity_name = entity.name();
    let set = entity.setname();
    let key = entity.primary_key_name();
    // UI5 liest UnitSpecificScale am Primaerschluessel ($Key), nicht am
    // Alternativschluessel. Der Code selbst ist ueber AlternateKeys angebunden.
    vec![
        format!(
            r#"<Annotations Target="Service.{entity_name}/{key}"><Annotation Term="Common.UnitSpecificScale" Path="{scale}"/></Annotations>"#
        ),
        format!(
            r#"<Annotations Target="Service.{entity_name}"><Annotation Term="Org.OData.Core.V1.AlternateKeys"><Collection><Record><PropertyValue Property="Key"><Collection><Record><PropertyValue Property="Name" PropertyPath="{code}"/><PropertyValue Property="Alias" String="{code}"/></Record></Collection></PropertyValue></Record></Collection></Annotation></Annotations>"#
        ),
        format!(
            r#"<Annotations Target="Service.EntityContainer"><Annotation Term="CodeList.{term}"><Record><PropertyValue Property="CollectionPath" String="{set}"/><PropertyValue Property="Url" String="."/></Record></Annotation></Annotations>"#
        ),
    ]
}

pub(crate) fn capability_annotations_for_entity(
    ddic: &DataDictionary,
    entity: &EntitySpec,
) -> Vec<String> {
    let entity_name = entity.name();
    let set_name = entity.setname();
    if !entity.draft_enabled() {
        let generated_id = if entity.primary_key().is_none() {
            "<PropertyPath>ID</PropertyPath>"
        } else {
            ""
        };
        let mut annotations = vec![format!(
            r##"<Annotations Target="Service.EntityContainer/{set_name}"><Annotation Term="Capabilities.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Capabilities.DeleteRestrictions"><Record><PropertyValue Property="Deletable" Bool="true"/></Record></Annotation><Annotation Term="Capabilities.InsertRestrictions"><Record><PropertyValue Property="Insertable" Bool="true"/><PropertyValue Property="NonInsertableProperties"><Collection>{generated_id}</Collection></PropertyValue></Record></Annotation>{}</Annotations>"##,
            navigation_restrictions(ddic, entity)
        )];
        if entity.primary_key().is_none() {
            annotations.push(format!(
                r##"<Annotations Target="Service.{entity_name}/ID"><Annotation Term="Core.Computed" Bool="true"/></Annotations>"##
            ));
        }
        return annotations;
    }
    let generated_id = if entity.primary_key().is_none() {
        "<PropertyPath>ID</PropertyPath>"
    } else {
        ""
    };
    // Ein Kind einer Composition ist DraftNode, alles andere ein eigenstaendiger DraftRoot.
    let draft_role = match ddic.owning_relationship(entity_name) {
        Some(_) => r##"<Annotation Term="Common.DraftNode"><Record Type="Common.DraftNodeType"><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation>"##.to_string(),
        None => r##"<Annotation Term="Common.DraftRoot"><Record Type="Common.DraftRootType"><PropertyValue Property="ActivationAction" String="Service.draftActivate"/><PropertyValue Property="EditAction" String="Service.draftEdit"/><PropertyValue Property="PreparationAction" String="Service.draftPrepare"/></Record></Annotation>"##.to_string(),
    };

    let result = vec![
        format!(
            r##"<Annotations Target="Service.EntityContainer/{set_name}"><Annotation Term="Org.OData.Capabilities.V1.UpdateRestrictions"><Record><PropertyValue Property="Updatable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.DeleteRestrictions"><Record><PropertyValue Property="Deletable" Bool="true"/></Record></Annotation><Annotation Term="Org.OData.Capabilities.V1.InsertRestrictions"><Record Type="Capabilities.InsertRestrictionsType"><PropertyValue Property="Insertable" Bool="true"/><PropertyValue Property="NonInsertableProperties"><Collection>{generated_id}<PropertyPath>IsActiveEntity</PropertyPath><PropertyPath>HasActiveEntity</PropertyPath><PropertyPath>HasDraftEntity</PropertyPath></Collection></PropertyValue></Record></Annotation>{draft_role}{navigation_restrictions}</Annotations>"##,
            generated_id = generated_id,
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
