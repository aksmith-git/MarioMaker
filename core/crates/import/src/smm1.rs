//! SMM1 UI import: the owner's Wii U `content/` folder (base game, plus the update's `content/`
//! overlaid on it) → layouts, animations, textures, fonts and messages for the layout player.
//!
//! Output (`<out>/`, outside the repository):
//!
//! ```text
//! manifest.json                         build, counts, layout / font / message lists
//! ui/layouts/<Layout>.json              { layout, animations: {name: anim}, textures: {stem: info} }
//! ui/textures/<Layout>/<stem>.mtex      every texture of the layout archive (GPU-ready)
//! ui/fonts/<Font>.json                  metrics, glyph widths, character map
//! ui/fonts/<Font>/sheet_<n>.mtex        glyph sheets
//! ui/messages/<Archive>/<Group>/<File>.json   MSBT tables (see mmk_formats::msbt)
//! ```
//! Layout archives are found as loose `Layout/*.szs` files and as `Layout/*.szs` members of
//! `Pack/*.pack`. A file in the update replaces the base file with the same path.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use mmk_assets::{Mtex, MtexFormat, MtexVariant};
use mmk_formats::archive::{self, Sarc};
use mmk_formats::bffnt::Font;
use mmk_formats::bflim::{Flim, Image, PixelKind};
use mmk_formats::lyt::{self, Animation, Layout, PaneKind};
use mmk_formats::msbt::Msbt;
use serde_json::{Value, json};

use crate::{ImportError, Progress, Result, read, write};

/// 2: costume stage (`costumes/`, module `smm1_costumes`).
pub const SMM1_IMPORTER_VERSION: u32 = 2;

#[derive(Clone, Debug)]
pub struct Smm1Config {
    /// Base game `content/` folder.
    pub base: PathBuf,
    /// Update `content/` folder, overlaid on the base (optional).
    pub update: Option<PathBuf>,
    /// Output folder (e.g. `~/smm-work/cache/smm1-v208`).
    pub out: PathBuf,
    /// Message language suffix, e.g. `USen`.
    pub language: String,
}

#[derive(Clone, Debug, Default)]
pub struct Smm1Report {
    pub build: String,
    pub out_dir: PathBuf,
    pub layouts: usize,
    pub animations: usize,
    pub textures: usize,
    pub fonts: usize,
    pub messages: usize,
    /// Mystery Mushroom costumes, their frames and sounds (`costumes/`).
    pub costumes: usize,
    pub costume_textures: usize,
    pub costume_sounds: usize,
    /// Files that could not be converted (path: reason).
    pub skipped: Vec<String>,
}

/// The base content with the update overlaid.
pub(crate) struct Overlay {
    roots: Vec<PathBuf>, // update first
}

impl Overlay {
    pub(crate) fn new(cfg: &Smm1Config) -> Self {
        let mut roots = Vec::new();
        if let Some(u) = &cfg.update {
            roots.push(u.clone());
        }
        roots.push(cfg.base.clone());
        Self { roots }
    }

    pub(crate) fn file(&self, rel: &str) -> Option<PathBuf> {
        self.roots.iter().map(|r| r.join(rel)).find(|p| p.is_file())
    }

    /// A file of the executable's folder (`<content>/../code/<name>`), update first.
    pub(crate) fn code_file(&self, name: &str) -> Option<PathBuf> {
        self.roots
            .iter()
            .filter_map(|r| Some(r.parent()?.join("code").join(name)))
            .find(|p| p.is_file())
    }

    /// Member `name` of pack `pack` (Yaz0 removed), looked up in the update's pack first, then
    /// the base game's (an update pack can drop members the base pack has).
    pub(crate) fn pack_member(&self, pack: &str, name: &str) -> Option<Vec<u8>> {
        self.roots.iter().find_map(|r| {
            let data = std::fs::read(r.join(pack)).ok()?;
            let sarc = Sarc::parse(archive::unwrap(data).ok()?).ok()?;
            archive::unwrap(sarc.get(name)?.to_vec()).ok()
        })
    }

    /// File names in `dir` across all roots (update wins), sorted.
    pub(crate) fn list(&self, dir: &str, ext: &str) -> Vec<(String, PathBuf)> {
        let mut out: BTreeMap<String, PathBuf> = BTreeMap::new();
        for root in self.roots.iter().rev() {
            let Ok(rd) = fs::read_dir(root.join(dir)) else {
                continue;
            };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(ext) && e.path().is_file() {
                    out.insert(name, e.path());
                }
            }
        }
        out.into_iter().collect()
    }
}

fn fmt_err(path: &Path) -> impl Fn(mmk_formats::Error) -> ImportError + '_ {
    move |source| ImportError::Format {
        path: path.to_path_buf(),
        source,
    }
}

/// Members of a layout archive (`<Name>.szs` → SARC → optional `<Name>.arc` SARC).
fn archive_members(data: Vec<u8>, ctx: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let sarc = Sarc::parse(archive::unwrap(data).map_err(fmt_err(ctx))?).map_err(fmt_err(ctx))?;
    let mut out = Vec::new();
    for (name, d) in sarc.files() {
        let nested = name.ends_with(".arc") || d.starts_with(b"SARC") || d.starts_with(b"Yaz0");
        if nested && let Ok(inner) = archive_members(d.to_vec(), ctx) {
            out.extend(inner);
            continue;
        }
        out.push((name.to_string(), d.to_vec()));
    }
    Ok(out)
}

pub fn to_mtex(img: &Image) -> Mtex {
    Mtex {
        format: match img.kind {
            PixelKind::Rgba8 => MtexFormat::Rgba8,
            PixelKind::Bc1 => MtexFormat::Bc1,
            PixelKind::Bc2 => MtexFormat::Bc2,
            PixelKind::Bc3 => MtexFormat::Bc3,
            PixelKind::Bc4 => MtexFormat::Bc4,
            PixelKind::Bc5 => MtexFormat::Bc5,
        },
        variant: if img.srgb {
            MtexVariant::Srgb
        } else {
            MtexVariant::Unorm
        },
        width: img.width,
        height: img.height,
        channel_select: img.channel_select,
        data: img.data.clone(),
    }
}

fn file_stem(name: &str) -> &str {
    let file = name.rsplit('/').next().unwrap_or(name);
    file.split('.').next().unwrap_or(file)
}

/// `title_version` from `<content>/../meta/meta.xml`, if present.
fn title_version(content: &Path) -> Option<String> {
    let meta = fs::read_to_string(content.parent()?.join("meta/meta.xml")).ok()?;
    let start = meta.find("<title_version")?;
    let rest = &meta[start..];
    let open = rest.find('>')? + 1;
    let close = rest.find("</title_version>")?;
    Some(rest[open..close].trim().to_string())
}

struct LayoutSource {
    name: String,
    origin: String,
    data: Vec<u8>,
}

/// Every layout archive: loose `Layout/*.szs`, then `Layout/*.szs` inside each pack.
fn collect_layouts(ov: &Overlay) -> Result<Vec<LayoutSource>> {
    let mut out: BTreeMap<String, LayoutSource> = BTreeMap::new();
    for (file, path) in ov.list("Layout", ".szs") {
        let name = file_stem(&file).to_string();
        out.entry(name.clone()).or_insert(LayoutSource {
            name,
            origin: format!("Layout/{file}"),
            data: read(&path)?,
        });
    }
    for (pack, path) in ov.list("Pack", ".pack") {
        let data = read(&path)?;
        let Ok(sarc) = Sarc::parse(archive::unwrap(data).map_err(fmt_err(&path))?) else {
            continue;
        };
        for (member, d) in sarc.files() {
            if member.starts_with("Layout/") && member.ends_with(".szs") {
                let name = file_stem(member).to_string();
                out.entry(name.clone()).or_insert(LayoutSource {
                    name,
                    origin: format!("Pack/{pack}/{member}"),
                    data: d.to_vec(),
                });
            }
        }
    }
    Ok(out.into_values().collect())
}

pub fn run_smm1(cfg: &Smm1Config, progress: Progress) -> Result<Smm1Report> {
    let ov = Overlay::new(cfg);
    progress("identify", 0, 1);
    let base_version = title_version(&cfg.base);
    let update_version = cfg.update.as_deref().and_then(title_version);
    let build = update_version
        .clone()
        .or(base_version.clone())
        .map(|v| format!("v{v}"))
        .unwrap_or_else(|| "unknown".into());
    let out = cfg.out.clone();
    let ui = out.join("ui");
    progress("identify", 1, 1);

    let mut report = Smm1Report {
        build: build.clone(),
        out_dir: out.clone(),
        ..Default::default()
    };

    // --- layouts, animations, textures
    let sources = collect_layouts(&ov)?;
    if sources.is_empty() {
        return Err(ImportError::Invalid(format!(
            "no layouts found under {} (expected Layout/*.szs or Pack/*.pack)",
            cfg.base.display()
        )));
    }
    let mut layout_names = Vec::new();
    let mut part_refs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut fonts_used: BTreeMap<String, usize> = BTreeMap::new();
    for (i, src) in sources.iter().enumerate() {
        progress("layouts", i, sources.len());
        let ctx = PathBuf::from(&src.origin);
        let members = archive_members(src.data.clone(), &ctx)?;
        let Some((_, blyt)) = members.iter().find(|(n, _)| n.ends_with(".bflyt")) else {
            report
                .skipped
                .push(format!("{}: no .bflyt in archive", src.origin));
            continue;
        };
        let layout = match Layout::parse(blyt) {
            Ok(l) => l,
            Err(e) => {
                report.skipped.push(format!("{}: {e}", src.origin));
                continue;
            }
        };
        for p in layout.panes() {
            if let PaneKind::Part(part) = &p.kind {
                part_refs
                    .entry(part.layout.clone())
                    .or_default()
                    .push(src.name.clone());
            }
        }
        for f in &layout.fonts {
            *fonts_used.entry(file_stem(f).to_string()).or_default() += 1;
        }
        let mut anims = serde_json::Map::new();
        let mut textures = serde_json::Map::new();
        for (member, data) in &members {
            let stem = file_stem(member);
            if member.ends_with(".bflan") {
                match Animation::parse(data) {
                    Ok(a) => {
                        let key = stem
                            .strip_prefix(&format!("{}_", src.name))
                            .unwrap_or(stem)
                            .to_string();
                        anims.insert(key, lyt::json::animation_to_json(&a));
                        report.animations += 1;
                    }
                    Err(e) => report.skipped.push(format!("{}/{member}: {e}", src.origin)),
                }
            } else if member.ends_with(".bflim") {
                match Flim::parse(data).and_then(|f| f.decode()) {
                    Ok(img) => {
                        let m = to_mtex(&img);
                        write(
                            &ui.join(format!("textures/{}/{stem}.mtex", src.name)),
                            &m.to_bytes(),
                        )?;
                        textures.insert(
                            stem.to_string(),
                            json!({ "w": m.width, "h": m.height, "format": m.format.name(),
                                    "srgb": img.srgb }),
                        );
                        report.textures += 1;
                    }
                    Err(e) => report.skipped.push(format!("{}/{member}: {e}", src.origin)),
                }
            }
        }
        let doc = json!({
            "source": src.origin,
            "layout": lyt::json::layout_to_json(&layout),
            "animations": Value::Object(anims),
            "textures": Value::Object(textures),
        });
        write(
            &ui.join(format!("layouts/{}.json", src.name)),
            serde_json::to_string(&doc).unwrap().as_bytes(),
        )?;
        layout_names.push(src.name.clone());
        report.layouts += 1;
    }
    progress("layouts", sources.len(), sources.len());
    let missing_parts: Vec<&String> = part_refs
        .keys()
        .filter(|k| !layout_names.contains(k))
        .collect();

    // --- fonts
    let mut font_names = Vec::new();
    if let Some(path) = ov.file("Font/BmpFont_US.szs") {
        let data = read(&path)?;
        let sarc =
            Sarc::parse(archive::unwrap(data).map_err(fmt_err(&path))?).map_err(fmt_err(&path))?;
        let members: Vec<(&str, &[u8])> = sarc.files().collect();
        for (i, (member, data)) in members.iter().enumerate() {
            progress("fonts", i, members.len());
            let stem = file_stem(member).to_string();
            if member.ends_with(".byml") {
                if let Ok(v) = mmk_formats::byml::parse(data) {
                    write(
                        &ui.join(format!("fonts/{stem}.json")),
                        serde_json::to_string_pretty(&v).unwrap().as_bytes(),
                    )?;
                }
                continue;
            }
            if !member.ends_with(".bffnt") {
                continue;
            }
            let font = match Font::parse(data) {
                Ok(f) => f,
                Err(e) => {
                    report.skipped.push(format!("Font/{member}: {e}"));
                    continue;
                }
            };
            let mut sheets_ok = 0;
            for s in 0..font.sheets.len() {
                match font.decode_sheet(s) {
                    Ok(img) => {
                        write(
                            &ui.join(format!("fonts/{stem}/sheet_{s}.mtex")),
                            &to_mtex(&img).to_bytes(),
                        )?;
                        sheets_ok += 1;
                    }
                    Err(e) => report.skipped.push(format!("Font/{member} sheet {s}: {e}")),
                }
            }
            write(
                &ui.join(format!("fonts/{stem}.json")),
                serde_json::to_string(&font_to_json(&font, sheets_ok))
                    .unwrap()
                    .as_bytes(),
            )?;
            font_names.push(stem);
            report.fonts += 1;
        }
        progress("fonts", members.len(), members.len());
    } else {
        report.skipped.push("Font/BmpFont_US.szs: not found".into());
    }

    // --- messages
    let mut message_files = Vec::new();
    for archive_name in ["LayoutMsg", "CommonMsg"] {
        let rel = format!("Message/{archive_name}_{}.szs", cfg.language);
        let Some(path) = ov.file(&rel) else {
            report.skipped.push(format!("{rel}: not found"));
            continue;
        };
        let data = read(&path)?;
        let sarc =
            Sarc::parse(archive::unwrap(data).map_err(fmt_err(&path))?).map_err(fmt_err(&path))?;
        let members: Vec<(&str, &[u8])> = sarc.files().collect();
        for (i, (member, data)) in members.iter().enumerate() {
            progress("messages", i, members.len());
            if !member.ends_with(".msbt") {
                continue;
            }
            match Msbt::parse(data) {
                Ok(m) => {
                    let rel_out = format!(
                        "{archive_name}/{}",
                        member.strip_suffix(".msbt").unwrap_or(member)
                    );
                    write(
                        &ui.join(format!("messages/{rel_out}.json")),
                        serde_json::to_string(&m.to_json()).unwrap().as_bytes(),
                    )?;
                    message_files.push(rel_out);
                    report.messages += 1;
                }
                Err(e) => report.skipped.push(format!("{rel}/{member}: {e}")),
            }
        }
    }

    // --- costumes (Mystery Mushroom art and sounds)
    let c = crate::smm1_costumes::import_costumes(&ov, &out, &cfg.language, progress)?;
    report.costumes = c.costumes;
    report.costume_textures = c.textures;
    report.costume_sounds = c.sounds;
    report
        .skipped
        .extend(c.skipped.into_iter().map(|s| format!("costumes: {s}")));

    // --- manifest
    let manifest = json!({
        "importer_version": SMM1_IMPORTER_VERSION,
        "game": "SMM1",
        "build": build,
        "base_title_version": base_version,
        "update_title_version": update_version,
        "language": cfg.language,
        "stages": ["identify", "layouts", "fonts", "messages", "costumes"],
        "costumes": report.costumes,
        "layouts": layout_names,
        "animations": report.animations,
        "textures": report.textures,
        "fonts": font_names,
        "fonts_used_by_layouts": fonts_used,
        "messages": message_files,
        "missing_part_layouts": missing_parts,
        "skipped": report.skipped,
    });
    write(
        &out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap().as_bytes(),
    )?;
    progress("done", 1, 1);
    Ok(report)
}

pub fn font_to_json(f: &Font, sheets: usize) -> Value {
    let w = |c: mmk_formats::bffnt::CharWidth| json!([c.left, c.glyph_width, c.char_width]);
    json!({
        "version": f.version,
        "height": f.height,
        "width": f.width,
        "ascent": f.ascent,
        "line_feed": f.line_feed,
        "alter_char_index": f.alter_char_index,
        "default_width": w(f.default_width),
        "cell": [f.cell_width, f.cell_height],
        "baseline": f.baseline,
        "max_char_width": f.max_char_width,
        "cells_per_row": f.cells_per_row,
        "cells_per_column": f.cells_per_column,
        "sheet_size": [f.sheet_width, f.sheet_height],
        "sheet_format": f.sheet_format,
        "sheets": sheets,
        // [glyph, left, glyph width, advance]
        "widths": f.widths.iter().map(|(g, c)| json!([g, c.left, c.glyph_width, c.char_width])).collect::<Vec<_>>(),
        // [code point, glyph]
        "cmap": f.cmap.iter().map(|(c, g)| json!([c, g])).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn sarc(files: &[(&str, &[u8])]) -> Vec<u8> {
        // minimal big-endian SARC writer (names stored, no hash ordering requirement for reading)
        let mut names = Vec::new();
        let mut name_offs = Vec::new();
        for (n, _) in files {
            name_offs.push(names.len() / 4);
            names.extend_from_slice(n.as_bytes());
            names.push(0);
            while names.len() % 4 != 0 {
                names.push(0);
            }
        }
        let mut data = Vec::new();
        let mut ranges = Vec::new();
        for (_, d) in files {
            while data.len() % 8 != 0 {
                data.push(0);
            }
            let s = data.len();
            data.extend_from_slice(d);
            ranges.push((s, data.len()));
        }
        let sfat = 0x14;
        let nodes = sfat + 0xC;
        let sfnt = nodes + files.len() * 16;
        let names_at = sfnt + 8;
        let mut data_off = names_at + names.len();
        data_off = data_off.div_ceil(8) * 8;
        let mut out = Vec::new();
        out.extend_from_slice(b"SARC");
        out.extend_from_slice(&0x14u16.to_be_bytes());
        out.extend_from_slice(&[0xFE, 0xFF]);
        out.extend_from_slice(&((data_off + data.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(data_off as u32).to_be_bytes());
        out.extend_from_slice(&[1, 0, 0, 0]);
        out.extend_from_slice(b"SFAT");
        out.extend_from_slice(&0xCu16.to_be_bytes());
        out.extend_from_slice(&(files.len() as u16).to_be_bytes());
        out.extend_from_slice(&0x65u32.to_be_bytes());
        for (i, (s, e)) in ranges.iter().enumerate() {
            out.extend_from_slice(&(i as u32).to_be_bytes());
            out.extend_from_slice(&(0x0100_0000u32 | name_offs[i] as u32).to_be_bytes());
            out.extend_from_slice(&(*s as u32).to_be_bytes());
            out.extend_from_slice(&(*e as u32).to_be_bytes());
        }
        out.extend_from_slice(b"SFNT");
        out.extend_from_slice(&8u16.to_be_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&names);
        out.resize(data_off, 0);
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn imports_a_synthetic_content_folder() {
        let tmp = std::env::temp_dir().join(format!("mmk-smm1-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let base = tmp.join("base/content");
        let upd = tmp.join("upd/content");
        fs::create_dir_all(base.join("Layout")).unwrap();
        fs::create_dir_all(upd.join("Pack")).unwrap();
        fs::create_dir_all(tmp.join("upd/meta")).unwrap();
        fs::write(
            tmp.join("upd/meta/meta.xml"),
            "<menu><title_version type=\"unsignedInt\" length=\"4\">208</title_version></menu>",
        )
        .unwrap();
        let lyt = mmk_formats::fixtures::sample_bflyt();
        let anim = mmk_formats::fixtures::sample_bflan();
        let tex = mmk_formats::bflim::build(&[0x80; 8 * 8 * 4], 8, 8, 9, 2, 0);
        let arc = sarc(&[
            ("blyt/Sample.bflyt", &lyt),
            ("anim/Sample_Move.bflan", &anim),
            ("timg/Box^s.bflim", &tex),
        ]);
        let szs = sarc(&[("Sample.arc", &arc)]);
        fs::write(base.join("Layout/Sample.szs"), &szs).unwrap();
        // the same layout inside a pack in the update, renamed, to check pack discovery
        let pack = sarc(&[("Layout/Other_00.szs", &szs)]);
        fs::write(upd.join("Pack/Ui.pack"), &pack).unwrap();

        let cfg = Smm1Config {
            base: base.clone(),
            update: Some(upd.clone()),
            out: tmp.join("out"),
            language: "USen".into(),
        };
        let r = run_smm1(&cfg, &mut |_, _, _| {}).unwrap();
        assert_eq!(r.build, "v208");
        assert_eq!(r.layouts, 2);
        assert_eq!(r.textures, 2);
        let doc: Value =
            serde_json::from_slice(&fs::read(tmp.join("out/ui/layouts/Sample.json")).unwrap())
                .unwrap();
        assert!(doc["animations"]["Move"].is_object());
        assert_eq!(doc["textures"]["Box^s"]["w"], 8);
        let l = lyt::json::layout_from_json(&doc["layout"]).unwrap();
        assert_eq!(l.name, "Sample");
        let m =
            Mtex::parse(&fs::read(tmp.join("out/ui/textures/Sample/Box^s.mtex")).unwrap()).unwrap();
        assert_eq!(m.data, vec![0x80; 256]);
        let manifest: Value =
            serde_json::from_slice(&fs::read(tmp.join("out/manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["layouts"], json!(["Other_00", "Sample"]));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn no_layouts_is_an_error() {
        let tmp = std::env::temp_dir().join(format!("mmk-smm1-empty-{}", std::process::id()));
        fs::create_dir_all(&tmp).unwrap();
        let cfg = Smm1Config {
            base: tmp.clone(),
            update: None,
            out: tmp.join("out"),
            language: "USen".into(),
        };
        assert!(run_smm1(&cfg, &mut |_, _, _| {}).is_err());
        let _ = fs::remove_dir_all(&tmp);
    }

    /// Runs against the owner's dump when present (skipped in CI).
    #[test]
    fn owner_dump_layouts_parse() {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let base = home.join("smm-work/extracted/smm1-disc/GM000500001018DC00000000000100/content");
        let upd = home.join("smm-work/extracted/smm1-update-v208/content");
        if !base.is_dir() {
            eprintln!("SMM1 dump not present; skipping");
            return;
        }
        let ov = Overlay::new(&Smm1Config {
            base,
            update: upd.is_dir().then_some(upd),
            out: PathBuf::new(),
            language: "USen".into(),
        });
        let sources = collect_layouts(&ov).unwrap();
        assert!(sources.len() > 300, "found {}", sources.len());
        let src = sources
            .iter()
            .find(|s| s.name == "Nwk_WorldMapEsy_00")
            .expect("world map layout");
        let members = archive_members(src.data.clone(), Path::new("x")).unwrap();
        let (_, blyt) = members.iter().find(|(n, _)| n.ends_with(".bflyt")).unwrap();
        let l = Layout::parse(blyt).unwrap();
        assert_eq!((l.width, l.height), (1280.0, 720.0));
        assert!(l.root.find("L_StartBtn_00").is_some());
    }
}
