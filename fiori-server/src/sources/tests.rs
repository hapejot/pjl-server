use super::*;
use axum::http::{StatusCode, header};
use std::io::{Cursor, Write};

/// Minimales Modell als ZIP im Layout `ddic.json`, `scripts/`, `ext/`.
/// Alle Inhalte stehen im Test selbst.
fn model_zip() -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        w.start_file("ddic.json", opts).unwrap();
        w.write_all(br#"{"entities":[],"apps":[]}"#).unwrap();
        w.start_file("scripts/CustomerSet.rhai", opts).unwrap();
        w.write_all(b"fn before_patch(ctx) { ctx }").unwrap();
        w.start_file("ext/videos/Player.js", opts).unwrap();
        w.write_all(b"var a;").unwrap();
        w.finish().unwrap();
    }
    buf.into_inner()
}

#[test]
fn model_root_zip_provides_ddic_scripts_and_ext() {
    let dir = tempfile::tempdir().unwrap();
    let zp = dir.path().join("model.zip");
    std::fs::write(&zp, model_zip()).unwrap();

    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.model_root = Some(format!("zip://{}", zp.display()));
    config.ddic = Some(PathBuf::from("ddic.json"));

    let sources = resolve(&config).unwrap();
    assert!(sources.ddic.repo.exists("ddic.json"));
    assert!(sources.overlay.is_none());
    let scripts = sources.scripts.expect("scripts");
    assert!(scripts.fs_dir.is_none(), "zip hat keinen Dateisystempfad");
    let hooks = script_hooks::ScriptHooks::load_from(scripts.repo, scripts.label).unwrap();
    assert!(hooks.status()["sets"].get("CustomerSet").is_some());
    assert!(sources.ext.expect("ext").repo.exists("videos/Player.js"));
}

#[test]
fn model_root_rejects_legacy_script_and_ext_dirs() {
    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.model_root = Some("zip:///nonexistent.zip".into());
    config.scripts_dir = Some(PathBuf::from("x"));
    assert!(resolve(&config).is_err());
}

#[test]
fn designer_root_must_be_file() {
    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.designer = true;
    config.designer_root = Some("zip:///x.zip".into());
    assert!(designer_path(&config).is_err());

    config.designer_root = Some("file:///srv/designer".into());
    config.ddic = Some(PathBuf::from("ddic.json"));
    assert_eq!(
        designer_path(&config).unwrap(),
        PathBuf::from("/srv/designer/ddic.json")
    );
}

#[test]
fn fiori_root_zip_is_served_with_content_type() {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("index.html", opts).unwrap();
        w.write_all(b"<html></html>").unwrap();
        w.start_file("apps/x/manifest.json", opts).unwrap();
        w.write_all(b"{}").unwrap();
        w.finish().unwrap();
    }
    let repo = file_repo::ZipRepository::from_bytes(
        bytes::Bytes::from(buf.into_inner()),
        &Limits::default(),
    )
    .unwrap();
    let ok = crate::repo_serve::serve(&repo, "");
    assert_eq!(ok.status(), StatusCode::OK);
    assert_eq!(ok.headers()[header::CONTENT_TYPE], "text/html");
    let json = crate::repo_serve::serve(&repo, "/apps/x/manifest.json");
    assert_eq!(json.headers()[header::CONTENT_TYPE], "application/json");
    assert_eq!(
        crate::repo_serve::serve(&repo, "../etc/passwd").status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        crate::repo_serve::serve(&repo, "missing.js").status(),
        StatusCode::NOT_FOUND
    );
}
