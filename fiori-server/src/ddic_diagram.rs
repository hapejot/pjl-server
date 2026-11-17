use axum::{
    Json,
    extract::State,
    http::{StatusCode, header},
    response::{Html, IntoResponse},
};
use spec::data_dictionary::DataDictionary;

use crate::AppState;

pub async fn index() -> Html<&'static str> {
    Html(include_str!("../static/ddic-diagram.html"))
}

pub async fn script() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )],
        include_str!("../static/ddic-diagram.js"),
    )
}

pub async fn model(
    State(state): State<AppState>,
) -> Result<Json<DataDictionary>, (StatusCode, String)> {
    if !state.is_designer() {
        return Ok(Json(state.base().as_ref().clone()));
    }

    let path = crate::sources::designer_path(&state.config).map_err(internal_error)?;
    let ddic = tokio::task::spawn_blocking(move || crate::ddic_file::load(&path))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(ddic))
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, String) {
    tracing::error!("DDIC diagram data could not be loaded: {error}");
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
