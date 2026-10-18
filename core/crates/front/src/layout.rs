//! An imported UI ([`Ui`]: the layout cache, its message tables and, for SMM2, the UI sounds)
//! and one playing layout with a small timeline ([`Layout`]): animations are scheduled on the
//! owner's 60 Hz tick counter and bound at their frames by [`Layout::apply`] before drawing or
//! reading pane positions. The port of the Swift app's `LayoutUI` / `UILayout`.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

use mmk_ui::{ARG_PLACEHOLDER, LayoutPlayer, PaneInfo, PaneOverride, Quad, UiCache};

use crate::sound::{SoundCue, UiSounds};

/// Which import a layout comes from (a host keeps one texture set per import).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UiKind {
    /// `mmk import-smm2-ui`: title, menus, Course World, in-course screens.
    Smm2,
    /// `mmk import-smm1`: the SMM1-layout 100 Mario Challenge.
    Smm1,
}

struct UiInner {
    kind: UiKind,
    cache: RefCell<UiCache>,
    sounds: Option<UiSounds>,
    cues: RefCell<Vec<SoundCue>>,
}

/// An opened UI import, shared by every screen that draws from it (cheap to clone).
#[derive(Clone)]
pub struct Ui(Rc<UiInner>);

impl Ui {
    /// `sounds`: the import's `ui/sound/sounds.json` (SMM2), if it has one.
    pub fn new(kind: UiKind, cache: UiCache, sounds: Option<UiSounds>) -> Self {
        Ui(Rc::new(UiInner {
            kind,
            cache: RefCell::new(cache),
            sounds,
            cues: RefCell::new(Vec::new()),
        }))
    }

    /// Open an import folder (reads `ui/sound/sounds.json` when present).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(kind: UiKind, dir: &std::path::Path) -> Result<Self, String> {
        let cache = UiCache::open(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let sounds = std::fs::read_to_string(dir.join("ui/sound/sounds.json"))
            .ok()
            .and_then(|s| UiSounds::from_json(&s));
        Ok(Self::new(kind, cache, sounds))
    }

    pub fn kind(&self) -> UiKind {
        self.0.kind
    }

    /// The layout cache (the host uploads `cache().textures` added since its last look).
    pub fn cache(&self) -> Ref<'_, UiCache> {
        self.0.cache.borrow()
    }

    pub fn cache_mut(&self) -> RefMut<'_, UiCache> {
        self.0.cache.borrow_mut()
    }

    pub fn same(&self, other: &Ui) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// A new instance of layout `name` (None when the import doesn't have it).
    pub fn layout(&self, name: &str) -> Option<Layout> {
        let res = self.0.cache.borrow_mut().layout(name).ok()?;
        Some(Layout::new(self.clone(), name, LayoutPlayer::new(res)))
    }

    /// Message `label` of text table `table` (a layout's, or a shared SMM2 one such as
    /// `GoalCondition`); value tags are [`ARG_PLACEHOLDER`].
    pub fn message(&self, table: &str, label: &str) -> Option<String> {
        self.0.cache.borrow_mut().table_message(table, label)
    }

    /// Message `label` of `table` with its value tags filled in order from `args`.
    pub fn format(&self, table: &str, label: &str, args: &[String]) -> Option<String> {
        self.message(table, label).map(|m| fill(&m, args))
    }

    /// The import's game build ("3.0.3"), for the title's version box.
    pub fn build(&self) -> String {
        let c = self.0.cache.borrow();
        match c.manifest.get("build").and_then(|v| v.as_str()) {
            Some(b) if b != "unknown" => b.to_string(),
            _ => String::new(),
        }
    }

    pub fn sounds(&self) -> Option<&UiSounds> {
        self.0.sounds.as_ref()
    }

    /// Queue resolved sound cues (taken by the host once per tick).
    pub(crate) fn push_cues(&self, cues: Vec<SoundCue>) {
        self.0.cues.borrow_mut().extend(cues);
    }

    pub fn take_cues(&self) -> Vec<SoundCue> {
        std::mem::take(&mut *self.0.cues.borrow_mut())
    }
}

/// A message template with its value slots filled in order from `args`.
pub fn fill(m: &str, args: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;
    for ch in m.chars() {
        if ch == ARG_PLACEHOLDER {
            if let Some(a) = args.get(i) {
                out.push_str(a);
            }
            i += 1;
        } else {
            out.push(ch);
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
struct Track {
    start: i32,
    from: f32,
    to: Option<f32>,
    looping: bool,
    speed: f32,
}

/// Where a pane is drawn: centre and size of its rectangle in layout units (y up), its global
/// alpha (0–1) and whether it is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub alpha: f32,
    pub visible: bool,
}

impl Rect {
    pub fn contains(&self, p: [f32; 2]) -> bool {
        (p[0] - self.center[0]).abs() <= self.size[0] / 2.0
            && (p[1] - self.center[1]).abs() <= self.size[1] / 2.0
    }
}

/// One playing layout plus its timeline.
pub struct Layout {
    pub player: LayoutPlayer,
    pub name: String,
    ui: Ui,
    tracks: Vec<(String, Track)>,
    lengths: HashMap<String, f32>,
}

impl Layout {
    fn new(ui: Ui, name: &str, player: LayoutPlayer) -> Self {
        Self {
            player,
            name: name.to_string(),
            ui,
            tracks: Vec::new(),
            lengths: HashMap::new(),
        }
    }

    pub fn ui(&self) -> &Ui {
        &self.ui
    }

    /// Layout size in layout units (1280 × 720 for full-screen layouts).
    pub fn size(&self) -> [f32; 2] {
        let [w, h] = self.player.size();
        if w > 0.0 && h > 0.0 {
            [w, h]
        } else {
            [1280.0, 720.0]
        }
    }

    /// Length in frames (−1 when the layout has no such animation).
    pub fn length(&mut self, anim: &str) -> f32 {
        if let Some(&l) = self.lengths.get(anim) {
            return l;
        }
        let l = self.player.animation_length(anim).unwrap_or(-1.0);
        self.lengths.insert(anim.to_string(), l);
        l
    }

    /// Length in whole ticks, at least `min`.
    pub fn ticks(&mut self, anim: &str, min: i32) -> i32 {
        (self.length(anim) as i32).max(min)
    }

    pub fn has(&mut self, anim: &str) -> bool {
        self.length(anim) >= 0.0
    }

    /// Play `anim` (`[Part/…/]Name`) from frame `from` starting at tick `tick`, at `speed`
    /// frames per tick, until `to` (default: its end), holding there; `looping` wraps instead.
    /// Before `tick` the animation sits at `from`.
    pub fn play_ex(
        &mut self,
        anim: &str,
        tick: i32,
        from: f32,
        to: Option<f32>,
        looping: bool,
        speed: f32,
    ) {
        if !self.has(anim) {
            return;
        }
        let t = Track {
            start: tick,
            from,
            to,
            looping,
            speed,
        };
        match self.tracks.iter_mut().find(|(n, _)| n == anim) {
            Some(e) => e.1 = t,
            None => self.tracks.push((anim.to_string(), t)),
        }
    }

    /// Play `anim` from its start at `tick`, holding its last frame.
    pub fn play(&mut self, anim: &str, tick: i32) {
        self.play_ex(anim, tick, 0.0, None, false, 1.0);
    }

    /// Play `anim` from `tick`, looping.
    pub fn play_loop(&mut self, anim: &str, tick: i32) {
        self.play_ex(anim, tick, 0.0, None, true, 1.0);
    }

    /// Show `anim` at a fixed frame.
    pub fn hold(&mut self, anim: &str, frame: f32) {
        self.play_ex(anim, 0, frame, Some(frame), false, 1.0);
    }

    /// Show `anim` at its last frame.
    pub fn hold_end(&mut self, anim: &str) {
        let l = self.length(anim);
        self.hold(anim, l);
    }

    pub fn stop(&mut self, anim: &str) {
        self.tracks.retain(|(n, _)| n != anim);
        self.player.stop(Some(anim));
    }

    pub fn is_playing(&self, anim: &str) -> bool {
        self.tracks.iter().any(|(n, _)| n == anim)
    }

    /// True once `anim` has reached its last frame (or `to`) at `tick`.
    pub fn finished(&mut self, anim: &str, tick: i32) -> bool {
        let Some(t) = self.tracks.iter().find(|(n, _)| n == anim).map(|e| e.1) else {
            return false;
        };
        if t.looping {
            return false;
        }
        let end = t.to.unwrap_or_else(|| self.length(anim));
        (tick - t.start) as f32 * t.speed + t.from >= end
    }

    fn frame_of(t: &Track, length: f32, tick: i32) -> f32 {
        let f = t.from + (tick - t.start).max(0) as f32 * t.speed;
        if t.looping {
            return if length > 0.0 { f % length } else { 0.0 };
        }
        let end = t.to.unwrap_or(length);
        if t.speed >= 0.0 {
            f.min(end)
        } else {
            f.max(end)
        }
    }

    /// Bind every scheduled animation at its frame for `tick`.
    pub fn apply(&mut self, tick: i32) {
        let tracks = self.tracks.clone();
        for (name, t) in &tracks {
            let len = self.length(name);
            self.player.play(name, Self::frame_of(t, len, tick));
        }
    }

    /// The draw list at `tick`.
    pub fn draw(&mut self, tick: i32, out: &mut Vec<Quad>) {
        self.apply(tick);
        self.player.draw(out);
    }

    pub fn text(&mut self, pane: &str, s: &str) {
        self.player.set_text(pane, s);
    }

    /// Force picture / window `pane`'s material white colour over its animations (`None`: give
    /// it back to them).
    pub fn material_white(&mut self, pane: &str, rgba: Option<[u8; 4]>) {
        let (black, _) = self.player.material_color(pane).unwrap_or((None, None));
        self.player.set_material_color(pane, black, rgba);
    }

    /// Width (pane units) of text pane `pane`'s widest line.
    pub fn text_width(&self, pane: &str) -> Option<f32> {
        self.player.text_width(pane)
    }

    /// Set `s` into text pane `pane`, breaking lines between words so that none is wider than
    /// `width` (pane units); at most `lines` lines, the rest cut.
    pub fn wrapped_text(&mut self, pane: &str, s: &str, width: f32, lines: usize) {
        let mut out: Vec<String> = Vec::new();
        let mut line = String::new();
        for word in s.split(' ') {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };
            self.text(pane, &candidate);
            if !line.is_empty() && self.text_width(pane).is_some_and(|w| w > width) {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            out.push(line);
        }
        out.truncate(lines.max(1));
        self.text(pane, &out.join("\n"));
    }

    /// Draw picture `pane` with the import's texture `ui/textures/<group>/<name>.mtex` (`None`:
    /// its own texture again). False when the texture or the picture is missing.
    pub fn picture(&mut self, pane: &str, group: &str, name: Option<&str>) -> bool {
        let tex = match name {
            Some(n) => match self.ui.cache_mut().texture_file(group, n) {
                Some(t) => Some(t),
                None => return false,
            },
            None => None,
        };
        self.player.set_picture_texture(pane, tex)
    }

    /// Force a pane shown / hidden (`None`: back to its animations).
    pub fn visible(&mut self, pane: &str, v: Option<bool>) {
        self.player.set_visible(pane, v);
    }

    pub fn show(&mut self, pane: &str, v: bool) {
        self.visible(pane, Some(v));
    }

    pub fn pane(&self, name: &str) -> Option<PaneInfo> {
        self.player.pane_info(name)
    }

    /// Where pane `name` (`[Part/…/]Pane`) is drawn (apply the timeline first).
    pub fn rect(&self, name: &str) -> Option<Rect> {
        let g = self.player.pane_global(name)?;
        let xs = g.corners.map(|c| c[0]);
        let ys = g.corners.map(|c| c[1]);
        let lo = [
            xs.iter().copied().fold(f32::MAX, f32::min),
            ys.iter().copied().fold(f32::MAX, f32::min),
        ];
        let hi = [
            xs.iter().copied().fold(f32::MIN, f32::max),
            ys.iter().copied().fold(f32::MIN, f32::max),
        ];
        Some(Rect {
            center: [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0],
            size: [hi[0] - lo[0], hi[1] - lo[1]],
            alpha: g.alpha,
            visible: g.visible,
        })
    }

    /// Force a pane's translate / size / scale / alpha (0–255) over its animations; all `None`
    /// gives it back to them.
    pub fn set_pane(&mut self, name: &str, o: PaneOverride) {
        self.player.set_pane(name, o);
    }

    pub fn set_translate(&mut self, name: &str, t: [f32; 2]) {
        self.set_pane(
            name,
            PaneOverride {
                translate: Some(t),
                ..Default::default()
            },
        );
    }

    pub fn set_alpha(&mut self, name: &str, a: f32) {
        self.set_pane(
            name,
            PaneOverride {
                alpha: Some(a),
                ..Default::default()
            },
        );
    }

    pub fn clear_pane(&mut self, name: &str) {
        self.set_pane(name, PaneOverride::default());
    }

    /// Draw a copy of part pane `part` as `name`, moved by `offset`.
    pub fn clone_part(&mut self, part: &str, name: &str, offset: [f32; 2]) -> bool {
        self.player.add_clone(part, name, offset)
    }

    /// A message of this layout's text table; value tags are [`ARG_PLACEHOLDER`].
    pub fn message(&self, label: &str) -> Option<String> {
        self.player.message(label)
    }

    pub fn format(&self, label: &str, args: &[String]) -> Option<String> {
        self.message(label).map(|m| fill(&m, args))
    }

    /// Set text pane `pane` to message `label` of this layout's table, when there is one.
    pub fn text_from(&mut self, pane: &str, label: &str) {
        if let Some(s) = self.message(label) {
            self.text(pane, &s);
        }
    }
}
