use super::*;
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Method, Request},
};
use tempfile::TempDir;
use tower::ServiceExt;

struct Fixture {
    _dir: TempDir,
    path: std::path::PathBuf,
    state: AppState,
    app: Router,
    package: String,
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ddic.json");
        std::fs::write(
            &path,
            r#"{"packages":[{"name":"test"}],"entities":[],"relationships":[],"apps":[]}"#,
        )
        .unwrap();
        let store = Arc::new(DdicStore::open(&path).unwrap());
        let state = AppState::designer(store.clone()).with_sticky_sessions(store.clone());
        let app = crate::batch::router(crate::srv::router(), state.clone())
            .with_state(state.clone())
            .layer(axum::middleware::from_fn_with_state(
                state.config.clone(),
                crate::auth::identity,
            ));
        let package = Uuid::new_v5(&Uuid::NAMESPACE_OID, b"PackageSet/test").to_string();
        Self {
            _dir: dir,
            path,
            state,
            app,
            package,
        }
    }

    async fn raw(
        &self,
        method: Method,
        path: &str,
        body: &str,
        content_type: &str,
        id: Option<&str>,
        user: &str,
    ) -> (StatusCode, HeaderMap, String) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", content_type)
            .header("x-forwarded-user", user);
        if let Some(id) = id {
            request = request.header(CONTEXT, id);
        }
        let response = self
            .app
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, headers, String::from_utf8(bytes.to_vec()).unwrap())
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Value,
        id: Option<&str>,
        expected: StatusCode,
    ) -> (HeaderMap, Value) {
        let (status, headers, text) = self
            .raw(
                method,
                path,
                &body.to_string(),
                "application/json",
                id,
                "alice",
            )
            .await;
        assert_eq!(status, expected, "{path}: {text}");
        let value = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap()
        };
        (headers, value)
    }

    async fn edit(&self) -> String {
        let (headers, _) = self
            .call(
                Method::POST,
                &format!("/PackageSet({})/Service.stickyEdit", self.package),
                json!({}),
                None,
                StatusCode::OK,
            )
            .await;
        headers[CONTEXT].to_str().unwrap().to_string()
    }

    fn disk(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(&self.path).unwrap()).unwrap()
    }
}

#[tokio::test]
async fn edit_is_isolated_until_save_and_keep_alive_refreshes_session() {
    let fixture = Fixture::new();
    let id = fixture.edit().await;
    let path = format!("/PackageSet({})", fixture.package);
    fixture
        .call(
            Method::PATCH,
            &path,
            json!({"name":"Session package"}),
            Some(&id),
            StatusCode::NO_CONTENT,
        )
        .await;
    let (_, isolated) = fixture
        .call(Method::GET, &path, Value::Null, Some(&id), StatusCode::OK)
        .await;
    assert_eq!(isolated["name"], "Session package");
    let (_, outside) = fixture
        .call(Method::GET, &path, Value::Null, None, StatusCode::OK)
        .await;
    assert_eq!(outside["name"], "test");
    assert_eq!(fixture.disk()["packages"][0]["name"], "test");
    let (headers, _) = fixture
        .call(Method::HEAD, "/", Value::Null, Some(&id), StatusCode::OK)
        .await;
    assert_eq!(headers[CONTEXT], id);
    assert_eq!(headers[TIMEOUT], "120");
    let (headers, saved) = fixture
        .call(
            Method::POST,
            &format!("{path}/Service.stickySave"),
            json!({}),
            Some(&id),
            StatusCode::OK,
        )
        .await;
    assert!(!headers.contains_key(CONTEXT));
    assert_eq!(saved["name"], "Session package");
    assert!(saved.get("IsActiveEntity").is_none());
    assert_eq!(fixture.disk()["packages"][0]["name"], "Session package");
    fixture
        .call(Method::GET, &path, Value::Null, Some(&id), StatusCode::GONE)
        .await;
}

#[tokio::test]
async fn discard_expiry_and_user_isolation_do_not_persist_changes() {
    let fixture = Fixture::new();
    let id = fixture.edit().await;
    let path = format!("/PackageSet({})", fixture.package);
    fixture
        .call(
            Method::PATCH,
            &path,
            json!({"name":"Discard me"}),
            Some(&id),
            StatusCode::NO_CONTENT,
        )
        .await;
    let (status, headers, _) = fixture
        .raw(Method::GET, &path, "", "application/json", Some(&id), "bob")
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!headers.contains_key(CONTEXT));
    fixture
        .call(
            Method::POST,
            "/stickyDiscard",
            json!({}),
            Some(&id),
            StatusCode::NO_CONTENT,
        )
        .await;
    assert_eq!(fixture.disk()["packages"][0]["name"], "test");
    let id = fixture.edit().await;
    let sessions = fixture.state.sticky.as_ref().unwrap();
    sessions
        .sessions
        .lock()
        .unwrap()
        .get_mut(&id)
        .unwrap()
        .touched = Instant::now() - SESSION_TIMEOUT - Duration::from_secs(1);
    let (headers, _) = fixture
        .call(Method::HEAD, "/", Value::Null, Some(&id), StatusCode::GONE)
        .await;
    assert_eq!(headers["sap-err-id"], "ICMENOSESSION");
    assert!(!headers.contains_key(CONTEXT));
    assert!(sessions.sessions.lock().unwrap().is_empty());
}

#[tokio::test]
async fn new_validates_at_save_and_keeps_failed_sessions_editable() {
    let fixture = Fixture::new();
    let (headers, entity) = fixture
        .call(
            Method::POST,
            "/EntitySet/Service.stickyNew",
            json!({}),
            None,
            StatusCode::OK,
        )
        .await;
    let id = headers[CONTEXT].to_str().unwrap();
    let key = entity["ID"].as_str().unwrap();
    let path = format!("/EntitySet({key})");
    let (headers, _) = fixture
        .call(
            Method::POST,
            &format!("{path}/Service.stickySave"),
            json!({}),
            Some(id),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(headers[CONTEXT], id);
    assert_eq!(fixture.disk()["entities"].as_array().unwrap().len(), 0);
    fixture.call(Method::PATCH, &path,
                json!({"name":"Customer","plural":"Customers","package_ID":fixture.package,"draft_enabled":false}),
                Some(id), StatusCode::NO_CONTENT).await;
    fixture
        .call(
            Method::POST,
            &format!("{path}/_Fields"),
            json!({"name":"name","label":"Name","type":"Edm.String"}),
            Some(id),
            StatusCode::CREATED,
        )
        .await;
    fixture
        .call(
            Method::POST,
            &format!("{path}/Service.stickySave"),
            json!({}),
            Some(id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(fixture.disk()["entities"][0]["name"], "Customer");
    assert_eq!(
        fixture.disk()["entities"][0]["fields"][0]["Atom"]["name"],
        "name"
    );
    assert_eq!(fixture.disk()["entities"][0]["draft_enabled"], false);
}

#[tokio::test]
async fn conflicting_edit_or_save_never_overwrites_another_change() {
    let fixture = Fixture::new();
    let id = fixture.edit().await;
    let path = format!("/PackageSet({})", fixture.package);
    fixture
        .call(
            Method::POST,
            &format!("{path}/Service.stickyEdit"),
            json!({}),
            None,
            StatusCode::CONFLICT,
        )
        .await;
    fixture
        .call(
            Method::PATCH,
            &path,
            json!({"name":"Other write"}),
            None,
            StatusCode::NO_CONTENT,
        )
        .await;
    fixture
        .call(
            Method::PATCH,
            &path,
            json!({"name":"Stale write"}),
            Some(&id),
            StatusCode::NO_CONTENT,
        )
        .await;
    let (headers, _) = fixture
        .call(
            Method::POST,
            &format!("{path}/Service.stickySave"),
            json!({}),
            Some(&id),
            StatusCode::CONFLICT,
        )
        .await;
    assert_eq!(headers[CONTEXT], id);
    assert_eq!(fixture.disk()["packages"][0]["name"], "Other write");
}

fn changeset(requests: &[String]) -> String {
    let mut body = "--batch\r\nContent-Type: multipart/mixed; boundary=changes\r\n\r\n".to_string();
    for (index, request) in requests.iter().enumerate() {
        body.push_str(&format!("--changes\r\nContent-Type: application/http\r\nContent-Transfer-Encoding: binary\r\nContent-ID: {}\r\n\r\n{request}\r\n", index + 1));
    }
    body.push_str("--changes--\r\n--batch--\r\n");
    body
}

#[tokio::test]
async fn changeset_save_is_atomic_and_rollback_preserves_session() {
    let fixture = Fixture::new();
    let id = fixture.edit().await;
    let path = format!("/PackageSet({})", fixture.package);
    let patch = format!(
        "PATCH {path} HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"Batch package\"}}"
    );
    let save = format!(
        "POST {path}/Service.stickySave HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{}}"
    );
    let failure = "PATCH /UnknownSet(00000000-0000-0000-0000-000000000001) HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}".to_string();
    let (_, headers, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &changeset(&[patch.clone(), save.clone(), failure]),
            "multipart/mixed; boundary=batch",
            Some(&id),
            "alice",
        )
        .await;
    assert!(text.contains("404 Not Found"), "{text}");
    assert_eq!(headers[CONTEXT], id);
    assert_eq!(fixture.disk()["packages"][0]["name"], "test");
    let (_, record) = fixture
        .call(Method::GET, &path, Value::Null, Some(&id), StatusCode::OK)
        .await;
    assert_eq!(record["name"], "test");
    let (_, headers, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &changeset(&[patch, save]),
            "multipart/mixed; boundary=batch",
            Some(&id),
            "alice",
        )
        .await;
    assert!(text.contains("200 OK"), "{text}");
    assert!(text.contains("Content-ID: 2"), "{text}");
    assert!(!headers.contains_key(CONTEXT));
    assert_eq!(fixture.disk()["packages"][0]["name"], "Batch package");
}

#[tokio::test]
async fn metadata_advertises_sap_session_actions_without_drafts() {
    let fixture = Fixture::new();
    let (_, _, xml) = fixture
        .raw(
            Method::GET,
            "/$metadata",
            "",
            "application/json",
            None,
            "alice",
        )
        .await;
    assert!(xml.contains("Session.StickySessionSupported"));
    assert!(xml.contains("Collection(Service.Entity)"));
    assert!(xml.contains(r#"<ActionImport Name="stickyDiscard" Action="Service.stickyDiscard"/>"#));
    assert!(!xml.contains("Common.DraftRoot"));
    assert!(!xml.contains("IsActiveEntity"));
    assert!(!xml.contains("DraftAdministrativeData"));
}

#[tokio::test]
async fn changeset_can_create_and_save_an_entity_tree_using_content_ids() {
    let fixture = Fixture::new();
    let body = changeset(&[
            "POST EntitySet/Service.stickyNew HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}".into(),
            format!("PATCH $1 HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"BatchEntity\",\"package_ID\":\"{}\"}}", fixture.package),
            "POST $1/_Fields HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{\"name\":\"code\",\"type\":\"Edm.String\"}".into(),
            "POST $1/Service.stickySave HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}".into(),
        ]);
    let (status, headers, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &body,
            "multipart/mixed; boundary=batch",
            None,
            "alice",
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert!(text.contains("201 Created"), "{text}");
    assert!(text.contains("Content-ID: 4"), "{text}");
    assert!(!headers.contains_key(CONTEXT));
    assert_eq!(fixture.disk()["entities"][0]["name"], "BatchEntity");
    assert_eq!(
        fixture.disk()["entities"][0]["fields"][0]["Atom"]["name"],
        "code"
    );
}

#[tokio::test]
async fn batch_carries_new_context_and_failed_save_restores_prior_work() {
    let fixture = Fixture::new();
    let body = changeset(&[
        "POST PackageSet/Service.stickyNew HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}"
            .into(),
        "PATCH $1 HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{\"name\":\"Pending\"}".into(),
    ]);
    let (_, headers, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &body,
            "multipart/mixed; boundary=batch",
            None,
            "alice",
        )
        .await;
    let id = headers[CONTEXT].to_str().unwrap();
    assert_eq!(headers[TIMEOUT], "120");
    assert!(
        text.contains("Pending") || text.contains("204 No Content"),
        "{text}"
    );
    assert_eq!(fixture.disk()["packages"].as_array().unwrap().len(), 1);
    let (_, collection) = fixture
        .call(
            Method::GET,
            "/PackageSet",
            Value::Null,
            Some(id),
            StatusCode::OK,
        )
        .await;
    let record = collection["value"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Pending")
        .unwrap();
    let path = format!("/PackageSet({})", record["ID"].as_str().unwrap());
    let body = changeset(&[
        format!("PATCH {path} HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"\"}}"),
        format!(
            "POST {path}/Service.stickySave HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{}}"
        ),
    ]);
    let (_, headers, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &body,
            "multipart/mixed; boundary=batch",
            Some(id),
            "alice",
        )
        .await;
    assert!(text.contains("409 Conflict"), "{text}");
    assert_eq!(headers[CONTEXT], id);
    let (_, record) = fixture
        .call(Method::GET, &path, Value::Null, Some(id), StatusCode::OK)
        .await;
    assert_eq!(record["name"], "Pending");
    assert_eq!(fixture.disk()["packages"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn direct_changesets_are_atomic_and_invalid_batches_return_errors() {
    let fixture = Fixture::new();
    let path = format!("/PackageSet({})", fixture.package);
    let patch = format!(
        "PATCH {path} HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"Atomic\"}}"
    );
    let failure = "PATCH $unknown HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}".to_string();
    let (_, _, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &changeset(&[patch.clone(), failure]),
            "multipart/mixed; boundary=batch",
            None,
            "alice",
        )
        .await;
    assert!(text.contains("400 Bad Request"), "{text}");
    assert_eq!(fixture.disk()["packages"][0]["name"], "test");
    let (_, _, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &changeset(&[patch]),
            "multipart/mixed; boundary=batch",
            None,
            "alice",
        )
        .await;
    assert!(text.contains("204 No Content"), "{text}");
    assert_eq!(fixture.disk()["packages"][0]["name"], "Atomic");
    for body in [
        "not a multipart body",
        "--batch\r\nContent-Type: application/http\r\n\r\nINVALID\r\n--batch--",
    ] {
        let (status, _, _) = fixture
            .raw(
                Method::POST,
                "/$batch",
                body,
                "multipart/mixed; boundary=batch",
                None,
                "alice",
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let (status, headers, _) = fixture
        .raw(
            Method::POST,
            "/$batch",
            "--batch--",
            "multipart/mixed; boundary=batch",
            Some(&Uuid::new_v4().to_string()),
            "alice",
        )
        .await;
    assert_eq!(status, StatusCode::GONE);
    assert!(!headers.contains_key(CONTEXT));
}

struct RejectPatch;

#[async_trait::async_trait]
impl crate::hook::Hook for RejectPatch {
    async fn before_read(
        &self,
        _: &crate::hook::Extender,
        _: &str,
        _: Option<&EntityKey>,
        query: &mut ODataQuery,
    ) -> Result<(), crate::hook::HookError> {
        if query.select.iter().any(|field| field == "name") {
            return Err(StoreError::Forbidden("Selected read denied".into()).into());
        }
        Ok(())
    }

    async fn before_patch(
        &self,
        _: &crate::hook::Extender,
        _: &str,
        _: &EntityKey,
        _: &mut Value,
    ) -> Result<(), crate::hook::HookError> {
        Err(StoreError::Forbidden("Read only".into()).into())
    }
}

impl crate::extension::Extension for RejectPatch {
    fn hooks(&self) -> Vec<Arc<dyn crate::hook::Hook>> {
        vec![Arc::new(RejectPatch)]
    }
}

#[tokio::test]
async fn session_stores_preserve_extension_hooks() {
    let mut fixture = Fixture::new();
    fixture.state = fixture
        .state
        .clone()
        .with_extensions(vec![Arc::new(RejectPatch)]);
    fixture.app = crate::batch::router(crate::srv::router(), fixture.state.clone())
        .with_state(fixture.state.clone())
        .layer(axum::middleware::from_fn_with_state(
            fixture.state.config.clone(),
            crate::auth::identity,
        ));
    let id = fixture.edit().await;
    fixture
        .call(
            Method::PATCH,
            &format!("/PackageSet({})", fixture.package),
            json!({"name":"Forbidden"}),
            Some(&id),
            StatusCode::FORBIDDEN,
        )
        .await;
    assert_eq!(fixture.disk()["packages"][0]["name"], "test");
    fixture
        .call(
            Method::POST,
            "/stickyDiscard",
            json!({}),
            Some(&id),
            StatusCode::NO_CONTENT,
        )
        .await;
    for path in [
        format!(
            "/PackageSet({})/Service.stickyEdit?$select=name",
            fixture.package
        ),
        "/PackageSet/Service.stickyNew?$select=name".to_string(),
    ] {
        let (headers, _) = fixture
            .call(Method::POST, &path, json!({}), None, StatusCode::FORBIDDEN)
            .await;
        assert!(!headers.contains_key(CONTEXT));
        assert!(
            fixture
                .state
                .sticky
                .as_ref()
                .unwrap()
                .sessions
                .lock()
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn changeset_rejects_mixed_session_lifecycles_without_partial_saves() {
    let fixture = Fixture::new();
    let id = fixture.edit().await;
    let path = format!("/PackageSet({})", fixture.package);
    let requests = [
        format!(
            "PATCH {path} HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"Unsaved\"}}"
        ),
        format!(
            "POST {path}/Service.stickySave HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{}}"
        ),
        "POST PackageSet/Service.stickyNew HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}"
            .into(),
    ];
    let (_, headers, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &changeset(&requests),
            "multipart/mixed; boundary=batch",
            Some(&id),
            "alice",
        )
        .await;
    assert!(text.contains("409 Conflict"), "{text}");
    assert_eq!(headers[CONTEXT], id);
    assert_eq!(fixture.disk()["packages"][0]["name"], "test");
    fixture
        .call(
            Method::POST,
            "/stickyDiscard",
            json!({}),
            Some(&id),
            StatusCode::NO_CONTENT,
        )
        .await;
    let requests = [requests[0].clone(), requests[2].clone()];
    let (_, headers, text) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &changeset(&requests),
            "multipart/mixed; boundary=batch",
            None,
            "alice",
        )
        .await;
    assert!(text.contains("409 Conflict"), "{text}");
    assert!(!headers.contains_key(CONTEXT));
    assert_eq!(fixture.disk()["packages"][0]["name"], "test");
}

#[tokio::test]
async fn unsupported_runtime_changesets_fail_before_mutating_the_store() {
    let mut fixture = Fixture::new();
    fixture.state.sticky = None;
    fixture.app = crate::batch::router(crate::srv::router(), fixture.state.clone())
        .with_state(fixture.state.clone());
    let patch = format!(
        "PATCH PackageSet({}) HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"MustNotPersist\"}}",
        fixture.package
    );
    let (status, _, _) = fixture
        .raw(
            Method::POST,
            "/$batch",
            &changeset(&[patch.clone(), patch]),
            "multipart/mixed; boundary=batch",
            None,
            "alice",
        )
        .await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(fixture.disk()["packages"][0]["name"], "test");
}

#[tokio::test]
async fn designer_expansions_preserve_session_state_and_nullable_navigation_shapes() {
    let fixture = Fixture::new();
    let (_, entity) = fixture
        .call(
            Method::POST,
            "/EntitySet",
            json!({"name":"Expanded","package_ID":fixture.package}),
            None,
            StatusCode::CREATED,
        )
        .await;
    let path = format!("/EntitySet({})", entity["ID"].as_str().unwrap());
    fixture
        .call(
            Method::POST,
            &format!("{path}/_Fields"),
            json!({"name":"code"}),
            None,
            StatusCode::CREATED,
        )
        .await;
    let (headers, edited) = fixture
        .call(
            Method::POST,
            &format!(
                "{path}/Service.stickyEdit?$select=ID,name,package&$expand=package($select=ID,name)"
            ),
            json!({}),
            None,
            StatusCode::OK,
        )
        .await;
    let id = headers[CONTEXT].to_str().unwrap();
    assert_eq!(edited["package"]["name"], "test");
    assert_eq!(edited["package"]["ID"], fixture.package);
    assert!(edited["package"].get("IsActiveEntity").is_none());
    fixture
        .call(
            Method::PATCH,
            &format!("/PackageSet({})", fixture.package),
            json!({"name":"SessionPackage"}),
            Some(id),
            StatusCode::NO_CONTENT,
        )
        .await;
    let (_, collection) = fixture
        .call(
            Method::GET,
            &format!(
                "/EntitySet?$filter=ID%20eq%20{}&$expand=package",
                entity["ID"].as_str().unwrap()
            ),
            Value::Null,
            Some(id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(collection["value"][0]["package"]["name"], "SessionPackage");
    let (_, parent) = fixture
        .call(
            Method::GET,
            &format!(
                "/PackageSet({})?$expand=_Entities($expand=_Fields;$count=true)",
                fixture.package
            ),
            Value::Null,
            Some(id),
            StatusCode::OK,
        )
        .await;
    assert_eq!(parent["_Entities@odata.count"], 1);
    assert_eq!(parent["_Entities"][0]["_Fields"][0]["name"], "code");
    assert!(
        parent["_Entities"][0]["_Fields"][0]
            .get("IsActiveEntity")
            .is_none()
    );
    fixture
        .call(
            Method::POST,
            "/stickyDiscard",
            json!({}),
            Some(id),
            StatusCode::NO_CONTENT,
        )
        .await;
    let (_, restored) = fixture
        .call(
            Method::GET,
            &format!("{path}?$select=package&$expand=package($select=ID,name)"),
            Value::Null,
            None,
            StatusCode::OK,
        )
        .await;
    assert_eq!(restored["package"]["name"], "test");
    let (headers, new) = fixture
        .call(
            Method::POST,
            "/EntitySet/Service.stickyNew?$select=ID,name,package&$expand=package($select=name)",
            json!({}),
            None,
            StatusCode::OK,
        )
        .await;
    assert!(new.get("package").unwrap().is_null());
    fixture
        .call(
            Method::POST,
            "/stickyDiscard",
            json!({}),
            Some(headers[CONTEXT].to_str().unwrap()),
            StatusCode::NO_CONTENT,
        )
        .await;
}
