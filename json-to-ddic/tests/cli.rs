use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use spec::data_dictionary::DataDictionary;

#[test]
fn cli_supports_xml_auto_detection_explicit_formats_and_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.XML");
    let output = dir.path().join("ddic.json");
    let xml = r#"<orders><order code="001"/><order code="002"/></orders>"#;
    fs::write(&input, xml).unwrap();
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_json-to-ddic"))
            .arg(&input)
            .arg("--out")
            .arg(&output)
            .args(extra)
            .output()
            .unwrap()
    };
    let result = run(&[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = fs::read(&output).unwrap();
    let mut ddic: DataDictionary = serde_json::from_slice(&original).unwrap();
    ddic.activate();
    spec_schema::schema_from_ddic(&ddic).unwrap();

    fs::write(&input, "<root><unclosed></root>").unwrap();
    let result = run(&["--force"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Ungueltiges XML"));
    assert_eq!(fs::read(&output).unwrap(), original);

    fs::write(&input, r#"{"name":"JSON despite XML extension"}"#).unwrap();
    let result = run(&["--format", "json", "--force"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(json["entities"][0]["fields"][0]["Atom"]["name"], "name");

    let mut child = Command::new(env!("CARGO_BIN_EXE_json-to-ddic"))
        .args(["-", "--format", "xml", "--force", "--out"])
        .arg(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(xml.as_bytes())
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&output).unwrap(), original);
}

#[test]
fn cli_persists_valid_ddic_protects_existing_files_and_reports_invalid_json() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.json");
    let output = dir.path().join("ddic.json");
    fs::write(
        &input,
        r#"{"name":"Sample","items":[{"quantity":1.23456}]}"#,
    )
    .unwrap();
    let run = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_json-to-ddic"))
            .arg(&input)
            .arg("--out")
            .arg(&output)
            .args(extra)
            .output()
            .unwrap()
    };
    let result = run(&["--entity", "Sample"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = fs::read(&output).unwrap();
    let mut ddic: DataDictionary = serde_json::from_slice(&original).unwrap();
    ddic.activate();
    spec_schema::schema_from_ddic(&ddic).unwrap();

    assert!(!run(&[]).status.success());
    assert_eq!(fs::read(&output).unwrap(), original);
    fs::write(&input, "invalid").unwrap();
    let invalid = run(&["--force"]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Ungueltiges JSON"));
    assert_eq!(fs::read(&output).unwrap(), original);

    let mut child = Command::new(env!("CARGO_BIN_EXE_json-to-ddic"))
        .args(["-", "--force", "--out"])
        .arg(&output)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"flag":true}"#)
        .unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(document["entities"][0]["fields"][0]["Atom"]["name"], "flag");
}
