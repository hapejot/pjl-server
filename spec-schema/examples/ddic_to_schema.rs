//! Example: load a `DataDictionary` from a JSON file, derive the low-level
//! `Schema`, and write the schema back out as JSON.
//!
//! Usage:
//!     cargo run -p spec-schema --example ddic_to_schema -- <ddic.json> <schema.json>
//!
//! If no arguments are given, a built-in default DDIC is used as input and the
//! output is written to `schema.json` in the current directory. This makes it
//! easy to generate a starter `ddic.json` from the built-in dictionary:
//!
//!     # dump the built-in DDIC to a file (via the same program):
//!     cargo run -p spec-schema --example ddic_to_schema -- --dump-default ddic.json

use std::fs::File;
use std::path::Path;
use std::process::ExitCode;

use spec::data_dictionary::DataDictionary;
use spec_schema::schema_from_ddic;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match args.as_slice() {
        [flag, out] if flag == "--dump-default" => match dump_default(out) {
            Ok(()) => {
                eprintln!("wrote built-in DDIC to {out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::FAILURE
            }
        },
        [ddic_in, schema_out] => match run(ddic_in, schema_out) {
            Ok(()) => {
                eprintln!("wrote schema to {schema_out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!(
                "usage:\n  \
                 ddic_to_schema <ddic.json> <schema.json>\n  \
                 ddic_to_schema --dump-default <ddic.json>"
            );
            ExitCode::FAILURE
        }
    }
}

fn run(ddic_path: &str, schema_path: &str) -> anyhow::Result<()> {
    // 1. Load DDIC from JSON.
    let file = File::open(Path::new(ddic_path))?;
    let mut ddic: DataDictionary = serde_json::from_reader(file)?;

    // 2. Activate lookup fields (required before schema derivation).
    ddic.activate();

    // 3. Derive the low-level Schema.
    let schema = schema_from_ddic(&ddic)?;

    // 4. Persist Schema as pretty JSON.
    let out = File::create(Path::new(schema_path))?;
    serde_json::to_writer_pretty(out, &schema)?;
    Ok(())
}

fn dump_default(ddic_path: &str) -> anyhow::Result<()> {
    let ddic = DataDictionary::new();
    let out = File::create(Path::new(ddic_path))?;
    serde_json::to_writer_pretty(out, &ddic)?;
    Ok(())
}
