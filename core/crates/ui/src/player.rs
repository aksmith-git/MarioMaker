//! A playing layout: bound animations at given frames → draw list.
//!
//! Per draw: start from the layout's base values, apply every bound animation (in binding
//! order) at its frame, then walk the pane tree (parents first) computing each pane's global
//! matrix, alpha and visibility, and emit quads for pictures, windows, text and parts.
//!
//! Pane transform: `parent · T(translate + anchor offset) · Rz · Ry · Rx · S(scale)`. The anchor
//! offset places the child relative to a point of the parent's rectangle (pane origin byte
//! bits 4-7; centre by default): `(anchor factor − parent origin factor) × parent size`.
//! Alpha: a pane's alpha multiplies into its children when it has the "influenced alpha" flag.

use std::collections::HashMap;
use std::sync::Arc;

use mmk_formats::lyt::{
    AnimTag, Material, Pane, PaneKind, PartBasicInfo, TexSrt, Window, WindowFrame,
};

use crate::cache::{LayoutRes, TexRef};
use crate::text;
use crate::*;

/// Affine 3D transform, rows of [m00 m01 m02 tx].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mtx(pub [[f32; 4]; 3]);

impl Mtx {
    pub const IDENTITY: Mtx = Mtx([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);

    pub fn mul(&self, b: &Mtx) -> Mtx {
        let a = &self.0;
        let b = &b.0;
        let mut o = [[0f32; 4]; 3];
        for (r, row) in o.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate() {
                let mut v = a[r][0] * b[0][c] + a[r][1] * b[1][c] + a[r][2] * b[2][c];
                if c == 3 {
                    v += a[r][3];
                }
                *cell = v;
            }
        }
        Mtx(o)
    }

    pub fn scale(sx: f32, sy: f32) -> Mtx {
        let mut m = Self::IDENTITY;
        m.0[0][0] = sx;
        m.0[1][1] = sy;
        m
    }

    /// T · Rz · Ry · Rx · S, rotations in degrees.
    pub fn srt(t: [f32; 3], r_deg: [f32; 3], s: [f32; 2]) -> Mtx {
        let (sx, cx) = r_deg[0].to_radians().sin_cos();
        let (sy, cy) = r_deg[1].to_radians().sin_cos();
        let (sz, cz) = r_deg[2].to_radians().sin_cos();
        // R = Rz * Ry * Rx
        let r = [
            [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
            [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
            [-sy, cy * sx, cy * cx],
        ];
        Mtx([
            [r[0][0] * s[0], r[0][1] * s[1], r[0][2], t[0]],
            [r[1][0] * s[0], r[1][1] * s[1], r[1][2], t[1]],
            [r[2][0] * s[0], r[2][1] * s[1], r[2][2], t[2]],
        ])
    }

    pub fn apply(&self, x: f32, y: f32) -> [f32; 2] {
        let m = &self.0;
        [
            m[0][0] * x + m[0][1] * y + m[0][3],
            m[1][0] * x + m[1][1] * y + m[1][3],
        ]
    }
}

#[derive(Clone, Debug)]
struct PaneState {
    t: [f32; 3],
    r: [f32; 3],
    s: [f32; 2],
    size: [f32; 2],
    alpha: f32,
    visible: bool,
    colors: [[f32; 4]; 4],
}

#[derive(Clone, Debug)]
struct MatState {
    black: [f32; 4],
    white: [f32; 4],
    srts: Vec<TexSrt>,
    /// Texture-pattern override per texture map.
    pattern: Vec<Option<TexRef>>,
}

fn rgba_f(c: [u8; 4]) -> [f32; 4] {
    c.map(|v| v as f32)
}

fn to_u8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// Horizontal / vertical factors of an origin code (0 centre, 1 left/top, 2 right/bottom).
fn factor(code: u8) -> f32 {
    match code & 3 {
        1 => 0.0,
        2 => 1.0,
        _ => 0.5,
    }
}

/// Pane rectangle in its own space: (left, top, width, height), y up.
pub fn pane_rect(origin: u8, size: [f32; 2]) -> (f32, f32, f32, f32) {
    let (w, h) = (size[0], size[1]);
    let l = -w * factor(origin);
    let t = h * factor(origin >> 2);
    (l, t, w, h)
}

/// Uniform sample positions of a quad's corners (TL, TR, BL, BR).
const UNIT_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

fn apply_srt(uv: [[f32; 2]; 4], srt: &TexSrt) -> [[f32; 2]; 4] {
    let (s, c) = srt.rotate.to_radians().sin_cos();
    uv.map(|[u, v]| {
        let (x, y) = ((u - 0.5) * srt.scale[0], (v - 0.5) * srt.scale[1]);
        [
            c * x - s * y + 0.5 + srt.translate[0],
            s * x + c * y + 0.5 + srt.translate[1],
        ]
    })
}

/// Reorder a quad's corner UVs for a window-frame flip code.
fn flip_uv(uv: [[f32; 2]; 4], flip: u8) -> [[f32; 2]; 4] {
    let [tl, tr, bl, br] = uv;
    match flip {
        1 => [tr, tl, br, bl],
        2 => [bl, br, tl, tr],
        3 => [bl, tl, br, tr],
        4 => [br, bl, tr, tl],
        5 => [tr, br, tl, bl],
        _ => uv,
    }
}

fn uv_rect(u0: f32, v0: f32, u1: f32, v1: f32) -> [[f32; 2]; 4] {
    [[u0, v0], [u1, v0], [u0, v1], [u1, v1]]
}

/// A playing instance of a layout (its part panes get their own instances).
#[derive(Clone, Debug)]
pub struct LayoutPlayer {
    res: Arc<LayoutRes>,
    anims: Vec<(String, f32)>,
    /// Texts set by the caller (`set_text`), by pane name.
    texts: HashMap<String, String>,
    /// Message ids given to this part's text panes by the enclosing layout's part properties,
    /// with the text stored alongside (used when no table has the id).
    text_ids: HashMap<String, (String, String)>,
    parts: HashMap<usize, LayoutPlayer>,
    /// Visibility set by the caller (`set_visible`), by node index; wins over animations.
    visibility: HashMap<usize, bool>,
    /// Basic-value overrides from the enclosing part pane's properties: pane name → (usage
    /// bits, values). See [`PartBasicInfo`] for the bits.
    basic_overrides: HashMap<String, (u8, Option<PartBasicInfo>)>,
    /// Picture panes whose content (material, texture coordinates, colours) the enclosing
    /// layout replaces: pane name → (enclosing layout, its replacement pane). The material is
    /// one of the enclosing layout's (e.g. the number on each world-map course dot).
    pane_overrides: HashMap<String, (Arc<LayoutRes>, Pane)>,
}

/// A pane's values after the bound animations (`LayoutPlayer::pane_info`), in its parent's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneInfo {
    pub translate: [f32; 3],
    pub rotate: [f32; 3],
    pub scale: [f32; 2],
    pub size: [f32; 2],
    /// 0–255.
    pub alpha: f32,
    pub visible: bool,
}

/// Message tables of the enclosing layouts, outermost (the screen) first.
type MessageChain<'a> = [&'a HashMap<String, String>];

struct Ctx<'a> {
    res: &'a LayoutRes,
    mats: &'a [MatState],
    /// Size of the pane being drawn (projection texture coordinates).
    pane_size: std::cell::Cell<[f32; 2]>,
}

/// Corner positions of a quad for projected texture coordinates: in the pane's own space and in
/// layout space.
struct Corners {
    local: [[f32; 2]; 4],
    global: [[f32; 2]; 4],
}

impl LayoutPlayer {
    pub fn new(res: Arc<LayoutRes>) -> Self {
        let mut parts = HashMap::new();
        for (i, n) in res.nodes.iter().enumerate() {
            if let PaneKind::Part(p) = &n.pane.kind
                && let Some(sub) = res.parts.get(&p.layout)
            {
                let mut inst = LayoutPlayer::new(sub.clone());
                for prop in &p.properties {
                    if prop.basic_usage != 0 {
                        inst.basic_overrides
                            .insert(prop.name.clone(), (prop.basic_usage, prop.basic.clone()));
                    }
                    if let Some(pane) = &prop.pane
                        && matches!(pane.kind, PaneKind::Picture(_))
                    {
                        inst.pane_overrides
                            .insert(prop.name.clone(), (res.clone(), (**pane).clone()));
                    }
                }
                // text overrides carried by the part pane's properties: a message id that the
                // screen's message table (or an enclosing layout's) resolves when drawing
                for prop in &p.properties {
                    if let Some(pane) = &prop.pane
                        && let PaneKind::Text(t) = &pane.kind
                    {
                        let label = if t.text_id.is_empty() {
                            pane.name.clone()
                        } else {
                            t.text_id.clone()
                        };
                        inst.text_ids
                            .insert(prop.name.clone(), (label, t.text.clone()));
                    }
                }
                parts.insert(i, inst);
            }
        }
        Self {
            res,
            anims: Vec::new(),
            texts: HashMap::new(),
            text_ids: HashMap::new(),
            parts,
            visibility: HashMap::new(),
            basic_overrides: HashMap::new(),
            pane_overrides: HashMap::new(),
        }
    }

    pub fn resource(&self) -> &Arc<LayoutRes> {
        &self.res
    }

    /// Layout size (screen units).
    pub fn size(&self) -> [f32; 2] {
        [self.res.layout.width, self.res.layout.height]
    }

    pub fn animation_names(&self) -> Vec<String> {
        self.res.animations.keys().cloned().collect()
    }

    /// The part instance for a `/`-separated path of part pane names.
    fn part_mut(&mut self, path: &[&str]) -> Option<&mut LayoutPlayer> {
        let Some((first, rest)) = path.split_first() else {
            return Some(self);
        };
        let idx = *self.res.pane_index.get(*first)?;
        self.parts.get_mut(&idx)?.part_mut(rest)
    }

    fn part_ref(&self, path: &[&str]) -> Option<&LayoutPlayer> {
        let Some((first, rest)) = path.split_first() else {
            return Some(self);
        };
        let idx = *self.res.pane_index.get(*first)?;
        self.parts.get(&idx)?.part_ref(rest)
    }

    fn split(path: &str) -> (Vec<&str>, &str) {
        let mut parts: Vec<&str> = path.split('/').collect();
        let last = parts.pop().unwrap_or("");
        (parts, last)
    }

    /// Bind (or move) animation `path` = `[PartPane/…/]AnimName` at `frame`. False if unknown.
    pub fn play(&mut self, path: &str, frame: f32) -> bool {
        let (parts, name) = Self::split(path);
        let Some(p) = self.part_mut(&parts) else {
            return false;
        };
        if !p.res.animations.contains_key(name) {
            return false;
        }
        match p.anims.iter_mut().find(|(n, _)| n == name) {
            Some(a) => a.1 = frame,
            None => p.anims.push((name.to_string(), frame)),
        }
        true
    }

    /// Unbind one animation (`None`: every animation here and in all parts).
    pub fn stop(&mut self, path: Option<&str>) {
        match path {
            Some(path) => {
                let (parts, name) = Self::split(path);
                if let Some(p) = self.part_mut(&parts) {
                    p.anims.retain(|(n, _)| n != name);
                }
            }
            None => {
                self.anims.clear();
                for p in self.parts.values_mut() {
                    p.stop(None);
                }
            }
        }
    }

    /// Frame count of an animation (`[PartPane/…/]AnimName`).
    pub fn animation_length(&mut self, path: &str) -> Option<f32> {
        let (parts, name) = Self::split(path);
        let p = self.part_mut(&parts)?;
        p.res.animations.get(name).map(|a| a.frame_size as f32)
    }

    /// Replace the text of a text pane (`[PartPane/…/]Pane`). False if there is no such pane.
    pub fn set_text(&mut self, path: &str, text: &str) -> bool {
        let (parts, name) = Self::split(path);
        let Some(p) = self.part_mut(&parts) else {
            return false;
        };
        match p.res.pane_index.get(name) {
            Some(&i) if matches!(p.res.nodes[i].pane.kind, PaneKind::Text(_)) => {
                p.texts.insert(name.to_string(), text.to_string());
                true
            }
            _ => false,
        }
    }

    /// Force a pane (`[PartPane/…/]Pane`) visible or hidden over its animations, or give it back
    /// to them (`None`). False if there is no such pane.
    pub fn set_visible(&mut self, path: &str, visible: Option<bool>) -> bool {
        let (parts, name) = Self::split(path);
        let Some(p) = self.part_mut(&parts) else {
            return false;
        };
        let Some(&i) = p.res.pane_index.get(name) else {
            return false;
        };
        match visible {
            Some(v) => p.visibility.insert(i, v),
            None => p.visibility.remove(&i),
        };
        true
    }

    /// A pane's values (`[PartPane/…/]Pane`) with the bound animations applied.
    pub fn pane_info(&self, path: &str) -> Option<PaneInfo> {
        let (parts, name) = Self::split(path);
        let p = self.part_ref(&parts)?;
        let &i = p.res.pane_index.get(name)?;
        let (mut panes, mut mats) = p.base_states();
        p.apply_animations(&mut panes, &mut mats);
        let st = &panes[i];
        Some(PaneInfo {
            translate: st.t,
            rotate: st.r,
            scale: st.s,
            size: st.size,
            alpha: st.alpha,
            visible: p.visibility.get(&i).copied().unwrap_or(st.visible),
        })
    }

    /// Message `label` with [`crate::ARG_PLACEHOLDER`] for each value it takes, from this
    /// layout's table or else its part layouts' (depth first).
    pub fn message(&self, label: &str) -> Option<String> {
        if let Some(t) = self.res.templates.get(label) {
            return Some(t.clone());
        }
        let mut keys: Vec<&usize> = self.parts.keys().collect();
        keys.sort();
        keys.into_iter().find_map(|k| self.parts[k].message(label))
    }

    fn base_states(&self) -> (Vec<PaneState>, Vec<MatState>) {
        let panes = self
            .res
            .nodes
            .iter()
            .map(|n| {
                let p = &n.pane;
                let colors = match &p.kind {
                    PaneKind::Picture(pic) => pic.vertex_colors.map(rgba_f),
                    PaneKind::Window(w) => w.content.vertex_colors.map(rgba_f),
                    _ => [[255.0; 4]; 4],
                };
                PaneState {
                    t: p.translate,
                    r: p.rotate,
                    s: p.scale,
                    size: p.size,
                    alpha: p.alpha as f32,
                    visible: p.visible(),
                    colors,
                }
            })
            .collect::<Vec<_>>();
        let mut panes = panes;
        for (name, (usage, info)) in &self.basic_overrides {
            let Some(&i) = self.res.pane_index.get(name) else {
                continue;
            };
            let st = &mut panes[i];
            if usage & 1 != 0 {
                st.visible = usage & 2 != 0;
            }
            if let Some(b) = info {
                if usage & 8 != 0 {
                    st.t = b.translate;
                }
                if usage & 16 != 0 {
                    st.size = b.size;
                }
                if usage & 32 != 0 {
                    st.s = b.scale;
                }
                if usage & 64 != 0 {
                    st.r = b.rotate;
                }
                if usage & 128 != 0 {
                    st.alpha = b.alpha as f32;
                }
            }
        }
        (panes, base_mats(&self.res))
    }

    fn apply_animations(&self, panes: &mut [PaneState], mats: &mut [MatState]) {
        for (name, frame) in &self.anims {
            let Some(anim) = self.res.animations.get(name) else {
                continue;
            };
            for c in &anim.contents {
                if c.kind == 1 {
                    if let Some(&m) = self.res.material_index.get(&c.name) {
                        for tag in &c.tags {
                            self.apply_material_tag(&mut mats[m], tag, *frame, &anim.textures);
                        }
                    }
                } else if let Some(&i) = self.res.pane_index.get(&c.name) {
                    for tag in &c.tags {
                        apply_pane_tag(&mut panes[i], tag, *frame);
                    }
                }
            }
        }
    }

    fn apply_material_tag(&self, m: &mut MatState, tag: &AnimTag, frame: f32, names: &[String]) {
        for e in &tag.entries {
            let v = e.eval(frame);
            let i = e.index as usize;
            match tag.magic.as_str() {
                "FLMC" => match e.target {
                    0..=3 => m.black[e.target as usize] = v,
                    4..=7 => m.white[e.target as usize - 4] = v,
                    _ => {}
                },
                "FLTS" => {
                    if let Some(s) = m.srts.get_mut(i) {
                        match e.target {
                            0 => s.translate[0] = v,
                            1 => s.translate[1] = v,
                            2 => s.rotate = v,
                            3 => s.scale[0] = v,
                            4 => s.scale[1] = v,
                            _ => {}
                        }
                    }
                }
                "FLTP" => {
                    let tex = names
                        .get(v.max(0.0) as usize)
                        .and_then(|n| self.res.textures_by_name.get(n.trim_end_matches(".bflim")))
                        .copied();
                    if let (Some(slot), Some(t)) = (m.pattern.get_mut(i), tex) {
                        *slot = Some(t);
                    }
                }
                _ => {}
            }
        }
    }

    /// Evaluate and append this layout's quads (identity placement, full alpha).
    pub fn draw(&self, out: &mut Vec<Quad>) {
        self.draw_at(&Mtx::IDENTITY, 1.0, &[], out);
    }

    /// Convenience: evaluate into a new list.
    pub fn draw_list(&self) -> Vec<Quad> {
        let mut v = Vec::new();
        self.draw(&mut v);
        v
    }

    /// The text a text pane shows: caller's text, else a message id from the enclosing layout
    /// (searched from the screen inwards), else the pane's own id (this layout's table first,
    /// then outwards), else the text stored in the layout.
    fn text_for<'a>(
        &'a self,
        pane: &str,
        t: &'a mmk_formats::lyt::Text,
        outer: &MessageChain<'a>,
    ) -> &'a str {
        if let Some(s) = self.texts.get(pane) {
            return s;
        }
        let own = &self.res.messages;
        if let Some((id, stored)) = self.text_ids.get(pane) {
            return outer
                .iter()
                .copied()
                .chain(std::iter::once(own))
                .find_map(|m| m.get(id))
                .unwrap_or(stored);
        }
        let label = if t.text_id.is_empty() {
            pane
        } else {
            &t.text_id
        };
        std::iter::once(own)
            .chain(outer.iter().rev().copied())
            .find_map(|m| m.get(label))
            .map(String::as_str)
            .unwrap_or(&t.text)
    }

    fn draw_at<'a>(
        &'a self,
        base: &Mtx,
        base_alpha: f32,
        outer: &MessageChain<'a>,
        out: &mut Vec<Quad>,
    ) {
        let (mut panes, mut mats) = self.base_states();
        self.apply_animations(&mut panes, &mut mats);
        for (&i, &v) in &self.visibility {
            panes[i].visible = v;
        }
        let n = self.res.nodes.len();
        let mut mtx = vec![Mtx::IDENTITY; n];
        let mut alpha = vec![1f32; n]; // global alpha of each pane
        let mut visible = vec![false; n];
        let ctx = Ctx {
            res: &self.res,
            mats: &mats,
            pane_size: std::cell::Cell::new([0.0; 2]),
        };
        for i in 0..n {
            let node = &self.res.nodes[i];
            let st = &panes[i];
            let own = st.alpha / 255.0;
            let (parent_mtx, inherited, parent_visible, anchor) = match node.parent {
                Some(p) => {
                    let pn = &self.res.nodes[p].pane;
                    let ps = &panes[p];
                    let anchor = [
                        (factor(node.pane.origin >> 4) - factor(pn.origin)) * ps.size[0],
                        (factor(pn.origin >> 2) - factor(node.pane.origin >> 6)) * ps.size[1],
                    ];
                    let inherited = if pn.influenced_alpha() { alpha[p] } else { 1.0 };
                    (mtx[p], inherited, visible[p], anchor)
                }
                None => (*base, base_alpha, true, [0.0, 0.0]),
            };
            let t = [st.t[0] + anchor[0], st.t[1] + anchor[1], st.t[2]];
            mtx[i] = parent_mtx.mul(&Mtx::srt(t, st.r, st.s));
            alpha[i] = own * inherited;
            visible[i] = parent_visible && st.visible;
            if !visible[i] {
                continue;
            }
            let rect = pane_rect(node.pane.origin, st.size);
            ctx.pane_size.set(st.size);
            match &node.pane.kind {
                PaneKind::Picture(pic) => {
                    match self.pane_overrides.get(&node.pane.name) {
                        Some((owner, Pane { kind: PaneKind::Picture(op), .. })) => {
                            let owner_mats = base_mats(owner);
                            let octx = Ctx {
                                res: owner,
                                mats: &owner_mats,
                                pane_size: std::cell::Cell::new(st.size),
                            };
                            let colors =
                                vertex_colors(&op.vertex_colors.map(rgba_f), alpha[i]);
                            octx.picture(op.material, &op.tex_coords, rect, &mtx[i], colors, out);
                        }
                        _ => {
                            let colors = vertex_colors(&st.colors, alpha[i]);
                            ctx.picture(pic.material, &pic.tex_coords, rect, &mtx[i], colors, out);
                        }
                    }
                }
                PaneKind::Window(w) => {
                    let colors = vertex_colors(&st.colors, alpha[i]);
                    ctx.window(w, rect, &mtx[i], colors, alpha[i], out);
                }
                PaneKind::Text(t) => {
                    let s = self.text_for(&node.pane.name, t, outer);
                    if let Some(Some(font)) = self.res.fonts.get(t.font as usize) {
                        let mat = self.res.layout.materials.get(t.material as usize);
                        let (black, white, blend) = match (mat, mats.get(t.material as usize)) {
                            (Some(m), Some(ms)) => (ms.black, ms.white, blend_flags(m)),
                            _ => ([0.0; 4], [255.0; 4], BLEND_NORMAL),
                        };
                        text::emit(
                            font,
                            t,
                            s,
                            rect,
                            &mtx[i],
                            alpha[i],
                            black.map(to_u8),
                            white.map(to_u8),
                            blend,
                            out,
                        );
                    }
                }
                PaneKind::Part(p) => {
                    if let Some(inst) = self.parts.get(&i) {
                        let m = mtx[i].mul(&Mtx::scale(p.magnify[0], p.magnify[1]));
                        let a = if node.pane.influenced_alpha() {
                            alpha[i]
                        } else {
                            inherited
                        };
                        let mut chain: Vec<&HashMap<String, String>> = outer.to_vec();
                        chain.push(&self.res.messages);
                        inst.draw_at(&m, a, &chain, out);
                    }
                }
                PaneKind::Null | PaneKind::Bounding => {}
            }
        }
    }
}

/// A layout's materials as authored (no animation).
fn base_mats(res: &LayoutRes) -> Vec<MatState> {
    res.layout
        .materials
        .iter()
        .map(|m| MatState {
            black: rgba_f(m.black),
            white: rgba_f(m.white),
            srts: m.tex_srts.clone(),
            pattern: vec![None; m.tex_maps.len()],
        })
        .collect()
}

fn apply_pane_tag(st: &mut PaneState, tag: &AnimTag, frame: f32) {
    for e in &tag.entries {
        let v = e.eval(frame);
        match tag.magic.as_str() {
            "FLPA" => match e.target {
                0..=2 => st.t[e.target as usize] = v,
                3..=5 => st.r[e.target as usize - 3] = v,
                6 | 7 => st.s[e.target as usize - 6] = v,
                8 | 9 => st.size[e.target as usize - 8] = v,
                _ => {}
            },
            "FLVI" => st.visible = v >= 0.5,
            "FLVC" => match e.target {
                0..=15 => st.colors[e.target as usize / 4][e.target as usize % 4] = v,
                16 => st.alpha = v,
                _ => {}
            },
            _ => {}
        }
    }
}

fn vertex_colors(c: &[[f32; 4]; 4], alpha: f32) -> [[u8; 4]; 4] {
    c.map(|[r, g, b, a]| [to_u8(r), to_u8(g), to_u8(b), to_u8(a * alpha)])
}

pub(crate) fn blend_flags(m: &Material) -> u32 {
    match m.blend {
        Some(b) => {
            ((b.op as u32 & 15) << SHIFT_BLEND_OP)
                | ((b.src as u32 & 15) << SHIFT_BLEND_SRC)
                | ((b.dst as u32 & 15) << SHIFT_BLEND_DST)
        }
        None => BLEND_NORMAL,
    }
}

impl Ctx<'_> {
    fn texture(&self, m: &Material, ms: &MatState, map: usize) -> Option<TexRef> {
        ms.pattern.get(map).copied().flatten().or_else(|| {
            let t = m.tex_maps.get(map)?;
            self.res.textures.get(t.texture as usize).copied().flatten()
        })
    }

    /// UVs of texture map `map`: coordinate set chosen by its generator, then the texture SRT.
    ///
    /// Projections map positions to texture space with the projection point at the texture
    /// centre and v pointing down: pane-based (4, 5) uses the pane's own space scaled by its
    /// size (so a pane whose origin is a corner needs a ±0.5 texture translation, as authored),
    /// orthographic (3) uses layout space scaled by the layout size.
    fn uvs(
        &self,
        m: &Material,
        ms: &MatState,
        map: usize,
        sets: &[[[f32; 2]; 4]],
        corners: &Corners,
    ) -> [[f32; 2]; 4] {
        let project = |pts: &[[f32; 2]; 4], size: [f32; 2], k: usize| {
            let p = m.proj_tex_gens.get(k).copied().unwrap_or_default();
            let sx = if p.scale[0] != 0.0 { p.scale[0] } else { 1.0 };
            let sy = if p.scale[1] != 0.0 { p.scale[1] } else { 1.0 };
            let (w, h) = (size[0].max(1e-6), size[1].max(1e-6));
            pts.map(|[x, y]| {
                [
                    0.5 + (x / w - p.translate[0]) / sx,
                    0.5 - (y / h - p.translate[1]) / sy,
                ]
            })
        };
        // which projection parameter block belongs to this generator
        let proj_index = m
            .tex_coord_gens
            .iter()
            .take(map)
            .filter(|g| g.source >= 3)
            .count();
        let base = match m.tex_coord_gens.get(map).map(|g| g.source) {
            Some(s @ 0..=2) => sets.get(s as usize).copied().unwrap_or(UNIT_UV),
            Some(3) => {
                let l = &self.res.layout;
                project(&corners.global, [l.width, l.height], proj_index)
            }
            Some(_) => project(&corners.local, self.pane_size.get(), proj_index),
            None => sets.get(map).or(sets.first()).copied().unwrap_or(UNIT_UV),
        };
        match ms.srts.get(map) {
            Some(srt) => apply_srt(base, srt),
            None => base,
        }
    }

    /// Material → (black, white, tex0, tex1, flags, uv1); `sets` are the pane's coordinate sets.
    #[allow(clippy::type_complexity)]
    fn material(
        &self,
        mat: u16,
        sets: &[[[f32; 2]; 4]],
        corners: &Corners,
    ) -> Option<(
        [u8; 4],
        [u8; 4],
        u32,
        u32,
        u32,
        [[f32; 2]; 4],
        [[f32; 2]; 4],
    )> {
        let m = self.res.layout.materials.get(mat as usize)?;
        let ms = self.mats.get(mat as usize)?;
        let (mut black, mut white) = (ms.black.map(to_u8), ms.white.map(to_u8));
        if m.texture_only {
            black = [0; 4];
            white = [255; 4];
        }
        let t0 = self.texture(m, ms, 0);
        let t1 = if m.tex_maps.len() > 1 {
            self.texture(m, ms, 1)
        } else {
            None
        };
        let mut flags = blend_flags(m);
        if let Some(t) = m.tex_maps.first() {
            flags |=
                (t.wrap_s as u32 & 3) << SHIFT_WRAP_S0 | (t.wrap_t as u32 & 3) << SHIFT_WRAP_T0;
            if t.mag_filter == 1 || t.min_filter == 1 {
                flags |= FLAG_LINEAR;
            }
        }
        if let Some(t) = m.tex_maps.get(1) {
            flags |=
                (t.wrap_s as u32 & 3) << SHIFT_WRAP_S1 | (t.wrap_t as u32 & 3) << SHIFT_WRAP_T1;
        }
        let (cc, ca) = match (t1, m.tev_stages.first()) {
            (Some(_), Some(st)) => (st.color_mode as u32 & 15, st.alpha_mode as u32 & 15),
            (Some(_), None) => (1, 1),
            _ => (COMBINE_NONE, COMBINE_NONE),
        };
        flags |= cc << SHIFT_COMBINE_COLOR | ca << SHIFT_COMBINE_ALPHA;
        if m.tex_maps.is_empty() {
            // untextured: a solid quad in the white colour
            return Some((
                black,
                white,
                TEXTURE_NONE,
                TEXTURE_NONE,
                flags,
                UNIT_UV,
                UNIT_UV,
            ));
        }
        let uv0 = self.uvs(m, ms, 0, sets, corners);
        let uv1 = if t1.is_some() {
            self.uvs(m, ms, 1, sets, corners)
        } else {
            UNIT_UV
        };
        Some((
            black,
            white,
            t0.map(|t| t.id).unwrap_or(TEXTURE_NONE),
            t1.map(|t| t.id).unwrap_or(TEXTURE_NONE),
            flags,
            uv0,
            uv1,
        ))
    }

    fn texture_size(&self, mat: u16) -> Option<(f32, f32)> {
        let m = self.res.layout.materials.get(mat as usize)?;
        let ms = self.mats.get(mat as usize)?;
        self.texture(m, ms, 0)
            .map(|t| (t.width.max(1) as f32, t.height.max(1) as f32))
    }

    #[allow(clippy::too_many_arguments)]
    fn quad(
        &self,
        mat: u16,
        sets: &[[[f32; 2]; 4]],
        (l, t, w, h): (f32, f32, f32, f32),
        mtx: &Mtx,
        color: [[u8; 4]; 4],
        uv_override: Option<[[f32; 2]; 4]>,
        out: &mut Vec<Quad>,
    ) {
        if w == 0.0 || h == 0.0 {
            return;
        }
        let local = [[l, t], [l + w, t], [l, t - h], [l + w, t - h]];
        let pos = local.map(|[x, y]| mtx.apply(x, y));
        let corners = Corners { local, global: pos };
        let Some((black, white, tex0, tex1, flags, mut uv0, uv1)) =
            self.material(mat, sets, &corners)
        else {
            return;
        };
        if let Some(uv) = uv_override {
            uv0 = uv;
        }
        out.push(Quad {
            pos,
            uv0,
            uv1,
            color,
            black,
            white,
            tex0,
            tex1,
            flags,
        });
    }

    fn picture(
        &self,
        mat: u16,
        sets: &[[[f32; 2]; 4]],
        rect: (f32, f32, f32, f32),
        mtx: &Mtx,
        color: [[u8; 4]; 4],
        out: &mut Vec<Quad>,
    ) {
        self.quad(mat, sets, rect, mtx, color, None, out);
    }

    fn window(
        &self,
        w: &Window,
        (l, t, ww, wh): (f32, f32, f32, f32),
        mtx: &Mtx,
        content_color: [[u8; 4]; 4],
        alpha: f32,
        out: &mut Vec<Quad>,
    ) {
        let [fl, fr, ft, fb] = w.frame_size.map(|v| v as f32);
        let frame_color = if w.flags & 2 != 0 {
            content_color
        } else {
            [[255, 255, 255, to_u8(255.0 * alpha)]; 4]
        };
        let kind = w.kind();
        let draw_content = w.flags & 0x10 == 0 && kind != 2;
        let frames = &w.frames;
        let frame = |i: usize| frames.get(i).copied().unwrap_or_default();
        let content_sets = &w.content.tex_coords;
        let [il, ir, it, ib] = w.inflation.map(|v| v as f32);
        if kind == 0 {
            if draw_content {
                let rect = (
                    l + fl - il,
                    t - ft + it,
                    ww - fl - fr + il + ir,
                    wh - ft - fb + it + ib,
                );
                self.quad(
                    w.content.material,
                    content_sets,
                    rect,
                    mtx,
                    content_color,
                    None,
                    out,
                );
            }
            match frames.len() {
                1 => {
                    let f = frame(0);
                    let m = f.material;
                    let base = |uv| flip_uv(uv, f.flip);
                    let (iw, ih) = (ww - fl - fr, wh - ft - fb);
                    let parts = [
                        ((l, t, fl, ft), uv_rect(0.0, 0.0, 1.0, 1.0)),
                        ((l + ww - fr, t, fr, ft), uv_rect(1.0, 0.0, 0.0, 1.0)),
                        ((l, t - wh + fb, fl, fb), uv_rect(0.0, 1.0, 1.0, 0.0)),
                        (
                            (l + ww - fr, t - wh + fb, fr, fb),
                            uv_rect(1.0, 1.0, 0.0, 0.0),
                        ),
                        ((l + fl, t, iw, ft), uv_rect(1.0, 0.0, 1.0, 1.0)),
                        ((l + fl, t - wh + fb, iw, fb), uv_rect(1.0, 1.0, 1.0, 0.0)),
                        ((l, t - ft, fl, ih), uv_rect(0.0, 1.0, 1.0, 1.0)),
                        ((l + ww - fr, t - ft, fr, ih), uv_rect(1.0, 1.0, 0.0, 1.0)),
                    ];
                    for (rect, uv) in parts {
                        self.quad(m, &[], rect, mtx, frame_color, Some(base(uv)), out);
                    }
                }
                4 => {
                    // each frame: its corner plus the following edge, texture extended by clamping
                    let sizes: Vec<(f32, f32)> = (0..4)
                        .map(|i| self.texture_size(frame(i).material).unwrap_or((1.0, 1.0)))
                        .collect();
                    let (tw0, _) = sizes[0];
                    let (_, th1) = sizes[1];
                    let (tw3, _) = sizes[3];
                    let (_, th2) = sizes[2];
                    let quads = [
                        (
                            0,
                            (l, t, ww - fr, ft),
                            uv_rect(0.0, 0.0, (ww - fr) / tw0, 1.0),
                        ),
                        (
                            1,
                            (l + ww - fr, t, fr, wh - fb),
                            uv_rect(0.0, 0.0, 1.0, (wh - fb) / th1),
                        ),
                        (
                            2,
                            (l, t - ft, fl, wh - ft),
                            uv_rect(0.0, 1.0 - (wh - ft) / th2, 1.0, 1.0),
                        ),
                        (
                            3,
                            (l + fl, t - wh + fb, ww - fl, fb),
                            uv_rect(1.0 - (ww - fl) / tw3, 0.0, 1.0, 1.0),
                        ),
                    ];
                    for (i, rect, uv) in quads {
                        let f = frame(i);
                        self.quad(
                            f.material,
                            &[],
                            rect,
                            mtx,
                            frame_color,
                            Some(flip_uv(uv, f.flip)),
                            out,
                        );
                    }
                }
                8 => {
                    let (iw, ih) = (ww - fl - fr, wh - ft - fb);
                    let rects = [
                        (l, t, fl, ft),
                        (l + ww - fr, t, fr, ft),
                        (l, t - wh + fb, fl, fb),
                        (l + ww - fr, t - wh + fb, fr, fb),
                        (l + fl, t, iw, ft),
                        (l + fl, t - wh + fb, iw, fb),
                        (l, t - ft, fl, ih),
                        (l + ww - fr, t - ft, fr, ih),
                    ];
                    for (i, rect) in rects.into_iter().enumerate() {
                        let f = frame(i);
                        self.quad(
                            f.material,
                            &[],
                            rect,
                            mtx,
                            frame_color,
                            Some(flip_uv(UNIT_UV, f.flip)),
                            out,
                        );
                    }
                }
                _ => {}
            }
        } else {
            // horizontal: a left and a right frame (the content between them unless kind 2)
            let left = frame(0);
            let right = if frames.len() > 1 {
                frame(1)
            } else {
                mirrored(left)
            };
            if draw_content {
                let rect = (l + fl - il, t + it, ww - fl - fr + il + ir, wh + it + ib);
                self.quad(
                    w.content.material,
                    content_sets,
                    rect,
                    mtx,
                    content_color,
                    None,
                    out,
                );
                self.quad(
                    left.material,
                    &[],
                    (l, t, fl, wh),
                    mtx,
                    frame_color,
                    Some(flip_uv(UNIT_UV, left.flip)),
                    out,
                );
                self.quad(
                    right.material,
                    &[],
                    (l + ww - fr, t, fr, wh),
                    mtx,
                    frame_color,
                    Some(flip_uv(UNIT_UV, right.flip)),
                    out,
                );
            } else {
                let (tw, _) = self.texture_size(left.material).unwrap_or((1.0, 1.0));
                let lw = ww - fr;
                self.quad(
                    left.material,
                    &[],
                    (l, t, lw, wh),
                    mtx,
                    frame_color,
                    Some(flip_uv(uv_rect(0.0, 0.0, lw / tw, 1.0), left.flip)),
                    out,
                );
                self.quad(
                    right.material,
                    &[],
                    (l + lw, t, fr, wh),
                    mtx,
                    frame_color,
                    Some(flip_uv(UNIT_UV, right.flip)),
                    out,
                );
            }
        }
    }
}

/// The right frame of a one-frame horizontal window: the left one, mirrored.
fn mirrored(f: WindowFrame) -> WindowFrame {
    WindowFrame {
        material: f.material,
        flip: if f.flip == 1 { 0 } else { 1 },
    }
}

#[cfg(test)]
mod tests;
