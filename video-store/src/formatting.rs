//! Anzeige-Formatierung der Video-Groessen (Bytes, Bitrate, Dauer) fuer den Fiori-Server.
//!
//! * Listen-Tabelle: formatierte Spalten neben den Rohspalten, die Rohspalten sind
//!   ausgeblendet. Rohwerte bleiben fuer Sortierung und Filter erhalten.
//! * Object Page: Abschnitt "Technische Daten" nach "General", die Rohfelder fehlen
//!   im Standard-Formular.
//! * `GET /ext/video/Format.js`: UI5-Modul mit den Formattern (`video.ext.Format.*`),
//!   wird ueber [`Extension::ui5_modules`] mit der Komponente geladen.
//!
//! Der fiori-server kennt diese Felder nicht; die Zuordnung steht hier.

use axum::{
    http::header,
    response::IntoResponse,
    routing::get,
    Router,
};
use fiori_server::extension::Extension;
use serde_json::{json, Value};

const FORMAT_JS: &str = include_str!("../ui5/Format.js");
const BYTES_FRAGMENT: &str = include_str!("../fragments/Bytes.fragment.xml");
const BITRATE_FRAGMENT: &str = include_str!("../fragments/Bitrate.fragment.xml");
const DURATION_FRAGMENT: &str = include_str!("../fragments/Duration.fragment.xml");
const FORMATTED_FIELDS_FRAGMENT: &str = include_str!("../fragments/FormattedFields.fragment.xml");

/// Formatierte Felder der Entitaet `Video`: (Rohfeld, Spalten-Schluessel, Spaltenueberschrift).
const VIDEO_FIELDS: &[(&str, &str, &str)] = &[
    ("file_size", "Bytes", "Größe"),
    ("bitrate", "Bitrate", "Bitrate"),
    ("duration_seconds", "Duration", "Dauer"),
];

#[derive(Default)]
pub struct VideoFormatting;

impl Extension for VideoFormatting {
    fn routes(&self, _state: fiori_server::AppState) -> Router {
        Router::new()
            .route("/ext/video/Format.js", get(format_js))
            .route("/ext/video/fragment/Bytes.fragment.xml", get(bytes_fragment))
            .route("/ext/video/fragment/Bitrate.fragment.xml", get(bitrate_fragment))
            .route("/ext/video/fragment/Duration.fragment.xml", get(duration_fragment))
            .route(
                "/ext/video/fragment/FormattedFields.fragment.xml",
                get(formatted_fields_fragment),
            )
    }

    fn ui5_modules(&self) -> Vec<String> {
        vec!["video/ext/Format".into()]
    }

    fn hidden_fields(&self, entity: &str) -> Vec<String> {
        if entity != "Video" {
            return Vec::new();
        }
        VIDEO_FIELDS.iter().map(|(field, _, _)| field.to_string()).collect()
    }

    fn list_columns(&self, entity: &str) -> Option<Value> {
        if entity != "Video" {
            return None;
        }
        let mut cols = serde_json::Map::new();
        for (field, key, header) in VIDEO_FIELDS {
            cols.insert(
                format!("Fmt{key}"),
                json!({
                    "header": header,
                    "template": format!("video.ext.fragment.{key}"),
                    "horizontalAlign": "End",
                    "width": "110px",
                    "position": { "anchor": field, "placement": "After" }
                }),
            );
            cols.insert(format!("DataField::{field}"), json!({ "visible": false }));
        }
        Some(Value::Object(cols))
    }

    fn object_page_content(&self, entity: &str) -> Option<Value> {
        if entity != "Video" {
            return None;
        }
        Some(json!({ "body": { "sections": {
            "FormattedFields": {
                "title": "Technische Daten",
                "template": "video.ext.fragment.FormattedFields",
                "position": { "placement": "After", "anchor": "GeneralSection" }
            }
        }}}))
    }
}

const XML: &str = "application/xml; charset=utf-8";
const JS: &str = "application/javascript; charset=utf-8";

async fn format_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, JS)], FORMAT_JS)
}

async fn bytes_fragment() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, XML)], BYTES_FRAGMENT)
}

async fn bitrate_fragment() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, XML)], BITRATE_FRAGMENT)
}

async fn duration_fragment() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, XML)], DURATION_FRAGMENT)
}

async fn formatted_fields_fragment() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, XML)], FORMATTED_FIELDS_FRAGMENT)
}
