//! BFLYT layouts: screen size, texture / font / material tables and the pane tree.
//!
//! ```text
//! header  "FLYT" bom u16 hdr_size u32 version u32 file_size u16 sections u16 pad
//! lyt1    u8 origin_type, pad[3], f32 width, f32 height, f32 max_parts_w, f32 max_parts_h, name
//! txl1    u16 count, pad, u32 offsets[count] (from the offset table), names     (textures)
//! fnl1    same as txl1                                                           (fonts)
//! mat1    u16 count, pad, u32 offsets[count] (from the section start), materials
//! pan1 pic1 txt1 wnd1 bnd1 prt1   panes; pas1 / pae1 open and close a child list
//! grp1    group (name, pane names); grs1 / gre1 open and close child groups
//! usd1    user data for the preceding pane (or the layout)
//! ```
//! Every pane section starts with the common pane block (0x4C bytes after the section header):
//! u8 flags, u8 origin, u8 alpha, u8 mag_flags, name[24], user_info[8], f32 translate[3],
//! f32 rotate[3] (degrees), f32 scale[2], f32 size[2].
//!
//! Switch files (version 8 and later, little-endian; SMM2 uses 9.0) differ in two places
//! (checked against every SMM2 layout):
//! - a material starts `name[28], u32 flags, u32 (unknown, always 00 02 04 08 so far), black,
//!   white` instead of `name[28], black, white, u32 flags`, and its texture-coordinate generator
//!   entries are 16 bytes;
//! - a group is `name[34], u16 pane_count, names[24]…` instead of `name[32], u32 count, …`.

use super::{FileHeader, Reader, file_header, sections};
use crate::error::Result;

pub const MAGIC: &[u8; 4] = b"FLYT";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub version: u32,
    pub name: String,
    /// lyt1 origin type (1 = drawn from the centre).
    pub origin_type: u8,
    pub width: f32,
    pub height: f32,
    /// Texture file names as listed (e.g. `WM_Mario_00^l.bflim`).
    pub textures: Vec<String>,
    /// Font file names as listed (e.g. `MessageFont.bffnt`).
    pub fonts: Vec<String>,
    pub materials: Vec<Material>,
    pub root: Pane,
    pub groups: Vec<Group>,
    pub user_data: Vec<UserData>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Material {
    pub name: String,
    /// Colour the texture's 0 maps to ("black colour") and its 1 maps to ("white colour").
    pub black: [u8; 4],
    pub white: [u8; 4],
    pub tex_maps: Vec<TexMap>,
    pub tex_srts: Vec<TexSrt>,
    pub tex_coord_gens: Vec<TexCoordGen>,
    pub tev_stages: Vec<TevStage>,
    /// (function, reference) when alpha compare is enabled.
    pub alpha_compare: Option<(u8, f32)>,
    pub blend: Option<BlendMode>,
    pub blend_logic: Option<BlendMode>,
    pub texture_only: bool,
    pub proj_tex_gens: Vec<ProjTexGen>,
    /// Font shadow (black, white) colours when present.
    pub font_shadow: Option<([u8; 4], [u8; 4])>,
    pub raw_flags: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TexMap {
    /// Index into `Layout::textures`.
    pub texture: u16,
    /// 0 clamp, 1 repeat, 2 mirror.
    pub wrap_s: u8,
    pub wrap_t: u8,
    /// 0 nearest, 1 linear.
    pub min_filter: u8,
    pub mag_filter: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexSrt {
    pub translate: [f32; 2],
    /// Degrees.
    pub rotate: f32,
    pub scale: [f32; 2],
}

impl Default for TexSrt {
    fn default() -> Self {
        Self {
            translate: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TexCoordGen {
    pub matrix: u8,
    /// 0..2 = texture coordinate set n, 3 = orthographic projection, 4 = pane-based
    /// projection, 5 = perspective projection.
    pub source: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TevStage {
    pub color_mode: u8,
    pub alpha_mode: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlendMode {
    pub op: u8,
    pub src: u8,
    pub dst: u8,
    pub logic: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProjTexGen {
    pub translate: [f32; 2],
    pub scale: [f32; 2],
    pub flags: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub kind: PaneKind,
    pub name: String,
    pub user_info: String,
    /// bit 0 visible, bit 1 children inherit this pane's alpha, bit 2 location adjust.
    pub flags: u8,
    /// bits 0-1 x origin (0 centre, 1 left, 2 right), bits 2-3 y origin (0 centre, 1 top,
    /// 2 bottom), bits 4-5 / 6-7 the same for the anchor point in the parent.
    pub origin: u8,
    pub alpha: u8,
    pub mag_flags: u8,
    pub translate: [f32; 3],
    /// Degrees.
    pub rotate: [f32; 3],
    pub scale: [f32; 2],
    pub size: [f32; 2],
    pub user_data: Vec<UserData>,
    pub children: Vec<Pane>,
}

impl Default for Pane {
    fn default() -> Self {
        Self {
            kind: PaneKind::Null,
            name: String::new(),
            user_info: String::new(),
            flags: 1,
            origin: 0,
            alpha: 255,
            mag_flags: 0,
            translate: [0.0; 3],
            rotate: [0.0; 3],
            scale: [1.0; 2],
            size: [0.0; 2],
            user_data: Vec::new(),
            children: Vec::new(),
        }
    }
}

impl Pane {
    pub fn visible(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn influenced_alpha(&self) -> bool {
        self.flags & 2 != 0
    }
    /// Depth-first search by name (this pane included).
    pub fn find(&self, name: &str) -> Option<&Pane> {
        if self.name == name {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }
    /// All panes, depth-first, parents before children.
    pub fn walk<'a>(&'a self, out: &mut Vec<&'a Pane>) {
        out.push(self);
        for c in &self.children {
            c.walk(out);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaneKind {
    Null,
    Picture(Picture),
    Text(Text),
    Window(Window),
    Bounding,
    Part(Part),
}

impl PaneKind {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Null => "pan1",
            Self::Picture(_) => "pic1",
            Self::Text(_) => "txt1",
            Self::Window(_) => "wnd1",
            Self::Bounding => "bnd1",
            Self::Part(_) => "prt1",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picture {
    /// Vertex colours: top-left, top-right, bottom-left, bottom-right.
    pub vertex_colors: [[u8; 4]; 4],
    pub material: u16,
    /// Per texture-coordinate set: (u, v) at TL, TR, BL, BR.
    pub tex_coords: Vec<[[f32; 2]; 4]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Text {
    pub max_bytes: u16,
    pub material: u16,
    /// Index into `Layout::fonts`.
    pub font: u16,
    /// Placement of the text block in the pane, packed like `Pane::origin` (bits 0-3).
    pub position: u8,
    /// 0 = follow `position`, 1 left, 2 centre, 3 right.
    pub line_alignment: u8,
    /// bit 0 shadow, bit 1 restricted length, bit 2 invisible border, bit 4 per-character
    /// transform.
    pub flags: u8,
    pub italic: f32,
    pub top_color: [u8; 4],
    pub bottom_color: [u8; 4],
    pub font_size: [f32; 2],
    pub char_space: f32,
    pub line_space: f32,
    /// Message id (the label in the layout's MSBT file; usually the pane name).
    pub text_id: String,
    /// Text stored in the layout (placeholder; the game normally sets it from the MSBT).
    pub text: String,
    pub shadow_offset: [f32; 2],
    pub shadow_scale: [f32; 2],
    pub shadow_top: [u8; 4],
    pub shadow_bottom: [u8; 4],
    pub shadow_italic: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Window {
    /// Content inflation (px, may be negative): left, right, top, bottom.
    pub inflation: [i16; 4],
    /// Frame thickness (px): left, right, top, bottom.
    pub frame_size: [u16; 4],
    pub flags: u8,
    pub content: Picture,
    pub frames: Vec<WindowFrame>,
}

impl Window {
    /// 0 = around (frames on every side), 1 = horizontal, 2 = horizontal without content.
    pub fn kind(&self) -> u8 {
        (self.flags >> 2) & 3
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowFrame {
    pub material: u16,
    /// 0 none, 1 flip horizontally, 2 flip vertically, 3 rotate 90, 4 rotate 180, 5 rotate 270.
    pub flip: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Part {
    /// Name of the layout drawn by this pane.
    pub layout: String,
    pub magnify: [f32; 2],
    pub properties: Vec<PartProperty>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartProperty {
    /// Pane in the part layout this entry overrides.
    pub name: String,
    pub usage: u8,
    pub basic_usage: u8,
    pub material_usage: u8,
    /// Replacement pane (when the override carries one).
    pub pane: Option<Box<Pane>>,
    /// Basic-info override values (which ones apply is given by `basic_usage`).
    pub basic: Option<PartBasicInfo>,
}

/// Basic values a part property can override on a part's pane. `basic_usage` bits [inf, from
/// the data]: 0 visibility override on, 1 visible value, 2 user info, 3 translate, 4 size,
/// 5 scale, 6 rotate, 7 alpha.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartBasicInfo {
    pub user_info: String,
    pub translate: [f32; 3],
    pub rotate: [f32; 3],
    pub scale: [f32; 2],
    pub size: [f32; 2],
    pub alpha: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub name: String,
    pub panes: Vec<String>,
    pub children: Vec<Group>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UserData {
    pub name: String,
    pub value: UserDataValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UserDataValue {
    String(String),
    Ints(Vec<i32>),
    Floats(Vec<f32>),
    Other(u8),
}

impl Layout {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let h = file_header(data, MAGIC, "BFLYT")?;
        let r = Reader::new(data, h.be, "BFLYT");
        parse_with(&r, &h)
    }

    /// Texture base name (file name without the `.bflim` extension).
    pub fn texture_stem(name: &str) -> &str {
        name.strip_suffix(".bflim").unwrap_or(name)
    }

    /// All panes, depth-first.
    pub fn panes(&self) -> Vec<&Pane> {
        let mut v = Vec::new();
        self.root.walk(&mut v);
        v
    }
}

fn is_pane(magic: &[u8; 4]) -> bool {
    matches!(
        magic,
        b"pan1" | b"pic1" | b"txt1" | b"wnd1" | b"bnd1" | b"prt1"
    )
}

fn name_table(r: &Reader, body: usize) -> Result<Vec<String>> {
    let count = r.u16(body)? as usize;
    let table = body + 4;
    (0..count)
        .map(|i| r.cstr(table + r.u32(table + i * 4)? as usize))
        .collect()
}

fn parse_with(r: &Reader, h: &FileHeader) -> Result<Layout> {
    let mut layout = Layout {
        version: h.version,
        ..Default::default()
    };
    // pane tree under construction: a stack of child lists; `last` = most recent pane path
    let mut stack: Vec<Vec<Pane>> = vec![Vec::new()];
    let mut group_stack: Vec<Vec<Group>> = vec![Vec::new()];
    let mut prev = [0u8; 4];
    for (magic, o, size) in sections(r, h)? {
        let after_pane = is_pane(&prev);
        prev = magic;
        let body = o + 8;
        match &magic {
            b"lyt1" => {
                layout.origin_type = r.u8(body)?;
                layout.width = r.f32(body + 4)?;
                layout.height = r.f32(body + 8)?;
                layout.name = r.cstr(body + 0x14)?;
            }
            b"txl1" => layout.textures = name_table(r, body)?,
            b"fnl1" => layout.fonts = name_table(r, body)?,
            b"mat1" => {
                let count = r.u16(body)? as usize;
                for i in 0..count {
                    let start = o + r.u32(body + 4 + i * 4)? as usize;
                    let end = if i + 1 < count {
                        o + r.u32(body + 4 + (i + 1) * 4)? as usize
                    } else {
                        o + size
                    };
                    layout
                        .materials
                        .push(parse_material(r, start, end, is_nx(h.version))?);
                }
            }
            b"pan1" | b"pic1" | b"txt1" | b"wnd1" | b"bnd1" | b"prt1" => {
                let pane = parse_pane(r, o, size)?;
                stack.last_mut().unwrap().push(pane);
            }
            b"pas1" => stack.push(Vec::new()),
            b"pae1" => {
                let children = stack.pop().unwrap_or_default();
                if stack.is_empty() {
                    return Err(r.err("pae1 without pas1"));
                }
                match stack.last_mut().unwrap().last_mut() {
                    Some(parent) => parent.children = children,
                    None => return Err(r.err("pas1 before any pane")),
                }
            }
            b"grp1" => {
                let (name, count) = if is_nx(h.version) {
                    (r.fixed_str(body, 0x22)?, r.u16(body + 0x22)? as usize)
                } else {
                    (r.cstr(body)?, r.u32(body + 0x20)? as usize)
                };
                let panes = (0..count)
                    .map(|i| r.fixed_str(body + 0x24 + i * 24, 24))
                    .collect::<Result<_>>()?;
                group_stack.last_mut().unwrap().push(Group {
                    name,
                    panes,
                    children: Vec::new(),
                });
            }
            b"grs1" => group_stack.push(Vec::new()),
            b"gre1" => {
                let children = group_stack.pop().unwrap_or_default();
                if group_stack.is_empty() {
                    return Err(r.err("gre1 without grs1"));
                }
                if let Some(parent) = group_stack.last_mut().unwrap().last_mut() {
                    parent.children = children;
                }
            }
            b"usd1" => {
                let ud = parse_user_data(r, body)?;
                // attach to the most recent pane at the current level, else to the layout
                let target = if after_pane {
                    stack.last_mut().and_then(|l| l.last_mut())
                } else {
                    None
                };
                match target {
                    Some(p) => p.user_data.extend(ud),
                    None => layout.user_data.extend(ud),
                }
            }
            _ => {} // cnt1 and anything newer: not needed for drawing
        }
    }
    let mut top = stack.into_iter().next().unwrap_or_default();
    layout.root = match top.len() {
        0 => Pane {
            name: "RootPane".into(),
            ..Default::default()
        },
        1 => top.pop().unwrap(),
        _ => Pane {
            name: "RootPane".into(),
            children: top,
            ..Default::default()
        },
    };
    layout.groups = group_stack.into_iter().next().unwrap_or_default();
    Ok(layout)
}

/// Switch layout family (version 8.0 and later).
pub(crate) fn is_nx(version: u32) -> bool {
    version >= 0x0800_0000
}

fn parse_material(r: &Reader, start: usize, end: usize, nx: bool) -> Result<Material> {
    // texture-coordinate generator entries are 16 bytes in Switch files, 8 in Wii U v5+ files
    // and 4 in older ones; pick the one that makes the material end where the offset table says
    let sizes: &[usize] = if nx { &[16, 8, 4] } else { &[8, 4] };
    let mut first = None;
    for &gen_size in sizes {
        let (m, e) = parse_material_with(r, start, gen_size, nx)?;
        if e == end {
            return Ok(m);
        }
        first.get_or_insert(m);
    }
    if nx {
        // Switch materials can carry blocks this parser doesn't know; say so rather than guess
        return Err(r.err(format!(
            "material at {start:#x} does not end at {end:#x} (flags {:#x})",
            r.u32(start + 0x1C)?
        )));
    }
    Ok(first.unwrap())
}

fn parse_material_with(
    r: &Reader,
    start: usize,
    gen_size: usize,
    nx: bool,
) -> Result<(Material, usize)> {
    let (black_at, white_at, flags_at, data_at) = if nx {
        (0x24, 0x28, 0x1C, 0x2C)
    } else {
        (0x1C, 0x20, 0x24, 0x28)
    };
    let mut m = Material {
        name: r.fixed_str(start, 0x1C)?,
        black: r.rgba(start + black_at)?,
        white: r.rgba(start + white_at)?,
        ..Default::default()
    };
    let flags = r.u32(start + flags_at)?;
    m.raw_flags = flags;
    let tex_count = flags & 3;
    let mtx_count = (flags >> 2) & 3;
    let gen_count = (flags >> 4) & 3;
    let tev_count = (flags >> 6) & 7;
    let alpha_compare = (flags >> 9) & 1 != 0;
    let blend = (flags >> 10) & 1 != 0;
    m.texture_only = (flags >> 11) & 1 != 0;
    let blend_logic = (flags >> 12) & 1 != 0;
    let ind = (flags >> 14) & 1 != 0;
    let proj_count = (flags >> 15) & 3;
    let font_shadow = (flags >> 17) & 1 != 0;
    let mut o = start + data_at;
    for _ in 0..tex_count {
        let a = r.u8(o + 2)?;
        let b = r.u8(o + 3)?;
        m.tex_maps.push(TexMap {
            texture: r.u16(o)?,
            wrap_s: a & 3,
            wrap_t: b & 3,
            min_filter: (a >> 2) & 3,
            mag_filter: (b >> 2) & 3,
        });
        o += 4;
    }
    for _ in 0..mtx_count {
        m.tex_srts.push(TexSrt {
            translate: [r.f32(o)?, r.f32(o + 4)?],
            rotate: r.f32(o + 8)?,
            scale: [r.f32(o + 12)?, r.f32(o + 16)?],
        });
        o += 20;
    }
    for _ in 0..gen_count {
        m.tex_coord_gens.push(TexCoordGen {
            matrix: r.u8(o)?,
            source: r.u8(o + 1)?,
        });
        o += gen_size;
    }
    for _ in 0..tev_count {
        m.tev_stages.push(TevStage {
            color_mode: r.u8(o)?,
            alpha_mode: r.u8(o + 1)?,
        });
        o += 4;
    }
    if alpha_compare {
        m.alpha_compare = Some((r.u8(o)?, r.f32(o + 4)?));
        o += 8;
    }
    if blend {
        m.blend = Some(BlendMode {
            op: r.u8(o)?,
            src: r.u8(o + 1)?,
            dst: r.u8(o + 2)?,
            logic: r.u8(o + 3)?,
        });
        o += 4;
    }
    if blend_logic {
        m.blend_logic = Some(BlendMode {
            op: r.u8(o)?,
            src: r.u8(o + 1)?,
            dst: r.u8(o + 2)?,
            logic: r.u8(o + 3)?,
        });
        o += 4;
    }
    if ind {
        o += 12; // indirect: rotate, scale x, scale y
    }
    for _ in 0..proj_count {
        m.proj_tex_gens.push(ProjTexGen {
            translate: [r.f32(o)?, r.f32(o + 4)?],
            scale: [r.f32(o + 8)?, r.f32(o + 12)?],
            flags: r.u8(o + 16)?,
        });
        o += 20;
    }
    if font_shadow {
        let b = r.bytes(o, 8)?;
        m.font_shadow = Some(([b[0], b[1], b[2], 255], [b[4], b[5], b[6], b[7]]));
        o += 8;
    }
    Ok((m, o))
}

fn parse_picture_block(r: &Reader, o: usize) -> Result<(Picture, usize)> {
    let mut p = Picture::default();
    for (i, c) in p.vertex_colors.iter_mut().enumerate() {
        *c = r.rgba(o + i * 4)?;
    }
    p.material = r.u16(o + 16)?;
    let n = r.u8(o + 18)? as usize;
    let mut q = o + 20;
    for _ in 0..n {
        let mut set = [[0f32; 2]; 4];
        for (k, uv) in set.iter_mut().enumerate() {
            *uv = [r.f32(q + k * 8)?, r.f32(q + k * 8 + 4)?];
        }
        p.tex_coords.push(set);
        q += 32;
    }
    Ok((p, q))
}

/// One pane section at `o` (section header included).
pub(crate) fn parse_pane(r: &Reader, o: usize, size: usize) -> Result<Pane> {
    let magic: [u8; 4] = r.bytes(o, 4)?.try_into().unwrap();
    let b = o + 8;
    let mut pane = Pane {
        flags: r.u8(b)?,
        origin: r.u8(b + 1)?,
        alpha: r.u8(b + 2)?,
        mag_flags: r.u8(b + 3)?,
        name: r.fixed_str(b + 4, 24)?,
        user_info: r.fixed_str(b + 0x1C, 8)?,
        translate: [r.f32(b + 0x24)?, r.f32(b + 0x28)?, r.f32(b + 0x2C)?],
        rotate: [r.f32(b + 0x30)?, r.f32(b + 0x34)?, r.f32(b + 0x38)?],
        scale: [r.f32(b + 0x3C)?, r.f32(b + 0x40)?],
        size: [r.f32(b + 0x44)?, r.f32(b + 0x48)?],
        ..Default::default()
    };
    let x = o + 0x54; // kind-specific data
    pane.kind = match &magic {
        b"pan1" => PaneKind::Null,
        b"bnd1" => PaneKind::Bounding,
        b"pic1" => PaneKind::Picture(parse_picture_block(r, x)?.0),
        b"txt1" => {
            let text_offset = r.u32(x + 0x10)? as usize;
            let str_bytes = r.u16(x + 2)? as usize;
            let id_offset = r.u32(x + 0x2C)? as usize;
            PaneKind::Text(Text {
                max_bytes: r.u16(x)?,
                material: r.u16(x + 4)?,
                font: r.u16(x + 6)?,
                position: r.u8(x + 8)?,
                line_alignment: r.u8(x + 9)?,
                flags: r.u8(x + 10)?,
                italic: r.f32(x + 0x0C)?,
                top_color: r.rgba(x + 0x14)?,
                bottom_color: r.rgba(x + 0x18)?,
                font_size: [r.f32(x + 0x1C)?, r.f32(x + 0x20)?],
                char_space: r.f32(x + 0x24)?,
                line_space: r.f32(x + 0x28)?,
                text_id: if id_offset != 0 && id_offset < size {
                    r.cstr(o + id_offset)?
                } else {
                    String::new()
                },
                text: if text_offset != 0 && text_offset < size {
                    r.utf16(o + text_offset, str_bytes / 2)?
                } else {
                    String::new()
                },
                shadow_offset: [r.f32(x + 0x30)?, r.f32(x + 0x34)?],
                shadow_scale: [r.f32(x + 0x38)?, r.f32(x + 0x3C)?],
                shadow_top: r.rgba(x + 0x40)?,
                shadow_bottom: r.rgba(x + 0x44)?,
                shadow_italic: r.f32(x + 0x48)?,
            })
        }
        b"wnd1" => {
            let mut w = Window::default();
            for i in 0..4 {
                w.inflation[i] = r.i16(x + i * 2)?;
                w.frame_size[i] = r.u16(x + 8 + i * 2)?;
            }
            let frame_count = r.u8(x + 0x10)? as usize;
            w.flags = r.u8(x + 0x11)?;
            let content = o + r.u32(x + 0x14)? as usize;
            let table = o + r.u32(x + 0x18)? as usize;
            w.content = parse_picture_block(r, content)?.0;
            for i in 0..frame_count {
                let f = o + r.u32(table + i * 4)? as usize;
                w.frames.push(WindowFrame {
                    material: r.u16(f)?,
                    flip: r.u8(f + 2)?,
                });
            }
            PaneKind::Window(w)
        }
        b"prt1" => {
            let count = r.u32(x)? as usize;
            let mut part = Part {
                magnify: [r.f32(x + 4)?, r.f32(x + 8)?],
                ..Default::default()
            };
            let props = x + 12;
            for i in 0..count {
                let p = props + i * 40;
                let pane_off = r.u32(p + 28)? as usize;
                let pane = if pane_off != 0 && pane_off + 8 <= size {
                    let po = o + pane_off;
                    let psize = r.u32(po + 4)? as usize;
                    let m = r.bytes(po, 4)?;
                    if matches!(m, b"pan1" | b"pic1" | b"txt1" | b"wnd1" | b"bnd1") {
                        Some(Box::new(parse_pane(r, po, psize)?))
                    } else {
                        None
                    }
                } else {
                    None
                };
                let basic_off = r.u32(p + 36)? as usize;
                let basic = if basic_off != 0 && basic_off + 52 <= size {
                    let b = o + basic_off;
                    let f = |i: usize| r.f32(b + 8 + i * 4);
                    Some(PartBasicInfo {
                        user_info: r.fixed_str(b, 8)?,
                        translate: [f(0)?, f(1)?, f(2)?],
                        rotate: [f(3)?, f(4)?, f(5)?],
                        scale: [f(6)?, f(7)?],
                        size: [f(8)?, f(9)?],
                        alpha: r.u8(b + 48)?,
                    })
                } else {
                    None
                };
                part.properties.push(PartProperty {
                    name: r.fixed_str(p, 24)?,
                    usage: r.u8(p + 24)?,
                    basic_usage: r.u8(p + 25)?,
                    material_usage: r.u8(p + 26)?,
                    pane,
                    basic,
                });
            }
            part.layout = r.cstr(props + count * 40)?;
            PaneKind::Part(part)
        }
        _ => PaneKind::Null,
    };
    Ok(pane)
}

fn parse_user_data(r: &Reader, body: usize) -> Result<Vec<UserData>> {
    let count = r.u16(body)? as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = body + 4 + i * 12;
        let name_off = r.u32(e)? as usize;
        let data_off = r.u32(e + 4)? as usize;
        let n = r.u16(e + 8)? as usize;
        let kind = r.u8(e + 10)?;
        let name = if name_off != 0 {
            r.cstr(e + name_off)?
        } else {
            String::new()
        };
        let d = e + data_off;
        let value = match kind {
            0 => UserDataValue::String(String::from_utf8_lossy(r.bytes(d, n)?).into_owned()),
            1 => UserDataValue::Ints(
                (0..n)
                    .map(|k| r.u32(d + k * 4).map(|v| v as i32))
                    .collect::<Result<_>>()?,
            ),
            2 => UserDataValue::Floats((0..n).map(|k| r.f32(d + k * 4)).collect::<Result<_>>()?),
            k => UserDataValue::Other(k),
        };
        out.push(UserData { name, value });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::sample_bflyt;

    #[test]
    fn parses_synthetic_layout() {
        let l = Layout::parse(&sample_bflyt()).unwrap();
        assert_eq!(l.name, "Sample");
        assert_eq!((l.width, l.height), (1280.0, 720.0));
        assert_eq!(l.textures, ["Box^s.bflim"]);
        assert_eq!(l.fonts, ["MessageFont.bffnt"]);
        let m = &l.materials[0];
        assert_eq!(m.name, "M_Box");
        assert_eq!(m.white, [200, 100, 50, 255]);
        assert_eq!(m.tex_maps[0].wrap_s, 1);
        assert_eq!(m.tex_srts[0].translate, [0.25, 0.0]);
        assert_eq!(m.tex_coord_gens.len(), 1);
        assert_eq!(l.root.name, "RootPane");
        assert_eq!(l.root.children.len(), 2);
        let pic = l.root.find("P_Box").unwrap();
        assert_eq!(pic.translate[0], 100.0);
        assert_eq!(pic.user_data[0].name, "Scalable");
        assert_eq!(pic.user_data[0].value, UserDataValue::Ints(vec![80]));
        let PaneKind::Picture(p) = &pic.kind else {
            panic!()
        };
        assert_eq!(p.tex_coords[0][3], [1.0, 1.0]);
        let PaneKind::Text(t) = &l.root.find("T_Label").unwrap().kind else {
            panic!()
        };
        assert_eq!(t.text, "Hi");
        assert_eq!(t.text_id, "T_Label");
        assert_eq!(t.bottom_color, [255, 0, 0, 255]);
        assert_eq!(t.font_size, [24.0, 28.0]);
        assert_eq!(l.groups[0].panes, ["P_Box"]);
    }

    #[test]
    fn parses_switch_layout() {
        let l = Layout::parse(&crate::fixtures::sample_bflyt_nx()).unwrap();
        assert_eq!(l.version, 0x0900_0000);
        assert_eq!(l.name, "SampleNx");
        assert_eq!(l.textures, ["Box^s"]);
        assert_eq!(Layout::texture_stem(&l.textures[0]), "Box^s");
        assert_eq!(l.fonts, ["MessageFont.fcpx"]);
        let m = &l.materials[0];
        assert_eq!((m.black, m.white), ([10, 20, 30, 0], [200, 100, 50, 255]));
        assert_eq!(m.tex_maps.len(), 1);
        assert_eq!(m.tex_srts[0].translate, [0.25, 0.0]);
        assert_eq!(m.tex_coord_gens.len(), 1);
        assert_eq!(l.materials[1].white, [28, 3, 3, 255]);
        assert_eq!(l.groups[0].name, "G_All");
        assert_eq!(l.groups[0].panes, ["P_Box", "T_Msg"]);
        let PaneKind::Part(p) = &l.root.find("L_Btn").unwrap().kind else {
            panic!()
        };
        assert_eq!(p.layout, "Sample");
        let over = p.properties[0].pane.as_ref().unwrap();
        let PaneKind::Text(t) = &over.kind else {
            panic!()
        };
        assert_eq!(t.text, "Ov");
        let PaneKind::Text(t) = &l.root.find("T_Msg").unwrap().kind else {
            panic!()
        };
        assert_eq!(
            (t.text.as_str(), t.material, t.line_alignment),
            ("Hi", 1, 2)
        );
        assert_eq!(t.font_size, [24.0, 36.0]);
    }

    #[test]
    fn switch_material_with_unknown_blocks_is_an_error() {
        // claim an extra TEV stage: the material no longer ends where the table says
        let mut d = crate::fixtures::sample_bflyt_nx();
        let at = d.windows(5).position(|w| w == b"M_Box").unwrap() + 0x1C;
        d[at] |= 1 << 6;
        assert!(Layout::parse(&d).is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(Layout::parse(b"nope").is_err());
        let mut d = sample_bflyt();
        d.truncate(0x40);
        assert!(Layout::parse(&d).is_err());
    }
}
