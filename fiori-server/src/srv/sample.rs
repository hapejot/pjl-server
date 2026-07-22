use super::*;
   pub fn sample_annotations() -> String {
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

pub fn sample_entity_set() -> EntitySet {
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

pub fn sample_entity() -> EntityType {
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
            NavigationProperty::new(
                "Navigations",
                "Collection(Service.EntityNavigation)",
                None,
            ),
            NavigationProperty::new(
                "TableFacets",
                "Collection(Service.EntityTableFacet)",
                None,
            ),
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
