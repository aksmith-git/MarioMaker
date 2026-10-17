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
    /// List the sounds in a BARS, a BWAV, or a SARC pack of BARS (wrappers removed): one JSON line
    /// per sound. With --name and --wav, decode that sound to a 16-bit WAV for inspection.
    Audio {
        file: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        wav: Option<PathBuf>,
    },
    /// Report which SMM2 courses the engine can fully play and which missing features block the
    /// rest (zlib or raw course data). Prints a summary, or one JSON line per course with --each.
    Coverage {
        files: Vec<PathBuf>,
        #[arg(long)]
        each: bool,
        /// Course key table for encrypted .bcd files (an asset cache's keys/course_table.bin).
        #[arg(long)]
        keys: Option<PathBuf>,
    },
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
    /// Run the open-source course server (courses + index in DIR).
    Server {
        #[arg(long, default_value = "mmk-server-data")]
        dir: PathBuf,
        #[arg(long, default_value = "0.0.0.0:8640")]
        addr: String,
    },
    /// Upload a course (raw, zlib or .bcd needs decrypting first) to a course server.
    Upload {
        file: PathBuf,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
        #[arg(long, default_value = "anonymous")]
        maker: String,
    },
    /// Search a course server.
    Search {
        #[arg(default_value = "")]
        query: String,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
        #[arg(long, default_value = "new")]
        sort: String,
    },
    /// Download a course from a course server by code.
    Download {
        code: String,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
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

/// Minimal WAV writer (PCM16, interleaved) for inspection output.
fn write_wav(path: &std::path::Path, channels: u16, rate: u32, pcm: &[i16]) -> std::io::Result<()> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, out)
}

fn audio_cmd(
    file: &PathBuf,
    name: Option<&str>,
    wav: Option<&std::path::Path>,
) -> Result<(), String> {
    use mmk_formats::audio::{self, Bars, Bwav};
    let data = std::fs::read(file).map_err(|e| e.to_string())?;
    let data = mmk_formats::archive::unwrap(data).map_err(|e| e.to_string())?;
    let mut blobs: Vec<(String, Vec<u8>)> = Vec::new();
    if data.starts_with(b"SARC") {
        let sarc = mmk_formats::archive::Sarc::parse(data).map_err(|e| e.to_string())?;
        for (n, d) in sarc.files() {
            blobs.push((n.to_string(), d.to_vec()));
        }
    } else {
        blobs.push((file.display().to_string(), data));
    }
    let describe = |container: &str, sound: &str, w: &Bwav| {
        let c = &w.channels[0];
        json!({ "container": container, "name": sound, "channels": w.channels.len(),
            "codec": format!("{:?}", c.codec), "rate": c.sample_rate, "samples": c.samples,
            "samples_here": c.samples_here, "loop": w.loop_range(), "prefetch": w.prefetch })
    };
    let mut found = false;
    for (container, blob) in &blobs {
        let mut sounds: Vec<(String, Bwav)> = Vec::new();
        if blob.starts_with(b"BARS") {
            let bars = Bars::parse(blob).map_err(|e| format!("{container}: {e}"))?;
            for e in &bars.entries {
                if let Some(w) = bars.bwav(e) {
                    sounds.push((e.name.clone(), w));
                }
            }
        } else if blob.starts_with(b"BWAV") {
            let stem = std::path::Path::new(container)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            sounds.push((stem, Bwav::parse(blob).map_err(|e| e.to_string())?));
        }
        for (sound, w) in sounds {
            if name.is_some_and(|n| n != sound) {
                continue;
            }
            found = true;
            println!("{}", describe(container, &sound, &w));
            if let Some(path) = wav {
                let (ch, _, pcm) = audio::decode_interleaved(&w, 2).map_err(|e| e.to_string())?;
                write_wav(path, ch as u16, w.sample_rate(), &pcm).map_err(|e| e.to_string())?;
                return Ok(());
            }
        }
    }
    if !found && name.is_some() {
        return Err("sound not found".into());
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
        Cmd::Coverage { files, each, keys } => {
            let table: Option<mmk_formats::smm2::KeyTable> = keys.and_then(|p| {
                let b = std::fs::read(p).ok()?;
                let words: Vec<u32> = b
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| u32::from_le_bytes(*c))
                    .collect();
                words.try_into().ok()
            });
            let mut blockers: std::collections::BTreeMap<String, usize> = Default::default();
            let (mut total, mut ok) = (0usize, 0usize);
            let mut by_style: std::collections::BTreeMap<String, (usize, usize)> =
                Default::default();
            for f in &files {
                let Ok(raw) = std::fs::read(f) else { continue };
                let data = if raw.len() == mmk_formats::smm2::COURSE_SIZE {
                    raw
                } else if raw.len() == mmk_formats::smm2::BCD_SIZE {
                    let Some(t) = &table else { continue };
                    let Ok(b) = mmk_formats::smm2::crypto::decrypt_bcd(&raw, t, false) else {
                        continue;
                    };
                    b.body
                } else {
                    let mut out = Vec::new();
                    if flate2::read::ZlibDecoder::new(&raw[..])
                        .read_to_end(&mut out)
                        .is_err()
                    {
                        continue;
                    }
                    out
                };
                let Ok(c) = mmk_formats::smm2::Course::parse(&data) else {
                    continue;
                };
                let style = c.header.game_style.as_str_lossy();
                let missing = mmk_sim::level::unsupported_features(&c);
                total += 1;
                let e = by_style.entry(style.clone()).or_default();
                e.0 += 1;
                if missing.is_empty() {
                    ok += 1;
                    e.1 += 1;
                }
                if each {
                    println!(
                        "{}",
                        serde_json::json!({"file": f.display().to_string(), "style": style, "missing": missing})
                    );
                }
                // a course counts toward a blocker only when it's the only thing missing … and
                // toward every blocker otherwise (weighted view below)
                for m in &missing {
                    *blockers.entry(m.clone()).or_default() += 1;
                }
            }
            println!("{ok} of {total} courses fully playable");
            for (s, (n, k)) in &by_style {
                println!("  {s}: {k}/{n}");
            }
            let mut v: Vec<_> = blockers.into_iter().collect();
            v.sort_by_key(|b| std::cmp::Reverse(b.1));
            println!("most common blockers (courses affected):");
            for (m, n) in v.iter().take(40) {
                println!("  {n:5}  {m}");
            }
            ExitCode::SUCCESS
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
        Cmd::Audio { file, name, wav } => match audio_cmd(&file, name.as_deref(), wav.as_deref()) {
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

        Cmd::Server { dir, addr } => {
            let store = match mmk_server::Store::open(&dir) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("{}: {e:?}", dir.display());
                    return ExitCode::FAILURE;
                }
            };
            eprintln!("mmk-server: {} on http://{addr}", dir.display());
            match mmk_server::serve(store, &addr) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("server: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Upload {
            file,
            server,
            maker,
        } => {
            let data = match std::fs::read(&file) {
                Ok(raw) if raw.len() == mmk_formats::smm2::COURSE_SIZE => raw,
                Ok(raw) => {
                    let mut out = Vec::new();
                    if flate2::read::ZlibDecoder::new(&raw[..])
                        .read_to_end(&mut out)
                        .is_err()
                    {
                        eprintln!("{}: not raw or zlib course data", file.display());
                        return ExitCode::FAILURE;
                    }
                    out
                }
                Err(e) => {
                    eprintln!("{}: {e}", file.display());
                    return ExitCode::FAILURE;
                }
            };
            match mmk_online::Client::new(server).upload(&data, &maker) {
                Ok(l) => {
                    println!("{} {}", l.code, l.name);
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("upload: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Search {
            query,
            server,
            sort,
        } => match mmk_online::Client::new(server).search(&query, None, &sort, 0, 50) {
            Ok(list) => {
                for l in list {
                    println!(
                        "{}  {:<3} {:>3}%  {:>5} plays  {}  — {}",
                        l.code, l.style, l.difficulty, l.plays, l.name, l.maker
                    );
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("search: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Download { code, server, out } => {
            match mmk_online::Client::new(server).download(&code) {
                Ok(b) => {
                    let path = out.unwrap_or_else(|| PathBuf::from(format!("{code}.bin")));
                    match std::fs::write(&path, b) {
                        Ok(()) => {
                            println!("{}", path.display());
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            eprintln!("{}: {e}", path.display());
                            ExitCode::FAILURE
                        }
                    }
                }
                Err(e) => {
                    eprintln!("download: {e}");
                    ExitCode::FAILURE
                }
            }
        }
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
                        "imported SMM2 {} ({} tables, {} textures, {} sounds, {} sprite quads, {} built-in courses verified) into {}",
                        r.build,
                        r.tables,
                        r.textures,
                        r.sounds,
                        r.sprites,
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
