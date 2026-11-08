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
                root: root
                    .as_ref()
                    .map(|(set, key)| (set.clone(), key.id.clone())),
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
mod tests;
