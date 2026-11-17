use axum::{
    extract::State,
    http::{HeaderName, header},
    response::{IntoResponse, Response},
};
use serde_json::Value;

use super::{
    Annotations, EntitySet, EntityType, app_entity_names, capability_annotations_for_entity,
    code_list_annotations,
};
use crate::AppState;

pub(crate) async fn metadata(State(state): State<AppState>) -> Response {
    let (state, _session_guard) = match crate::sticky::request_state(&state).await {
        Ok(state) => state,
        Err(response) => return response,
    };
    let ddic = state.ddic();
    let entity_lst: Vec<EntityType> = ddic
        .active_entities()
        .map(|x| EntityType::from_entity(&ddic, x))
        .collect();
    let entity_set_lst: Vec<EntitySet> = ddic
        .active_entities()
        .map(|x| EntitySet::from_entity(&ddic, x))
        .collect();
    let app_entities = app_entity_names(&ddic);
    let annotation_lst: Vec<Annotations> = ddic
        .active_entities()
        .map(|x| {
            Annotations::from_entity(
                &ddic,
                x,
                &crate::extension::hidden_fields(&state.extensions, x.name()),
                &app_entities,
            )
        })
        .flatten()
        .collect();

    let meta = edmx_template(ddic.active_entities().any(|e| e.draft_enabled()));
    let mut type_defs = vec![];
    for x in entity_lst {
        type_defs.push(quick_xml::se::to_string(&x).unwrap());
    }
    let mut entity_sets = vec![];
    for x in entity_set_lst {
        entity_sets.push(quick_xml::se::to_string(&x).unwrap());
    }
    let mut annotations = vec![];
    for x in annotation_lst {
        annotations.push(quick_xml::se::to_string(&x).unwrap());
    }
    for x in ddic.active_entities() {
        annotations.extend(capability_annotations_for_entity(&ddic, x));
    }
    for x in ddic.active_entities() {
        annotations.extend(code_list_annotations(x));
    }

    let mut actions = vec![];
    let mut action_imports = String::new();
    if state.sticky.is_some() {
        action_imports
            .push_str(r#"<ActionImport Name="stickyDiscard" Action="Service.stickyDiscard"/>"#);
        actions.push(r#"<Action Name="stickyDiscard"/>"#.to_string());
        for entity in ddic
            .active_entities()
            .filter(|entity| !entity.draft_enabled())
        {
            let name = entity.name();
            let set = entity.setname();
            actions.push(format!(
                r#"<Action Name="stickyNew" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="Collection(Service.{name})"/><ReturnType Type="Service.{name}"/></Action>
                <Action Name="stickyEdit" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="Service.{name}"/><ReturnType Type="Service.{name}"/></Action>
                <Action Name="stickySave" IsBound="true" EntitySetPath="in"><Parameter Name="in" Type="Service.{name}"/><ReturnType Type="Service.{name}"/></Action>"#
            ));
            annotations.push(format!(
                r#"<Annotations Target="Service.EntityContainer/{set}"><Annotation Term="Session.StickySessionSupported"><Record Type="Session.StickySessionSupportedType"><PropertyValue Property="NewAction" String="Service.stickyNew"/><PropertyValue Property="EditAction" String="Service.stickyEdit"/><PropertyValue Property="SaveAction" String="Service.stickySave"/><PropertyValue Property="DiscardAction" String="stickyDiscard"/></Record></Annotation></Annotations>"#
            ));
        }
    }
    for e in ddic.active_entities() {
        let entity_name = format!("Service.{}", e.name());
        if e.draft_enabled() {
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
        }
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
                a.name, quick_xml::escape::escape(&header.text)
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
        .replace("%ACTION-IMPORTS%", &action_imports)
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
        .into_response()
}

fn edmx_template(drafts: bool) -> String {
    let meta = r##"<?xml version="1.0" encoding="utf-8"?>
<edmx:Edmx Version="4.0" xmlns:edmx="http://docs.oasis-open.org/odata/ns/edmx">

 <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Aggregation.V1.xml"> <edmx:Include Alias="Aggregation" Namespace="Org.OData.Aggregation.V1"/> </edmx:Reference>
 <edmx:Reference Uri="https://oasis-tcs.github.io/odata-vocabularies/vocabularies/Org.OData.Capabilities.V1.xml"> <edmx:Include Alias="Capabilities" Namespace="Org.OData.Capabilities.V1"/> </edmx:Reference>
 <edmx:Reference Uri="https://sap.github.io/odata-vocabularies/vocabularies/Common.xml"> <edmx:Include Alias="Common" Namespace="com.sap.vocabularies.Common.v1"/> </edmx:Reference>
 <edmx:Reference Uri="https://sap.github.io/odata-vocabularies/vocabularies/Session.xml"> <edmx:Include Alias="Session" Namespace="com.sap.vocabularies.Session.v1"/> </edmx:Reference>
<edmx:Reference Uri="https://sap.github.io/odata-vocabularies/vocabularies/CodeList.xml"> <edmx:Include Alias="CodeList" Namespace="com.sap.vocabularies.CodeList.v1"/> </edmx:Reference>
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
%DRAFT-TYPE%
%ACTIONS%

%ANNOTATIONS%
   </Schema>
  </edmx:DataServices>
</edmx:Edmx>"##;
    let draft_type = r##"<EntityType Name="DraftAdministrativeData">
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
    </EntityType>"##;
    meta.replace("%DRAFT-TYPE%", if drafts { draft_type } else { "" })
}
