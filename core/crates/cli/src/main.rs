//! `mmk` — command-line companion to the engine.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use mmk_formats::smm1;
use mmk_formats::smm2::{self, crypto};
use serde_json::{Value, json};

mod refcourses;
mod trace;

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
    /// Write the renderer reference courses (one per style × theme × night, the same first
    /// screen each) as raw course files; with --romfs, also the last one selected as replacement
    /// built-in title courses for an emulator mod folder (needs --table).
    RefCourses {
        out: PathBuf,
        /// Course key table (an asset cache's keys/course_table.bin).
        #[arg(long)]
        table: Option<PathBuf>,
        /// Only courses whose name equals or contains this.
        #[arg(long)]
        only: Option<String>,
        /// Mod RomFS folder to write `Course/title_course_data_NN.szs` into.
        #[arg(long)]
        romfs: Option<PathBuf>,
        /// Wrap this course file (raw, zlib or .bcd) for --romfs instead of a reference course.
        #[arg(long)]
        course: Option<PathBuf>,
    },
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
        /// The base game's extracted RomFS (or one with the update already applied).
        #[arg(long)]
        romfs: PathBuf,
        /// The base game's extracted ExeFS.
        #[arg(long)]
        exefs: PathBuf,
        /// A separately extracted update's RomFS, laid over --romfs (update files win).
        #[arg(long)]
        update_romfs: Option<PathBuf>,
        /// The update's ExeFS (used instead of --exefs).
        #[arg(long)]
        update_exefs: Option<PathBuf>,
        /// Cache root (default: the SMCraft data folder's `assets`: %LOCALAPPDATA%\SMCraft on
        /// Windows, ~/Library/Application Support/SMCraft on macOS, ~/.local/share/smcraft on
        /// Linux; $MMK_HOME overrides)
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
    /// Import the SMM2 UI (layouts, animations, textures, fonts, text) from the user's own
    /// extracted RomFS into a layout-player cache folder (same form as `import-smm1`).
    ImportSmm2Ui {
        /// Extracted RomFS (update applied).
        romfs: PathBuf,
        /// Output folder, e.g. ~/smm-work/cache/smm2-ui-3.0.3
        out: PathBuf,
        /// Extracted ExeFS, to name the build in the manifest.
        #[arg(long)]
        exefs: Option<PathBuf>,
        /// Message language suffix.
        #[arg(long, default_value = "USen")]
        language: String,
    },
    /// Import only the gameplay sound database and its waves (`audio/sound.json`, `audio/se/`,
    /// `audio/music/`) from the user's own extracted RomFS into an asset cache folder (the
    /// build folder, e.g. ~/smm-work/cache/smm2-3.0.3). `mmk import` runs this stage too.
    ImportSound {
        /// Extracted RomFS (update applied).
        romfs: PathBuf,
        /// Cache folder of one build.
        out: PathBuf,
    },
    /// Import only the Story Mode / Dojo data (event flows, Story tables, hub map, lesson list)
    /// into an existing SMM2 asset cache folder (the `modes` stage of `import`).
    ImportModes {
        /// Extracted RomFS (update applied).
        romfs: PathBuf,
        /// The cache folder, e.g. ~/smm-work/cache/smm2-3.0.3
        out: PathBuf,
    },
    /// Run only the effects stage of `import` (particle effects and their links, from the
    /// user's own extracted RomFS) into an existing asset cache folder (`<root>/smm2-<build>`).
    ImportEffects {
        /// Extracted RomFS (update applied).
        romfs: PathBuf,
        /// The asset cache folder to add the effects to.
        cache: PathBuf,
    },
    /// Write effect test courses (`fx_<style>.course`, raw course data): flat ground with
    /// Goombas, a Koopa, coins ahead of the start, for comparing the engine's
    /// effects with the game's on the same scripted run.
    FxCourses {
        #[arg(short, long, default_value = ".")]
        out: PathBuf,
    },
    /// Decode a BFLYT / BFLAN / BFFNT / MSBT file (Yaz0 removed) and print it as JSON.
    Lyt { file: PathBuf },
    /// Smoke-run courses headlessly with a scripted run-and-jump input: reports crashes, how far
    /// right the player got (of the goal distance), deaths and clears. One JSON line per course
    /// with --each.
    /// Write test courses laying out every supported part in a row (pages of ≈ 220 tiles) for a
    /// style, for visual checks: `<out>/gallery_<style>[_alt]_<page>.bin` (raw course data).
    Gallery {
        #[arg(long, default_value = "M1")]
        style: String,
        /// Place the alternate (0x4) forms instead.
        #[arg(long)]
        alt: bool,
        #[arg(long, default_value = "0")]
        theme: u8,
        #[arg(long)]
        night: bool,
        #[arg(short, long, default_value = ".")]
        out: PathBuf,
    },
    /// List the sprites of courses whose art the asset cache lacks (counting 3D meshes for
    /// NSMBU / 3D World), walking each area view by view with every actor spawned.
    MissingArt {
        files: Vec<PathBuf>,
        #[arg(long)]
        assets: PathBuf,
        /// Course key table for encrypted .bcd files (an asset cache's keys/course_table.bin).
        #[arg(long)]
        keys: Option<PathBuf>,
    },
    /// Convert SMM2 course data IDs to course codes (XXX-XXX-XXX) and back.
    Code { ids: Vec<String> },
    /// Run a per-frame input script (see `trace.rs`) through the simulation on a course and write
    /// the player's state after every frame as CSV (for frame-by-frame comparison with a trace
    /// recorded in the original game).
    Trace {
        course: PathBuf,
        inputs: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Course key table for an encrypted .bcd course.
        #[arg(long)]
        keys: Option<PathBuf>,
        /// Start position "x,y" (px, feet centre) instead of the course start.
        #[arg(long)]
        start: Option<String>,
        /// Start facing: 1 right, -1 left.
        #[arg(long, allow_hyphen_values = true)]
        facing: Option<i32>,
        /// Start form: small, super, fire, raccoon, cape, propeller, cat, builder, superball.
        #[arg(long)]
        form: Option<String>,
        /// Start speed (px/frame, signed).
        #[arg(long, allow_hyphen_values = true)]
        vx: Option<f32>,
    },
    /// Write the physics test courses (`<style>_<kind>.course`) used with `mmk trace`; with
    /// --title and --table, also the selected one as replacement title courses (encrypted .szs).
    TraceCourses {
        out: PathBuf,
        #[arg(long)]
        only: Option<String>,
        #[arg(long)]
        table: Option<PathBuf>,
        #[arg(long)]
        title: Option<PathBuf>,
    },
    /// Render the sound of a headless scripted run (run right, jump) to a WAV file, through the
    /// same sound director and mixer the players use, and print what played (frame, user, key)
    /// with --log.
    AudioRender {
        /// Course file (raw / zlib course data, or .bcd with the cache's key table).
        course: PathBuf,
        /// Asset cache of one build (with `audio/`).
        #[arg(long)]
        cache: PathBuf,
        /// Frames to run (60 per second).
        #[arg(long, default_value_t = 1200)]
        frames: u32,
        #[arg(long)]
        out: PathBuf,
        /// Input script: "run" (run right, jump in bursts), "idle", or "walk".
        #[arg(long, default_value = "run")]
        script: String,
        #[arg(long)]
        log: bool,
        /// Only this bus: "music" or "sfx" (the other is muted).
        #[arg(long)]
        only: Option<String>,
    },
    Simulate {
        files: Vec<PathBuf>,
        #[arg(long)]
        each: bool,
        /// Course key table for encrypted .bcd files (an asset cache's keys/course_table.bin).
        #[arg(long)]
        keys: Option<PathBuf>,
        /// Frames per course (60 per second).
        #[arg(long, default_value_t = 3600)]
        frames: u32,
        /// SMM1 extra: SMM1 shell physics (mid-air shell kicks, shell jumps).
        #[arg(long)]
        smm1_shell_kicks: bool,
        /// SMM1 extra: SMM1 draw order (the player travels between blocks and pipes, coins behind
        /// blocks, tracks in front of pipes, …).
        #[arg(long)]
        smm1_layering: bool,
        /// SMM1 extra: Super Mushrooms in SMB1-style courses become Mystery Mushrooms or Weird
        /// Mushrooms: off, mystery, weird.
        #[arg(long, value_name = "off|mystery|weird")]
        smb1_mushroom: Option<String>,
        /// SMM1 extra: the Mystery Mushroom costume id, or "random" (default).
        #[arg(long, value_name = "ID|random")]
        costume: Option<String>,
        /// Any engine option as NAME=VALUE (repeatable).
        #[arg(long = "option", value_name = "NAME=VALUE")]
        options: Vec<String>,
        /// Print each course's final state hash (compare two runs, e.g. in separate processes,
        /// to catch nondeterminism that would desync online play).
        #[arg(long)]
        checksum: bool,
    },
    /// Run the open-source course server (courses + index in DIR) and the online-play relay
    /// (UDP, same port).
    Server {
        #[arg(long, default_value = "mmk-server-data")]
        dir: PathBuf,
        #[arg(long, default_value = "0.0.0.0:8640")]
        addr: String,
        /// Moderation token (ban accounts, delete courses); default $MMK_ADMIN_TOKEN.
        #[arg(long)]
        admin_token: Option<String>,
        /// The relay's WebSocket address for browsers (default: the HTTP port + 1).
        #[arg(long)]
        relay_ws: Option<String>,
        /// World records only from replays the server re-simulates (typed-in times count as
        /// clears only).
        #[arg(long)]
        verified_records: bool,
        /// Request threads.
        #[arg(long, default_value_t = 8)]
        threads: usize,
        /// Writes (uploads, votes, comments, clears) per client address per minute.
        #[arg(long, default_value_t = 60)]
        writes_per_minute: u32,
        /// New accounts per client address per hour.
        #[arg(long, default_value_t = 5)]
        accounts_per_hour: u32,
        /// Behind your own reverse proxy: take client addresses from X-Forwarded-For.
        #[arg(long)]
        trust_proxy: bool,
    },
    /// Upload a course (raw, zlib or .bcd needs decrypting first) to a course server.
    /// Upload courses (raw, zlib, or .bcd with --keys); several at once seed a new server.
    Upload {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Course key table for encrypted .bcd files (an asset cache's keys/course_table.bin).
        #[arg(long)]
        keys: Option<PathBuf>,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
        #[arg(long, default_value = "anonymous")]
        maker: String,
        /// Account token (default $MMK_TOKEN): uploads under that account.
        #[arg(long)]
        token: Option<String>,
    },
    /// Create an account on a course server; prints the maker ID and the token (keep it secret,
    /// pass it with --token or $MMK_TOKEN).
    Register {
        name: String,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
    },
    /// Like (or with --boo, boo) a course as your account.
    Like {
        code: String,
        #[arg(long)]
        boo: bool,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
        #[arg(long)]
        token: Option<String>,
    },
    /// List a course's comments, or post TEXT as your account.
    Comment {
        code: String,
        text: Option<String>,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
        #[arg(long)]
        token: Option<String>,
    },
    /// Leaderboards: maker_points, clears, first_clears or records.
    Rankings {
        #[arg(default_value = "maker_points")]
        kind: String,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
    },
    /// Show a maker's profile and courses.
    Maker {
        maker_id: String,
        #[arg(long, default_value = "http://localhost:8640")]
        server: String,
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
    /// Summarise a BFRES model file as JSON: models (bones, shapes, LODs, materials) and
    /// skeletal / visibility / material animations. Accepts a model `.zs`, a raw BFRES, or a
    /// `Pack/*.pack` with `--member <model name>`.
    Bfres {
        file: PathBuf,
        #[arg(long)]
        member: Option<String>,
        /// Also print every curve of every skeletal animation.
        #[arg(long)]
        curves: bool,
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
        if blob.starts_with(b"FRES") && blob.get(4) == Some(&3) {
            // Wii U BFRES (SMM1): GX2 textures, written as PNG when they decode to RGBA8
            for t in mmk_formats::bfres_wiiu::textures(&blob).map_err(|e| e.to_string())? {
                println!(
                    "{}",
                    json!({ "name": t.name, "w": t.width, "h": t.height,
                        "gx2_format": format!("{:#x}", t.format), "tile_mode": t.tile_mode })
                );
                if let Ok(img) = t.decode()
                    && img.kind == mmk_formats::bflim::PixelKind::Rgba8
                {
                    write_png(
                        &out.join(format!("{}.png", t.name)),
                        t.width as usize,
                        t.height as usize,
                        &img.data,
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            continue;
        }
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
    // Wii U waves (SMM1): converted to BWAV first
    for (_, blob) in blobs.iter_mut() {
        if blob.starts_with(b"FWAV") {
            *blob = audio::Fwav::parse(blob)
                .map_err(|e| e.to_string())?
                .to_bwav();
        }
    }
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
    // apply the channel mapping and composite over a grey checkerboard (MMK_TEXPNG_RAW=1: the
    // channels as they are)
    let raw = std::env::var_os("MMK_TEXPNG_RAW").is_some();
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
        if raw {
            // the decoded channels as they are (BC4 / BC5 masks: R, G), opaque
            px[..3].copy_from_slice(&src[..3]);
            px[3] = 255;
            continue;
        }
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

/// BFRES bytes from a model `.zs`, a raw BFRES, or a pack member.
fn load_bfres(file: &PathBuf, member: Option<&str>) -> Result<Vec<u8>, String> {
    use mmk_formats::archive::{self, Sarc};
    let e = |x: mmk_formats::Error| x.to_string();
    let mut data = archive::unwrap(std::fs::read(file).map_err(|e| e.to_string())?).map_err(e)?;
    for _ in 0..3 {
        if data.starts_with(b"FRES") {
            return Ok(data);
        }
        let sarc = Sarc::parse(data).map_err(e)?;
        let next = match member {
            Some(m) => sarc
                .files()
                .find(|(n, _)| n.rsplit('/').next().and_then(|f| f.split('.').next()) == Some(m))
                .map(|(_, d)| d.to_vec()),
            None => None,
        }
        .or_else(|| {
            sarc.files()
                .find(|(_, d)| d.starts_with(b"FRES"))
                .map(|(_, d)| d.to_vec())
        })
        .ok_or("no BFRES (or --member) in the archive")?;
        data = archive::unwrap(next).map_err(e)?;
    }
    Err("archive nesting too deep".into())
}

fn dump_bfres(file: &PathBuf, member: Option<&str>, curves: bool) -> Result<Value, String> {
    use mmk_formats::bfres;
    let f = bfres::parse(&load_bfres(file, member)?).map_err(|e| e.to_string())?;
    let models: Vec<Value> = f
        .models
        .iter()
        .map(|m| {
            json!({
                "name": m.name,
                "skeleton": { "flags": m.skeleton.flags, "smooth": m.skeleton.smooth_count,
                              "rigid": m.skeleton.rigid_count, "matrix_to_bone": m.skeleton.matrix_to_bone },
                "bones": m.bones.iter().map(|b| json!([b.name, b.parent, format!("{:#x}", b.flags),
                    b.smooth_index, b.rigid_index, b.scale, b.rotation, b.translation])).collect::<Vec<_>>(),
                "materials": m.materials.iter().map(|x| json!({"name": x.name, "textures": x.textures,
                    "samplers": x.samplers.iter().map(|s| s.name.clone()).collect::<Vec<_>>(),
                    "shader": [x.shader.0, x.shader.1],
                    "render_info": x.render_info.iter().map(|(n, v)| (n.clone(), match v {
                        mmk_formats::bfres::RenderInfo::Int(i) => json!(i),
                        mmk_formats::bfres::RenderInfo::Float(f) => json!(f),
                        mmk_formats::bfres::RenderInfo::Str(s) => json!(s),
                    })).collect::<serde_json::Map<_, _>>(),
                    "params": x.params.iter().map(|p| (p.name.clone(), json!([p.kind, p.values])))
                        .collect::<serde_json::Map<_, _>>(),
                    "options": x.options.iter().map(|(a, b)| (a.clone(), json!(b)))
                        .collect::<serde_json::Map<_, _>>(),
                    "sampler_assign": x.sampler_assign.iter().map(|(a, b)| (a.clone(), json!(b)))
                        .collect::<serde_json::Map<_, _>>(),
                })).collect::<Vec<_>>(),
                "shapes": m.shapes.iter().map(|s| json!({
                    "name": s.name, "material": s.material, "bone": s.bone, "skin": s.skin_count,
                    "vertices": s.vertex_count,
                    "attributes": s.attributes.iter().map(|a| format!("{}:{:#06x}", a.name, a.format)).collect::<Vec<_>>(),
                    "lods": s.lods.iter().map(|l| json!([l.primitive, l.indices.len()])).collect::<Vec<_>>(),
                    "skin_bones": s.skin_bones,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let curve_json = |c: &bfres::Curve| {
        json!({"target": format!("{:#x}", c.target), "kind": format!("{:?}", c.kind), "start": c.start,
               "end": c.end, "frames": c.frames, "keys": c.keys})
    };
    let ska: Vec<Value> = f
        .skeletal_anims
        .iter()
        .map(|a| {
            let mut v = json!({"name": a.name, "flags": format!("{:#x}", a.flags), "frames": a.frame_count,
                "bones": a.bone_anims.len(),
                "curves": a.bone_anims.iter().map(|b| b.curves.len()).sum::<usize>()});
            if curves {
                v["tracks"] = a.bone_anims.iter().map(|b| json!({"bone": b.name, "flags": format!("{:#x}", b.flags),
                    "S": b.base_scale, "R": b.base_rotate, "T": b.base_translate,
                    "curves": b.curves.iter().map(curve_json).collect::<Vec<_>>()})).collect();
            }
            v
        })
        .collect();
    let vis: Vec<Value> = f
        .visibility_anims
        .iter()
        .map(|a| json!({"name": a.name, "flags": format!("{:#x}", a.flags), "frames": a.frame_count,
            "hidden": a.bones.iter().filter(|b| !b.1).map(|b| b.0.clone()).collect::<Vec<_>>(),
            "curves": a.curves.iter().map(|c| json!([a.bones.get(c.target as usize).map(|b| b.0.clone()),
                format!("{:?}", c.kind), c.frames, c.keys.iter().map(|k| k[0]).collect::<Vec<_>>()])).collect::<Vec<_>>()}))
        .collect();
    let fmaa: Vec<Value> = f
        .material_anims
        .iter()
        .map(|a| json!({"name": a.name, "frames": a.frame_count, "textures": a.textures.len()}))
        .collect();
    Ok(
        json!({"name": f.name, "models": models, "skeletal_anims": ska, "visibility_anims": vis,
              "material_anims": fmaa}),
    )
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
        Cmd::RefCourses {
            out,
            table,
            only,
            romfs,
            course,
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
            let course = match course.map(|p| std::fs::read(&p).map(|b| (p, b))) {
                Some(Ok((p, b))) => match course_data(b, table.as_ref()) {
                    Some(c) => Some(c),
                    None => {
                        eprintln!("{}: not course data", p.display());
                        return ExitCode::FAILURE;
                    }
                },
                Some(Err(e)) => {
                    eprintln!("course: {e}");
                    return ExitCode::FAILURE;
                }
                None => None,
            };
            let only = if course.is_some() && only.is_none() {
                Some("-".to_string())
            } else {
                only
            };
            match refcourses::run(
                &out,
                table.as_ref(),
                only.as_deref(),
                romfs.as_deref(),
                course,
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("ref-courses: {e}");
                    ExitCode::FAILURE
                }
            }
        }
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
        Cmd::Simulate {
            files,
            each,
            keys,
            frames,
            smm1_shell_kicks,
            smm1_layering,
            smb1_mushroom,
            costume,
            options,
            checksum,
        } => {
            let mut o = mmk_sim::Options {
                smm1_shell_kicks,
                smm1_layering,
                ..Default::default()
            };
            let assignments = smb1_mushroom
                .map(|m| format!("smb1_mushroom={m}"))
                .into_iter()
                .chain(costume.map(|c| format!("costume={c}")))
                .chain(options);
            for a in assignments {
                if let Err(e) = o.apply(&a) {
                    eprintln!("{e}");
                    return ExitCode::FAILURE;
                }
            }
            simulate(&files, each || checksum, keys, frames, checksum, o)
        }
        Cmd::MissingArt {
            files,
            assets,
            keys,
        } => missing_art(&files, &assets, keys),
        Cmd::Trace {
            course,
            inputs,
            out,
            keys,
            start,
            facing,
            form,
            vx,
        } => {
            let table = read_key_table(keys);
            let Some(data) = load(&course)
                .ok()
                .and_then(|raw| course_data(raw, table.as_ref()))
            else {
                return fail("course", "unreadable");
            };
            let script = match std::fs::read_to_string(&inputs)
                .map_err(|e| e.to_string())
                .and_then(|t| trace::parse_script(&t))
            {
                Ok(s) => s,
                Err(e) => return fail("inputs", e),
            };
            let pos = match start.as_deref().map(|s| {
                let v: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                if v.len() == 2 {
                    Ok((v[0], v[1]))
                } else {
                    Err("--start x,y")
                }
            }) {
                Some(Ok(p)) => Some(p),
                Some(Err(e)) => return fail("start", e),
                None => None,
            };
            let form = match form.as_deref().map(trace::parse_form) {
                Some(Ok(f)) => Some(f),
                Some(Err(e)) => return fail("form", e),
                None => None,
            };
            let start = trace::Start {
                pos,
                facing,
                form,
                vx,
            };
            match trace::run(&data, &script, &start)
                .and_then(|csv| std::fs::write(&out, csv).map_err(|e| e.to_string()))
            {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail("trace", e),
            }
        }
        Cmd::TraceCourses {
            out,
            only,
            table,
            title,
        } => {
            let table = read_key_table(table);
            match trace::write_courses(&out, only.as_deref(), table.as_ref(), title.as_deref()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail("trace-courses", e),
            }
        }
        Cmd::AudioRender {
            course,
            cache,
            frames,
            out,
            script,
            log,
            only,
        } => match audio_render(&course, &cache, frames, &out, &script, log, only.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("audio-render: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::Code { ids } => {
            for id in ids {
                match id.parse::<u32>() {
                    Ok(n) => println!(
                        "{id}\t{}",
                        mmk_formats::course_id::smm2_data_id_to_code(n, false)
                    ),
                    Err(_) => match mmk_formats::course_id::smm2_code_to_data_id(&id) {
                        Ok((n, maker)) => {
                            println!("{id}\t{n}{}", if maker { " (maker)" } else { "" })
                        }
                        Err(e) => println!("{id}\t{e:?}"),
                    },
                }
            }
            ExitCode::SUCCESS
        }
        Cmd::Gallery {
            style,
            alt,
            theme,
            night,
            out,
        } => match gallery(&style, alt, theme, night, &out) {
            Ok(files) => {
                for f in files {
                    println!("{}", f.display());
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("gallery: {e}");
                ExitCode::FAILURE
            }
        },
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
                        "imported SMM1 {} UI: {} layouts, {} animations, {} textures, {} fonts, {} message files; {} costumes ({} frames, {} sounds) ({} skipped) into {}",
                        r.build,
                        r.layouts,
                        r.animations,
                        r.textures,
                        r.fonts,
                        r.messages,
                        r.costumes,
                        r.costume_textures,
                        r.costume_sounds,
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
        Cmd::ImportSound { romfs, out } => {
            let mut last = String::new();
            let mut progress = |stage: &str, done: usize, total: usize| {
                if stage != last || done == total {
                    eprintln!("{stage}: {done}/{total}");
                    last = stage.to_string();
                }
            };
            match mmk_import::sound::import_sounds(&romfs, &out, &mut progress) {
                Ok(r) => {
                    for m in &r.missing {
                        eprintln!("missing wave: {m}");
                    }
                    println!(
                        "imported {} sound users, {} waves, {} streams ({} missing) into {}",
                        r.users,
                        r.waves,
                        r.streams,
                        r.missing.len(),
                        out.join("audio").display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("import failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::ImportModes { romfs, out } => match mmk_import::modes::import_modes(&romfs, &out) {
            Ok(r) => {
                println!(
                    "imported {} Story flows, {} lesson flows, {} jobs, {} builds, {} flags, {} hub objects, {} lessons into {}",
                    r.story_flows,
                    r.lesson_flows,
                    r.jobs,
                    r.builds,
                    r.flags,
                    r.hub_objects,
                    r.lessons,
                    out.display()
                );
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("import failed: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::FxCourses { out } => match fx_courses(&out) {
            Ok(paths) => {
                for p in paths {
                    println!("{}", p.display());
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("fx-courses: {e}");
                ExitCode::FAILURE
            }
        },
        Cmd::ImportEffects { romfs, cache } => {
            let mut last = String::new();
            let mut progress = |stage: &str, done: usize, total: usize| {
                if stage != last || done == total {
                    eprintln!("{stage}: {done}/{total}");
                    last = stage.to_string();
                }
            };
            match mmk_import::effects::import_effects(&romfs, &cache, &mut progress) {
                Ok(c) => {
                    println!(
                        "imported {} effect users, {} emitter sets, {} textures into {}",
                        c.users,
                        c.sets,
                        c.textures,
                        cache.display()
                    );
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("import failed: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::ImportSmm2Ui {
            romfs,
            out,
            exefs,
            language,
        } => {
            let cfg = mmk_import::Smm2UiConfig {
                romfs,
                exefs,
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
            match mmk_import::run_smm2_ui(&cfg, &mut progress) {
                Ok(r) => {
                    for s in &r.skipped {
                        eprintln!("skipped: {s}");
                    }
                    println!(
                        "imported SMM2 {} UI: {} layouts, {} animations, {} textures, {} fonts, {} message files ({} skipped) into {}",
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

        Cmd::Server {
            dir,
            addr,
            admin_token,
            relay_ws,
            verified_records,
            threads,
            writes_per_minute,
            accounts_per_hour,
            trust_proxy,
        } => {
            let admin = admin_token.or_else(|| std::env::var("MMK_ADMIN_TOKEN").ok());
            let store = match mmk_server::Store::open(&dir) {
                Ok(s) => s.with_admin(admin).with_verified_records(verified_records),
                Err(e) => {
                    eprintln!("{}: {e:?}", dir.display());
                    return ExitCode::FAILURE;
                }
            };
            // the online-play relay: UDP on the same port number, WebSocket on the next
            let ws = relay_ws.unwrap_or_else(|| match addr.parse::<std::net::SocketAddr>() {
                Ok(mut a) => {
                    a.set_port(a.port().wrapping_add(1));
                    a.to_string()
                }
                Err(_) => "0.0.0.0:8641".into(),
            });
            match mmk_server::start_relay(&addr, &ws) {
                Ok(_) => eprintln!("mmk-server: online-play relay on udp://{addr} and ws://{ws}"),
                Err(e) => eprintln!("mmk-server: no relay ({e})"),
            }
            eprintln!("mmk-server: {} on http://{addr}", dir.display());
            let opts = mmk_server::ServeOptions {
                threads,
                limits: mmk_server::limits::Limits {
                    writes_per_minute,
                    accounts_per_hour,
                },
                trust_proxy,
            };
            match mmk_server::serve_with(store, &addr, opts) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("server: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cmd::Upload {
            files,
            keys,
            server,
            maker,
            token,
        } => {
            let table = read_key_table(keys);
            let client = online(server, token);
            let (mut ok, mut failed) = (0usize, 0usize);
            for file in &files {
                let Some(data) = std::fs::read(file)
                    .ok()
                    .and_then(|raw| course_data(raw, table.as_ref()))
                else {
                    eprintln!(
                        "{}: not course data (raw, zlib or .bcd with --keys)",
                        file.display()
                    );
                    failed += 1;
                    continue;
                };
                match client.upload(&data, &maker) {
                    Ok(l) => {
                        println!("{} {}", l.code, l.name);
                        ok += 1;
                    }
                    Err(e) => {
                        eprintln!("{}: {e}", file.display());
                        failed += 1;
                    }
                }
            }
            if files.len() > 1 {
                eprintln!("uploaded {ok}, failed {failed}");
            }
            if failed == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Cmd::Register { name, server } => match mmk_online::Client::new(server).register(&name) {
            Ok((a, token)) => {
                println!("maker ID {}  ({})", a.maker_id, a.name);
                println!("token {token}");
                ExitCode::SUCCESS
            }
            Err(e) => fail("register", e),
        },
        Cmd::Like {
            code,
            boo,
            server,
            token,
        } => match online(server, token).vote(&code, !boo) {
            Ok(l) => {
                println!("{} likes, {} boos", l.likes, l.boos);
                ExitCode::SUCCESS
            }
            Err(e) => fail("vote", e),
        },
        Cmd::Comment {
            code,
            text,
            server,
            token,
        } => {
            let c = online(server, token);
            if let Some(Err(e)) = text.map(|t| c.comment(&code, &t)) {
                return fail("comment", e);
            }
            match c.comments(&code) {
                Ok(list) => {
                    for m in list {
                        println!("{} ({}): {}", m.name, m.maker_id, m.text);
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => fail("comments", e),
            }
        }
        Cmd::Rankings { kind, server } => {
            match mmk_online::Client::new(server).rankings(&kind, 50) {
                Ok(rows) => {
                    for (rank, score, id, name) in rows {
                        println!("{rank:>3}. {score:>6}  {id}  {name}");
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => fail("rankings", e),
            }
        }
        Cmd::Maker { maker_id, server } => match mmk_online::Client::new(server).maker(&maker_id) {
            Ok((a, courses)) => {
                println!("{} — {}", a.maker_id, a.name);
                for l in courses {
                    println!("  {}  {:>5} plays  {}", l.code, l.plays, l.name);
                }
                ExitCode::SUCCESS
            }
            Err(e) => fail("maker", e),
        },
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
        Cmd::Bfres {
            file,
            member,
            curves,
        } => match dump_bfres(&file, member.as_deref(), curves) {
            Ok(v) => {
                println!("{}", serde_json::to_string_pretty(&v).unwrap());
                ExitCode::SUCCESS
            }
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
        Cmd::Import {
            romfs,
            exefs,
            update_romfs,
            update_exefs,
            out,
        } => {
            let Some(out_root) = out.or_else(mmk_host::paths::assets_root) else {
                return fail("import", "no data folder: pass --out");
            };
            let exefs = update_exefs.unwrap_or(exefs);
            let merge_dir = out_root.join(".romfs-overlay");
            let romfs = match &update_romfs {
                Some(update) => {
                    eprintln!("merging the update over the base RomFS (links, no copy)");
                    if let Err(e) = mmk_import::overlay::overlay_romfs(&romfs, update, &merge_dir) {
                        return fail("merging update RomFS", e);
                    }
                    merge_dir.clone()
                }
                None => romfs,
            };
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
            let result = mmk_import::run(&cfg, &mut progress);
            if update_romfs.is_some() {
                let _ = std::fs::remove_dir_all(&merge_dir);
            }
            match result {
                Ok(r) => {
                    println!(
                        "imported SMM2 {} ({} tables, {} textures, {} sounds, {} sprite quads, {} meshes, {} animations, {} built-in courses verified) into {}",
                        r.build,
                        r.tables,
                        r.textures,
                        r.sounds,
                        r.sprites,
                        r.meshes,
                        r.anims,
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

/// Course data from a file: raw, zlib, or an encrypted .bcd (with the key table).
fn course_data(raw: Vec<u8>, table: Option<&smm2::KeyTable>) -> Option<Vec<u8>> {
    if raw.len() == smm2::COURSE_SIZE {
        Some(raw)
    } else if raw.len() == smm2::BCD_SIZE {
        crypto::decrypt_bcd(&raw, table?, false)
            .ok()
            .map(|b| b.body)
    } else {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(&raw[..])
            .read_to_end(&mut out)
            .ok()?;
        Some(out)
    }
}

fn read_key_table(path: Option<PathBuf>) -> Option<smm2::KeyTable> {
    let b = std::fs::read(path?).ok()?;
    let words: Vec<u32> = b
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    words.try_into().ok()
}

/// One scripted run: hold right + run, jump in 24-frame bursts every 40 frames, and a long jump
/// when stuck for a second. Returns (progress 0-1 toward the goal, deaths, cleared).
fn simulate_course(
    course: &smm2::Course,
    frames: u32,
    options: mmk_sim::Options,
) -> (f32, u32, bool, u64) {
    use mmk_sim::input::{self, Input};
    let mut w = mmk_sim::World::from_course(course);
    w.set_options(options);
    w.lives = 9999; // keep retrying for the whole run
    let start_x = w.player.x;
    let goal_x = w.level.goal.map(|g| g.0).unwrap_or(w.level.right);
    let lives0 = w.lives;
    let (mut best, mut stuck, mut last_x) = (start_x, 0u32, start_x);
    let mut cleared = false;
    for t in 0..frames {
        let mut b = input::RIGHT | input::RUN;
        let long = stuck > 60 && stuck % 90 < 40;
        if t % 40 < 24 || long {
            b |= input::JUMP;
        }
        w.step(Input { buttons: b });
        best = best.max(w.player.x);
        if (w.player.x - last_x).abs() < 0.2 {
            stuck += 1;
        } else {
            stuck = 0;
        }
        last_x = w.player.x;
        if w.goal_phase != mmk_sim::world::GoalPhase::None {
            cleared = true;
            break;
        }
    }
    let span = (goal_x - start_x).max(1.0);
    (
        ((best - start_x) / span).clamp(0.0, 1.0),
        lives0.saturating_sub(w.lives),
        cleared,
        w.checksum(),
    )
}

fn audio_render(
    course: &std::path::Path,
    cache: &std::path::Path,
    frames: u32,
    out: &std::path::Path,
    script: &str,
    log: bool,
    only: Option<&str>,
) -> Result<(), String> {
    use mmk_sim::input::{self, Input};
    let table = read_key_table(Some(cache.join("keys/course_table.bin")));
    let raw = std::fs::read(course).map_err(|e| format!("{}: {e}", course.display()))?;
    let data = course_data(raw, table.as_ref()).ok_or("not a course file")?;
    let c = smm2::Course::parse(&data).map_err(|e| e.to_string())?;
    let bank = mmk_audio::Bank::load_cache(cache).map_err(|e| e.to_string())?;
    let audio = mmk_host::audio::AudioOut::new(bank).with_cache_dir(cache);
    if audio.library().is_none() {
        eprintln!("no audio/sound.json in the cache: fixed SMB1 sounds only");
    }
    let mut w = mmk_sim::World::from_course(&c);
    w.lives = 9999;
    let (music, sfx) = match only {
        Some("music") => (1.0, 0.0),
        Some("sfx") => (0.0, 1.0),
        _ => (1.0, 1.0),
    };
    audio.send(mmk_audio::Command::SetVolumes {
        master: 1.0,
        music,
        sfx,
    });
    let mut d = audio.director();
    d.start_course(&w);
    let per_frame = (audio.rate() / 60) as usize;
    let mut pcm = Vec::with_capacity(frames as usize * per_frame * 2);
    let mut buf = vec![0.0f32; per_frame * 2];
    let (mut stuck, mut last_x) = (0u32, w.player.x);
    for t in 0..frames {
        let b = match script {
            "idle" => 0,
            "walk" => input::RIGHT,
            _ => {
                let mut b = input::RIGHT | input::RUN;
                let long = stuck > 60 && stuck % 90 < 40;
                if t % 40 < 24 || long {
                    b |= input::JUMP;
                }
                b
            }
        };
        w.step(Input { buttons: b });
        if (w.player.x - last_x).abs() < 0.2 {
            stuck += 1;
        } else {
            stuck = 0;
        }
        last_x = w.player.x;
        d.after_step(&w);
        if log {
            for (user, key) in d.played() {
                println!("{t}\t{user}\t{key}");
            }
        }
        audio.render(&mut buf);
        pcm.extend_from_slice(&buf);
    }
    std::fs::write(out, mmk_audio::wav::stereo_f32(audio.rate(), &pcm))
        .map_err(|e| format!("{}: {e}", out.display()))?;
    let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    eprintln!(
        "{} s of sound ({} style, theme {}), peak {peak:.2} → {}",
        frames / 60,
        w.level.style,
        w.level.theme,
        out.display()
    );
    Ok(())
}

fn simulate(
    files: &[PathBuf],
    each: bool,
    keys: Option<PathBuf>,
    frames: u32,
    checksum: bool,
    options: mmk_sim::Options,
) -> ExitCode {
    let table = read_key_table(keys);
    let (mut total, mut panics, mut clears) = (0usize, 0usize, 0usize);
    let mut progress = 0.0f32;
    for f in files {
        let Some(data) = std::fs::read(f)
            .ok()
            .and_then(|raw| course_data(raw, table.as_ref()))
        else {
            continue;
        };
        let Ok(c) = smm2::Course::parse(&data) else {
            continue;
        };
        total += 1;
        let style = c.header.game_style.as_str_lossy();
        let run = std::panic::catch_unwind(|| simulate_course(&c, frames, options));
        let line = match run {
            Ok((p, deaths, cleared, hash)) => {
                progress += p;
                clears += cleared as usize;
                let mut v = json!({"file": f.display().to_string(), "style": style, "progress": p,
                       "deaths": deaths, "cleared": cleared});
                if checksum {
                    v["checksum"] = json!(format!("{hash:016x}"));
                }
                v
            }
            Err(e) => {
                panics += 1;
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                json!({"file": f.display().to_string(), "style": style, "panic": msg})
            }
        };
        if each || line.get("panic").is_some() {
            println!("{line}");
        }
    }
    println!(
        "{total} courses: {panics} crashed, {clears} cleared by the scripted run, mean progress {:.0}%",
        100.0 * progress / (total.saturating_sub(panics)).max(1) as f32
    );
    if panics > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Gallery courses: every supported part on a two-tile floor, two tiles apart, in pages.
fn gallery(
    style: &str,
    alt: bool,
    theme: u8,
    night: bool,
    out: &std::path::Path,
) -> Result<Vec<PathBuf>, String> {
    // parts that aren't placeable objects of their own (header-made goal / start, track and
    // scroll nodes, sound effects)
    const SKIP: &[i16] = &[26, 27, 37, 38, 59, 89, 109];
    // (which parts a style offers: the editor's rules)
    const PAGE: i32 = 220;
    let parts: Vec<i16> = mmk_sim::level::SUPPORTED_PARTS
        .iter()
        .copied()
        .filter(|p| !SKIP.contains(p) && mmk_editor::rules::style_allows(style, *p))
        .collect();
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    let mut rest = &parts[..];
    let mut page = 0;
    while !rest.is_empty() {
        let mut ed = mmk_editor::Editor::new(style);
        ed.set_theme(theme);
        ed.set_night(night);
        ed.set_timer(500);
        ed.set_name(&format!("Gallery {style} {}", page + 1));
        let width = PAGE + 30;
        ed.course.main.header.boundary_right = width * 16;
        ed.course.header.goal_x = ((width - 10) * 10) as i16;
        for x in 0..width {
            for y in 0..2 {
                if !ed.protected(x, y) {
                    ed.paint_ground(x, y).map_err(|e| format!("{e:?}"))?;
                }
            }
        }
        let mut x = mmk_editor::rules::START_W + 1; // clear of the start zone
        let mut placed = 0;
        for &part in rest {
            let (w, _) = mmk_editor::default_size(part);
            if x + w as i32 > PAGE {
                break;
            }
            let i = ed
                .place(part, x, 2)
                .map_err(|e| format!("part {part}: {e:?}"))?;
            if alt {
                ed.toggle_flags(i, 0x4);
            }
            x += w as i32 + 2;
            placed += 1;
        }
        rest = &rest[placed.max(1)..];
        let data = ed.save().map_err(|e| format!("{e:?}"))?;
        let name = format!(
            "gallery_{style}{}_{}.bin",
            if alt { "_alt" } else { "" },
            page + 1
        );
        let path = out.join(name);
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        written.push(path);
        page += 1;
    }
    Ok(written)
}

/// Effect test courses, one per style (see `Cmd::FxCourses`).
fn fx_courses(out: &std::path::Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    for style in ["M1", "M3", "MW", "WU", "3W"] {
        // (what the game needs to accept a course as its own, e.g. as a title course: a length,
        // start / goal heights, the header fields of a course made in the game, no ground over
        // the start or goal grounds)
        let len = 120;
        let mut ed = mmk_editor::Editor::new(style);
        ed.set_name(&format!("Effects {style}"));
        ed.set_theme(0);
        ed.set_timer(500);
        ed.set_length(len);
        {
            let h = &mut ed.course.header;
            (h.year, h.month, h.day, h.hour, h.minute) = (2019, 4, 16, 16, 5);
            h.game_version = 1;
            h.management_flags = 1;
            h.clear_check_time = u32::MAX;
            h.creation_id = 1;
            h.goal_x = (len * 10 - 95) as i16;
        }
        ed.course.main.header.boundary_type = 1;
        // floor height: the game rejects a 3D World course with start height 1
        let base: i32 = if style == "3W" { 2 } else { 1 };
        ed.set_start_goal(base as u8, base as u8);
        if style == "3W" {
            for a in [&mut ed.course.main.header, &mut ed.course.sub.header] {
                a.liquid_start_height = 1;
                a.liquid_end_height = 1;
            }
            ed.course.main.header.boundary_type = 0;
            ed.course.sub.header.boundary_type = 1;
        }
        let err = |e| format!("{style}: {e:?}");
        for x in 7..=len - 12 {
            for y in 0..base {
                ed.paint_ground(x, y).map_err(err)?;
            }
        }
        for x in [16, 26, 36] {
            ed.place(0, x, base).map_err(err)?; // Goomba
        }
        ed.place(1, 46, base).map_err(err)?; // Koopa
        for x in 54..62 {
            ed.place(8, x, base + 1).map_err(err)?; // coins
        }
        for x in [70, 76] {
            ed.place(0, x, base).map_err(err)?;
        }
        // courses made in the game carry the default flags in the (unused) child flags too;
        // the game rejects objects with zero there
        for o in &mut ed.course.main.objects.items {
            if o.child_flags == 0 {
                o.child_flags = 0x0600_0040;
            }
        }
        let data = ed.save().map_err(|e| format!("{e:?}"))?;
        let path = out.join(format!("fx_{style}.course"));
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        written.push(path);
    }
    Ok(written)
}

/// Sprites without art: for each course and area, step the view over the area, resolve every
/// sprite against the cache and count the (model, texture) names that resolve to nothing.
fn missing_art(files: &[PathBuf], assets: &std::path::Path, keys: Option<PathBuf>) -> ExitCode {
    let cache = match mmk_assets::Cache::open(assets) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}: {e:?}", assets.display());
            return ExitCode::FAILURE;
        }
    };
    let meshes = assets.join("meshes");
    let table = read_key_table(keys);
    let mut missing: std::collections::BTreeMap<(String, String, String), usize> =
        Default::default();
    let mut checked = 0;
    for f in files {
        let Some(data) = std::fs::read(f)
            .ok()
            .and_then(|raw| course_data(raw, table.as_ref()))
        else {
            eprintln!("{}: not course data", f.display());
            continue;
        };
        let Ok(c) = smm2::Course::parse(&data) else {
            eprintln!("{}: unreadable course", f.display());
            continue;
        };
        checked += 1;
        let mut w = mmk_sim::World::from_course(&c);
        let style = w.level.style.clone();
        let mut resolver = mmk_host::SpriteResolver::default();
        for area in 0..2 {
            w.show_area(area);
            let (left, bottom, right, top) = w.area_bounds();
            let (vw, vh) = (mmk_sim::world::VIEW_W, mmk_sim::world::VIEW_H);
            let mut y = bottom;
            while y < top {
                let mut x = left;
                while x < right {
                    w.look_at(x, y);
                    for s in w.sprites() {
                        if resolver.resolve_one(&style, &s, &cache).texture.is_some() {
                            continue;
                        }
                        // the 3D styles draw many models as meshes
                        let rest = s.image.model.strip_prefix("M1_").unwrap_or(s.image.model);
                        let mesh = matches!(style.as_str(), "WU" | "3W")
                            && (meshes.join(format!("{style}_{rest}.mmesh")).exists()
                                || meshes.join(format!("{}.mmesh", s.image.model)).exists()
                                || s.image.model.starts_with("M1_Player_"));
                        if !mesh {
                            *missing
                                .entry((
                                    style.clone(),
                                    s.image.model.to_string(),
                                    s.image.texture.to_string(),
                                ))
                                .or_default() += 1;
                        }
                    }
                    x += vw;
                }
                y += vh;
            }
        }
    }
    for ((style, model, tex), n) in &missing {
        println!("{style}  {model}  {tex}  ({n})");
    }
    println!(
        "{checked} courses: {} missing (style, model, texture) names",
        missing.len()
    );
    ExitCode::SUCCESS
}

/// A course-server client signed in with `token` or $MMK_TOKEN when set.
fn online(server: String, token: Option<String>) -> mmk_online::Client {
    let token = token.or_else(|| std::env::var("MMK_TOKEN").ok().filter(|t| !t.is_empty()));
    mmk_online::Client::new(server).with_token(token)
}

fn fail(what: &str, e: impl std::fmt::Display) -> ExitCode {
    eprintln!("{what}: {e}");
    ExitCode::FAILURE
}
