use super::*;

fn cli(args: &[&str]) -> Cli {
    let mut argv = vec!["test", "--model-root", "model"];
    argv.extend_from_slice(args);
    Cli::try_parse_from(argv).expect("parse")
}

#[test]
fn runtime_defaults() {
    let runtime = cli(&[]).into_config(ServerMode::Runtime);
    assert_eq!(runtime.port, 3034);
    assert_eq!(runtime.data_dir, PathBuf::from("rel-data"));
    assert_eq!(runtime.model_root.as_deref(), Some("model"));
    assert!(!runtime.designer);
}

#[test]
fn designer_flag_keeps_the_runtime_binary() {
    let c = cli(&["--designer"]).into_config(ServerMode::Runtime);
    assert!(c.designer);
    assert_eq!(c.model_root.as_deref(), Some("model"));
    assert!(c.ddic_overlay.is_none());
}

#[test]
fn explicit_flags_win() {
    let c = cli(&["--port", "9000", "--data-dir", "/tmp/db"]).into_config(ServerMode::Runtime);
    assert_eq!(c.port, 9000);
    assert_eq!(c.data_dir, PathBuf::from("/tmp/db"));
}

#[test]
fn model_root_is_required_and_legacy_options_are_rejected() {
    assert!(Cli::try_parse_from(["test"]).is_err());
    for option in ["--ddic", "--scripts-dir", "--ext-dir", "--designer-root"] {
        assert!(Cli::try_parse_from(["test", "--model-root", "model", option, "x"]).is_err());
    }
}
