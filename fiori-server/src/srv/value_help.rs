use std::collections::HashSet;

use super::{
    Annotation, Annotations, BinOp, Collection, EntitySet, EntitySpec, EntityType, Expr, FieldSpec,
    GetResult, ODataError, ODataQuery, PathSegment, PropertyValue, Record, Relationship,
    and_filter, materialize, resolve_path,
};
use crate::AppState;
use spec::data_dictionary::DataDictionary;

pub(super) fn set_name(entity: &str, property: &str) -> String {
    format!("{entity}_{property}_ValueHelpSet")
}

fn type_name(entity: &str, property: &str) -> String {
    format!("{entity}_{property}_ValueHelp")
}

pub(super) struct Projection<'a> {
    relationship: &'a Relationship,
    owner: &'a EntitySpec,
    source: &'a EntitySpec,
    set_name: String,
    type_name: String,
    key_property: String,
    filter_properties: Vec<String>,
    refresh_navigations: Vec<String>,
}

fn scalar_property(
    ddic: &DataDictionary,
    entity: &EntitySpec,
    property: &str,
) -> Result<(String, String), String> {
    if property == entity.primary_key_name() {
        return Ok((property.into(), entity.primary_key_type().into()));
    }
    if property == "IsActiveEntity" && entity.draft_enabled() {
        return Ok((property.into(), "Edm.Boolean".into()));
    }
    for field in entity.fields() {
        match field {
            FieldSpec::Lookup {
                name,
                entity: target,
                collection: false,
                ..
            } => {
                let column = ddic.fk_column(target, name);
                if property == name || property == column {
                    let target = ddic
                        .active_entity(target)
                        .ok_or_else(|| format!("Unbekannte Entity '{target}'"))?;
                    return Ok((column, target.primary_key_type().into()));
                }
            }
            FieldSpec::Atom { .. } | FieldSpec::Measure { .. } if field.name() == property => {
                return Ok((property.into(), field.edm_type().into()));
            }
            _ => {}
        }
    }
    Err(format!(
        "Unbekannte oder nicht skalare Property '{}.{}'",
        entity.name(),
        property
    ))
}

pub(super) fn projections(ddic: &DataDictionary) -> Result<Vec<Projection<'_>>, String> {
    let mut result = Vec::new();
    let mut names = HashSet::new();
    for relationship in ddic.relationships() {
        let Some(source_config) = relationship.value_list_source() else {
            continue;
        };
        let error = |message: String| format!("Wertehilfe '{}': {message}", relationship.name());
        let owner_name = relationship
            .many_opt()
            .ok_or_else(|| error("Die many-Entity fehlt".into()))?;
        let property = relationship
            .many_property_opt()
            .ok_or_else(|| error("Die many-Property fehlt".into()))?;
        let target_name = relationship
            .one_opt()
            .ok_or_else(|| error("Die Ziel-Entity fehlt".into()))?;
        let owner = ddic
            .active_entity(owner_name)
            .ok_or_else(|| error(format!("Unbekannte Entity '{owner_name}'")))?;
        let target = ddic
            .active_entity(target_name)
            .ok_or_else(|| error(format!("Unbekannte Entity '{target_name}'")))?;
        let source = ddic
            .active_entity(source_config.entity())
            .ok_or_else(|| error(format!("Unbekannte Entity '{}'", source_config.entity())))?;
        if !matches!(
            owner.field_by_name(property),
            Some(FieldSpec::Lookup { entity, collection: false, .. }) if entity == target_name
        ) {
            return Err(error(format!(
                "'{owner_name}.{property}' ist kein Lookup auf '{target_name}'"
            )));
        }
        if !matches!(
            source.field_by_name("name"),
            Some(FieldSpec::Atom { edm_type, .. }) if edm_type == "Edm.String"
        ) {
            return Err(error(format!(
                "'{}' benoetigt eine String-Property 'name'",
                source.name()
            )));
        }
        if source_config.use_local_draft() && !(owner.draft_enabled() && source.draft_enabled()) {
            return Err(error(
                "'use_local_draft' erfordert Draft-Unterstuetzung auf beiden Entities".into(),
            ));
        }

        let (key_property, key_type) =
            scalar_property(ddic, source, source_config.key_property()).map_err(&error)?;
        if key_type != target.primary_key_type() {
            return Err(error(format!(
                "'{}.{key_property}' und '{target_name}' haben unterschiedliche Schluesseltypen",
                source.name()
            )));
        }

        let mut filter_properties = Vec::new();
        for filter in relationship.value_list_filters() {
            let (_, local_type) =
                scalar_property(ddic, owner, filter.local_property()).map_err(&error)?;
            let (target_property, target_type) =
                scalar_property(ddic, source, filter.target_property()).map_err(&error)?;
            if local_type != target_type {
                return Err(error(format!(
                    "Die Filter-Properties '{}' und '{target_property}' haben unterschiedliche Typen",
                    filter.local_property()
                )));
            }
            if target_property == key_property
                || (source.draft_enabled() && target_property == "IsActiveEntity")
            {
                return Err(error(format!(
                    "Die Filter-Property '{target_property}' ist bereits durch die Wertehilfe belegt"
                )));
            }
            if !filter_properties.contains(&target_property) {
                filter_properties.push(target_property);
            }
        }
        if source.draft_enabled() {
            filter_properties.push("IsActiveEntity".into());
        }

        let mut refresh_navigations = Vec::new();
        for composition in ddic.relationships().filter(|candidate| {
            candidate.is_owned()
                && candidate.one_opt() == Some(owner_name)
                && candidate.many_opt() == Some(source.name())
        }) {
            let (Some(navigation), Some(foreign_key)) =
                (composition.one_field_opt(), composition.many_property_opt())
            else {
                continue;
            };
            let column = ddic.fk_column(owner_name, foreign_key);
            if relationship.value_list_filters().iter().any(|filter| {
                filter.local_property() == owner.primary_key_name()
                    && (filter.target_property() == foreign_key
                        || filter.target_property() == column)
            }) && !refresh_navigations.iter().any(|name| name == navigation)
            {
                refresh_navigations.push(navigation.to_string());
            }
        }

        let set_name = set_name(owner_name, property);
        let type_name = type_name(owner_name, property);
        if ddic.active_entity_by_setname(&set_name).is_some()
            || ddic.active_entity(&type_name).is_some()
            || !names.insert(set_name.clone())
        {
            return Err(error(format!(
                "Der generierte Name '{set_name}' ist bereits vergeben"
            )));
        }
        result.push(Projection {
            relationship,
            owner,
            source,
            set_name,
            type_name,
            key_property,
            filter_properties,
            refresh_navigations,
        });
    }
    Ok(result)
}

impl Projection<'_> {
    pub(super) fn set_name(&self) -> &str {
        &self.set_name
    }

    pub(super) fn owner_side_effect(&self) -> Option<(&str, Annotation)> {
        if self.refresh_navigations.is_empty() {
            return None;
        }
        let sources = self
            .refresh_navigations
            .iter()
            .fold(Collection::new(), |paths, name| {
                paths.add_navigation_property_path(name)
            });
        let targets = Collection::new()
            .add_navigation_property_path(format!("/Service.EntityContainer/{}", self.set_name));
        let mut annotation = Annotation::new("Common.SideEffects").record(
            Record::new("Common.SideEffectsType")
                .add(PropertyValue::new("SourceEntities").collection(sources))
                .add(PropertyValue::new("TargetEntities").collection(targets)),
        );
        annotation.qualifier = Some(self.type_name.clone());
        Some((self.owner.name(), annotation))
    }

    pub(super) fn entity_type(&self, ddic: &DataDictionary) -> EntityType {
        let mut entity_type = EntityType::from_entity(ddic, self.source);
        entity_type.name = self.type_name.clone();
        entity_type
    }

    pub(super) fn entity_set(&self, ddic: &DataDictionary) -> EntitySet {
        let mut entity_set = EntitySet::from_entity(ddic, self.source);
        entity_set.name = self.set_name.clone();
        entity_set.entity_type = format!("Service.{}", self.type_name);
        entity_set
    }

    pub(super) fn annotations(
        &self,
        ddic: &DataDictionary,
        hidden: &[String],
        app_entities: &[String],
    ) -> Vec<Annotations> {
        let source_prefix = format!("Service.{}/", self.source.name());
        let alias_prefix = format!("Service.{}/", self.type_name);
        let mut annotations: Vec<Annotations> =
            Annotations::from_entity(ddic, self.source, hidden, app_entities)
                .into_iter()
                .filter_map(|mut block| {
                    let property = block.target.strip_prefix(&source_prefix)?;
                    if self.filter_properties.iter().any(|name| name == property) {
                        block.annotations.retain(|a| a.term == "Common.Label");
                        block
                            .annotations
                            .push(Annotation::new("UI.HiddenFilter").boolean(true));
                    }
                    block.target = format!("{alias_prefix}{property}");
                    Some(block)
                })
                .collect();
        for property in &self.filter_properties {
            let target = format!("{alias_prefix}{property}");
            if !annotations.iter().any(|block| block.target == target) {
                annotations.push(Annotations {
                    target,
                    annotations: vec![Annotation::new("UI.HiddenFilter").boolean(true)],
                });
            }
        }
        annotations.push(Annotations {
            target: format!("Service.{}", self.type_name),
            annotations: vec![
                Annotation::new("UI.PresentationVariant").record(
                    Record::new("UI.PresentationVariantType").add(
                        PropertyValue::new("SortOrder").collection(
                            Collection::new().add_record(
                                Record::new("Common.SortOrderType")
                                    .add(PropertyValue::new("Property").property_path("name"))
                                    .add(PropertyValue::new("Descending").boolean(false)),
                            ),
                        ),
                    ),
                ),
            ],
        });
        let mut non_sortable = Collection::new();
        for field in self.source.fields() {
            if let FieldSpec::Lookup {
                name,
                collection: false,
                ..
            } = field
            {
                non_sortable = non_sortable.add_property_path(format!("{name}/name"));
            }
        }
        annotations.push(Annotations {
            target: format!("Service.EntityContainer/{}", self.set_name),
            annotations: vec![
                Annotation::new("Capabilities.SortRestrictions").record(
                    Record::new("Capabilities.SortRestrictionsType")
                        .add(PropertyValue::new("NonSortableProperties").collection(non_sortable)),
                ),
                Annotation::new("Capabilities.InsertRestrictions").record(
                    Record::new("Capabilities.InsertRestrictionsType")
                        .add(PropertyValue::new("Insertable").boolean(false)),
                ),
                Annotation::new("Capabilities.UpdateRestrictions").record(
                    Record::new("Capabilities.UpdateRestrictionsType")
                        .add(PropertyValue::new("Updatable").boolean(false)),
                ),
                Annotation::new("Capabilities.DeleteRestrictions").record(
                    Record::new("Capabilities.DeleteRestrictionsType")
                        .add(PropertyValue::new("Deletable").boolean(false)),
                ),
            ],
        });
        annotations
    }
}

fn has_bound_filter(expr: &Expr, property: &str, require_true: bool) -> bool {
    fn column(expr: &Expr, name: &str) -> bool {
        matches!(expr, Expr::Identifier(value) if value == name)
            || matches!(expr, Expr::Path(parts) if parts.len() == 1 && parts[0] == name)
    }
    fn literal(expr: &Expr, require_true: bool) -> bool {
        if require_true {
            return matches!(expr, Expr::Bool(true));
        }
        match expr {
            Expr::String(value) | Expr::Guid(value) | Expr::DateTime(value) => !value.is_empty(),
            Expr::Bool(_) | Expr::Number(_) => true,
            _ => false,
        }
    }
    match expr {
        Expr::BinaryOp {
            op: BinOp::And,
            left,
            right,
        } => {
            has_bound_filter(left, property, require_true)
                || has_bound_filter(right, property, require_true)
        }
        Expr::BinaryOp {
            op: BinOp::Eq,
            left,
            right,
        } => {
            (column(left, property) && literal(right, require_true))
                || (column(right, property) && literal(left, require_true))
        }
        _ => false,
    }
}

pub(super) async fn resolve_get(
    state: &AppState,
    query: &ODataQuery,
    segments: &[PathSegment],
) -> Result<Option<GetResult>, ODataError> {
    let Some(root) = segments.first() else {
        return Ok(None);
    };
    if !root.name.ends_with("_ValueHelpSet") {
        return Ok(None);
    }
    let ddic = state.ddic();
    let projections = projections(&ddic).map_err(ODataError::internal)?;
    let Some(projection) = projections.iter().find(|item| item.set_name == root.name) else {
        return Ok(None);
    };
    if !root.keys.is_empty()
        || segments.len() > 2
        || (segments.len() == 2 && segments[1].name != "$count")
    {
        return Err(ODataError::bad_request(
            "Die Wertehilfe unterstuetzt nur den lesenden Zugriff auf die Sammlung",
        ));
    }
    for property in &projection.filter_properties {
        let require_true = property == "IsActiveEntity"
            && !projection
                .relationship
                .value_list_source()
                .expect("projection has a source")
                .use_local_draft();
        if !query
            .filter
            .as_ref()
            .is_some_and(|filter| has_bound_filter(filter, property, require_true))
        {
            return Err(ODataError::bad_request(format!(
                "Die Wertehilfe erfordert einen Kontextfilter fuer '{property}'"
            )));
        }
    }
    let mut scoped_query = query.clone();
    scoped_query.filter = Some(and_filter(
        scoped_query.filter,
        Expr::BinaryOp {
            op: BinOp::Ne,
            left: Box::new(Expr::Identifier(projection.key_property.clone())),
            right: Box::new(Expr::Null),
        },
    ));
    let mut canonical_segments = segments.to_vec();
    canonical_segments[0].name = projection.source.setname();
    let mut resolved = resolve_path(state, &scoped_query, &canonical_segments).await?;
    resolved.context = projection.set_name.clone();
    let result = materialize(
        state,
        resolved.cursor,
        resolved.suffix,
        &resolved.query,
        &resolved.context,
    )?;
    Ok(Some(result))
}
