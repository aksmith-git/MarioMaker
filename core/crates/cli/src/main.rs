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
    /// Export the textures inside a BFRES/BNTX file (wrappers removed) as PNG for inspection.
    /// RGBA8 only; block-compressed textures are listed but not written.
    Textures { file: PathBuf, out: PathBuf },
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
    /// Import the SMM1 UI (layouts, animations, textures, fonts, English text) from the user's
    /// own Wii U `content/` folders into a layout-player cache folder.
    ImportSmm1 {
        /// Base game content folder.
        base: PathBuf,
        /// Update content folder (overlaid on the base); pass "-" for none.
        update: PathBuf,
        /// Output folder, e.g. ~/smm-work/cache/smm1-v208
        out: PathBuf,
        /// Message language suffix.
        #[arg(long, default_value = "USen")]
        language: String,
    },
    /// Decode a BFLYT / BFLAN / BFFNT / MSBT file (Yaz0 removed) and print it as JSON.
    Lyt { file: PathBuf },
    /// Write a texture as PNG for inspection: an engine `.mtex` file, a Wii U `.bflim`, or one
    /// glyph sheet of a `.bffnt` (`--sheet`, optional GX2 `--swizzle` 0-7 to experiment).
    TexPng {
        file: PathBuf,
        out: PathBuf,
        #[arg(long, default_value_t = 0)]
        sheet: usize,
        #[arg(long)]
        swizzle: Option<u32>,
        /// GX2 tile mode for font sheets (default 4).
        #[arg(long, default_value_t = 4)]
        tile: u32,
        /// Flip the output vertically.
        #[arg(long)]
        flip_y: bool,
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

/// Minimal PNG writer (RGBA8, zlib via flate2) for inspection output.
fn write_png(path: &std::path::Path, w: usize, h: usize, rgba: &[u8]) -> std::io::Result<()> {
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let c = crc32(&out[start..]);
        out.extend_from_slice(&c.to_be_bytes());
    }
    let mut raw = Vec::with_capacity((w * 4 + 1) * h);
    for row in rgba.chunks_exact(w * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw)?;
    let idat = z.finish()?;
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &idat);
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png)
}

fn export_textures(file: &PathBuf, out: &std::path::Path) -> Result<(), String> {
    use mmk_formats::bntx;
    let data = std::fs::read(file).map_err(|e| e.to_string())?;
    let data = mmk_formats::archive::unwrap(data).map_err(|e| e.to_string())?;
    // A .zs model file is a SARC holding the BFRES; search every member for an embedded BNTX.
    let blobs: Vec<Vec<u8>> = match mmk_formats::archive::Sarc::parse(data.clone()) {
        Ok(s) => s.files().map(|(_, d)| d.to_vec()).collect(),
        Err(_) => vec![data],
    };
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    for blob in blobs {
        let Some(b) = bntx::find_embedded(&blob) else {
            continue;
        };
        for t in bntx::parse(b).map_err(|e| e.to_string())? {
            let info = json!({ "name": t.name, "w": t.width, "h": t.height, "mips": t.mip_count,
                "format": format!("{:?}", t.format), "variant": format!("{:?}", t.variant),
                "channels": t.channel_select });
            println!("{info}");
            if t.format == bntx::PixelFormat::Rgba8 {
                write_png(
                    &out.join(format!("{}.png", t.name)),
                    t.width,
                    t.height,
                    &t.data,
                )
                .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

fn tex_png(
    file: &PathBuf,
    out: &std::path::Path,
    sheet: usize,
    swizzle: Option<u32>,
    tile: u32,
    flip_y: bool,
) -> Result<(), String> {
    use mmk_formats::bflim::{Flim, FlimFormat, Image, PixelKind, decode_surface};
    let e = |x: mmk_formats::Error| x.to_string();
    let data = std::fs::read(file).map_err(|e| e.to_string())?;
    let data = mmk_formats::archive::unwrap(data).map_err(e)?;
    let img: Image = if data.starts_with(b"MTEX") {
        let m = mmk_assets::Mtex::parse(&data).map_err(|e| e.to_string())?;
        let kind = match m.format.name() {
            "rgba8" => PixelKind::Rgba8,
            "bc1" => PixelKind::Bc1,
            "bc2" => PixelKind::Bc2,
            "bc3" => PixelKind::Bc3,
            "bc4" => PixelKind::Bc4,
            "bc5" => PixelKind::Bc5,
            other => return Err(format!("{other} not supported")),
        };
        Image {
            width: m.width,
            height: m.height,
            kind,
            srgb: false,
            channel_select: m.channel_select,
            data: m.data,
        }
    } else if data.starts_with(b"FFNT") {
        let f = mmk_formats::bffnt::Font::parse(&data).map_err(e)?;
        match swizzle {
            Some(s) => {
                let fmt = f.sheet_flim_format().unwrap_or(FlimFormat::Bc4A);
                let raw = f.sheets.get(sheet).ok_or("no such sheet")?;
                let (w, h) = (f.sheet_width as u32, f.sheet_height as u32);
                decode_surface(raw, w, h, fmt, tile, s << 8).map_err(e)?
            }
            None => f.decode_sheet(sheet).map_err(e)?,
        }
    } else {
        Flim::parse(&data).and_then(|f| f.decode()).map_err(e)?
    };
    let mut rgba = match img.kind {
        PixelKind::Rgba8 => img.data.clone(),
        k => {
            let code = match k {
                PixelKind::Bc1 => 1,
                PixelKind::Bc2 => 2,
                PixelKind::Bc3 => 3,
                PixelKind::Bc4 => 4,
                _ => 5,
            };
            mmk_formats::bcn::decode(code, &img.data, img.width, img.height)
        }
    };
    // apply the channel mapping and composite over a grey checkerboard
    let w = img.width as usize;
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let src = *px;
        let pick = |c: u8| match c {
            0 => 0,
            1 => 255,
            2 => src[0],
            3 => src[1],
            4 => src[2],
            _ => src[3],
        };
        let m = img.channel_select.map(pick);
        let a = m[3] as u32;
        let bg = if ((i % w) / 8 + (i / w) / 8).is_multiple_of(2) {
            40
        } else {
            80
        };
        for k in 0..3 {
            px[k] = ((m[k] as u32 * a + bg * (255 - a)) / 255) as u8;
        }
        px[3] = 255;
    }
    if flip_y {
        let rows: Vec<&[u8]> = rgba.chunks_exact(w * 4).rev().collect();
        rgba = rows.concat();
    }
    write_png(out, w, img.height as usize, &rgba).map_err(|e| e.to_string())
}

fn dump_lyt(file: &PathBuf) -> Result<Value, String> {
    use mmk_formats::lyt;
    let data = std::fs::read(file).map_err(|e| e.to_string())?;
    let data = mmk_formats::archive::unwrap(data).map_err(|e| e.to_string())?;
    let e = |x: mmk_formats::Error| x.to_string();
    Ok(match data.get(..4) {
        Some(b"FLYT") => lyt::json::layout_to_json(&lyt::Layout::parse(&data).map_err(e)?),
        Some(b"FLAN") => lyt::json::animation_to_json(&lyt::Animation::parse(&data).map_err(e)?),
        Some(b"FFNT") => {
            mmk_import::smm1::font_to_json(&mmk_formats::bffnt::Font::parse(&data).map_err(e)?, 0)
        }
        Some(b"MsgS") => mmk_formats::msbt::Msbt::parse(&data).map_err(e)?.to_json(),
        _ => return Err("not a BFLYT, BFLAN, BFFNT or MSBT file".into()),
    })
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
        Cmd::Textures { file, out } => match export_textures(&file, &out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{}: {e}", file.display());
                ExitCode::FAILURE
            }
        },
        Cmd::ImportSmm1 {
            base,
            update,
            out,
            language,
        } => {
            let cfg = mmk_import::Smm1Config {
                base,
                update: (update.as_os_str() != "-").then_some(update),
                out,
                language,
            };
            let mut last = String::new();
            let mut progress = |stage: &str, done: usize, total: usize| {
                if stage != last || done == total {
                    eprintln!("{stage}: {done}/{total}");
                    last = stage.to_string();
                }
            };
            match mmk_import::run_smm1(&cfg, &mut progress) {
                Ok(r) => {
                    for s in &r.skipped {
                        eprintln!("skipped: {s}");
                    }
                    println!(
                        "imported SMM1 {} UI: {} layouts, {} animations, {} textures, {} fonts, {} message files ({} skipped) into {}",
                        r.build,
                        r.layouts,
                        r.animations,
                        r.textures,
                        r.fonts,
                        r.messages,
                        r.skipped.len(),
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
        Cmd::TexPng {
            file,
            out,
            sheet,
            swizzle,
            tile,
            flip_y,
        } => match tex_png(&file, &out, sheet, swizzle, tile, flip_y) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{}: {e}", file.display());
                ExitCode::FAILURE
            }
        },
        Cmd::Lyt { file } => match dump_lyt(&file) {
            Ok(v) => {
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{}: {e}", file.display());
                ExitCode::FAILURE
            }
        },
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
                        "imported SMM2 {} ({} tables, {} textures, {} built-in courses verified) into {}",
                        r.build,
                        r.tables,
                        r.textures,
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
