//! The UI asset cache: layouts (with their animations and textures), fonts and messages,
//! loaded on demand from an import's files (see `mmk_import::smm1` / `smm2_ui` for the layout):
//! a folder on disk, or files a host hands over ([`crate::files`]), whose textures may arrive
//! later ([`UiCache::take_requests`] / [`UiCache::provide`]); or registered in memory (tests,
//! tools).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::files::{DirFiles, Fetch, UiFiles};

use mmk_assets::Mtex;
use mmk_formats::lyt::{self, Animation, Layout, Pane, PaneKind};
use serde_json::Value;

use crate::font::FontRes;

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("layout '{0}' not found")]
    NoLayout(String),
}

/// One texture known to the cache. Indices into `UiCache::textures` are stable for the
/// lifetime of the cache, so a renderer can upload each one once.
#[derive(Clone, Debug)]
pub struct TextureEntry {
    /// `<layout>/<texture stem>` or `font:<font>/<sheet>`.
    pub name: String,
    pub texture: Mtex,
    /// False while the pixels are on their way ([`UiCache::take_requests`]): `texture` is then
    /// an empty stand-in of the right size (0 × 0 when not known), not to be drawn.
    pub ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TexRef {
    pub id: u32,
    pub width: u32,
    pub height: u32,
}

/// A flattened pane: its base values (children removed) and tree links.
#[derive(Clone, Debug)]
pub struct Node {
    pub pane: Pane,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
}

/// Everything needed to play one layout.
#[derive(Clone, Debug)]
pub struct LayoutRes {
    pub name: String,
    pub layout: Layout,
    pub animations: BTreeMap<String, Animation>,
    /// Panes in depth-first order (parents first); index 0 is the root.
    pub nodes: Vec<Node>,
    pub pane_index: HashMap<String, usize>,
    pub material_index: HashMap<String, usize>,
    /// Per `layout.textures` entry.
    pub textures: Vec<Option<TexRef>>,
    /// Every texture of the archive by stem (texture-pattern animations use names).
    pub textures_by_name: HashMap<String, TexRef>,
    /// Per `layout.fonts` entry (falls back to the default font when the layout's font is a
    /// system font the game does not ship, e.g. `CafeStd_25`).
    pub fonts: Vec<Option<Arc<FontRes>>>,
    /// Message label → plain text (this layout's MSBT file).
    pub messages: HashMap<String, String>,
    /// Message label → text with each value tag (numbers, names: tag groups 2 and 4, SMM2's
    /// number tags group 250) replaced by [`ARG_PLACEHOLDER`], for callers that fill in values
    /// (`LayoutPlayer::message`).
    pub templates: HashMap<String, String>,
    /// Part layouts by name.
    pub parts: HashMap<String, Arc<LayoutRes>>,
}

fn flatten(root: &Pane) -> Vec<Node> {
    fn go(p: &Pane, parent: Option<usize>, out: &mut Vec<Node>) {
        let idx = out.len();
        let mut pane = p.clone();
        pane.children.clear();
        out.push(Node {
            pane,
            parent,
            children: Vec::new(),
        });
        if let Some(par) = parent {
            out[par].children.push(idx);
        }
        for c in &p.children {
            go(c, Some(idx), out);
        }
    }
    let mut out = Vec::new();
    go(root, None, &mut out);
    out
}

/// Inputs to build a [`LayoutRes`] without files.
#[derive(Default)]
pub struct LayoutSource {
    pub layout: Layout,
    pub animations: BTreeMap<String, Animation>,
    /// Texture stem → texture.
    pub textures: Vec<(String, Mtex)>,
}

/// Stands for one value tag in [`LayoutRes::templates`].
pub const ARG_PLACEHOLDER: char = '\u{FFFC}';

/// The default font used when a layout names one the cache does not have (the Wii U system
/// fonts `CafeStd_*` are not part of the game).
pub const DEFAULT_FONT: &str = "MessageFont";
/// The game font with the widest character set (kana, kanji, symbols): fallback for characters
/// missing from the others.
pub const FALLBACK_FONT: &str = "SpecialFont";

#[derive(Default)]
pub struct UiCache {
    files: Option<Box<dyn UiFiles>>,
    pub manifest: Value,
    pub textures: Vec<TextureEntry>,
    texture_ids: HashMap<String, u32>,
    layouts: HashMap<String, Arc<LayoutRes>>,
    sources: HashMap<String, LayoutSource>,
    fonts: HashMap<String, Option<Arc<FontRes>>>,
    messages: HashMap<String, HashMap<String, String>>,
    templates: HashMap<String, HashMap<String, String>>,
    message_files: HashMap<String, String>,
    /// Textures asked for and not yet handed to the host: (id, file).
    wanted: Vec<(u32, String)>,
    /// Textures waiting for their pixels (asked for, not provided).
    waiting: usize,
    /// Bumped each time a texture's pixels arrive.
    revision: u64,
}

fn io_error(files: &dyn UiFiles, rel: &str, kind: std::io::ErrorKind) -> UiError {
    UiError::Io {
        path: Path::new(&files.describe()).join(rel),
        source: kind.into(),
    }
}

/// A file that must be there now (layouts, font metrics, message tables, the manifest).
fn read(files: &dyn UiFiles, rel: &str) -> Result<Vec<u8>, UiError> {
    match files.read(rel) {
        Fetch::Ready(b) => Ok(b),
        Fetch::Later => Err(io_error(files, rel, std::io::ErrorKind::WouldBlock)),
        Fetch::Missing => Err(io_error(files, rel, std::io::ErrorKind::NotFound)),
    }
}

fn read_json(files: &dyn UiFiles, rel: &str) -> Result<Value, UiError> {
    serde_json::from_slice(&read(files, rel)?)
        .map_err(|e| UiError::Invalid(format!("{}/{rel}: {e}", files.describe())))
}

/// An empty stand-in for a texture on its way.
fn stand_in(width: u32, height: u32) -> Mtex {
    Mtex {
        format: mmk_assets::MtexFormat::Rgba8,
        variant: mmk_assets::MtexVariant::Unorm,
        width,
        height,
        channel_select: [2, 3, 4, 5],
        data: Vec::new(),
    }
}

/// A name that stays inside its folder.
fn plain_name(s: &str) -> bool {
    !s.contains(['/', '\\']) && !s.starts_with('.')
}

impl UiCache {
    /// An empty cache (register layouts and fonts with `add_*`).
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Open an imported UI folder (the one holding `manifest.json` and `ui/`): an SMM1 import
    /// (`mmk import-smm1`) or an SMM2 one (`mmk import-smm2-ui`); both have the same form.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, UiError> {
        Self::from_files(Box::new(DirFiles::new(dir.as_ref())))
    }

    /// Open an import from `files` (see [`crate::files`]): the manifest, layouts, fonts'
    /// metrics and message tables must be readable at once; textures may come later.
    pub fn from_files(files: Box<dyn UiFiles>) -> Result<Self, UiError> {
        let manifest = read_json(&*files, "manifest.json")?;
        let game = manifest.get("game").and_then(Value::as_str);
        if !matches!(game, Some("SMM1" | "SMM2")) || !files.is_dir("ui/layouts") {
            return Err(UiError::Invalid(format!(
                "{} is not an SMM1 or SMM2 UI import",
                files.describe()
            )));
        }
        let mut message_files = HashMap::new();
        // <archive>/<group>/<Layout>.json (SMM1) or <archive>/<Layout>.json (SMM2), keyed by
        // layout name; LayoutMsg wins over CommonMsg over ReferenceMsg (SMM2's shared tables:
        // clear conditions, part names, …)
        fn add_json(files: &dyn UiFiles, dir: &str, out: &mut HashMap<String, String>) {
            let mut names = files.list(dir);
            names.sort();
            for n in names {
                if let Some(stem) = n.strip_suffix(".json") {
                    out.insert(stem.to_string(), format!("{dir}/{n}"));
                }
            }
        }
        for archive in ["ReferenceMsg", "CommonMsg", "LayoutMsg"] {
            let base = format!("ui/messages/{archive}");
            add_json(&*files, &base, &mut message_files);
            let mut groups = files.list(&base);
            groups.sort();
            for g in groups {
                let dir = format!("{base}/{g}");
                if files.is_dir(&dir) {
                    add_json(&*files, &dir, &mut message_files);
                }
            }
        }
        Ok(Self {
            files: Some(files),
            manifest,
            message_files,
            ..Default::default()
        })
    }

    /// The import folder, when the cache reads one on disk.
    pub fn root(&self) -> Option<&Path> {
        self.files.as_ref().and_then(|f| f.dir())
    }

    /// Textures that were asked for and whose pixels the host must now read: (texture id, file
    /// relative to the import root), each listed once; hand the bytes back with
    /// [`UiCache::provide`]. Always empty for a cache on disk.
    pub fn take_requests(&mut self) -> Vec<(u32, String)> {
        std::mem::take(&mut self.wanted)
    }

    /// The pixels of texture `id` (an `.mtex` file's bytes; `None`: unreadable, it stays empty).
    /// True if they were usable.
    pub fn provide(&mut self, id: u32, bytes: Option<&[u8]>) -> bool {
        let Some(e) = self.textures.get_mut(id as usize) else {
            return false;
        };
        if e.ready {
            return false;
        }
        let t = bytes.and_then(|b| Mtex::parse(b).ok());
        let ok = t.is_some();
        if let Some(t) = t {
            e.texture = t;
        }
        e.ready = true;
        self.waiting = self.waiting.saturating_sub(1);
        self.revision += 1;
        ok
    }

    /// How many textures wait for [`UiCache::provide`].
    pub fn waiting(&self) -> usize {
        self.waiting
    }

    /// Changes each time [`UiCache::provide`] hands over pixels (renderers then look again at
    /// the textures that were not ready).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Texture `name` from file `rel`, once: read now, or (a file that comes later) an empty
    /// stand-in of `size` plus a request.
    fn texture_at(&mut self, name: String, rel: &str, size: (u32, u32)) -> Result<TexRef, UiError> {
        if let Some(&id) = self.texture_ids.get(&name) {
            let t = &self.textures[id as usize].texture;
            return Ok(TexRef {
                id,
                width: t.width,
                height: t.height,
            });
        }
        let files = self
            .files
            .as_deref()
            .ok_or_else(|| UiError::Invalid(format!("texture {name}: no files")))?;
        match files.read(rel) {
            Fetch::Ready(b) => {
                let t = Mtex::parse(&b)
                    .map_err(|e| UiError::Invalid(format!("{}/{rel}: {e}", files.describe())))?;
                Ok(self.add_texture(name, t))
            }
            Fetch::Later => {
                let r = self.add_texture(name, stand_in(size.0, size.1));
                self.textures[r.id as usize].ready = false;
                self.wanted.push((r.id, rel.to_string()));
                self.waiting += 1;
                Ok(r)
            }
            Fetch::Missing => Err(io_error(files, rel, std::io::ErrorKind::NotFound)),
        }
    }

    /// Names of all importable layouts.
    pub fn layout_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .manifest
            .get("layouts")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        v.extend(self.sources.keys().cloned());
        v.sort();
        v.dedup();
        v
    }

    fn add_texture(&mut self, name: String, texture: Mtex) -> TexRef {
        let r = |id: u32, t: &Mtex| TexRef {
            id,
            width: t.width,
            height: t.height,
        };
        if let Some(&id) = self.texture_ids.get(&name) {
            return r(id, &self.textures[id as usize].texture);
        }
        let id = self.textures.len() as u32;
        let out = r(id, &texture);
        self.texture_ids.insert(name.clone(), id);
        self.textures.push(TextureEntry {
            name,
            texture,
            ready: true,
        });
        out
    }

    /// Register a layout from memory (replaces any earlier one with that name).
    pub fn add_layout(&mut self, src: LayoutSource) {
        let name = src.layout.name.clone();
        self.layouts.remove(&name);
        self.sources.insert(name, src);
    }

    /// Register a font from memory with its decoded sheets.
    pub fn add_font(&mut self, name: &str, font: mmk_formats::bffnt::Font, sheets: Vec<Mtex>) {
        let ids = sheets
            .into_iter()
            .enumerate()
            .map(|(i, t)| self.add_texture(format!("font:{name}/{i}"), t).id)
            .collect();
        self.fonts.insert(
            name.to_string(),
            Some(Arc::new(FontRes::from_font(name, font, ids))),
        );
    }

    /// Register message texts for a layout (label → text).
    pub fn add_messages(&mut self, layout: &str, messages: HashMap<String, String>) {
        self.messages.insert(layout.to_string(), messages);
    }

    /// A font by name, with the fallback font attached (`None` if the cache does not have it).
    pub fn font(&mut self, name: &str) -> Option<Arc<FontRes>> {
        if let Some(f) = self.fonts.get(name) {
            return f.clone();
        }
        let mut loaded = self.load_font(name).ok();
        if name != FALLBACK_FONT
            && let Some(f) = &loaded
            && f.fallback.is_none()
            && let Some(fb) = self.font(FALLBACK_FONT)
        {
            let mut with = (**f).clone();
            with.fallback = Some(fb);
            loaded = Some(Arc::new(with));
        }
        self.fonts.insert(name.to_string(), loaded.clone());
        loaded
    }

    fn load_font(&mut self, name: &str) -> Result<Arc<FontRes>, UiError> {
        if !plain_name(name) {
            return Err(UiError::Invalid(format!("font {name}")));
        }
        let files = self
            .files
            .as_deref()
            .ok_or_else(|| UiError::Invalid(format!("font {name} not registered")))?;
        let v = read_json(files, &format!("ui/fonts/{name}.json"))?;
        let sheets = v.get("sheets").and_then(Value::as_u64).unwrap_or(0) as usize;
        let size = |i: usize| v["sheet_size"][i].as_u64().unwrap_or(0) as u32;
        let (w, h) = (size(0), size(1));
        let mut ids = Vec::with_capacity(sheets);
        for i in 0..sheets {
            let rel = format!("ui/fonts/{name}/sheet_{i}.mtex");
            ids.push(
                self.texture_at(format!("font:{name}/{i}"), &rel, (w, h))?
                    .id,
            );
        }
        Ok(Arc::new(FontRes::from_json(name, &v, ids)?))
    }

    /// (plain texts, templates) of a layout's message table.
    fn messages_for(&mut self, layout: &str) -> (HashMap<String, String>, HashMap<String, String>) {
        if let Some(m) = self.messages.get(layout) {
            let t = self
                .templates
                .get(layout)
                .cloned()
                .unwrap_or_else(|| m.clone());
            return (m.clone(), t);
        }
        let mut out = HashMap::new();
        let mut templates = HashMap::new();
        if let Some(rel) = self.message_files.get(layout)
            && let Some(files) = self.files.as_deref()
            && let Ok(v) = read_json(files, rel)
            && let Ok(msbt) = mmk_formats::msbt::Msbt::from_json(&v)
        {
            use mmk_formats::msbt::MsgPart;
            for m in &msbt.messages {
                out.insert(m.label.clone(), m.plain());
                let t: String = m
                    .parts
                    .iter()
                    .filter_map(|p| match p {
                        MsgPart::Text(t) => Some(t.clone()),
                        MsgPart::Tag {
                            group: 2 | 4 | 250, ..
                        } => Some(ARG_PLACEHOLDER.to_string()),
                        _ => None,
                    })
                    .collect();
                templates.insert(m.label.clone(), t);
            }
        }
        self.messages.insert(layout.to_string(), out.clone());
        self.templates.insert(layout.to_string(), templates.clone());
        (out, templates)
    }

    /// Message `label` of message table `table` (a layout's, or a shared one such as SMM2's
    /// `GoalCondition`), with [`ARG_PLACEHOLDER`] for each value it takes.
    pub fn table_message(&mut self, table: &str, label: &str) -> Option<String> {
        self.messages_for(table).1.get(label).cloned()
    }

    /// Texture `ui/textures/<group>/<stem>.mtex` of the import (loaded once), for callers that
    /// put a texture of their choice into a picture (`LayoutPlayer::set_picture_texture`).
    pub fn texture_file(&mut self, group: &str, stem: &str) -> Option<TexRef> {
        let name = format!("{group}/{stem}");
        if let Some(&id) = self.texture_ids.get(&name) {
            let t = &self.textures[id as usize].texture;
            return Some(TexRef {
                id,
                width: t.width,
                height: t.height,
            });
        }
        if !plain_name(stem) || !plain_name(group) {
            return None;
        }
        let rel = format!("ui/textures/{name}.mtex");
        self.texture_at(name, &rel, (0, 0)).ok()
    }

    /// Register a texture from memory under `<group>/<stem>` (tests, tools).
    pub fn add_texture_file(&mut self, group: &str, stem: &str, texture: Mtex) -> TexRef {
        self.add_texture(format!("{group}/{stem}"), texture)
    }

    /// Load (once) a layout with its parts, textures, fonts and messages.
    pub fn layout(&mut self, name: &str) -> Result<Arc<LayoutRes>, UiError> {
        self.layout_inner(name, &mut Vec::new())
    }

    fn layout_inner(
        &mut self,
        name: &str,
        stack: &mut Vec<String>,
    ) -> Result<Arc<LayoutRes>, UiError> {
        if let Some(l) = self.layouts.get(name) {
            return Ok(l.clone());
        }
        if stack.iter().any(|s| s == name) {
            return Err(UiError::Invalid(format!(
                "part cycle: {} → {name}",
                stack.join(" → ")
            )));
        }
        let src = match self.sources.remove(name) {
            Some(s) => s,
            None => self.load_source(name)?,
        };
        stack.push(name.to_string());

        let mut textures_by_name = HashMap::new();
        for (stem, tex) in &src.textures {
            let r = self.add_texture(format!("{name}/{stem}"), tex.clone());
            textures_by_name.insert(stem.clone(), r);
        }
        let textures = src
            .layout
            .textures
            .iter()
            .map(|t| textures_by_name.get(Layout::texture_stem(t)).copied())
            .collect();
        let mut fonts = Vec::new();
        for f in &src.layout.fonts {
            let stem = f.split('.').next().unwrap_or(f).to_string();
            let font = self.font(&stem).or_else(|| self.font(DEFAULT_FONT));
            fonts.push(font);
        }
        let (messages, templates) = self.messages_for(name);
        let mut parts = HashMap::new();
        for p in src.layout.panes() {
            if let PaneKind::Part(part) = &p.kind
                && !parts.contains_key(&part.layout)
            {
                // a missing part layout is drawn as nothing rather than failing the whole screen
                if let Ok(res) = self.layout_inner(&part.layout, stack) {
                    parts.insert(part.layout.clone(), res);
                }
            }
        }
        stack.pop();

        let nodes = flatten(&src.layout.root);
        let pane_index = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.pane.name.clone(), i))
            .collect();
        let material_index = src
            .layout
            .materials
            .iter()
            .enumerate()
            .map(|(i, m)| (m.name.clone(), i))
            .collect();
        let res = Arc::new(LayoutRes {
            name: name.to_string(),
            layout: src.layout,
            animations: src.animations,
            nodes,
            pane_index,
            material_index,
            textures,
            textures_by_name,
            fonts,
            messages,
            templates,
            parts,
        });
        self.layouts.insert(name.to_string(), res.clone());
        Ok(res)
    }

    fn load_source(&mut self, name: &str) -> Result<LayoutSource, UiError> {
        let files = self
            .files
            .as_deref()
            .ok_or_else(|| UiError::NoLayout(name.to_string()))?;
        let rel = format!("ui/layouts/{name}.json");
        let path = format!("{}/{rel}", files.describe());
        let doc: Value = match files.read(&rel) {
            Fetch::Ready(b) if plain_name(name) => {
                serde_json::from_slice(&b).map_err(|e| UiError::Invalid(format!("{path}: {e}")))?
            }
            Fetch::Later => return Err(io_error(files, &rel, std::io::ErrorKind::WouldBlock)),
            _ => return Err(UiError::NoLayout(name.to_string())),
        };
        let err = |e: mmk_formats::Error| UiError::Invalid(format!("{path}: {e}"));
        let layout = lyt::json::layout_from_json(
            doc.get("layout")
                .ok_or_else(|| UiError::Invalid(format!("{path}: no layout")))?,
        )
        .map_err(err)?;
        let mut animations = BTreeMap::new();
        if let Some(a) = doc.get("animations").and_then(Value::as_object) {
            for (k, v) in a {
                animations.insert(k.clone(), lyt::json::animation_from_json(v).map_err(err)?);
            }
        }
        let mut textures = Vec::new();
        if let Some(t) = doc.get("textures").and_then(Value::as_object) {
            for (stem, info) in t {
                let rel = format!("ui/textures/{name}/{stem}.mtex");
                let n = |k: &str| info[k].as_u64().unwrap_or(0) as u32;
                // registered here (a stand-in while the pixels are on their way); the layout's
                // registration finds it by name, so the source only carries the size
                let r = self.texture_at(format!("{name}/{stem}"), &rel, (n("w"), n("h")))?;
                textures.push((stem.clone(), stand_in(r.width, r.height)));
            }
        }
        Ok(LayoutSource {
            layout,
            animations,
            textures,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::MemFiles;
    use mmk_formats::fixtures::sample_bflyt;

    fn tex(w: u32, h: u32) -> Mtex {
        Mtex {
            data: vec![255; (w * h * 4) as usize],
            ..stand_in(w, h)
        }
    }

    #[test]
    fn textures_that_come_later() {
        let layout = Layout::parse(&sample_bflyt()).unwrap();
        let doc = serde_json::json!({
            "layout": lyt::json::layout_to_json(&layout),
            "textures": {"Box^s": {"w": 8, "h": 8}, "Tex1": {"w": 4, "h": 4}},
        });
        let mut m = MemFiles::new("memory");
        m.insert("manifest.json", br#"{"game": "SMM2"}"#.to_vec());
        m.insert("ui/layouts/Sample.json", doc.to_string().into_bytes());
        m.insert_later("ui/textures/Sample/Box^s.mtex");
        m.insert("ui/textures/Sample/Tex1.mtex", tex(4, 4).to_bytes());
        m.insert_later("ui/textures/Icons/A.mtex");
        let mut c = UiCache::from_files(Box::new(m)).unwrap();
        assert!(c.root().is_none());
        let res = c.layout("Sample").unwrap();
        let boxed = res.textures_by_name["Box^s"];
        assert_eq!((boxed.width, boxed.height), (8, 8));
        assert!(!c.textures[boxed.id as usize].ready);
        assert!(c.textures[res.textures_by_name["Tex1"].id as usize].ready);
        let icon = c.texture_file("Icons", "A").unwrap();
        assert!(c.texture_file("Icons", "B").is_none());
        assert_eq!(c.waiting(), 2);
        let wanted = c.take_requests();
        assert_eq!(
            wanted,
            [
                (boxed.id, "ui/textures/Sample/Box^s.mtex".to_string()),
                (icon.id, "ui/textures/Icons/A.mtex".to_string())
            ]
        );
        assert!(c.take_requests().is_empty());
        let rev = c.revision();
        assert!(c.provide(boxed.id, Some(&tex(8, 8).to_bytes())));
        assert!(!c.provide(boxed.id, Some(&tex(8, 8).to_bytes())));
        assert!(!c.provide(icon.id, None));
        assert!(c.textures[boxed.id as usize].ready);
        assert_eq!(c.textures[boxed.id as usize].texture.data.len(), 8 * 8 * 4);
        assert_eq!((c.waiting(), c.revision() > rev), (0, true));
        // opened once: neither the layout nor its textures are asked for again
        c.layout("Sample").unwrap();
        assert!(c.take_requests().is_empty());
    }
}
