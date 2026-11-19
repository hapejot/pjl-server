use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use data_store::{DataStore, DraftDataStore, EntityKey, StaticSchema};
use fiori_server::{
    AppState,
    config::{ServerConfig, ServerMode},
    srv,
};
use serde_json::{Value, json};
use spec::{
    EntitySpec, FieldSpec, Relationship, ValueListFilter, ValueListSource,
    data_dictionary::DataDictionary, package::Package,
};
use sqlite_relational_store::RelationalStore;
use tower::ServiceExt;

const PROJECT: &str = "10000000-0000-4000-8000-000000000001";
const OTHER_PROJECT: &str = "10000000-0000-4000-8000-000000000002";
const CONTACT: &str = "20000000-0000-4000-8000-000000000001";
const DRAFT_CONTACT: &str = "20000000-0000-4000-8000-000000000002";
const OTHER_CONTACT: &str = "20000000-0000-4000-8000-000000000003";
const MEMBER: &str = "30000000-0000-4000-8000-000000000001";
const VALUE_HELP: &str = "/Project_primary_contact_ValueHelpSet";

fn dictionary(use_local_draft: bool) -> DataDictionary {
    let mut ddic = DataDictionary::empty();
    ddic.add_package(Package::new("test"));
    for name in ["Project", "Contact", "ProjectMember"] {
        ddic.add_entity(
            EntitySpec::new(name, "test", name)
                .without_audit()
                .field(FieldSpec::string("name", "Name", 80)),
        );
    }
    ddic.add_relationship(
        Relationship::new("project_members", "test")
            .with_one("Project", "_Members")
            .with_many("ProjectMember", "project")
            .owned(),
    );
    ddic.add_relationship(
        Relationship::new("contact_memberships", "test")
            .with_one("Contact", "_Memberships")
            .with_many("ProjectMember", "contact"),
    );
    let source = ValueListSource::new("ProjectMember", "contact");
    let source = if use_local_draft {
        source.with_local_draft()
    } else {
        source
    };
    ddic.add_relationship(
        Relationship::new("primary_contact", "test")
            .with_one("Contact", "_PrimaryProjects")
            .with_many("Project", "primary_contact")
            .with_value_list_source(source)
            .with_value_list_filter(ValueListFilter::new("ID", "project")),
    );
    ddic
}

async fn app(use_local_draft: bool) -> AppState {
    let base = dictionary(use_local_draft);
    let mut active = base.clone();
    active.activate();
    let schema = spec_schema::schema_from_ddic(&active).unwrap();
    let store = Arc::new(
        RelationalStore::open_in_memory(Arc::new(StaticSchema::new(schema)))
            .await
            .unwrap(),
    );
    for (id, name) in [(PROJECT, "Project"), (OTHER_PROJECT, "Other project")] {
        store
            .create_entity(
                "ProjectSet",
                &json!({"ID": id, "IsActiveEntity": true, "name": name}),
            )
            .await
            .unwrap();
    }
    for (id, name) in [
        (CONTACT, "Saved member"),
        (DRAFT_CONTACT, "Draft member"),
        (OTHER_CONTACT, "Other project member"),
    ] {
        store
            .create_entity(
                "ContactSet",
                &json!({"ID": id, "IsActiveEntity": true, "name": name}),
            )
            .await
            .unwrap();
    }
    for (id, project, contact, name) in [
        (MEMBER, PROJECT, Some(CONTACT), "Saved role"),
        (
            "30000000-0000-4000-8000-000000000002",
            PROJECT,
            None,
            "No contact",
        ),
        (
            "30000000-0000-4000-8000-000000000003",
            OTHER_PROJECT,
            Some(OTHER_CONTACT),
            "Other role",
        ),
    ] {
        store
            .create_entity(
                "ProjectMemberSet",
                &json!({
                    "ID": id,
                    "IsActiveEntity": true,
                    "project_ID": project,
                    "contact_ID": contact,
                    "name": name,
                }),
            )
            .await
            .unwrap();
    }
    store
        .draft_edit("ProjectSet", &EntityKey::new_active(PROJECT))
        .await
        .unwrap();
    store
        .create_entity(
            "ProjectMemberSet",
            &json!({
                "ID": "30000000-0000-4000-8000-000000000004",
                "IsActiveEntity": false,
                "project_ID": PROJECT,
                "contact_ID": DRAFT_CONTACT,
                "name": "Draft role",
            }),
        )
        .await
        .unwrap();
    AppState::new(
        store,
        Arc::new(ServerConfig::defaults(ServerMode::Runtime)),
        base,
    )
}

async fn request(state: &AppState, method: Method, uri: &str) -> (StatusCode, String) {
    let response = srv::router()
        .with_state(state.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn value_help(state: &AppState, filter: &str) -> Value {
    let uri = format!("{VALUE_HELP}?$filter={filter}&$count=true&$orderby=name&$expand=contact");
    let (status, body) = request(state, Method::GET, &uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).unwrap()
}

#[tokio::test]
async fn project_value_help_respects_project_draft_and_contact() {
    let state = app(true).await;
    let draft = value_help(
        &state,
        &format!("project_ID%20eq%20{PROJECT}%20and%20IsActiveEntity%20eq%20false"),
    )
    .await;
    assert_eq!(draft["@odata.count"], 2);
    assert_eq!(draft["value"][0]["contact_ID"], DRAFT_CONTACT);
    assert_eq!(draft["value"][0]["contact"]["name"], "Draft member");
    assert_eq!(draft["value"][1]["contact_ID"], CONTACT);
    let (status, count) = request(
        &state,
        Method::GET,
        &format!(
            "{VALUE_HELP}/$count?$filter=project_ID%20eq%20{PROJECT}%20and%20IsActiveEntity%20eq%20false"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{count}");
    assert_eq!(count, "2");

    let active = value_help(
        &state,
        &format!("project_ID%20eq%20{PROJECT}%20and%20IsActiveEntity%20eq%20true"),
    )
    .await;
    assert_eq!(active["@odata.count"], 1);
    assert_eq!(active["value"][0]["contact_ID"], CONTACT);

    let typed = value_help(
        &state,
        &format!(
            "contact/name%20eq%20%27Draft%20member%27%20and%20project_ID%20eq%20{PROJECT}%20and%20IsActiveEntity%20eq%20false"
        ),
    )
    .await;
    assert_eq!(typed["@odata.count"], 1);
    assert_eq!(typed["value"][0]["contact_ID"], DRAFT_CONTACT);
    let nonmember = value_help(
        &state,
        &format!(
            "contact/name%20eq%20%27Other%20project%20member%27%20and%20project_ID%20eq%20{PROJECT}%20and%20IsActiveEntity%20eq%20false"
        ),
    )
    .await;
    assert_eq!(nonmember["@odata.count"], 0);

    state
        .store
        .delete_entity_tree_only("ProjectMemberSet", &EntityKey::new(MEMBER, false))
        .await
        .unwrap();
    let draft_after_removal = value_help(
        &state,
        &format!("project_ID%20eq%20{PROJECT}%20and%20IsActiveEntity%20eq%20false"),
    )
    .await;
    assert_eq!(draft_after_removal["@odata.count"], 1);
    assert_eq!(draft_after_removal["value"][0]["contact_ID"], DRAFT_CONTACT);
}

#[tokio::test]
async fn value_help_requires_conjunctive_context_and_is_read_only() {
    let state = app(true).await;
    for filter in [
        String::new(),
        format!("?$filter=project_ID%20eq%20{PROJECT}"),
        format!("?$filter=IsActiveEntity%20eq%20false"),
        format!(
            "?$filter=(project_ID%20eq%20{PROJECT}%20or%20project_ID%20eq%20{OTHER_PROJECT})%20and%20IsActiveEntity%20eq%20false"
        ),
        format!(
            "?$filter=project_ID%20eq%20{PROJECT}%20and%20(IsActiveEntity%20eq%20false%20or%20IsActiveEntity%20eq%20true)"
        ),
        "?$filter=project_ID%20eq%20null%20and%20IsActiveEntity%20eq%20false".into(),
    ] {
        let (status, body) = request(&state, Method::GET, &format!("{VALUE_HELP}{filter}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{filter}: {body}");
    }
    let (status, _) = request(&state, Method::POST, VALUE_HELP).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn metadata_scopes_hidden_filters_and_refreshes_when_members_change() {
    let state = app(true).await;
    let (status, xml) = request(&state, Method::GET, "/$metadata").await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    let block = |target: &str| {
        xml.split(&format!("<Annotations Target=\"{target}\">"))
            .nth(1)
            .unwrap()
            .split("</Annotations>")
            .next()
            .unwrap()
    };
    let project = block("Service.Project");
    assert!(
        project
            .contains(r#"Term="Common.SideEffects" Qualifier="Project_primary_contact_ValueHelp""#)
    );
    assert!(project.contains("<NavigationPropertyPath>_Members</NavigationPropertyPath>"));
    assert!(project.contains(
        "<NavigationPropertyPath>/Service.EntityContainer/Project_primary_contact_ValueHelpSet</NavigationPropertyPath>"
    ));
    let alias = block("Service.Project_primary_contact_ValueHelp/project_ID");
    assert!(alias.contains(r#"Term="UI.HiddenFilter" Bool="true""#));
    assert!(!block("Service.ProjectMember/project_ID").contains("UI.HiddenFilter"));
    let alias_set = block("Service.EntityContainer/Project_primary_contact_ValueHelpSet");
    assert!(!alias_set.contains("Common.DraftNode"));
    assert!(alias_set.contains(r#"Property="Insertable" Bool="false""#));
    assert!(block("Service.EntityContainer/ProjectMemberSet").contains("Common.DraftNode"));

    let (status, body) = request(
        &state,
        Method::GET,
        &format!("/ProjectSet(ID={PROJECT},IsActiveEntity=false)?$select=primary_contact_ID"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let project: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(project["primary_contact_ID"], Value::Null);
}

#[tokio::test]
async fn value_help_without_local_draft_uses_only_active_source_rows() {
    let state = app(false).await;
    let (status, xml) = request(&state, Method::GET, "/$metadata").await;
    assert_eq!(status, StatusCode::OK, "{xml}");
    assert!(xml.contains(r#"Type="Common.ValueListParameterConstant""#));
    assert!(xml.contains(r#"Property="Constant" Bool="true""#));

    let active = value_help(
        &state,
        &format!("project_ID%20eq%20{PROJECT}%20and%20IsActiveEntity%20eq%20true"),
    )
    .await;
    assert_eq!(active["@odata.count"], 1);
    assert_eq!(active["value"][0]["contact_ID"], CONTACT);

    let (status, _) = request(
        &state,
        Method::GET,
        &format!(
            "{VALUE_HELP}?$filter=project_ID%20eq%20{PROJECT}%20and%20IsActiveEntity%20eq%20false"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn local_draft_source_requires_draft_enabled_entities() {
    let state = app(true).await;
    let mut invalid = dictionary(true);
    invalid.disable_drafts();
    let invalid_state = AppState::new(state.store.clone(), state.config.clone(), invalid);
    let (status, body) = request(&invalid_state, Method::GET, "/$metadata").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("use_local_draft"), "{body}");
}
