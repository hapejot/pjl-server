use super::*;
use std::io::Write;

fn make_zip(entries: &[(&str, &[u8])]) -> Bytes {
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            if name.ends_with('/') {
                w.add_directory(*name, opts).unwrap();
            } else {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
        }
        w.finish().unwrap();
    }
    Bytes::from(buf.into_inner())
}

fn fixtures() -> Vec<(&'static str, &'static [u8])> {
    vec![
        ("ddic.json", b"{}"),
        ("scripts/CustomerSet.rhai", b"// x"),
        ("scripts/components/TimeRange.rhai", b"// y"),
        ("ext/videos/Player.js", b"var a;"),
    ]
}

/// Dasselbe Dateilayout als Verzeichnis und als ZIP.
fn both() -> (
    tempfile::TempDir,
    Box<dyn FileRepository>,
    Box<dyn FileRepository>,
) {
    let dir = tempfile::tempdir().unwrap();
    for (name, data) in fixtures() {
        let p = dir.path().join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, data).unwrap();
    }
    let d = DirRepository::new(dir.path());
    let z = ZipRepository::from_bytes(make_zip(&fixtures()), &Limits::default()).unwrap();
    (dir, Box::new(d), Box::new(z))
}

#[test]
fn conformance_read_list_exists() {
    let (_dir, d, z) = both();
    for repo in [&d, &z] {
        assert_eq!(repo.read("ddic.json").unwrap(), Bytes::from_static(b"{}"));
        assert!(matches!(
            repo.read("missing.json"),
            Err(RepoError::NotFound(_))
        ));
        assert_eq!(
            repo.read("./scripts//CustomerSet.rhai").unwrap(),
            Bytes::from_static(b"// x")
        );
        assert_eq!(repo.list("").unwrap(), vec!["ddic.json", "ext", "scripts"]);
        assert_eq!(
            repo.list("scripts").unwrap(),
            vec!["CustomerSet.rhai".to_string(), "components".to_string()]
        );
        assert!(repo.list("nope").unwrap().is_empty());
        assert!(repo.exists("ext/videos"));
        assert!(repo.exists("ext/videos/Player.js"));
        assert!(!repo.exists("ext/videos/Other.js"));
    }
}

#[test]
fn rejects_unsafe_paths() {
    let (_dir, d, z) = both();
    for repo in [&d, &z] {
        assert!(matches!(
            repo.read("../etc/passwd"),
            Err(RepoError::UnsafePath(_))
        ));
        assert!(matches!(
            repo.read("/etc/passwd"),
            Err(RepoError::UnsafePath(_))
        ));
        assert!(matches!(
            repo.read("a\\..\\b"),
            Err(RepoError::UnsafePath(_))
        ));
        assert!(matches!(repo.read("C:x"), Err(RepoError::UnsafePath(_))));
    }
}

#[test]
fn zip_slip_entries_are_rejected_at_open() {
    let data = make_zip(&[("../evil.txt", b"x")]);
    let r = ZipRepository::from_bytes(data, &Limits::default());
    assert!(matches!(r, Err(RepoError::UnsafePath(_))));
}

#[test]
fn limit_archive_bytes() {
    let data = make_zip(&fixtures());
    let limits = Limits {
        max_archive_bytes: 10,
        ..Limits::default()
    };
    assert!(matches!(
        ZipRepository::from_bytes(data, &limits),
        Err(RepoError::LimitExceeded(_))
    ));
}

#[test]
fn limit_entries() {
    let data = make_zip(&fixtures());
    let limits = Limits {
        max_entries: 2,
        ..Limits::default()
    };
    assert!(matches!(
        ZipRepository::from_bytes(data, &limits),
        Err(RepoError::LimitExceeded(_))
    ));
}

#[test]
fn limit_uncompressed_total() {
    let data = make_zip(&fixtures());
    let limits = Limits {
        max_uncompressed_bytes: 5,
        ..Limits::default()
    };
    assert!(matches!(
        ZipRepository::from_bytes(data, &limits),
        Err(RepoError::LimitExceeded(_))
    ));
}

#[test]
fn limit_ratio_on_large_entries() {
    let big = vec![b'a'; (RATIO_CHECK_MIN_BYTES as usize) + 1024];
    let data = make_zip(&[("big.txt", &big)]);
    let limits = Limits {
        max_ratio: 2,
        ..Limits::default()
    };
    assert!(matches!(
        ZipRepository::from_bytes(data, &limits),
        Err(RepoError::LimitExceeded(_))
    ));
    // Auch mit Standardgrenze (100:1) ist 1 MiB gleicher Bytes eine Zip-Bombe.
    assert!(matches!(
        ZipRepository::from_bytes(make_zip(&[("big.txt", &big)]), &Limits::default()),
        Err(RepoError::LimitExceeded(_))
    ));
}

#[test]
fn small_entries_skip_ratio_check() {
    let small = vec![b'a'; 64 * 1024];
    let limits = Limits {
        max_ratio: 2,
        ..Limits::default()
    };
    assert!(ZipRepository::from_bytes(make_zip(&[("s.txt", &small)]), &limits).is_ok());
}

#[test]
fn duplicate_entries_are_rejected_by_zip_crate() {
    // Der zip-Writer verweigert Duplikate; der Leser prueft sie ebenfalls.
    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default();
        w.start_file("a.txt", opts).unwrap();
        w.write_all(b"1").unwrap();
        assert!(w.start_file("a.txt", opts).is_err());
        w.finish().unwrap();
    }
}

#[test]
fn uri_parsing() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().to_string_lossy().to_string();
    let limits = Limits::default();
    assert!(open(&format!("file://{p}"), &limits).unwrap().exists(""));
    assert!(open(&p, &limits).unwrap().exists(""));
    assert!(matches!(open("", &limits), Err(RepoError::InvalidUri(_))));
    assert!(matches!(
        open("zip://", &limits),
        Err(RepoError::InvalidUri(_))
    ));
    assert!(matches!(
        open("zip:///nonexistent.zip", &limits),
        Err(RepoError::Io(_))
    ));
    assert!(is_file_uri("file:///x"));
    assert!(!is_file_uri("zip:///x.zip"));
}

#[test]
fn scoped_view_prefixes_paths() {
    let data = make_zip(&fixtures());
    let z: Arc<dyn FileRepository> =
        Arc::new(ZipRepository::from_bytes(data, &Limits::default()).unwrap());
    let sc = scoped(z, "scripts").unwrap();
    assert_eq!(
        sc.read("CustomerSet.rhai").unwrap(),
        Bytes::from_static(b"// x")
    );
    assert_eq!(
        sc.list("").unwrap(),
        vec!["CustomerSet.rhai".to_string(), "components".to_string()]
    );
    assert!(sc.exists("components"));
    assert!(matches!(
        sc.read("../ddic.json"),
        Err(RepoError::UnsafePath(_))
    ));
    assert!(file_path("zip:///x.zip").is_none());
    assert_eq!(file_path("file:///tmp/x"), Some(PathBuf::from("/tmp/x")));
}

#[test]
fn open_zip_from_uri() {
    let dir = tempfile::tempdir().unwrap();
    let zp = dir.path().join("model.zip");
    fs::write(&zp, make_zip(&fixtures())).unwrap();
    let repo = open(&format!("zip://{}", zp.display()), &Limits::default()).unwrap();
    assert!(repo.exists("ddic.json"));
}
