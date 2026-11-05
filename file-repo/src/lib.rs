//! Lesender Zugriff auf Dateien ueber eine gemeinsame Abstraktion.
//!
//! Quellen werden per URI angegeben:
//! - `file://<pfad>` oder ein Pfad ohne Schema: Verzeichnis auf dem Dateisystem.
//! - `zip://<pfad>`: ZIP-Datei, die komplett in den Speicher geladen und
//!   vor dem Start geprueft wird.
//!
//! Alle Pfade im Repository sind relativ zur Wurzel und mit `/` getrennt.
//! Absolute Pfade und `..` werden abgelehnt.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;

/// Fehler beim Oeffnen oder Lesen eines Repositories.
#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("ungueltige Quelle '{0}': erwartet file://<pfad>, zip://<pfad> oder einen Pfad")]
    InvalidUri(String),
    #[error("unsicherer Pfad '{0}'")]
    UnsafePath(String),
    #[error("Limit verletzt: {0}")]
    LimitExceeded(String),
    #[error("ZIP-Datei fehlerhaft: {0}")]
    Zip(String),
    #[error("E/A-Fehler: {0}")]
    Io(#[from] std::io::Error),
}

/// Obergrenzen fuer ZIP-Quellen. Werden beim Oeffnen und beim Packen geprueft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Maximale Groesse der ZIP-Datei in Bytes.
    pub max_archive_bytes: u64,
    /// Maximale Summe aller unkomprimierten Eintraege in Bytes.
    pub max_uncompressed_bytes: u64,
    /// Maximale Anzahl Eintraege.
    pub max_entries: usize,
    /// Maximales Verhaeltnis unkomprimiert/komprimiert. Gilt fuer Eintraege
    /// ab `RATIO_CHECK_MIN_BYTES`, damit kleine Dateien nicht falsch auffallen.
    pub max_ratio: u64,
}

/// Ab dieser unkomprimierten Groesse greift die Verhaeltnispruefung.
pub const RATIO_CHECK_MIN_BYTES: u64 = 1024 * 1024;

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 512 * 1024 * 1024,
            max_uncompressed_bytes: 1024 * 1024 * 1024,
            max_entries: 10_000,
            max_ratio: 100,
        }
    }
}

/// Lesender Dateizugriff. Implementierungen muessen threadsicher sein.
pub trait FileRepository: Send + Sync {
    /// Inhalt einer Datei. `Ok(None)`, wenn es die Datei nicht gibt.
    fn read(&self, path: &str) -> Result<Option<Bytes>, RepoError>;

    /// Namen der direkten Kinder eines Verzeichnisses (Dateien und Unterverzeichnisse),
    /// sortiert. Leer, wenn das Verzeichnis nicht existiert.
    fn list(&self, dir: &str) -> Result<Vec<String>, RepoError>;

    /// Existiert eine Datei oder ein Verzeichnis unter diesem Pfad?
    fn exists(&self, path: &str) -> bool;
}

/// Prueft und normalisiert einen relativen Pfad. Ergebnis hat keine leeren,
/// `.`- oder `..`-Segmente und verwendet `/`.
pub fn normalize(path: &str) -> Result<String, RepoError> {
    if path.contains('\0') || path.contains('\\') || path.contains(':') {
        return Err(RepoError::UnsafePath(path.to_string()));
    }
    if path.starts_with('/') {
        return Err(RepoError::UnsafePath(path.to_string()));
    }
    let mut parts = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => return Err(RepoError::UnsafePath(path.to_string())),
            s => parts.push(s),
        }
    }
    Ok(parts.join("/"))
}

/// Verzeichnis auf dem Dateisystem (`file://`).
#[derive(Debug, Clone)]
pub struct DirRepository {
    root: PathBuf,
}

impl DirRepository {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn full(&self, path: &str) -> Result<PathBuf, RepoError> {
        let rel = normalize(path)?;
        let mut p = self.root.clone();
        for c in Path::new(&rel).components() {
            if let Component::Normal(s) = c {
                p.push(s);
            }
        }
        Ok(p)
    }
}

impl FileRepository for DirRepository {
    fn read(&self, path: &str) -> Result<Option<Bytes>, RepoError> {
        let full = self.full(path)?;
        if !full.is_file() {
            return Ok(None);
        }
        Ok(Some(Bytes::from(fs::read(full)?)))
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, RepoError> {
        let full = self.full(dir)?;
        if !full.is_dir() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        for entry in fs::read_dir(full)? {
            names.push(entry?.file_name().to_string_lossy().into_owned());
        }
        names.sort();
        Ok(names)
    }

    fn exists(&self, path: &str) -> bool {
        self.full(path).map(|p| p.exists()).unwrap_or(false)
    }
}

/// ZIP-Archiv, vollstaendig im Speicher. Alle Dateien sind beim Oeffnen
/// bereits dekomprimiert und geprueft.
#[derive(Debug, Clone)]
pub struct ZipRepository {
    files: BTreeMap<String, Bytes>,
    dirs: BTreeSet<String>,
}

impl ZipRepository {
    /// Oeffnet ein ZIP aus dem Speicher und prueft alle Limits.
    pub fn from_bytes(data: Bytes, limits: &Limits) -> Result<Self, RepoError> {
        if data.len() as u64 > limits.max_archive_bytes {
            return Err(RepoError::LimitExceeded(format!(
                "ZIP ist {} Bytes gross, erlaubt sind {}",
                data.len(),
                limits.max_archive_bytes
            )));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(data))
            .map_err(|e| RepoError::Zip(e.to_string()))?;
        if archive.len() > limits.max_entries {
            return Err(RepoError::LimitExceeded(format!(
                "{} Eintraege, erlaubt sind {}",
                archive.len(),
                limits.max_entries
            )));
        }

        // Erst Metadaten pruefen (Summe, Verhaeltnis, Pfade), dann dekomprimieren.
        let mut total: u64 = 0;
        for i in 0..archive.len() {
            let entry = archive
                .by_index(i)
                .map_err(|e| RepoError::Zip(e.to_string()))?;
            let size = entry.size();
            let compressed = entry.compressed_size();
            total = total.saturating_add(size);
            if total > limits.max_uncompressed_bytes {
                return Err(RepoError::LimitExceeded(format!(
                    "unkomprimierte Groesse ueberschreitet {} Bytes",
                    limits.max_uncompressed_bytes
                )));
            }
            if size >= RATIO_CHECK_MIN_BYTES
                && (compressed == 0 || size / compressed.max(1) > limits.max_ratio)
            {
                return Err(RepoError::LimitExceeded(format!(
                    "Kompressionsverhaeltnis von '{}' ueber {}:1",
                    entry.name(),
                    limits.max_ratio
                )));
            }
        }

        let mut files = BTreeMap::new();
        let mut dirs = BTreeSet::new();
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| RepoError::Zip(e.to_string()))?;
            let raw_name = entry.name().to_string();
            let is_dir = entry.is_dir() || raw_name.ends_with('/');
            let name = normalize(&raw_name)?;
            if name.is_empty() {
                continue;
            }
            // Verzeichnisse aus allen Segmenten ableiten.
            let segs: Vec<&str> = name.split('/').collect();
            let upto = if is_dir { segs.len() } else { segs.len() - 1 };
            for n in 1..=upto {
                dirs.insert(segs[..n].join("/"));
            }
            if is_dir {
                continue;
            }
            if files.contains_key(&name) {
                return Err(RepoError::Zip(format!("doppelter Eintrag '{name}'")));
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry
                .read_to_end(&mut buf)
                .map_err(|e| RepoError::Zip(e.to_string()))?;
            files.insert(name, Bytes::from(buf));
        }
        Ok(Self { files, dirs })
    }
}

impl FileRepository for ZipRepository {
    fn read(&self, path: &str) -> Result<Option<Bytes>, RepoError> {
        let name = normalize(path)?;
        Ok(self.files.get(&name).cloned())
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, RepoError> {
        let prefix = normalize(dir)?;
        let prefix = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let mut names = BTreeSet::new();
        for key in self.files.keys().chain(self.dirs.iter()) {
            if let Some(rest) = key.strip_prefix(&prefix) {
                if let Some(first) = rest.split('/').next() {
                    if !first.is_empty() {
                        names.insert(first.to_string());
                    }
                }
            }
        }
        Ok(names.into_iter().collect())
    }

    fn exists(&self, path: &str) -> bool {
        match normalize(path) {
            Ok(n) if n.is_empty() => true,
            Ok(n) => self.files.contains_key(&n) || self.dirs.contains(&n),
            Err(_) => false,
        }
    }
}

/// Oeffnet eine Quelle aus einer URI-Angabe (`file://...`, `zip://...` oder Pfad).
pub fn open(uri: &str, limits: &Limits) -> Result<Box<dyn FileRepository>, RepoError> {
    if uri.is_empty() {
        return Err(RepoError::InvalidUri(uri.to_string()));
    }
    if let Some(rest) = uri.strip_prefix("zip://") {
        if rest.is_empty() {
            return Err(RepoError::InvalidUri(uri.to_string()));
        }
        let meta = fs::metadata(rest)?;
        if meta.len() > limits.max_archive_bytes {
            return Err(RepoError::LimitExceeded(format!(
                "ZIP '{rest}' ist {} Bytes gross, erlaubt sind {}",
                meta.len(),
                limits.max_archive_bytes
            )));
        }
        let data = Bytes::from(fs::read(rest)?);
        return Ok(Box::new(ZipRepository::from_bytes(data, limits)?));
    }
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    if path.is_empty() {
        return Err(RepoError::InvalidUri(uri.to_string()));
    }
    Ok(Box::new(DirRepository::new(path)))
}

/// Sicht auf ein Unterverzeichnis einer Quelle (z. B. `scripts/` im Model-ZIP).
pub struct Scoped {
    inner: Arc<dyn FileRepository>,
    prefix: String,
}

/// Begrenzt `inner` auf das Unterverzeichnis `prefix`.
pub fn scoped(inner: Arc<dyn FileRepository>, prefix: &str) -> Result<Arc<dyn FileRepository>, RepoError> {
    Ok(Arc::new(Scoped {
        inner,
        prefix: normalize(prefix)?,
    }))
}

impl Scoped {
    fn join(&self, path: &str) -> Result<String, RepoError> {
        let rel = normalize(path)?;
        Ok(match (self.prefix.is_empty(), rel.is_empty()) {
            (true, _) => rel,
            (false, true) => self.prefix.clone(),
            (false, false) => format!("{}/{}", self.prefix, rel),
        })
    }
}

impl FileRepository for Scoped {
    fn read(&self, path: &str) -> Result<Option<Bytes>, RepoError> {
        self.inner.read(&self.join(path)?)
    }

    fn list(&self, dir: &str) -> Result<Vec<String>, RepoError> {
        self.inner.list(&self.join(dir)?)
    }

    fn exists(&self, path: &str) -> bool {
        self.join(path).map(|p| self.inner.exists(&p)).unwrap_or(false)
    }
}

/// Dateisystempfad einer `file://`-Angabe (oder eines Pfads ohne Schema). `None` bei `zip://`.
pub fn file_path(uri: &str) -> Option<PathBuf> {
    if uri.starts_with("zip://") {
        return None;
    }
    let path = uri.strip_prefix("file://").unwrap_or(uri);
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Liegt die Quelle auf dem Dateisystem? Noetig fuer Hot-Reload und Designer.
pub fn is_file_uri(uri: &str) -> bool {
    !uri.starts_with("zip://")
}

#[cfg(test)]
mod tests {
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
    fn both() -> (tempfile::TempDir, Box<dyn FileRepository>, Box<dyn FileRepository>) {
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
            assert_eq!(repo.read("ddic.json").unwrap().unwrap(), Bytes::from_static(b"{}"));
            assert!(repo.read("missing.json").unwrap().is_none());
            assert_eq!(
                repo.read("./scripts//CustomerSet.rhai").unwrap().unwrap(),
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
            assert!(matches!(repo.read("../etc/passwd"), Err(RepoError::UnsafePath(_))));
            assert!(matches!(repo.read("/etc/passwd"), Err(RepoError::UnsafePath(_))));
            assert!(matches!(repo.read("a\\..\\b"), Err(RepoError::UnsafePath(_))));
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
        let limits = Limits { max_archive_bytes: 10, ..Limits::default() };
        assert!(matches!(ZipRepository::from_bytes(data, &limits), Err(RepoError::LimitExceeded(_))));
    }

    #[test]
    fn limit_entries() {
        let data = make_zip(&fixtures());
        let limits = Limits { max_entries: 2, ..Limits::default() };
        assert!(matches!(ZipRepository::from_bytes(data, &limits), Err(RepoError::LimitExceeded(_))));
    }

    #[test]
    fn limit_uncompressed_total() {
        let data = make_zip(&fixtures());
        let limits = Limits { max_uncompressed_bytes: 5, ..Limits::default() };
        assert!(matches!(ZipRepository::from_bytes(data, &limits), Err(RepoError::LimitExceeded(_))));
    }

    #[test]
    fn limit_ratio_on_large_entries() {
        let big = vec![b'a'; (RATIO_CHECK_MIN_BYTES as usize) + 1024];
        let data = make_zip(&[("big.txt", &big)]);
        let limits = Limits { max_ratio: 2, ..Limits::default() };
        assert!(matches!(ZipRepository::from_bytes(data, &limits), Err(RepoError::LimitExceeded(_))));
        // Auch mit Standardgrenze (100:1) ist 1 MiB gleicher Bytes eine Zip-Bombe.
        assert!(matches!(
            ZipRepository::from_bytes(make_zip(&[("big.txt", &big)]), &Limits::default()),
            Err(RepoError::LimitExceeded(_))
        ));
    }

    #[test]
    fn small_entries_skip_ratio_check() {
        let small = vec![b'a'; 64 * 1024];
        let limits = Limits { max_ratio: 2, ..Limits::default() };
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
        assert!(matches!(open("zip://", &limits), Err(RepoError::InvalidUri(_))));
        assert!(matches!(open("zip:///nonexistent.zip", &limits), Err(RepoError::Io(_))));
        assert!(is_file_uri("file:///x"));
        assert!(!is_file_uri("zip:///x.zip"));
    }

    #[test]
    fn scoped_view_prefixes_paths() {
        let data = make_zip(&fixtures());
        let z: Arc<dyn FileRepository> = Arc::new(ZipRepository::from_bytes(data, &Limits::default()).unwrap());
        let sc = scoped(z, "scripts").unwrap();
        assert_eq!(sc.read("CustomerSet.rhai").unwrap().unwrap(), Bytes::from_static(b"// x"));
        assert_eq!(sc.list("").unwrap(), vec!["CustomerSet.rhai".to_string(), "components".to_string()]);
        assert!(sc.exists("components"));
        assert!(matches!(sc.read("../ddic.json"), Err(RepoError::UnsafePath(_))));
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
}
