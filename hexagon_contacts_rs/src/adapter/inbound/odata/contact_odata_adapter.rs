//! Minimal OData V2-style inbound adapter for the Contact entity set.
//!
//! This is NOT a full OData implementation - it covers GET/POST/DELETE on
//! /Contacts and a simple $filter=CustomerID eq '...' to demonstrate the
//! pattern. For production use, consider a proper OData crate.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::domain::entity::Contact;
use crate::domain::error::DomainError;
use crate::port::inbound::contact_port::ContactUseCase;

const SERVICE_ROOT: &str = "/odata/v2/ContactService.svc";

/// Inbound adapter state - only holds the port, never the concrete service.
#[derive(Clone)]
struct AppState {
    contact_use_case: Arc<dyn ContactUseCase>,
}

pub fn create_odata_app(contact_use_case: Arc<dyn ContactUseCase>) -> Router {
    let state = AppState { contact_use_case };

    Router::new()
        .route(&format!("{SERVICE_ROOT}/$metadata"), get(metadata))
        .route(
            &format!("{SERVICE_ROOT}/Contacts"),
            get(list_contacts).post(create_contact),
        )
        .route(
            &format!("{SERVICE_ROOT}/Contacts/:id"),
            get(get_contact).delete(delete_contact),
        )
        .with_state(state)
}

async fn metadata() -> impl IntoResponse {
    let xml = r#"<?xml version="1.0"?>
<edmx:Edmx Version="1.0" xmlns:edmx="http://schemas.microsoft.com/ado/2007/06/edmx">
  <edmx:DataServices>
    <Schema Namespace="ContactService">
      <EntityType Name="Contact">
        <Key><PropertyRef Name="ID"/></Key>
        <Property Name="ID" Type="Edm.Int32"/>
        <Property Name="CustomerID" Type="Edm.String"/>
        <Property Name="Name" Type="Edm.String"/>
        <Property Name="Email" Type="Edm.String"/>
        <Property Name="Phone" Type="Edm.String"/>
      </EntityType>
      <EntityContainer Name="ContactServiceContainer" m:IsDefaultEntityContainer="true">
        <EntitySet Name="Contacts" EntityType="ContactService.Contact"/>
      </EntityContainer>
    </Schema>
  </edmx:DataServices>
</edmx:Edmx>"#;
    ([("content-type", "application/xml")], xml)
}

#[derive(Serialize)]
struct ODataEnvelope<T> {
    d: T,
}

#[derive(Serialize)]
struct ODataResults<T> {
    results: Vec<T>,
}

#[derive(Serialize)]
struct ODataContact {
    #[serde(rename = "ID")]
    id: Option<i32>,
    #[serde(rename = "CustomerID")]
    customer_id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Email")]
    email: String,
    #[serde(rename = "Phone")]
    phone: String,
}

impl From<Contact> for ODataContact {
    fn from(c: Contact) -> Self {
        ODataContact {
            id: c.id,
            customer_id: c.customer_id,
            name: c.name,
            email: c.email,
            phone: c.phone,
        }
    }
}

#[derive(Deserialize)]
struct CreateContactBody {
    #[serde(rename = "CustomerID")]
    customer_id: String,
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Email")]
    email: String,
    #[serde(rename = "Phone", default)]
    phone: String,
}

fn extract_customer_id(filter: &str) -> Option<String> {
    // Matches: CustomerID eq 'K123'
    let filter = filter.trim();
    let prefix = "CustomerID eq '";
    if filter.starts_with(prefix) && filter.ends_with('\'') {
        Some(filter[prefix.len()..filter.len() - 1].to_string())
    } else {
        None
    }
}

fn domain_error_to_response(err: DomainError) -> Response {
    match err {
        DomainError::Validation(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
        DomainError::NotFound(msg) => (StatusCode::NOT_FOUND, msg).into_response(),
        DomainError::Repository(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
    }
}

async fn list_contacts(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let filter = params.get("$filter").cloned().unwrap_or_default();
    let Some(customer_id) = extract_customer_id(&filter) else {
        return (
            StatusCode::BAD_REQUEST,
            "This demo only supports $filter=CustomerID eq '...'",
        )
            .into_response();
    };

    match state.contact_use_case.list_contacts_for_customer(&customer_id).await {
        Ok(contacts) => Json(ODataEnvelope {
            d: ODataResults {
                results: contacts.into_iter().map(ODataContact::from).collect(),
            },
        })
        .into_response(),
        Err(e) => domain_error_to_response(e),
    }
}

async fn get_contact(State(state): State<AppState>, Path(id): Path<i32>) -> Response {
    match state.contact_use_case.get_contact(id).await {
        Ok(contact) => Json(ODataEnvelope { d: ODataContact::from(contact) }).into_response(),
        Err(e) => domain_error_to_response(e),
    }
}

async fn create_contact(State(state): State<AppState>, Json(body): Json<CreateContactBody>) -> Response {
    match state
        .contact_use_case
        .create_contact(body.customer_id, body.name, body.email, body.phone)
        .await
    {
        Ok(contact) => (
            StatusCode::CREATED,
            Json(ODataEnvelope { d: ODataContact::from(contact) }),
        )
            .into_response(),
        Err(e) => domain_error_to_response(e),
    }
}

async fn delete_contact(State(state): State<AppState>, Path(id): Path<i32>) -> Response {
    match state.contact_use_case.delete_contact(id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => domain_error_to_response(e),
    }
}
