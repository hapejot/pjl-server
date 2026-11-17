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
fn model_root_is_required() {
    let config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    assert!(resolve(&config).is_err());
}

#[test]
fn designer_uses_model_root_and_requires_file() {
    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.designer = true;
    config.model_root = Some("zip:///x.zip".into());
    assert!(designer_path(&config).is_err());

    config.model_root = Some("file:///srv/designer".into());
    assert_eq!(
        designer_path(&config).unwrap(),
        PathBuf::from("/srv/designer/ddic.json")
    );
}

#[test]
fn directory_model_supplies_runtime_sources_but_designer_skips_them() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("scripts")).unwrap();
    std::fs::create_dir(dir.path().join("ext")).unwrap();
    std::fs::write(
        dir.path().join("scripts/OrderSet.rhai"),
        "fn before_read(ctx) { ctx }",
    )
    .unwrap();
    std::fs::write(dir.path().join("ddic.json"), r#"{"entities":[],"apps":[]}"#).unwrap();
    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.model_root = Some(dir.path().to_string_lossy().into_owned());
    let sources = resolve(&config).unwrap();
    assert_eq!(sources.ddic.fs_path, Some(dir.path().join("ddic.json")));
    assert_eq!(
        sources.scripts.unwrap().fs_dir,
        Some(dir.path().join("scripts"))
    );
    assert_eq!(sources.ext.unwrap().fs_dir, Some(dir.path().join("ext")));
    config.designer = true;
    let sources = resolve(&config).unwrap();
    assert!(sources.scripts.is_none());
    assert!(sources.ext.is_none());
    assert_eq!(
        designer_path(&config).unwrap(),
        dir.path().join("ddic.json")
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

#[test]
fn embedded_web_assets_are_served_without_a_web_directory() {
    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.model_root = Some(".".into());

    let sources = resolve(&config).unwrap();
    assert!(sources.fiori.exists("favicon.svg"));
    assert!(sources.fiori.exists("apps/EntityConfigs/manifest.json"));
    let response = crate::repo_serve::serve(sources.fiori.as_ref(), "favicon.svg");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
}

#[test]
fn web_root_assets_override_and_extend_embedded_assets() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("favicon.svg"), b"custom").unwrap();
    std::fs::write(dir.path().join("custom.js"), b"custom asset").unwrap();

    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.model_root = Some(".".into());
    config.web_root = Some(dir.path().to_path_buf());

    let sources = resolve(&config).unwrap();
    assert_eq!(sources.fiori.read("favicon.svg").unwrap(), b"custom"[..]);
    assert!(sources.fiori.exists("custom.js"));
    assert!(sources.fiori.exists("logo.svg"));
    assert_eq!(
        sources.fiori.list("").unwrap(),
        vec![
            "apps".to_string(),
            "custom.js".to_string(),
            "favicon.svg".to_string(),
            "logo.svg".to_string()
        ]
    );
}

#[test]
fn fiori_root_zip_overrides_embedded_assets() {
    let dir = tempfile::tempdir().unwrap();
    let zip_path = dir.path().join("fiori.zip");
    let mut buf = Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buf);
        writer
            .start_file("favicon.svg", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"custom").unwrap();
        writer.finish().unwrap();
    }
    std::fs::write(&zip_path, buf.into_inner()).unwrap();

    let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    config.model_root = Some(".".into());
    config.fiori_root = Some(format!("zip://{}", zip_path.display()));

    let sources = resolve(&config).unwrap();
    assert_eq!(sources.fiori.read("favicon.svg").unwrap(), b"custom"[..]);
    assert!(sources.fiori.exists("logo.svg"));
}
