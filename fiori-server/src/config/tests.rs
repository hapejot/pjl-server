use super::*;

fn cli(args: &[&str]) -> Cli {
    let mut argv = vec!["test"];
    argv.extend_from_slice(args);
    Cli::try_parse_from(argv).expect("parse")
}

#[test]
fn runtime_defaults() {
    let runtime = cli(&[]).into_config(ServerMode::Runtime);
    assert_eq!(runtime.port, 3034);
    assert_eq!(runtime.data_dir, PathBuf::from("rel-data"));
    assert_eq!(runtime.ddic, Some(PathBuf::from("ddic.json")));
    assert!(!runtime.designer);
}

#[test]
fn designer_flag_keeps_the_runtime_binary() {
    let c = cli(&["--designer", "--ddic", "x.json"]).into_config(ServerMode::Runtime);
    assert!(c.designer);
    assert_eq!(c.ddic, Some(PathBuf::from("x.json")));
    assert!(c.ddic_overlay.is_none());
}

#[test]
fn explicit_flags_win() {
    let c = cli(&["--port", "9000", "--data-dir", "/tmp/db"]).into_config(ServerMode::Runtime);
    assert_eq!(c.port, 9000);
    assert_eq!(c.data_dir, PathBuf::from("/tmp/db"));
}

#[test]
fn runtime_ddic_default_is_ddic_json() {
    let c = cli(&[]).into_config(ServerMode::Runtime);
    assert_eq!(c.ddic, Some(PathBuf::from("ddic.json")));
}
