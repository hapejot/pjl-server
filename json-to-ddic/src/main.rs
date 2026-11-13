use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use json_to_ddic::{generate, xml::parse_xml};

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Auto,
    Json,
    Xml,
}

#[derive(Parser)]
#[command(about = "Erzeugt ein initiales ddic.json aus fachlichem Beispiel-JSON oder -XML")]
struct Args {
    /// JSON-/XML-Datei oder - fuer Standardeingabe (UTF-8).
    input: PathBuf,
    /// Eingabeformat; auto waehlt XML bei .xml-Dateien, sonst JSON.
    #[arg(long, value_enum, default_value = "auto")]
    format: Format,
    /// Zieldatei.
    #[arg(long, default_value = "ddic.json")]
    out: PathBuf,
    /// Name der Wurzelentitaet (ASCII-Bezeichner).
    #[arg(long, default_value = "Root")]
    entity: String,
    /// Paketname (ASCII-Bezeichner).
    #[arg(long, default_value = "imported")]
    package: String,
    /// Vorhandene Zieldatei ueberschreiben.
    #[arg(long)]
    force: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut input = String::new();
    if args.input.as_os_str() == "-" {
        io::stdin()
            .read_to_string(&mut input)
            .context("Standardeingabe lesen")?;
    } else {
        input = fs::read_to_string(&args.input)
            .with_context(|| format!("Eingabe lesen: {}", args.input.display()))?;
    }
    let input = input.trim_start_matches('\u{feff}');
    let format = match args.format {
        Format::Auto => {
            if args
                .input
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
            {
                Format::Xml
            } else {
                Format::Json
            }
        }
        format => format,
    };
    let value = match format {
        Format::Xml => parse_xml(input).context("Ungueltiges XML")?,
        Format::Json | Format::Auto => serde_json::from_str(input).context("Ungueltiges JSON")?,
    };
    let generated = generate(&value, &args.entity, &args.package)?;
    for warning in generated.warnings {
        eprintln!("Warnung: {warning}");
    }
    let mut bytes = serde_json::to_vec_pretty(&generated.ddic)?;
    bytes.push(b'\n');
    let mut options = OpenOptions::new();
    options.write(true);
    if args.force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut output = options
        .open(&args.out)
        .with_context(|| format!("Zieldatei oeffnen: {} (ggf. --force)", args.out.display()))?;
    output.write_all(&bytes).context("DDIC schreiben")?;
    output.sync_all().context("DDIC persistieren")?;
    eprintln!("DDIC geschrieben: {}", args.out.display());
    Ok(())
}
