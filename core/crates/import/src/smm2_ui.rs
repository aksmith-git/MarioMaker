//! SMM2 UI import: the owner's extracted RomFS → layouts, animations, textures, fonts and
//! messages for the layout player, in the same cache form as the SMM1 UI import
//! (`super::smm1`), so `mmk_ui` and the app load either.
//!
//! ```text
//! manifest.json                         game "SMM2", build, counts, layout / font / message lists
//! ui/layouts/<Layout>.json              { layout, animations: {name: anim}, textures: {name: info} }
//! ui/textures/<Layout>/<name>.mtex      every texture of the layout's `timg/__Combined.bntx`
//! ui/fonts/<Font>.json, <Font>/sheet_<n>.mtex
//!                                       bitmap fonts (Font/BmpFont_<region>.szs, BFFNT v4) and
//!                                       the scalable-font combinations (Font/Fcpx.szs +
//!                                       Font/ScalableFont.szs), rasterised (`super::smm2_font`)
//! ui/messages/<LayoutMsg|CommonMsg|ReferenceMsg>/<File>.json   MSBT tables of the chosen
//!                                       language (ReferenceMsg: clear conditions, part names …)
//! ui/textures/MissionIcon/<name>.mtex   the clear-condition icons of every game style
//!                                       (`Model/<style>_Lyt_Mission_RGBA8`, BFRES textures)
//! ui/textures/GameSkin/<name>.mtex      the game styles' logos (`Model/<style>_Lyt_Logo`; the
//!                                       game puts them into `Cmn_GameSkin_00`, e.g. Course
//!                                       World's course rows)
//! ui/sound/sounds.json, <sound>.wav     the layouts' button and animation sounds
//!                                       (`super::smm2_ui_sound`)
//! courses/title_course_data_<n>.bcd     the title screen's demo courses (as in the RomFS)
//! ```
//! Layout archives: `Layout/*.szs` (zstd/Yaz0 → SARC → `<Name>.arc` SARC with `blyt/`,
//! `anim/`, `timg/__Combined.bntx`, `fcpx/`), plus `Layout/*.szs` members of `Pack/*.pack` not
//! found loose.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use ab_glyph::FontVec;
use mmk_assets::Mtex;
use mmk_formats::archive::{self, Sarc};
use mmk_formats::bfcpx::{self, Combination};
use mmk_formats::bffnt::Font;
use mmk_formats::bntx;
use mmk_formats::lyt::{self, Animation, Layout, PaneKind};
use mmk_formats::msbt::Msbt;
use serde_json::{Value, json};

use crate::smm1::{font_to_json, to_mtex};
use crate::{ImportError, Progress, Result, read, write};

pub const SMM2_UI_IMPORTER_VERSION: u32 = 4;

#[derive(Clone, Debug)]
pub struct Smm2UiConfig {
    /// Extracted RomFS (with the update applied).
    pub romfs: PathBuf,
    /// Extracted ExeFS, to name the build (optional).
    pub exefs: Option<PathBuf>,
    /// Output folder (e.g. `~/smm-work/cache/smm2-ui-3.0.3`).
    pub out: PathBuf,
    /// Message language, e.g. `USen`.
    pub language: String,
}

#[derive(Clone, Debug, Default)]
pub struct Smm2UiReport {
    pub build: String,
    pub out_dir: PathBuf,
    pub layouts: usize,
    pub animations: usize,
    pub textures: usize,
    pub fonts: usize,
    pub messages: usize,
    /// Files that could not be converted (path: reason).
    pub skipped: Vec<String>,
}

fn fmt_err(path: &Path) -> impl Fn(mmk_formats::Error) -> ImportError + '_ {
    move |source| ImportError::Format {
        path: path.to_path_buf(),
        source,
    }
}

fn file_stem(name: &str) -> &str {
    let file = name.rsplit('/').next().unwrap_or(name);
    file.split('.').next().unwrap_or(file)
}

/// Members of an archive, nested SARCs (`.arc`, or any member that is one) flattened.
pub(crate) fn archive_members(data: Vec<u8>, ctx: &Path) -> Result<Vec<(String, Vec<u8>)>> {
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

pub(crate) struct LayoutSource {
    pub name: String,
    pub origin: String,
    pub data: Vec<u8>,
}

fn sorted_files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = crate::read_dir(dir)
        .map(|paths| {
            paths
                .into_iter()
                .filter(|p| crate::is_file(p) && p.to_string_lossy().ends_with(ext))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Every layout archive: loose `Layout/*.szs`, then `Layout/*.szs` members of each pack.
pub(crate) fn collect_layouts(romfs: &Path) -> Result<Vec<LayoutSource>> {
    let mut out: BTreeMap<String, LayoutSource> = BTreeMap::new();
    for path in sorted_files(&romfs.join("Layout"), ".szs") {
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        let name = file_stem(&file).to_string();
        out.entry(name.clone()).or_insert(LayoutSource {
            name,
            origin: format!("Layout/{file}"),
            data: read(&path)?,
        });
    }
    for path in sorted_files(&romfs.join("Pack"), ".pack") {
        let pack = path.file_name().unwrap().to_string_lossy().to_string();
        if !pack.starts_with("Layout") {
            continue;
        }
        let Ok(sarc) = Sarc::parse(archive::unwrap(read(&path)?).map_err(fmt_err(&path))?) else {
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

fn bntx_to_mtex(t: &bntx::Texture) -> Option<Mtex> {
    use mmk_assets::{MtexFormat as F, MtexVariant as V};
    use mmk_formats::bntx::{PixelFormat as P, Variant};
    Some(Mtex {
        format: match t.format {
            P::Rgba8 => F::Rgba8,
            P::Bc1 => F::Bc1,
            P::Bc2 => F::Bc2,
            P::Bc3 => F::Bc3,
            P::Bc4 => F::Bc4,
            P::Bc5 => F::Bc5,
            P::Bc6h => F::Bc6h,
            P::Bc7 => F::Bc7,
            P::Other(_) => return None,
        },
        variant: match t.variant {
            Variant::Srgb => V::Srgb,
            Variant::Snorm => V::Snorm,
            _ => V::Unorm,
        },
        width: t.width as u32,
        height: t.height as u32,
        channel_select: t.channel_select,
        data: t.data.clone(),
    })
}

/// Messages of `archive` (`LayoutMsg` / `CommonMsg` / `ReferenceMsg`) in `language`: member
/// path → table.
fn messages(romfs: &Path, archive_name: &str, language: &str) -> Result<Vec<(String, Msbt)>> {
    let path = romfs.join(format!("Message/{archive_name}_{language}.szs"));
    let sarc = Sarc::parse(archive::unwrap(read(&path)?).map_err(fmt_err(&path))?)
        .map_err(fmt_err(&path))?;
    let mut out = Vec::new();
    for (member, data) in sarc.files() {
        if let Some(stem) = member.strip_suffix(".msbt")
            && let Ok(m) = Msbt::parse(data)
        {
            out.push((stem.to_string(), m));
        }
    }
    Ok(out)
}

/// Characters the baked fonts need: printable ASCII, Latin-1, and everything in the messages.
fn charset(tables: &[(String, Msbt)]) -> BTreeSet<char> {
    let mut set: BTreeSet<char> = (0x20u8..0x7F).map(char::from).collect();
    set.extend((0xA0u32..=0xFF).filter_map(char::from_u32));
    for (_, m) in tables {
        for msg in &m.messages {
            set.extend(msg.plain().chars().filter(|c| !c.is_control()));
        }
    }
    set
}

/// The decrypted scalable fonts of `Font/ScalableFont.szs`, by file name.
fn scalable_fonts(romfs: &Path, report: &mut Smm2UiReport) -> Result<HashMap<String, FontVec>> {
    let path = romfs.join("Font/ScalableFont.szs");
    let mut out = HashMap::new();
    let Ok(data) = read(&path) else {
        report
            .skipped
            .push("Font/ScalableFont.szs: not found".into());
        return Ok(out);
    };
    for (member, d) in archive_members(data, &path)? {
        let file = member.rsplit('/').next().unwrap_or(&member).to_string();
        match bfcpx::decrypt_scalable(&d)
            .map_err(|e| e.to_string())
            .and_then(|plain| FontVec::try_from_vec(plain).map_err(|e| e.to_string()))
        {
            Ok(f) => {
                out.insert(file, f);
            }
            Err(e) => report.skipped.push(format!("Font/{member}: {e}")),
        }
    }
    Ok(out)
}

fn region(language: &str) -> &'static str {
    match language.get(..2) {
        Some("EU") => "EU",
        Some("JP") => "JP",
        Some("KR") => "KR",
        Some("CN") => "CN",
        Some("TW") => "TW",
        _ => "US",
    }
}

/// Texture group (`ui/textures/<group>/`) of the clear-condition icons.
pub const MISSION_ICONS: &str = "MissionIcon";
/// Texture group of the game styles' logos.
pub const GAME_SKIN_ICONS: &str = "GameSkin";

/// Layout icon models per game style, and the texture group each goes to: the clear-condition
/// icons and the style logos.
const ICON_MODELS: [(&str, &str); 2] = [
    ("_Lyt_Mission_RGBA8", MISSION_ICONS),
    ("_Lyt_Logo", GAME_SKIN_ICONS),
];

/// The icon models `<style><suffix>` of the five styles: loose in `Model/`, else members of
/// `Pack/*.pack` (v3.0.3 keeps them in `StaticModel.pack`).
fn icon_models(romfs: &Path, suffix: &str) -> Vec<(String, Vec<u8>)> {
    let wanted: Vec<String> = ["M1", "M3", "MW", "WU", "3W"]
        .iter()
        .map(|s| format!("{s}{suffix}.Nin_NX_NVN.zs"))
        .collect();
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    for w in &wanted {
        if let Ok(d) = read(&romfs.join("Model").join(w)) {
            out.push((format!("Model/{w}"), d));
        }
    }
    for path in sorted_files(&romfs.join("Pack"), ".pack") {
        if out.len() == wanted.len() {
            break;
        }
        let Ok(data) = read(&path) else { continue };
        let Ok(sarc) = archive::unwrap(data).and_then(Sarc::parse) else {
            continue;
        };
        for (member, d) in sarc.files() {
            let file = member.rsplit('/').next().unwrap_or(member);
            if wanted.iter().any(|w| w == file) && !out.iter().any(|(n, _)| n.ends_with(file)) {
                let pack = path.file_name().unwrap().to_string_lossy();
                out.push((format!("Pack/{pack}/{member}"), d.to_vec()));
            }
        }
    }
    out
}

pub fn run_smm2_ui(cfg: &Smm2UiConfig, progress: Progress) -> Result<Smm2UiReport> {
    progress("identify", 0, 1);
    let build = match &cfg.exefs {
        Some(e) => crate::identify(e)?.1,
        None => "unknown".into(),
    };
    let out = cfg.out.clone();
    let ui = out.join("ui");
    let mut report = Smm2UiReport {
        build: build.clone(),
        out_dir: out.clone(),
        ..Default::default()
    };
    progress("identify", 1, 1);

    // --- layouts, animations, textures
    let sources = collect_layouts(&cfg.romfs)?;
    if sources.is_empty() {
        return Err(ImportError::Invalid(format!(
            "no layouts found under {} (expected Layout/*.szs)",
            cfg.romfs.display()
        )));
    }
    let mut layout_names = Vec::new();
    let mut part_refs: BTreeSet<String> = BTreeSet::new();
    let mut fonts_used: BTreeMap<String, usize> = BTreeMap::new();
    for (i, src) in sources.iter().enumerate() {
        progress("layouts", i, sources.len());
        let ctx = PathBuf::from(&src.origin);
        let members = match archive_members(src.data.clone(), &ctx) {
            Ok(m) => m,
            Err(e) => {
                report.skipped.push(format!("{}: {e}", src.origin));
                continue;
            }
        };
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
                part_refs.insert(part.layout.clone());
            }
        }
        for f in &layout.fonts {
            *fonts_used.entry(file_stem(f).to_string()).or_default() += 1;
        }
        let mut anims = serde_json::Map::new();
        let mut textures = serde_json::Map::new();
        for (member, data) in &members {
            if member.ends_with(".bflan") {
                let stem = file_stem(member);
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
            } else if member.ends_with(".bntx") {
                let texs = match bntx::parse(data) {
                    Ok(t) => t,
                    Err(e) => {
                        report.skipped.push(format!("{}/{member}: {e}", src.origin));
                        continue;
                    }
                };
                for t in &texs {
                    let Some(m) = bntx_to_mtex(t) else {
                        report.skipped.push(format!(
                            "{}/{member}/{}: format {:?}",
                            src.origin, t.name, t.format
                        ));
                        continue;
                    };
                    write(
                        &ui.join(format!("textures/{}/{}.mtex", src.name, t.name)),
                        &m.to_bytes(),
                    )?;
                    textures.insert(
                        t.name.clone(),
                        json!({ "w": m.width, "h": m.height, "format": m.format.name(),
                                "srgb": m.variant == mmk_assets::MtexVariant::Srgb }),
                    );
                    report.textures += 1;
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
        .iter()
        .filter(|k| !layout_names.contains(k))
        .collect();

    // --- messages (read first: the baked fonts cover their characters)
    let mut tables = Vec::new();
    let mut message_files = Vec::new();
    for archive_name in ["LayoutMsg", "CommonMsg", "ReferenceMsg"] {
        match messages(&cfg.romfs, archive_name, &cfg.language) {
            Ok(list) => {
                for (i, (stem, m)) in list.iter().enumerate() {
                    progress("messages", i, list.len());
                    let rel = format!("{archive_name}/{stem}");
                    write(
                        &ui.join(format!("messages/{rel}.json")),
                        serde_json::to_string(&m.to_json()).unwrap().as_bytes(),
                    )?;
                    message_files.push(rel);
                    report.messages += 1;
                }
                tables.extend(list);
            }
            Err(e) => report
                .skipped
                .push(format!("Message/{archive_name}_{}: {e}", cfg.language)),
        }
    }

    // --- bitmap fonts
    let mut font_names = Vec::new();
    let bmp = format!("Font/BmpFont_{}.szs", region(&cfg.language));
    match read(&cfg.romfs.join(&bmp)) {
        Ok(data) => {
            let members = archive_members(data, Path::new(&bmp))?;
            for (i, (member, data)) in members.iter().enumerate() {
                progress("fonts", i, members.len());
                if !member.ends_with(".bffnt") {
                    continue;
                }
                let stem = file_stem(member).to_string();
                let font = match Font::parse(data) {
                    Ok(f) => f,
                    Err(e) => {
                        report.skipped.push(format!("{bmp}/{member}: {e}"));
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
                        Err(e) => report
                            .skipped
                            .push(format!("{bmp}/{member} sheet {s}: {e}")),
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
        }
        Err(_) => report.skipped.push(format!("{bmp}: not found")),
    }

    // --- scalable fonts → baked bitmap fonts
    let chars = charset(&tables);
    let sources_fonts = scalable_fonts(&cfg.romfs, &mut report)?;
    let mut baked_info = serde_json::Map::new();
    let fcpx_path = cfg.romfs.join("Font/Fcpx.szs");
    match read(&fcpx_path) {
        Ok(data) => {
            let members = archive_members(data, &fcpx_path)?;
            for (i, (member, data)) in members.iter().enumerate() {
                progress("scalable fonts", i, members.len());
                let stem = file_stem(member).to_string();
                // the region variants (MessageFont_CNzh …) only matter for those languages
                if !member.ends_with(".bfcpx") || stem.contains('_') && stem != "NX_std" {
                    continue;
                }
                let comb = match Combination::parse(data) {
                    Ok(c) => c,
                    Err(e) => {
                        report.skipped.push(format!("Font/Fcpx.szs/{member}: {e}"));
                        continue;
                    }
                };
                let baked = crate::smm2_font::bake(&comb, &sources_fonts, &chars);
                for (s, sheet) in baked.sheets.iter().enumerate() {
                    write(
                        &ui.join(format!("fonts/{stem}/sheet_{s}.mtex")),
                        &sheet.to_bytes(),
                    )?;
                }
                let mut doc = font_to_json(&baked.font, baked.sheets.len());
                doc["source"] = json!({
                    "combination": member,
                    "size": comb.size,
                    "fonts": comb.fonts.iter().map(|f| f.file.clone()).collect::<Vec<_>>(),
                    "glyphs_from": baked.sources,
                });
                write(
                    &ui.join(format!("fonts/{stem}.json")),
                    serde_json::to_string(&doc).unwrap().as_bytes(),
                )?;
                baked_info.insert(
                    stem.clone(),
                    json!({
                        "glyphs": baked.font.cmap.len(),
                        "missing": baked.missing.iter().map(|c| format!("U+{:04X}", *c as u32)).collect::<Vec<_>>(),
                    }),
                );
                font_names.push(stem);
                report.fonts += 1;
            }
        }
        Err(_) => report.skipped.push("Font/Fcpx.szs: not found".into()),
    }

    // --- the title screen's demo courses (still encrypted; the engine decrypts them with the
    // asset cache's key table), so the title has them even next to an asset cache without
    // its built-in courses
    let mut title_courses = Vec::new();
    for path in sorted_files(&cfg.romfs.join("Course"), ".szs") {
        let stem = file_stem(&path.file_name().unwrap().to_string_lossy()).to_string();
        if !stem.starts_with("title_course_data_") {
            continue;
        }
        match archive_members(read(&path)?, &path) {
            Ok(members) => {
                if let Some((_, bcd)) = members.iter().find(|(n, _)| n.ends_with(".bcd")) {
                    write(&out.join(format!("courses/{stem}.bcd")), bcd)?;
                    title_courses.push(stem);
                }
            }
            Err(e) => report.skipped.push(format!("Course/{stem}: {e}")),
        }
    }

    // --- clear-condition icons (the HUD and pause menu put one into Cmn_MissionIcon's picture)
    // and the style logos (Cmn_GameSkin_00)
    let mut mission_icons = 0usize;
    let mut skin_icons = 0usize;
    let models: Vec<(&str, String, Vec<u8>)> = ICON_MODELS
        .iter()
        .flat_map(|(suffix, group)| {
            icon_models(&cfg.romfs, suffix)
                .into_iter()
                .map(move |(n, d)| (*group, n, d))
        })
        .collect();
    for (group, name, data) in models {
        progress("condition icons", mission_icons + skin_icons, 0);
        let ctx = PathBuf::from(&name);
        let blobs: Vec<Vec<u8>> = match archive::unwrap(data) {
            Ok(d) => match Sarc::parse(d.clone()) {
                Ok(s) => s.files().map(|(_, b)| b.to_vec()).collect(),
                Err(_) => vec![d],
            },
            Err(e) => {
                report.skipped.push(format!("{}: {e}", ctx.display()));
                continue;
            }
        };
        for blob in blobs {
            let Some(b) = bntx::find_embedded(&blob) else {
                continue;
            };
            match bntx::parse(b) {
                Ok(texs) => {
                    for t in &texs {
                        let Some(m) = bntx_to_mtex(t) else {
                            report
                                .skipped
                                .push(format!("{name}/{}: format {:?}", t.name, t.format));
                            continue;
                        };
                        write(
                            &ui.join(format!("textures/{group}/{}.mtex", t.name)),
                            &m.to_bytes(),
                        )?;
                        if group == MISSION_ICONS {
                            mission_icons += 1;
                        } else {
                            skin_icons += 1;
                        }
                        report.textures += 1;
                    }
                }
                Err(e) => report.skipped.push(format!("{name}: {e}")),
            }
        }
    }

    // --- the layouts' sounds (button / animation → sound, from the game's data)
    progress("ui sounds", 0, 1);
    let sound = crate::smm2_ui_sound::run(&cfg.romfs, &ui)?;
    report.skipped.extend(sound.skipped.iter().cloned());
    progress("ui sounds", 1, 1);

    // --- manifest
    let manifest = json!({
        "importer_version": SMM2_UI_IMPORTER_VERSION,
        "game": "SMM2",
        "build": build,
        "language": cfg.language,
        "stages": ["identify", "layouts", "messages", "fonts", "scalable fonts", "condition icons"],
        "layouts": layout_names,
        "animations": report.animations,
        "textures": report.textures,
        "fonts": font_names,
        "fonts_used_by_layouts": fonts_used,
        "baked_fonts": baked_info,
        "messages": message_files,
        "title_courses": title_courses,
        "mission_icons": mission_icons,
        "game_skin_icons": skin_icons,
        "ui_sounds": {"layouts": sound.layouts, "users": sound.users, "sounds": sound.sounds},
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Minimal little-endian SARC writer (names stored; readers don't need hash order).
    fn sarc(files: &[(&str, &[u8])]) -> Vec<u8> {
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
        let nodes = 0x14 + 0xC;
        let sfnt = nodes + files.len() * 16;
        let data_off = (sfnt + 8 + names.len()).div_ceil(8) * 8;
        let mut out = Vec::new();
        out.extend_from_slice(b"SARC");
        out.extend_from_slice(&0x14u16.to_le_bytes());
        out.extend_from_slice(&[0xFF, 0xFE]);
        out.extend_from_slice(&((data_off + data.len()) as u32).to_le_bytes());
        out.extend_from_slice(&(data_off as u32).to_le_bytes());
        out.extend_from_slice(&[0, 1, 0, 0]);
        out.extend_from_slice(b"SFAT");
        out.extend_from_slice(&0xCu16.to_le_bytes());
        out.extend_from_slice(&(files.len() as u16).to_le_bytes());
        out.extend_from_slice(&0x65u32.to_le_bytes());
        for (i, (s, e)) in ranges.iter().enumerate() {
            out.extend_from_slice(&(i as u32).to_le_bytes());
            out.extend_from_slice(&(0x0100_0000u32 | name_offs[i] as u32).to_le_bytes());
            out.extend_from_slice(&(*s as u32).to_le_bytes());
            out.extend_from_slice(&(*e as u32).to_le_bytes());
        }
        out.extend_from_slice(b"SFNT");
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&names);
        out.resize(data_off, 0);
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn imports_a_synthetic_romfs() {
        let tmp = std::env::temp_dir().join(format!("mmk-smm2ui-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let romfs = tmp.join("romfs");
        fs::create_dir_all(romfs.join("Layout")).unwrap();
        fs::create_dir_all(romfs.join("Message")).unwrap();
        let lyt = mmk_formats::fixtures::sample_bflyt_nx();
        let anim = mmk_formats::fixtures::sample_bflan_nx();
        let px = vec![0x40u8; 8 * 8 * 4];
        let tex = mmk_formats::fixtures::sample_bntx("Box^s", 0x0B01, 8, 8, 4, [2, 3, 4, 5], &[px]);
        let arc = sarc(&[
            ("blyt/SampleNx.bflyt", &lyt),
            ("anim/SampleNx_Fade.bflan", &anim),
            ("timg/__Combined.bntx", &tex),
        ]);
        let szs = sarc(&[("SampleNx.arc", &arc)]);
        fs::write(romfs.join("Layout/SampleNx.Nin_NX_NVN.szs"), &szs).unwrap();
        let msbt = mmk_formats::fixtures::sample_msbt();
        let msg = sarc(&[("SampleNx.msbt", &msbt)]);
        fs::write(romfs.join("Message/LayoutMsg_USen.szs"), &msg).unwrap();

        let cfg = Smm2UiConfig {
            romfs: romfs.clone(),
            exefs: None,
            out: tmp.join("out"),
            language: "USen".into(),
        };
        let r = run_smm2_ui(&cfg, &mut |_, _, _| {}).unwrap();
        assert_eq!(
            (r.layouts, r.animations, r.textures, r.messages),
            (1, 1, 1, 1)
        );
        let doc: Value =
            serde_json::from_slice(&fs::read(tmp.join("out/ui/layouts/SampleNx.json")).unwrap())
                .unwrap();
        assert!(doc["animations"]["Fade"].is_object());
        assert_eq!(doc["textures"]["Box^s"]["w"], 8);
        let m = Mtex::parse(&fs::read(tmp.join("out/ui/textures/SampleNx/Box^s.mtex")).unwrap())
            .unwrap();
        assert_eq!(m.data, vec![0x40; 256]);
        assert!(
            tmp.join("out/ui/messages/LayoutMsg/SampleNx.json")
                .is_file()
        );
        let manifest: Value =
            serde_json::from_slice(&fs::read(tmp.join("out/manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["game"], "SMM2");
        assert_eq!(manifest["layouts"], json!(["SampleNx"]));
        assert_eq!(manifest["missing_part_layouts"], json!(["Sample"]));
        let _ = fs::remove_dir_all(&tmp);
    }

    fn owner_romfs() -> Option<PathBuf> {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        [
            home.join("smm-work/extracted/smm2-v303-romfs"),
            PathBuf::from("/Volumes/SSD/smm-work/extracted/smm2-update-v303/romfs"),
        ]
        .into_iter()
        .find(|p| p.join("Layout").is_dir())
    }

    /// Every SMM2 layout and animation parses (owner's dump; skipped when absent).
    #[test]
    fn owner_dump_layouts_parse() {
        let Some(romfs) = owner_romfs() else {
            eprintln!("SMM2 RomFS not present; skipping");
            return;
        };
        let sources = collect_layouts(&romfs).unwrap();
        assert!(sources.len() > 500, "found {}", sources.len());
        let mut layouts = 0;
        let mut anims = 0;
        let mut textures = 0;
        for src in &sources {
            for (member, data) in archive_members(src.data.clone(), Path::new(&src.origin)).unwrap()
            {
                if member.ends_with(".bflyt") {
                    let l = Layout::parse(&data).unwrap_or_else(|e| panic!("{}: {e}", src.origin));
                    assert_eq!(l.version, 0x0900_0000);
                    layouts += 1;
                } else if member.ends_with(".bflan") {
                    Animation::parse(&data).unwrap_or_else(|e| panic!("{member}: {e}"));
                    anims += 1;
                } else if member.ends_with(".bntx") {
                    for t in bntx::parse(&data).unwrap() {
                        assert!(bntx_to_mtex(&t).is_some(), "{}: {:?}", t.name, t.format);
                        textures += 1;
                    }
                }
            }
        }
        assert!(
            layouts > 500 && anims > 2000 && textures > 3000,
            "{layouts} {anims} {textures}"
        );
        // the title screen, as the app uses it
        let title = sources.iter().find(|s| s.name == "Play_Title_00").unwrap();
        let members = archive_members(title.data.clone(), Path::new("x")).unwrap();
        let (_, blyt) = members.iter().find(|(n, _)| n.ends_with(".bflyt")).unwrap();
        let l = Layout::parse(blyt).unwrap();
        assert!(l.root.find("L_QuestBtn_00").is_some());
        assert_eq!(l.groups.len(), 1);
        assert_eq!(l.groups[0].children[0].name, "G_Btn_00");
    }

    /// The scalable fonts decrypt and the combinations parse (owner's dump; skipped when absent).
    #[test]
    fn owner_dump_fonts() {
        let Some(romfs) = owner_romfs() else {
            eprintln!("SMM2 RomFS not present; skipping");
            return;
        };
        let mut report = Smm2UiReport::default();
        let fonts = scalable_fonts(&romfs, &mut report).unwrap();
        assert!(report.skipped.is_empty(), "{:?}", report.skipped);
        assert!(fonts.contains_key("MARIOMAKER.bfotf"));
        let path = romfs.join("Font/Fcpx.szs");
        let members = archive_members(read(&path).unwrap(), &path).unwrap();
        let (_, special) = members
            .iter()
            .find(|(n, _)| n.ends_with("SpecialFont.bfcpx"))
            .unwrap();
        let comb = Combination::parse(special).unwrap();
        let chars: BTreeSet<char> = "Story Mode 0123".chars().collect();
        let baked = crate::smm2_font::bake(&comb, &fonts, &chars);
        assert!(baked.missing.is_empty(), "{:?}", baked.missing);
        assert_eq!(baked.font.width as f32, comb.size.round());
        let bmp = romfs.join("Font/BmpFont_US.szs");
        for (member, data) in archive_members(read(&bmp).unwrap(), &bmp).unwrap() {
            let f = Font::parse(&data).unwrap_or_else(|e| panic!("{member}: {e}"));
            assert!(f.is_nx());
            f.decode_sheet(0)
                .unwrap_or_else(|e| panic!("{member}: {e}"));
        }
    }
}
