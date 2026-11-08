//! SAP sticky sessions for the file-backed designer. Session changes live only
//! in memory; Save validates and atomically replaces the dictionary.

use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use data_store::{DataStore, EntityKey, StoreError, datastore::current_principal};
use odata::query::ODataQuery;
use serde_json::{Value, json};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use uuid::Uuid;

use crate::{
    AppState,
    ddic_store::{DdicStore, Snapshot},
};

pub const CONTEXT: &str = "sap-contextid";
pub const TIMEOUT: &str = "sap-http-session-timeout";
const SESSION_TIMEOUT: Duration = Duration::from_secs(120);

tokio::task_local! {
    static REQUEST_CONTEXT: RefCell<Option<String>>;
    static CHANGESET: RefCell<Option<String>>;
    static DIRECT_STORE: Arc<DdicStore>;
}

pub fn context_id() -> Option<String> {
    REQUEST_CONTEXT
        .try_with(|id| id.borrow().clone())
        .ok()
        .flatten()
}

fn set_context(id: Option<String>) {
    let _ = REQUEST_CONTEXT.try_with(|current| *current.borrow_mut() = id);
}

pub async fn context(req: Request, next: Next) -> Response {
    let id = match req.headers().get(CONTEXT) {
        Some(value) => match value.to_str() {
            Ok(value) if !value.is_empty() => Some(value.to_string()),
            _ => return error(StatusCode::BAD_REQUEST, "Invalid sap-contextid"),
        },
        None => None,
    };
    REQUEST_CONTEXT
        .scope(RefCell::new(id), async {
            let mut response = next.run(req).await;
            response
                .headers_mut()
                .insert("odata-version", HeaderValue::from_static("4.0"));
            if let Some(id) = context_id() {
                if let Ok(value) = HeaderValue::from_str(&id) {
                    response.headers_mut().insert(CONTEXT, value);
                    response
                        .headers_mut()
                        .insert(TIMEOUT, HeaderValue::from_static("120"));
                }
            }
            response
        })
        .await
}

pub fn error(status: StatusCode, message: impl Into<String>) -> Response {
    let mut response = (
        status,
        axum::Json(json!({"error":{"code":status.as_str(),"message":message.into()}})),
    )
        .into_response();
    response
        .headers_mut()
        .insert("odata-version", HeaderValue::from_static("4.0"));
    if status == StatusCode::GONE {
        response
            .headers_mut()
            .insert("sap-err-id", HeaderValue::from_static("ICMENOSESSION"));
    }
    response
}

#[derive(Clone)]
struct Session {
    owner: String,
    touched: Instant,
    baseline: Snapshot,
    working: Arc<DdicStore>,
    root: Option<(String, data_store::key::KeyValue)>,
}

pub struct StickySessions {
    original: Arc<DdicStore>,
    sessions: Mutex<HashMap<String, Session>>,
    gate: Arc<AsyncMutex<()>>,
    timeout: Duration,
}

impl StickySessions {
    pub fn new(store: Arc<DdicStore>) -> Self {
        Self {
            original: store,
            sessions: Mutex::new(HashMap::new()),
            gate: Arc::new(AsyncMutex::new(())),
            timeout: SESSION_TIMEOUT,
        }
    }

    fn session(&self, id: &str) -> Result<Session, Response> {
        let mut sessions = self.sessions.lock().expect("session lock");
        sessions.retain(|_, session| session.touched.elapsed() < self.timeout);
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| error(StatusCode::GONE, "Sticky session expired or unknown"))?;
        if session.owner != current_principal() {
            return Err(error(
                StatusCode::FORBIDDEN,
                "Sticky session belongs to another user",
            ));
        }
        session.touched = Instant::now();
        Ok(session.clone())
    }

    pub(crate) fn validate_context(&self, id: &str) -> Result<(), Response> {
        self.session(id).map(|_| ())
    }

    pub async fn request_state(
        &self,
        state: &AppState,
    ) -> Result<(AppState, Option<OwnedMutexGuard<()>>), Response> {
        let in_changeset = CHANGESET.try_with(|_| ()).is_ok();
        let guard = if in_changeset {
            None
        } else {
            Some(self.gate.clone().lock_owned().await)
        };
        let mut state = state.clone();
        if let Some(id) = context_id() {
            match self.session(&id) {
                Ok(session) => {
                    state.store = crate::server::wrap_store(session.working, &state.extensions)
                        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                }
                Err(response) => {
                    set_context(None);
                    return Err(response);
                }
            }
        } else if let Ok(store) = DIRECT_STORE.try_with(Arc::clone) {
            state.store = crate::server::wrap_store(store, &state.extensions)
                .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }
        Ok((state, guard))
    }

    async fn begin(&self, root: Option<(String, EntityKey)>) -> Result<Session, Response> {
        if CHANGESET
            .try_with(|pending| pending.borrow().is_some())
            .unwrap_or(false)
        {
            return Err(error(
                StatusCode::CONFLICT,
                "Cannot start another session after Save within a changeset",
            ));
        }
        let existing = context_id();
        if existing.is_none()
            && DIRECT_STORE
                .try_with(|store| store.changed_since(&self.original.snapshot()))
                .unwrap_or(false)
        {
            return Err(error(
                StatusCode::CONFLICT,
                "Cannot start a session after writes outside the session within a changeset",
            ));
        }
        let session = if let Some(id) = &existing {
            self.session(id)?
        } else {
            Session {
                owner: current_principal(),
                touched: Instant::now(),
                baseline: self.original.snapshot(),
                working: Arc::new(self.original.fork()),
                root: root.as_ref().map(|(set, key)| (set.clone(), key.id.clone())),
            }
        };
        if let Some((set, key)) = root {
            session
                .working
                .read_entity(&set, &key, &ODataQuery::new())
                .await
                .map_err(store_error)?;
        }
        if existing.is_none() {
            let mut sessions = self.sessions.lock().expect("session lock");
            sessions.retain(|_, s| s.touched.elapsed() < self.timeout);
            if session
                .root
                .as_ref()
                .is_some_and(|root| sessions.values().any(|s| s.root.as_ref() == Some(root)))
            {
                return Err(error(
                    StatusCode::CONFLICT,
                    "Entity is already being edited",
                ));
            }
            let id = Uuid::new_v4().to_string();
            sessions.insert(id.clone(), session.clone());
            set_context(Some(id));
        }
        Ok(session)
    }

    async fn entity_json(
        &self,
        state: &AppState,
        session: &Session,
        set: &str,
        key: &EntityKey,
        query: &ODataQuery,
    ) -> Result<Value, Response> {
        let mut state = state.clone();
        state.store = crate::server::wrap_store(session.working.clone(), &state.extensions)
            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        crate::srv::action_entity_json(&state, set, key, query).await
    }

    async fn start(
        &self,
        state: &AppState,
        set: &str,
        key: Option<&EntityKey>,
        query: &ODataQuery,
    ) -> Response {
        let new_session = context_id().is_none();
        let session = match self
            .begin(key.map(|key| (set.to_string(), key.clone())))
            .await
        {
            Ok(session) => session,
            Err(response) => return response,
        };
        let snapshot = session.working.snapshot();
        let result = async {
            let entity_key = match key {
                Some(key) => key.clone(),
                None => {
                    let store =
                        crate::server::wrap_store(session.working.clone(), &state.extensions)
                            .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    let record = store
                        .create_entity(set, &json!({}))
                        .await
                        .map_err(store_error)?;
                    EntityKey::new(record.id(), true)
                }
            };
            let value = self
                .entity_json(state, &session, set, &entity_key, query)
                .await?;
            let mut response = axum::Json(value).into_response();
            if key.is_none() {
                response.headers_mut().insert(
                    axum::http::header::LOCATION,
                    HeaderValue::from_str(&format!("/Service/{set}({})", entity_key.id))
                        .expect("validated entity set and UUID"),
                );
            }
            Ok::<_, Response>(response)
        }
        .await;
        match result {
            Ok(response) => response,
            Err(response) => {
                session.working.restore(snapshot);
                if new_session {
                    if let Some(id) = context_id() {
                        self.sessions.lock().expect("session lock").remove(&id);
                    }
                    set_context(None);
                }
                response
            }
        }
    }

    pub async fn action(
        &self,
        state: &AppState,
        set: Option<&str>,
        key: Option<&EntityKey>,
        action: &str,
        query: &ODataQuery,
    ) -> Response {
        match action {
            "stickyNew" if set.is_some() && key.is_none() => {
                self.start(state, set.expect("collection binding"), None, query)
                    .await
            }
            "stickyEdit" if set.is_some() && key.is_some() => {
                self.start(state, set.expect("entity binding"), key, query)
                    .await
            }
            "stickySave" if set.is_some() && key.is_some() => {
                let Some(id) = context_id() else {
                    return error(StatusCode::BAD_REQUEST, "Save requires a sticky session");
                };
                let session = match self.session(&id) {
                    Ok(s) => s,
                    Err(r) => return r,
                };
                let (Some(set), Some(key)) = (set, key) else {
                    unreachable!()
                };
                let record = match self.entity_json(state, &session, set, key, query).await {
                    Ok(record) => record,
                    Err(response) => return response,
                };
                if CHANGESET
                    .try_with(|pending| *pending.borrow_mut() = Some(id.clone()))
                    .is_err()
                {
                    if let Err(e) = self.commit(&id) {
                        return e;
                    }
                }
                set_context(None);
                axum::Json(record).into_response()
            }
            "stickyDiscard" if set.is_none() => {
                let Some(id) = context_id() else {
                    return error(StatusCode::BAD_REQUEST, "Discard requires a sticky session");
                };
                if let Err(response) = self.session(&id) {
                    return response;
                }
                self.sessions.lock().expect("session lock").remove(&id);
                set_context(None);
                StatusCode::NO_CONTENT.into_response()
            }
            _ => error(StatusCode::BAD_REQUEST, "Invalid sticky action binding"),
        }
    }

    fn commit(&self, id: &str) -> Result<(), Response> {
        let session = self.session(id)?;
        self.original
            .save_session(&session.baseline, &session.working)
            .map_err(store_error)?;
        self.sessions.lock().expect("session lock").remove(id);
        Ok(())
    }

    pub(crate) async fn changeset<F, Fut>(&self, execute: F) -> Result<Vec<Response>, Response>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Vec<Response>>,
    {
        let _guard = self.gate.clone().lock_owned().await;
        let baseline = self.sessions.lock().expect("session lock").clone();
        let original = self.original.snapshot();
        let direct = Arc::new(self.original.fork());
        let snapshots: HashMap<_, _> = baseline
            .iter()
            .map(|(id, session)| (id.clone(), session.working.snapshot()))
            .collect();
        let (responses, pending) = DIRECT_STORE
            .scope(
                direct.clone(),
                CHANGESET.scope(RefCell::new(None), async {
                    let responses = execute().await;
                    (
                        responses,
                        CHANGESET.with(|pending| pending.borrow().clone()),
                    )
                }),
            )
            .await;
        let failed = responses.iter().any(|r| !r.status().is_success());
        let commit = if !failed {
            if direct.changed_since(&original) {
                if pending.is_some() {
                    Err(error(
                        StatusCode::CONFLICT,
                        "Cannot mix a session save with writes outside the session",
                    ))
                } else {
                    self.original
                        .save_session(&original, &direct)
                        .map_err(store_error)
                }
            } else {
                match pending {
                    Some(id) => self.commit(&id),
                    None => Ok(()),
                }
            }
        } else {
            Ok(())
        };
        if failed || commit.is_err() {
            let mut sessions = self.sessions.lock().expect("session lock");
            *sessions = baseline;
            for (id, snapshot) in snapshots {
                if let Some(session) = sessions.get(&id) {
                    session.working.restore(snapshot);
                }
            }
        }
        commit?;
        Ok(responses)
    }
}

fn store_error(store_error: StoreError) -> Response {
    let status = match store_error {
        StoreError::NotFound { .. } => StatusCode::NOT_FOUND,
        StoreError::Forbidden(_) => StatusCode::FORBIDDEN,
        StoreError::Conflict(_) => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    };
    error(status, store_error.to_string())
}

pub async fn request_state(
    state: &AppState,
) -> Result<(AppState, Option<OwnedMutexGuard<()>>), Response> {
    match &state.sticky {
        Some(sessions) => sessions.request_state(state).await,
        None if context_id().is_some() => Err(error(
            StatusCode::BAD_REQUEST,
            "Sticky sessions are not enabled",
        )),
        None => Ok((state.clone(), None)),
    }
}

#[cfg(test)]
mod tests {
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
        let mut body =
            "--batch\r\nContent-Type: multipart/mixed; boundary=changes\r\n\r\n".to_string();
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
        assert!(
            xml.contains(r#"<ActionImport Name="stickyDiscard" Action="Service.stickyDiscard"/>"#)
        );
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
            "POST PackageSet/Service.stickyNew HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}".into(),
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
            format!(
                "PATCH {path} HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"\"}}"
            ),
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
        let failure =
            "PATCH $unknown HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}".to_string();
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
            format!("PATCH {path} HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"Unsaved\"}}"),
            format!("POST {path}/Service.stickySave HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{{}}"),
            "POST PackageSet/Service.stickyNew HTTP/1.1\r\nContent-Type: application/json\r\n\r\n{}".into(),
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
        let (headers, edited) = fixture.call(Method::POST,
            &format!("{path}/Service.stickyEdit?$select=ID,name,package&$expand=package($select=ID,name)"),
            json!({}), None, StatusCode::OK).await;
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
        let (headers, new) = fixture.call(Method::POST,
            "/EntitySet/Service.stickyNew?$select=ID,name,package&$expand=package($select=name)",
            json!({}), None, StatusCode::OK).await;
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
}
