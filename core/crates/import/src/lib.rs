//! First-launch importer: the user's own extracted SMM2 RomFS + ExeFS → engine asset cache.
//!
//! Stages implemented so far (docs/architecture.md §4): identify, course key table, tables,
//! manifest. Textures, models and audio follow in later steps. Nothing read or written here ever
//! goes into the repository; the cache lives in the user's Application Support folder.

use std::fs;
use std::path::{Path, PathBuf};

use mmk_formats::archive::{self, Sarc};
use mmk_formats::nso::Nso;
use mmk_formats::smm2::crypto::{KeyTable, decrypt_bcd};
use mmk_formats::smm2::keyfind;
use serde_json::{Value, json};

pub const IMPORTER_VERSION: u32 = 1;

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

    // 4. manifest
    let manifest: Value = json!({
        "importer_version": IMPORTER_VERSION,
        "game": "SMM2",
        "build": build,
        "module_id": nso.module_id_hex(),
        "stages": ["identify", "keys", "tables"],
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
        builtin_courses_verified: courses.len(),
    })
}
