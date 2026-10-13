//! Laufzeitserver fuer die Video-Datenbank: `pjl-runtime` plus Video-Erweiterungen
//! (Thumbnail-Proxy und Custom Section). Der fiori-server selbst bleibt generisch.

use fiori_server::config::{ServerConfig, ServerMode};
use fiori_server::server;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use video_store::convert::VideoConvert;
use video_store::formatting::VideoFormatting;
use video_store::player::VideoPlayer;
use video_store::tags::VideoTagFacets;
use video_store::thumbnails::VideoThumbnails;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let config = ServerConfig::parse(ServerMode::Runtime);
    let extensions: fiori_server::extension::Extensions =
        vec![
            std::sync::Arc::new(VideoThumbnails::from_env()),
            std::sync::Arc::new(VideoTagFacets::new()),
            std::sync::Arc::new(VideoConvert),
            std::sync::Arc::new(VideoPlayer::from_env()),
            std::sync::Arc::new(VideoFormatting),
        ];
    match server::run_with(config, extensions).await {
        Ok(()) => {
            info!("runtime finished");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            error!("runtime shutdown with error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
