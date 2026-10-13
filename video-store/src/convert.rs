//! Aktion `convert` auf einem Video: legt eine `VideoJob`-Zeile an. Die Abarbeitung
//! uebernimmt der Agent des Videos; der Server startet nichts selbst.
//!
//! * `POST /Service/VideoSet(<id>)/Service.convert` (gebunden, ohne Parameter, ohne Rueckgabe)
//! * Button "Konvertieren" im Header der Video-Object-Page (`UI.Identification`).

use std::sync::Arc;

use data_store::EntityKey;
use fiori_server::{
    extension::{ActionError, ActionHandler, Availability, BoundAction, Extension, HeaderButton},
    AppState,
};
use odata::query::ODataQuery;
use serde_json::{json, Value};
use tracing::info;

const VIDEO_SET: &str = "VideoSet";
const JOB_SET: &str = "VideoJobSet";
/// Wert von `VideoJob.job_type` fuer die Konvertierung.
pub const JOB_TYPE_CONVERT: &str = "convert";

pub struct VideoConvert;

struct ConvertHandler;

#[async_trait::async_trait]
impl ActionHandler for ConvertHandler {
    async fn invoke(&self, state: &AppState, key: &EntityKey) -> Result<Value, ActionError> {
        // Jobs haengen am aktiven Video. Ein Entwurf ohne Aktivierung ist dort
        // noch nicht vorhanden, daher immer die aktive Version lesen.
        let active = EntityKey::new(key.id, true);
        let video = state
            .store
            .read_entity(VIDEO_SET, &active, &ODataQuery::default())
            .await?;
        let agent = video
            .get("agent_ID")
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| ActionError::Failed("Video hat keinen Agenten".into()))?;
        // Nur nicht streambare Videos werden konvertiert (gleiche Regel wie
        // `Core.OperationAvailable` im $metadata; hier erzwungen, nicht nur angezeigt).
        if video.get("is_streamable").and_then(|v| v.as_bool()) != Some(false) {
            return Err(ActionError::Failed(
                "Nur Videos mit is_streamable = false koennen konvertiert werden".into(),
            ));
        }
        let title = video
            .get("name")
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "Video".to_string());

        let job = state
            .store
            .create_entity(
                JOB_SET,
                &json!({
                    "name": format!("Konvertierung: {title}"),
                    "job_type": JOB_TYPE_CONVERT,
                    "video_ID": key.id.to_string(),
                    "agent_ID": agent,
                    "IsActiveEntity": true,
                }),
            )
            .await?;
        info!(video = %key.id, job = %job.id(), "convert job created");
        Ok(Value::Null)
    }
}

impl Extension for VideoConvert {
    fn actions(&self) -> Vec<BoundAction> {
        vec![BoundAction {
            set: VIDEO_SET.into(),
            name: "convert".into(),
            returns: None,
            handler: Arc::new(ConvertHandler),
            header: Some(HeaderButton {
                text: "Konvertieren".into(),
            }),
            available_if: Some(Availability {
                property: "is_streamable".into(),
                equals: json!(false),
            }),
        }]
    }
}
