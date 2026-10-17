//! `mmk` — command-line companion to the engine.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mmk_formats::smm1;
use mmk_formats::smm2::{self, crypto};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "mmk", version, about = "Course tools for the engine")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse course files and print one JSON line per file: parsed fields and a round-trip check.
    /// Accepts SMM2 course data (0x5BFC0, optionally zlib-compressed), SMM2 .bcd (needs --table)
    /// and SMM1 .cdt files.
    Dump {
        files: Vec<PathBuf>,
        /// SMM2 course key table (from the user's own executable), needed for .bcd files.
        #[arg(long)]
        table: Option<PathBuf>,
        /// Only print the round-trip result, not the parsed fields.
        #[arg(long)]
        brief: bool,
    },
    /// Decode a BYML file (Yaz0/zstd wrappers are removed) and print it as JSON.
    Byml { file: PathBuf },
    /// List a SARC archive (Yaz0/zstd wrappers are removed): one JSON line per entry with name,
    /// size and CRC32 of the contents.
    Sarc { file: PathBuf },
    /// Import the user's own extracted SMM2 RomFS + ExeFS into an engine asset cache.
    Import {
        #[arg(long)]
        romfs: PathBuf,
        #[arg(long)]
        exefs: PathBuf,
        /// Cache root (default: ~/Library/Application Support/mmk/assets)
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn crc32(data: &[u8]) -> u32 {
    // small local CRC32 (IEEE) to avoid another dependency in the CLI
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn load(path: &PathBuf) -> std::io::Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    if data.len() > 2 && data[0] == 0x78 && matches!(data[1], 0x01 | 0x5e | 0x9c | 0xda) {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&data[..]).read_to_end(&mut out)?;
        return Ok(out);
    }
    Ok(data)
}

fn dump_one(path: &PathBuf, table: Option<&crypto::KeyTable>) -> Result<Value, String> {
    let data = load(path).map_err(|e| e.to_string())?;
    match data.len() {
        crypto::BCD_SIZE => {
            let table = table.ok_or(".bcd files need --table")?;
            let bcd = crypto::decrypt_bcd(&data, table, true).map_err(|e| e.to_string())?;
            let c = smm2::Course::parse(&bcd.body).map_err(|e| e.to_string())?;
            let body = c.to_bytes().map_err(|e| e.to_string())?;
            let again = crypto::reencrypt(&bcd, table, Some(&body)).map_err(|e| e.to_string())?;
            Ok(json!({ "roundtrip_ok": body == bcd.body && again == data, "course": c.to_json() }))
        }
        smm2::COURSE_SIZE => {
            let c = smm2::Course::parse(&data).map_err(|e| e.to_string())?;
            Ok(
                json!({ "roundtrip_ok": c.to_bytes().map_err(|e| e.to_string())? == data, "course": c.to_json() }),
            )
        }
        smm1::CDT_SIZE => {
            let c = smm1::Course::parse(&data).map_err(|e| e.to_string())?;
            Ok(json!({
                "roundtrip_ok": c.to_bytes().map_err(|e| e.to_string())? == data,
                "crc_ok": smm1::Course::stored_crc_ok(&data),
                "course": c.to_json(),
            }))
        }
        n => Err(format!("unrecognised size {n:#x}")),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Dump {
            files,
            table,
            brief,
        } => {
            let table = match table.map(|p| {
                std::fs::read(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|b| crypto::table_from_bytes(&b).map_err(|e| e.to_string()))
            }) {
                Some(Ok(t)) => Some(t),
                Some(Err(e)) => {
                    eprintln!("table: {e}");
                    return ExitCode::FAILURE;
                }
                None => None,
            };
            let mut out = std::io::stdout().lock();
            let mut failed = false;
            for f in &files {
                let mut line = match dump_one(f, table.as_ref()) {
                    Ok(v) => v,
                    Err(e) => {
                        failed = true;
                        json!({ "error": e })
                    }
                };
                if brief && let Some(o) = line.as_object_mut() {
                    o.remove("course");
                }
                line["file"] = Value::String(f.display().to_string());
                if line.get("roundtrip_ok") == Some(&Value::Bool(false)) {
                    failed = true;
                }
                writeln!(out, "{line}").ok();
            }
            if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Cmd::Byml { file } => {
            let res = std::fs::read(&file)
                .map_err(|e| e.to_string())
                .and_then(|d| mmk_formats::archive::unwrap(d).map_err(|e| e.to_string()))
                .and_then(|d| mmk_formats::byml::parse(&d).map_err(|e| e.to_string()));
            match res {
                Ok(v) => {
                    println!("{v}");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}: {e}", file.display());
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Sarc { file } => {
            let res = std::fs::read(&file)
                .map_err(|e| e.to_string())
                .and_then(|d| mmk_formats::archive::unwrap(d).map_err(|e| e.to_string()))
                .and_then(|d| mmk_formats::archive::Sarc::parse(d).map_err(|e| e.to_string()));
            match res {
                Ok(sarc) => {
                    for (name, data) in sarc.files() {
                        println!(
                            "{}",
                            json!({ "name": name, "size": data.len(), "crc32": crc32(data) })
                        );
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("{}: {e}", file.display());
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Import { romfs, exefs, out } => {
            let out_root = out.unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join("Library/Application Support/mmk/assets")
            });
            let cfg = mmk_import::ImportConfig {
                romfs,
                exefs,
                out_root,
            };
            let mut last = String::new();
            let mut progress = |stage: &str, done: usize, total: usize| {
                if stage != last || done == total {
                    eprintln!("{stage}: {done}/{total}");
                    last = stage.to_string();
                }
            };
            match mmk_import::run(&cfg, &mut progress) {
                Ok(r) => {
                    println!(
                        "imported SMM2 {} ({} tables, {} built-in courses verified) into {}",
                        r.build,
                        r.tables,
                        r.builtin_courses_verified,
                        r.out_dir.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("import failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
