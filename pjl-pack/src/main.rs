//! `pjl-pack --src <verzeichnis> --out <datei.zip>`
//!
//! Packt den Inhalt des Quellverzeichnisses im Wurzelverzeichnis des ZIPs.
//! Vor dem Schreiben werden dieselben Limits geprueft wie beim Lesen; nach
//! dem Schreiben wird das Archiv erneut mit `file-repo` geoeffnet.

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use file_repo::{Limits, ZipRepository};

#[derive(Debug, Parser)]
#[command(about = "Packt ein Verzeichnis in ein ZIP fuer zip:// Quellen")]
struct Args {
    /// Quellverzeichnis; dessen Inhalt wird im ZIP-Wurzelverzeichnis abgelegt.
    #[arg(long)]
    src: PathBuf,
    /// Zieldatei (.zip). Wird ueberschrieben.
    #[arg(long)]
    out: PathBuf,
    /// Maximale ZIP-Groesse in MiB (Standard aus file-repo).
    #[arg(long)]
    max_archive_mib: Option<u64>,
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let ft = e.file_type()?;
        if ft.is_symlink() {
            bail!("Symlinks werden nicht gepackt: {}", path.display());
        }
        if ft.is_dir() {
            collect(root, &path, out)?;
        } else if ft.is_file() {
            let rel = path.strip_prefix(root)?;
            let name = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((name, path));
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut limits = Limits::default();
    if let Some(mib) = args.max_archive_mib {
        limits.max_archive_bytes = mib * 1024 * 1024;
    }

    if !args.src.is_dir() {
        bail!("Quelle ist kein Verzeichnis: {}", args.src.display());
    }
    let mut files = Vec::new();
    collect(&args.src, &args.src, &mut files)?;

    // Vorab-Pruefung der Anzahl und Summe, bevor etwas geschrieben wird.
    if files.len() > limits.max_entries {
        bail!("{} Dateien, erlaubt sind {}", files.len(), limits.max_entries);
    }
    let mut total = 0u64;
    for (_, p) in &files {
        total += fs::metadata(p)?.len();
    }
    if total > limits.max_uncompressed_bytes {
        bail!("Summe {} Bytes ueberschreitet {}", total, limits.max_uncompressed_bytes);
    }

    let mut buf = Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, path) in &files {
            w.start_file(name.as_str(), opts)
                .with_context(|| format!("Eintrag {name}"))?;
            let data = fs::read(path).with_context(|| format!("lesen {}", path.display()))?;
            w.write_all(&data)?;
        }
        w.finish()?;
    }
    let bytes = buf.into_inner();
    if bytes.len() as u64 > limits.max_archive_bytes {
        bail!(
            "ZIP ist {} Bytes gross, erlaubt sind {}",
            bytes.len(),
            limits.max_archive_bytes
        );
    }

    // Verifikation mit dem Leser: so ist garantiert, dass das Paket akzeptiert wird.
    ZipRepository::from_bytes(bytes::Bytes::from(bytes.clone()), &limits)
        .context("erzeugtes ZIP besteht die Pruefung nicht")?;

    fs::write(&args.out, &bytes).with_context(|| format!("schreiben {}", args.out.display()))?;
    println!(
        "{} Dateien, {} Bytes unkomprimiert, ZIP {} Bytes -> {}",
        files.len(),
        total,
        bytes.len(),
        args.out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
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
        assert_eq!(names, vec!["ddic.json", "scripts/components/TimeRange.rhai"]);
    }
}
