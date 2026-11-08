use super::*;

#[test]
fn packs_and_reads_back() {
    let src = tempfile::tempdir().unwrap();
    fs::write(src.path().join("ddic.json"), b"{}").unwrap();
    fs::create_dir_all(src.path().join("scripts/components")).unwrap();
    fs::write(src.path().join("scripts/components/TimeRange.rhai"), b"//").unwrap();

    let mut files = Vec::new();
    collect(src.path(), src.path(), &mut files).unwrap();
    let names: Vec<_> = files.iter().map(|(n, _)| n.clone()).collect();
    assert_eq!(
        names,
        vec!["ddic.json", "scripts/components/TimeRange.rhai"]
    );
}
