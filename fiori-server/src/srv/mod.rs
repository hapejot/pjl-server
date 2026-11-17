use axum::middleware::Next;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderName, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use data_store::{DataRecord, EntityKey, StoreError};
use odata::{
    path::{PathSegment, parse_odata_path},
    query::{BinOp, ExpandItem, Expr, ODataQuery, OrderByItem, SortDirection},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spec::{
    EntitySpec, FieldSpec, Relationship, app::App, data_dictionary::DataDictionary,
    package::Package,
};
use std::collections::HashMap;
use std::time::Instant;
use tracing::*;

use crate::AppState;

mod metadata;
use metadata::metadata;

mod annotations;
pub use annotations::{Annotation, Annotations, Collection, PropertyValue, Record};
pub use annotations::{DEFAULT_FACET, GENERAL_FACET};
#[cfg(test)]
pub(crate) use annotations::{
    annotations_for_atom_value_list, facet_target_path, facet_title, field_group, field_record,
    line_item_annotations, object_page_sections,
};
pub(crate) use annotations::{
    capability_annotations_for_entity, code_list_annotations, list_column_widths, target_key_of,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "EntityType")]
pub(crate) struct EntityType {
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
    pub fn from_entity(ddic: &DataDictionary, x: &EntitySpec) -> Self {
        let mut properties = Vec::new();
        let mut navigation_properties = Vec::new();

        // Eigener Primaerschluessel: das Atom ist der Schluessel, kein separates ID-Feld.
        if x.primary_key().is_none() {
            properties.push(Property::new("ID", "Edm.Guid").nullable(false));
        }
        if x.draft_enabled() {
            properties.push(Property::new("IsActiveEntity", "Edm.Boolean").nullable(false));
            properties.push(Property::new("HasActiveEntity", "Edm.Boolean").nullable(false));
            properties.push(Property::new("HasDraftEntity", "Edm.Boolean").nullable(false));
        }

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
                        let ref_name = ddic.fk_column(entity, name);
                        let (target_key, target_type) = target_key_of(ddic, entity);
                        properties.push(Property::new(&ref_name, target_type.as_str()));
                        NavigationProperty::new(name, format!("Service.{}", entity), None)
                            .ref_constraint(ref_name.as_str(), target_key.as_str())
                    };
                    navigation_properties.push(p);
                }
                FieldSpec::Atom {
                    name,
                    edm_type,
                    max_length,
                    precision,
                    scale,
                    unit_scale_field,
                    ..
                } => {
                    let mut property = Property::new(name, edm_type);
                    // Code einer Code-Liste (Einheit/Waehrung) ist nicht nullable: ValueListHelper._enrichPath
                    // uebernimmt einen ValueListParameterOut nur fuer nicht nullable Eigenschaften als Schluessel.
                    if unit_scale_field.is_some()
                        || x.primary_key().as_deref() == Some(name.as_str())
                    {
                        property = property.nullable(false);
                    }
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
                    name, precision, ..
                } => {
                    // Measures: Nachkommastellen je Einheit/Waehrung (SAP-Konvention).
                    let mut property = Property::new(name, "Edm.Decimal").scale_variable();
                    if let Some(precision) = precision {
                        property = property.precision(*precision);
                    }
                    properties.push(property);
                }
            }
        }
        if x.draft_enabled() {
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
        }
        Self {
            name: x.name().into(),
            key: Key::new(&x.key_fields()),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyRef {
    #[serde(rename = "@Name")]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub scale: Option<String>,

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
        self.scale = Some(scale.to_string());
        self
    }

    /// `Scale="variable"`: Nachkommastellen kommen von der Einheit (`Common.UnitSpecificScale`).
    pub fn scale_variable(mut self) -> Self {
        self.scale = Some("variable".into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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

    fn ref_constraint(mut self, property: &str, referenced_property: &str) -> NavigationProperty {
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
        if x.draft_enabled() {
            navigation_property_bindings.push(NavigationPropertyBinding {
                path: "SiblingEntity".into(),
                target: x.setname().into(),
            });
            navigation_property_bindings.push(NavigationPropertyBinding {
                path: "DraftAdministrativeData".into(),
                target: "DraftAdministrativeData".into(),
            });
        }
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

#[derive(Debug, Serialize, Deserialize)]
pub struct NavigationPropertyBinding {
    #[serde(rename = "@Path")]
    pub path: String,

    #[serde(rename = "@Target")]
    pub target: String,
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

async fn log_request(req: axum::extract::Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let start = Instant::now();

    let res = next.run(req).await;

    tracing::trace!(
        method = %method,
        uri = %uri,
        status = res.status().as_u16(),
        latency_ms = start.elapsed().as_millis() as u64,
        "request"
    );

    res
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
        .layer(axum::middleware::from_fn(log_request))
        .layer(axum::middleware::from_fn(crate::sticky::context))
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
            )));
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
            StoreError::Forbidden(msg) => Self {
                status: StatusCode::FORBIDDEN,
                code: "Forbidden",
                message: msg,
            },
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
        spec: EntitySpec,
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
    let (state, _session_guard) = match crate::sticky::request_state(&state).await {
        Ok(state) => state,
        Err(response) => return response,
    };
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
        state,
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
        let record = read_single(state, &root.name, &entity_key(&spec, root)?, root_query).await?;
        Cursor::Entity {
            spec,
            set: root.name.clone(),
            record: Box::new(record),
        }
    };

    for (idx, segment) in nav_segments.iter().enumerate().skip(1) {
        debug!("processing segment {:?}", segment);
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
    state: &AppState,
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
        (Cursor::Entity { spec, record, .. }, None) => {
            let mut value = entity_json(state, &spec, json!(record));
            touch_selected_fields(&mut value, query);
            value["@odata.context"] = json!(format!("{context}/$entity"));
            Ok(GetResult::Json(value))
        }
        (
            Cursor::Collection {
                spec,
                records,
                count,
            },
            None,
        ) => {
            let records: Vec<Value> = records
                .into_iter()
                .map(|r| entity_json(state, &spec, json!(r)))
                .collect();
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

const DRAFT_PROPERTIES: &[&str] = &[
    "IsActiveEntity",
    "HasActiveEntity",
    "HasDraftEntity",
    "DraftAdministrativeData",
    "DraftAdministrativeData_ID",
    "DraftMessages",
    "SiblingEntity",
];

fn entity_json(state: &AppState, spec: &EntitySpec, mut value: Value) -> Value {
    if let Some(items) = value.as_array_mut() {
        for item in items {
            *item = entity_json(state, spec, item.take());
        }
        return value;
    }
    if let Some(object) = value.as_object_mut() {
        if !spec.draft_enabled() {
            for name in DRAFT_PROPERTIES {
                object.remove(*name);
            }
        }
        // Der Store fuehrt den Schluessel unter `ID`; ein eigener Schluessel steht schon als Atom im Objekt.
        if spec.primary_key().is_some() {
            object.remove("ID");
        }
        let ddic = state.ddic();
        for field in spec.fields() {
            if let FieldSpec::Lookup { name, entity, .. } = field {
                if let (Some(target), Some(nested)) =
                    (ddic.active_entity(entity), object.get_mut(name))
                {
                    *nested = entity_json(state, target, nested.take());
                }
            }
        }
    }
    value
}

/// Schreibt den eigenen Primaerschluessel auch nach `ID`, damit der Store ihn kennt.
/// Ohne eigenen Schluessel passiert nichts.
fn to_store_body(spec: &EntitySpec, body: &mut Value) {
    if let (Some(pk), Some(object)) = (spec.primary_key(), body.as_object_mut()) {
        if let Some(value) = object.get(pk).cloned().filter(|v| !v.is_null()) {
            object.insert("ID".into(), value);
        }
    }
}

fn validate_write(spec: &EntitySpec, body: &Value) -> Result<(), ODataError> {
    if !body.is_object() {
        return Err(ODataError::bad_request("Expected an entity object"));
    }
    if let Some(pk) = spec.primary_key() {
        if body.get("ID").is_some() {
            return Err(ODataError::bad_request(format!(
                "'ID' ist kein Feld von '{}', Schluessel ist '{pk}'",
                spec.name()
            )));
        }
    }
    if !spec.draft_enabled() {
        if let Some(name) = DRAFT_PROPERTIES
            .iter()
            .find(|name| body.get(**name).is_some())
        {
            return Err(ODataError::bad_request(format!(
                "{} does not support draft property '{name}'",
                spec.name()
            )));
        }
    }
    Ok(())
}

fn validate_query(
    state: &AppState,
    spec: &EntitySpec,
    query: &ODataQuery,
) -> Result<(), ODataError> {
    if !spec.draft_enabled() {
        for name in query
            .select
            .iter()
            .map(String::as_str)
            .chain(query.expand.iter().map(|e| e.navigation_property.as_str()))
        {
            if DRAFT_PROPERTIES.contains(&name) {
                return Err(ODataError::bad_request(format!(
                    "Unknown property '{name}'"
                )));
            }
        }
    }
    let ddic = state.ddic();
    for expand in &query.expand {
        if let Some(FieldSpec::Lookup { entity, .. }) = spec
            .fields()
            .find(|f| f.name() == expand.navigation_property)
        {
            if let Some(target) = ddic.active_entity(entity) {
                validate_query(state, target, &expand.nested_query)?;
            }
        }
    }
    Ok(())
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
    let key = if spec.draft_enabled() {
        format!(
            "ID={},IsActiveEntity={}",
            record.id(),
            record.is_active_entity()
        )
    } else {
        format!("ID={}", record.id())
    };
    context.push_str(&format!("({key})/{}", segment.name));
    if !spec.draft_enabled() && DRAFT_PROPERTIES.contains(&segment.name.as_str()) {
        return Err(ODataError::not_found(format!(
            "Unknown property '{}'",
            segment.name
        )));
    }

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
    let foreign_key = state.ddic().fk_column(target.name(), name);
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
        .map(|f| state.ddic().fk_column(spec.name(), &f))
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

    let record = read_single(state, &set, &entity_key(&target, segment)?, query).await?;
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

fn entity_key(spec: &EntitySpec, segment: &PathSegment) -> Result<EntityKey, ODataError> {
    // Eigener Primaerschluessel: genau ein Wert, Name `pk` oder unbenannt.
    if let Some(pk) = spec.primary_key() {
        let [key] = segment.keys.as_slice() else {
            return Err(ODataError::bad_request(format!(
                "Expected key '{pk}' for '{}'",
                spec.name()
            )));
        };
        if key.name.as_deref().is_some_and(|n| n != pk) {
            return Err(ODataError::bad_request(format!(
                "Unknown key for '{}'",
                spec.name()
            )));
        }
        let value = data_store::key::KeyValue::parse(&key.value.to_string_value());
        return Ok(EntityKey::new(value, true));
    }
    if segment.keys.len() != spec.key_names().len() {
        return Err(ODataError::bad_request(format!(
            "Expected keys {} for '{}'",
            spec.key_names().join(", "),
            spec.name()
        )));
    }
    for (index, key) in segment.keys.iter().enumerate() {
        if segment.keys[..index]
            .iter()
            .any(|other| other.name == key.name)
        {
            return Err(ODataError::bad_request("Duplicate key"));
        }
        if key.name.is_none() && spec.draft_enabled() {
            return Err(ODataError::bad_request("Composite keys must be named"));
        }
        if let Some(name) = &key.name {
            if !spec.key_names().contains(&name.as_str()) {
                return Err(ODataError::bad_request(format!(
                    "Unknown key '{name}' for '{}'",
                    spec.name()
                )));
            }
        }
    }
    if spec.draft_enabled()
        && !segment.keys.iter().any(|key| {
            key.name.as_deref() == Some("IsActiveEntity") && key.value.as_bool().is_some()
        })
    {
        return Err(ODataError::bad_request(
            "Missing boolean key 'IsActiveEntity'",
        ));
    }
    let id = segment.id().ok_or_else(|| {
        ODataError::bad_request(format!("Missing key 'ID' in '{}'", segment.name))
    })?;
    Ok(EntityKey::new(
        id,
        !spec.draft_enabled() || segment.is_active_entity(),
    ))
}

fn touch_selected_fields(value: &mut Value, query: &ODataQuery) {
    for field in query.select.iter() {
        if field == "*"
            || query
                .expand
                .iter()
                .any(|item| item.navigation_property == *field)
        {
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
    let ddic = state.ddic();
    let spec = ddic
        .active_entity_by_setname(set)
        .ok_or_else(|| ODataError::not_found(format!("Unknown entity set '{set}'")))?;
    validate_query(state, spec, query)?;
    let mut record = state
        .store
        .read_entity(set, key, query)
        .await
        .map_err(ODataError::from_store)?;
    expand_designer_rows(state, spec, set, std::slice::from_mut(&mut record), query).await?;
    Ok(record)
}

fn expand_designer_rows<'a>(
    state: &'a AppState,
    spec: &'a EntitySpec,
    set: &'a str,
    records: &'a mut [DataRecord],
    query: &'a ODataQuery,
) -> futures_util::future::BoxFuture<'a, Result<(), ODataError>> {
    // Nested expansions re-enter collection reads.
    Box::pin(async move {
        if state.sticky.is_none() {
            return Ok(());
        }
        for item in &query.expand {
            let name = &item.navigation_property;
            let segment = PathSegment {
                name: name.clone(),
                keys: vec![],
            };
            for record in records.iter_mut() {
                if record.get(name).is_some() {
                    continue;
                }
                let cursor = Cursor::Entity {
                    spec: spec.clone(),
                    set: set.into(),
                    record: Box::new(record.clone()),
                };
                let mut context = set.to_string();
                let expanded =
                    follow_segment(state, cursor, &segment, &item.nested_query, &mut context)
                        .await?;
                record[name] = match expanded {
                    Cursor::Entity {
                        spec,
                        set,
                        record: mut child,
                    } => {
                        expand_designer_rows(
                            state,
                            &spec,
                            &set,
                            std::slice::from_mut(child.as_mut()),
                            &item.nested_query,
                        )
                        .await?;
                        json!(child)
                    }
                    Cursor::Collection { records, count, .. } => {
                        if let Some(count) = count {
                            record[&format!("{name}@odata.count")] = json!(count);
                        }
                        json!(records)
                    }
                    Cursor::Empty => Value::Null,
                    Cursor::Property { .. } => {
                        return Err(ODataError::bad_request(format!(
                            "'{name}' is not a navigation property"
                        )));
                    }
                };
            }
        }
        Ok(())
    })
}

async fn read_collection(
    state: &AppState,
    set: &str,
    query: &ODataQuery,
) -> Result<Cursor, ODataError> {
    let spec = state
        .ddic()
        .active_entity_by_setname(set)
        .cloned()
        .ok_or_else(|| ODataError::not_found(format!("Unknown entity set '{set}'")))?;
    validate_query(state, &spec, query)?;
    // Ein Aufruf: Hooks laufen einmal pro Request, Count beruecksichtigt Entfernungen.
    let (mut records, count) = state
        .store
        .read_collection(set, query, query.count)
        .await
        .map_err(ODataError::from_store)?;
    expand_designer_rows(state, &spec, set, &mut records, query).await?;
    Ok(Cursor::Collection {
        spec,
        records,
        count,
    })
}

pub(crate) async fn action_entity_json(
    state: &AppState,
    set: &str,
    key: &EntityKey,
    query: &ODataQuery,
) -> Result<Value, Response> {
    let record = read_single(state, set, key, query)
        .await
        .map_err(IntoResponse::into_response)?;
    let spec = state
        .ddic()
        .active_entity_by_setname(set)
        .cloned()
        .ok_or_else(|| {
            ODataError::not_found(format!("Unknown entity set '{set}'")).into_response()
        })?;
    let result = materialize(
        state,
        Cursor::Entity {
            spec,
            set: set.into(),
            record: Box::new(record),
        },
        None,
        query,
        set,
    )
    .map_err(IntoResponse::into_response)?;
    match result {
        GetResult::Json(value) => Ok(value),
        _ => unreachable!("single entity JSON representation"),
    }
}

async fn post_handler(
    path: Path<String>,
    state: State<AppState>,
    params: Query<HashMap<String, String>>,
    payload: Option<Json<Value>>,
) -> Response {
    let (state, _session_guard) = match crate::sticky::request_state(&state).await {
        Ok(state) => state,
        Err(response) => return response,
    };
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
    let body = payload.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    if let Some(sessions) = &state.sticky {
        match path_struct.segments.as_slice() {
            [action] if action.name == "stickyDiscard" && action.keys.is_empty() => {
                return sessions
                    .action(&state, None, None, "stickyDiscard", &query)
                    .await;
            }
            [entity, action]
                if action.name.starts_with("Service.sticky") && action.keys.is_empty() =>
            {
                let spec = match entity_spec_for_segment(&state, entity) {
                    Ok(spec) => spec,
                    Err(error) => return error.into_response(),
                };
                let key = if entity.keys.is_empty() {
                    None
                } else {
                    match entity_key(&spec, entity) {
                        Ok(key) => Some(key),
                        Err(error) => return error.into_response(),
                    }
                };
                return sessions
                    .action(
                        &state,
                        Some(&entity.name),
                        key.as_ref(),
                        action.name.trim_start_matches("Service."),
                        &query,
                    )
                    .await;
            }
            _ => {}
        }
    }

    match path_struct.segments.as_slice() {
        [entity] if entity.keys.is_empty() => {
            let spec = match entity_spec_for_segment(&state, entity).and_then(|spec| {
                validate_write(&spec, &body)?;
                Ok(spec)
            }) {
                Ok(spec) => spec,
                Err(e) => return e.into_response(),
            };
            let mut body = body;
            if !spec.draft_enabled() {
                body["IsActiveEntity"] = json!(true);
            }
            to_store_body(&spec, &mut body);
            match state.store.create_entity(&entity.name, &body).await {
                Ok(r) => created_response(
                    entity_json(&state, &spec, json!(r)),
                    &path,
                    spec.primary_key(),
                ),
                Err(e) => ODataError::from_store(e).into_response(),
            }
        }
        [entity, action]
            if !entity.keys.is_empty() && action.keys.is_empty() && action.name.contains('.') =>
        {
            let action_name = action.name.rsplit('.').next().unwrap_or_default();
            let spec = match entity_spec_for_segment(&state, entity) {
                Ok(spec) => spec,
                Err(e) => return e.into_response(),
            };
            let key = match entity_key(&spec, entity) {
                Ok(key) => key,
                Err(e) => return e.into_response(),
            };
            if let Some(a) =
                crate::extension::find_action(&state.extensions, &entity.name, action_name)
            {
                return match a.handler.invoke(&state, &key).await {
                    Ok(Value::Null) => StatusCode::NO_CONTENT.into_response(),
                    Ok(r) => to_json_response(entity_json(&state, &spec, r)).into_response(),
                    Err(e) => ODataError::from(e).into_response(),
                };
            }
            match draft_action(&state, &entity.name, &key, action_name).await {
                Ok(r) => to_json_response(entity_json(&state, &spec, r)).into_response(),
                Err(e) => e.into_response(),
            }
        }
        segments if segments.len() > 1 => {
            match create_in_navigation(&state, segments, body).await {
                Ok((r, pk)) => created_response(r, &path, pk.as_deref()),
                Err(e) => e.into_response(),
            }
        }
        _ => ODataError::bad_request("POST is not supported for this path").into_response(),
    }
}

fn created_response(body: Value, path: &str, pk: Option<&str>) -> Response {
    // Eigener Schluessel: Location als `pk='wert'`, sonst GUID als `ID=...`.
    let key = match pk {
        Some(pk) => format!("{pk}='{}'", body[pk].as_str().expect("created entity key")),
        None => {
            let id = body["ID"].as_str().expect("created entity ID");
            match body.get("IsActiveEntity").and_then(Value::as_bool) {
                Some(active) => format!("ID={id},IsActiveEntity={active}"),
                None => id.to_string(),
            }
        }
    };
    (
        StatusCode::CREATED,
        [
            content_type_json(),
            odata_version(),
            (header::LOCATION, format!("/Service/{path}({key})")),
        ],
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
    let ddic = state.ddic();
    let spec = ddic
        .active_entity_by_setname(set)
        .ok_or_else(|| ODataError::not_found(format!("Unknown entity set '{set}'")))?;
    if !spec.draft_enabled() {
        return Err(ODataError::not_found(format!(
            "Draft actions are not supported for '{set}'"
        )));
    }
    match action {
        "draftEdit" => {
            let r = state
                .store
                .draft_edit(set, key)
                .await
                .map_err(ODataError::from_store)?;
            for (child_set, foreign_key) in owned_children_of(state, set) {
                for child in children_of(state, &child_set, &foreign_key, &key.id).await? {
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
) -> Result<(Value, Option<String>), ODataError> {
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
    validate_write(target, &body)?;
    let foreign_key = ddic
        .foreign_key_field(&spec, &field)
        .map(|f| ddic.fk_column(spec.name(), &f))
        .ok_or_else(|| {
            ODataError::not_found(format!(
                "No foreign key for navigation '{}'",
                navigation.name
            ))
        })?;

    body[foreign_key] = json!(record.id().to_string());
    // Das Kind teilt den Draft-Zustand seines Parents.
    body["IsActiveEntity"] = json!(!target.draft_enabled() || record.is_active_entity());
    let created = state
        .store
        .create_entity(&target.setname(), &body)
        .await
        .map_err(ODataError::from_store)?;
    Ok((
        entity_json(state, target, json!(created)),
        target.primary_key().map(str::to_string),
    ))
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
                ddic.fk_column(parent.name(), relationship.many_property()),
            ))
        })
        .collect()
}

async fn children_of(
    state: &AppState,
    set: &str,
    foreign_key: &str,
    parent: &data_store::key::KeyValue,
) -> Result<Vec<DataRecord>, ODataError> {
    let query = ODataQuery::new().filter(parent.equals_filter(foreign_key));
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
    let (state, _session_guard) = match crate::sticky::request_state(&state).await {
        Ok(state) => state,
        Err(response) => return response,
    };
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
    let spec = match entity_spec_for_segment(&state, key) {
        Ok(spec) => spec,
        Err(e) => return e.into_response(),
    };
    if let Err(e) = validate_write(&spec, &body) {
        return e.into_response();
    }
    let ekey = match entity_key(&spec, key) {
        Ok(key) => key,
        Err(e) => return e.into_response(),
    };
    // Der Schluessel ist unveraenderlich.
    if let Some(pk) = spec.primary_key() {
        if body.get(pk).is_some_and(|v| {
            v.as_str().map(|s| data_store::key::KeyValue::parse(s)) != Some(ekey.id.clone())
        }) {
            return ODataError::bad_request(format!("Schluessel '{pk}' ist nicht aenderbar"))
                .into_response();
        }
    }
    let mut body = body;
    to_store_body(&spec, &mut body);
    match state.store.patch_entity(&key.name, &ekey, &body).await {
        Ok(_) => (StatusCode::NO_CONTENT, [odata_version()]).into_response(),
        Err(e) => ODataError::from_store(e).into_response(),
    }
}

async fn delete_handler(path: Path<String>, state: State<AppState>) -> Response {
    let (state, _session_guard) = match crate::sticky::request_state(&state).await {
        Ok(state) => state,
        Err(response) => return response,
    };
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
    let Cursor::Entity { spec, set, record } = resolved.cursor else {
        return Err(ODataError::bad_request(
            "DELETE requires a single entity addressed by key",
        ));
    };

    let key = EntityKey::from(&*record);
    delete_owned_children(state, &set, &key).await?;
    if !spec.draft_enabled() {
        return state
            .store
            .delete_entity_tree_only(&set, &key)
            .await
            .map_err(ODataError::from_store);
    }

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
        for child in children_of(state, &child_set, &foreign_key, &parent.id).await? {
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

async fn index(State(state): State<AppState>) -> Response {
    let (state, _session_guard) = match crate::sticky::request_state(&state).await {
        Ok(state) => state,
        Err(response) => return response,
    };
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
    .into_response()
}
/// Namen der Hauptentitaeten aller Apps (Ziele der semantischen Verlinkung).
pub(crate) fn app_entity_names(ddic: &DataDictionary) -> Vec<String> {
    ddic.apps()
        .map(|app| app.main_entity(ddic).name().to_string())
        .collect()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod list_view_annotation_tests;

#[cfg(test)]
mod value_list_annotation_tests;
