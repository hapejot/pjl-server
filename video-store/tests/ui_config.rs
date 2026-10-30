//! Prueft die `ui`-Bloecke der echten `ddic.json` gegen die Asset-Dateien unter `ext/`.
//! Jeder Verweis `<app>.ext...` muss eine Datei haben. Der Namespace `fiori.ext` (Server-
//! eigene Formatierung) wird nicht gegen Dateien geprueft.

use std::path::{Path, PathBuf};

use fiori_server::extension::Extension;
use fiori_server::ui_ext::UiExtension;
use spec::data_dictionary::DataDictionary;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ext")
}

fn ext() -> UiExtension {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("ddic.json"))
        .expect("ddic.json");
    let ddic: DataDictionary = serde_json::from_str(&text).expect("ddic.json gueltig");
    UiExtension::new(&ddic, Some(root()))
}

/// `videos.ext.fragment.Player` -> `ext/videos/fragment/Player.fragment.xml`.
/// `None` fuer Verweise, die nicht aus den Assets kommen (`fiori.ext`).
fn fragment_file(template: &str) -> Option<PathBuf> {
    let rest = template.strip_prefix("videos.ext.fragment.")?;
    Some(root().join("videos/fragment").join(format!("{rest}.fragment.xml")))
}

#[test]
fn every_fragment_referenced_by_the_video_app_exists() {
    let ext = ext();
    let page = ext.object_page_content("Video").expect("Object-Page-Beitraege");
    let mut checked = 0;
    for (key, section) in page["body"]["sections"].as_object().unwrap() {
        if let Some(file) = section["template"].as_str().and_then(fragment_file) {
            assert!(file.is_file(), "Section {key}: {} fehlt", file.display());
            checked += 1;
        }
    }
    for (key, col) in ext.list_columns("Video").expect("Spalten").as_object().unwrap() {
        if let Some(file) = col["template"].as_str().and_then(fragment_file) {
            assert!(file.is_file(), "Spalte {key}: {} fehlt", file.display());
            checked += 1;
        }
    }
    for view in ext.list_views("Video") {
        if let Some(file) = view["template"].as_str().and_then(fragment_file) {
            assert!(file.is_file(), "Listenansicht: {} fehlt", file.display());
            checked += 1;
        }
    }
    assert!(checked >= 4, "erwartet: Player, Thumbnails, PreviewColumn, TagSearch");
}

#[test]
fn modules_and_controllers_exist_as_files() {
    let ext = ext();
    for m in ext.ui5_modules() {
        if let Some(rest) = m.strip_prefix("videos/ext/") {
            let file = root().join("videos").join(format!("{rest}.js"));
            assert!(file.is_file(), "Modul {m}: {} fehlt", file.display());
        }
    }
    for (target, controller) in ext.controller_extensions() {
        if let Some(rest) = controller.strip_prefix("videos.ext.") {
            let file = root().join("videos").join(format!("{rest}.controller.js"));
            assert!(file.is_file(), "Controller {target}: {} fehlt", file.display());
        }
    }
}

#[test]
fn list_template_and_hidden_facet_are_configured() {
    let ext = ext();
    let views = ext.list_views("Video");
    assert_eq!(views.len(), 1);
    assert_eq!(views[0]["key"], "TagSearch");
    assert_eq!(views[0]["template"], "videos.ext.fragment.TagSearch");
    let page = ext.object_page_content("Video").unwrap();
    assert_eq!(page["body"]["sections"]["_VideoThumbnailsSection"]["visible"], false);
}
