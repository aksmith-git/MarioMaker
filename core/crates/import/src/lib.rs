//! First-launch importer: the user's own extracted SMM2 RomFS + ExeFS → engine asset cache.
//!
//! Stages implemented so far (docs/architecture.md §4): identify, course key table, tables,
//! textures, audio, sprites (2D quad placement + texture-pattern timing), manifest. 3D models
//! follow in a later step. Nothing read or written here ever goes into the repository; the cache
//! lives in the user's Application Support folder.

pub mod audio;

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use mmk_assets::{Mtex, MtexFormat, MtexVariant};
use mmk_formats::archive::{self, Sarc};
use mmk_formats::bfres;
use mmk_formats::bntx::{self, PixelFormat, Variant};
use mmk_formats::nso::Nso;
use mmk_formats::smm2::crypto::{KeyTable, decrypt_bcd};
use mmk_formats::smm2::keyfind;
use serde_json::{Value, json};

pub mod sprites;

pub const IMPORTER_VERSION: u32 = 3;

/// Builds this importer knows, identified by the NSO module id (first 16 bytes; the rest is 0).
pub const KNOWN_BUILDS: &[(&str, &str)] = &[
    ("3ebb1eb2fffb029a8b87579b961a7b00", "1.0.0"),
    ("c2dc405ac414c37c8b1c50219c7a0f0c", "3.0.3"),
];

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Format {
        path: PathBuf,
        source: mmk_formats::Error,
    },
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, ImportError>;

#[derive(Clone, Debug)]
pub struct ImportConfig {
    /// Extracted RomFS (with the update already applied, as Ryujinx "Extract Data" produces).
    pub romfs: PathBuf,
    /// Extracted ExeFS (needs `main`).
    pub exefs: PathBuf,
    /// Cache root; the import goes into `<out_root>/<build>/`.
    pub out_root: PathBuf,
}

#[derive(Clone, Debug)]
pub struct ImportReport {
    pub build: String,
    pub module_id: String,
    pub out_dir: PathBuf,
    pub tables: usize,
    pub textures: usize,
    pub sounds: usize,
    /// Textures with a known sprite quad.
    pub sprites: usize,
    pub builtin_courses_verified: usize,
}

/// Progress callback: (stage, done, total).
pub type Progress<'a> = &'a mut dyn FnMut(&str, usize, usize);

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| ImportError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ImportError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, data).map_err(|source| ImportError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn fmt_err(path: &Path) -> impl Fn(mmk_formats::Error) -> ImportError + '_ {
    move |source| ImportError::Format {
        path: path.to_path_buf(),
        source,
    }
}

/// Yaz0/zstd-wrapped SARC from a RomFS path.
fn open_sarc(path: &Path) -> Result<Sarc> {
    let data = archive::unwrap(read(path)?).map_err(fmt_err(path))?;
    Sarc::parse(data).map_err(fmt_err(path))
}

pub fn identify(exefs: &Path) -> Result<(Nso, String)> {
    let path = exefs.join("main");
    let nso = Nso::parse(&read(&path)?).map_err(fmt_err(&path))?;
    let id = nso.module_id_hex();
    let build = KNOWN_BUILDS
        .iter()
        .find(|(prefix, _)| id.starts_with(prefix))
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| format!("unknown-{}", &id[..16]));
    Ok((nso, build))
}

/// Built-in course files (`Course/*.szs` → `.bcd`).
fn builtin_bcds(romfs: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let dir = romfs.join("Course");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|source| ImportError::Io {
            path: dir.clone(),
            source,
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "szs"))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        for (name, data) in open_sarc(&p)?.files() {
            if name.ends_with(".bcd") {
                out.push((name.to_string(), data.to_vec()));
            }
        }
    }
    Ok(out)
}

/// Game styles whose models are imported. Phase 5 needs SMB1 only; widen as styles are added.
pub const STYLES: &[&str] = &["M1"];

fn to_mtex(t: &bntx::Texture) -> Option<Mtex> {
    let format = match t.format {
        PixelFormat::Rgba8 => MtexFormat::Rgba8,
        PixelFormat::Bc1 => MtexFormat::Bc1,
        PixelFormat::Bc2 => MtexFormat::Bc2,
        PixelFormat::Bc3 => MtexFormat::Bc3,
        PixelFormat::Bc4 => MtexFormat::Bc4,
        PixelFormat::Bc5 => MtexFormat::Bc5,
        PixelFormat::Bc6h => MtexFormat::Bc6h,
        PixelFormat::Bc7 => MtexFormat::Bc7,
        PixelFormat::Other(_) => return None,
    };
    let variant = match t.variant {
        Variant::Srgb => MtexVariant::Srgb,
        Variant::Snorm => MtexVariant::Snorm,
        _ => MtexVariant::Unorm,
    };
    Some(Mtex {
        format,
        variant,
        width: t.width as u32,
        height: t.height as u32,
        channel_select: t.channel_select,
        data: t.data.clone(),
    })
}

/// Model name from a RomFS path like `Model/M1_Enemy_kuribo.Nin_NX_NVN.zs`.
fn model_name(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.split('.').next().unwrap_or(file).to_string()
}

/// Every model of the imported styles: `Model/<style>_*.zs` and members of `Pack/<style>_Model.pack`.
fn collect_models(romfs: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut models = Vec::new();
    let dir = romfs.join("Model");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .map_err(|source| ImportError::Io {
            path: dir.clone(),
            source,
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    paths.sort();
    for p in paths {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if STYLES.iter().any(|s| name.starts_with(&format!("{s}_"))) && name.ends_with(".zs") {
            models.push((model_name(&name), read(&p)?));
        }
    }
    for style in STYLES {
        let pack_path = romfs.join(format!("Pack/{style}_Model.pack"));
        if pack_path.is_file() {
            for (name, data) in open_sarc(&pack_path)?.files() {
                if name.ends_with(".zs") {
                    models.push((model_name(name), data.to_vec()));
                }
            }
        }
    }
    models.sort_by(|a, b| a.0.cmp(&b.0));
    models.dedup_by(|a, b| a.0 == b.0);
    Ok(models)
}

/// Textures and sprite data of every model of the imported styles. Returns (textures, sprites).
fn import_models(cfg: &ImportConfig, out: &Path, progress: Progress) -> Result<(usize, usize)> {
    let models = collect_models(&cfg.romfs)?;
    let mut index = serde_json::Map::new();
    let mut sprite_index = serde_json::Map::new();
    let (mut count, mut sprite_count) = (0, 0);
    for (i, (model, data)) in models.iter().enumerate() {
        progress("textures", i, models.len());
        let ctx = cfg.romfs.join("Model").join(model);
        let sarc = Sarc::parse(archive::unwrap(data.clone()).map_err(fmt_err(&ctx))?)
            .map_err(fmt_err(&ctx))?;
        let mut entry = serde_json::Map::new();
        let mut sizes = HashMap::new();
        let mut bfres = None;
        for (_, member) in sarc.files() {
            if member.starts_with(b"FRES") {
                bfres = Some(bfres::parse(member).map_err(fmt_err(&ctx))?);
            }
            let Some(b) = bntx::find_embedded(member) else {
                continue;
            };
            for t in bntx::parse(b).map_err(fmt_err(&ctx))? {
                let Some(m) = to_mtex(&t) else { continue };
                write(
                    &out.join(format!("textures/{model}/{}.mtex", t.name)),
                    &m.to_bytes(),
                )?;
                entry.insert(
                    t.name.clone(),
                    json!({ "w": m.width, "h": m.height, "format": m.format.name(), "srgb": m.variant == MtexVariant::Srgb }),
                );
                sizes.insert(t.name.clone(), (m.width, m.height));
                count += 1;
            }
        }
        if !entry.is_empty() {
            index.insert(model.clone(), Value::Object(entry));
        }
        if let Some(f) = bfres {
            let s = sprites::model_sprites(&f, &sizes);
            sprite_count += s.quads.len();
            if !s.quads.is_empty() || !s.anims.is_empty() {
                sprite_index.insert(model.clone(), s.to_json());
            }
        }
    }
    progress("textures", models.len(), models.len());
    write(
        &out.join("textures/index.json"),
        serde_json::to_string(&index).unwrap().as_bytes(),
    )?;
    write(
        &out.join("sprites.json"),
        serde_json::to_string(&json!({ "version": 1, "models": sprite_index }))
            .unwrap()
            .as_bytes(),
    )?;
    Ok((count, sprite_count))
}

pub fn run(cfg: &ImportConfig, progress: Progress) -> Result<ImportReport> {
    // 1. identify
    progress("identify", 0, 1);
    let (nso, build) = identify(&cfg.exefs)?;
    if !cfg.romfs.join("Pack/Static.pack").is_file() {
        return Err(ImportError::Invalid(format!(
            "{} does not look like an SMM2 RomFS",
            cfg.romfs.display()
        )));
    }
    let out = cfg.out_root.join(format!("smm2-{build}"));
    progress("identify", 1, 1);

    // 2. course key table, verified against every built-in course
    let courses = builtin_bcds(&cfg.romfs)?;
    let (_, sample) = courses
        .first()
        .ok_or_else(|| ImportError::Invalid("no built-in courses found".into()))?;
    progress("keys", 0, courses.len());
    let table: KeyTable = keyfind::find_in_nso(&nso, sample)
        .ok_or_else(|| ImportError::Invalid("course key table not found in exefs/main".into()))?;
    for (i, (name, data)) in courses.iter().enumerate() {
        decrypt_bcd(data, &table, true)
            .map_err(|e| ImportError::Invalid(format!("{name}: {e}")))?;
        progress("keys", i + 1, courses.len());
    }
    let table_bytes: Vec<u8> = table.iter().flat_map(|w| w.to_le_bytes()).collect();
    write(&out.join("keys/course_table.bin"), &table_bytes)?;

    // 3. tables: every Mush/*.byml in Static.pack → tables/<name>.json
    let static_path = cfg.romfs.join("Pack/Static.pack");
    let pack = open_sarc(&static_path)?;
    let mush: Vec<(&str, &[u8])> = pack
        .files()
        .filter(|(n, _)| n.starts_with("Mush/") && n.ends_with(".byml"))
        .collect();
    let mut tables = Vec::new();
    for (i, (name, data)) in mush.iter().enumerate() {
        progress("tables", i, mush.len());
        let value = mmk_formats::byml::parse(data).map_err(fmt_err(&static_path.join(name)))?;
        let stem = Path::new(name)
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let json = serde_json::to_vec(&value).expect("serialise table");
        write(&out.join(format!("tables/{stem}.json")), &json)?;
        tables.push(stem);
    }
    progress("tables", mush.len(), mush.len());

    // 4. textures and sprite data (quads, texture-pattern timing) of every model of the
    //    imported styles
    let (texture_count, sprite_count) = import_models(cfg, &out, progress)?;

    // 5. sounds of the imported styles
    let sound_count = audio::import_audio(&cfg.romfs, &out, STYLES, audio::M1_SOUNDS, progress)?;

    // 6. manifest
    let manifest: Value = json!({
        "importer_version": IMPORTER_VERSION,
        "game": "SMM2",
        "build": build,
        "module_id": nso.module_id_hex(),
        "stages": ["identify", "keys", "tables", "textures", "audio", "sprites"],
        "texture_styles": STYLES,
        "textures": texture_count,
        "sounds": sound_count,
        "sprites": sprite_count,
        "tables": tables,
        "builtin_courses": courses.len(),
    });
    write(
        &out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap().as_bytes(),
    )?;

    Ok(ImportReport {
        build,
        module_id: nso.module_id_hex(),
        out_dir: out,
        tables: tables.len(),
        textures: texture_count,
        sounds: sound_count,
        sprites: sprite_count,
        builtin_courses_verified: courses.len(),
    })
}
