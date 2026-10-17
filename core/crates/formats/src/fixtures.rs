//! Synthetic files in the formats this crate reads, built from scratch for tests (this crate's
//! and the importer / layout player's). They contain no game data.

use crate::lyt::bflan::MAGIC as FLAN;
use crate::lyt::bflyt::MAGIC as FLYT;

#[derive(Default)]
pub struct W(pub Vec<u8>);

impl W {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.u32(v.to_bits())
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }
    pub fn name(&mut self, s: &str, n: usize) -> &mut Self {
        let mut b = s.as_bytes().to_vec();
        b.resize(n, 0);
        self.bytes(&b)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn pad_to(&mut self, align: usize) -> &mut Self {
        while !self.0.len().is_multiple_of(align) {
            self.0.push(0);
        }
        self
    }
}

/// A section: magic + u32 size + body.
pub fn section(magic: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = magic.to_vec();
    v.extend_from_slice(&((body.len() + 8) as u32).to_be_bytes());
    v.extend_from_slice(body);
    v
}

/// A whole file: 0x14-byte header (big-endian) + sections.
pub fn file(magic: &[u8; 4], version: u32, sections: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = sections.concat();
    let mut v = magic.to_vec();
    v.extend_from_slice(&[0xFE, 0xFF]);
    v.extend_from_slice(&0x14u16.to_be_bytes());
    v.extend_from_slice(&version.to_be_bytes());
    v.extend_from_slice(&((0x14 + body.len()) as u32).to_be_bytes());
    v.extend_from_slice(&(sections.len() as u16).to_be_bytes());
    v.extend_from_slice(&[0, 0]);
    v.extend_from_slice(&body);
    v
}

pub fn pane_block(w: &mut W, name: &str, origin: u8, t: [f32; 2], size: [f32; 2]) {
    w.u8(1).u8(origin).u8(255).u8(0).name(name, 24).name("", 8);
    w.f32(t[0]).f32(t[1]).f32(0.0);
    w.f32(0.0).f32(0.0).f32(0.0);
    w.f32(1.0).f32(1.0);
    w.f32(size[0]).f32(size[1]);
}

/// A small synthetic layout: Root → [P_Box (picture, material 0, texture 0), T_Label (text)].
pub fn sample_bflyt() -> Vec<u8> {
    let mut lyt = W::default();
    lyt.u8(1)
        .bytes(&[0; 3])
        .f32(1280.0)
        .f32(720.0)
        .f32(0.0)
        .f32(0.0);
    lyt.bytes(b"Sample\0").pad_to(4);

    let mut txl = W::default();
    txl.u16(1).u16(0).u32(4).bytes(b"Box^s.bflim\0").pad_to(4);
    let mut fnl = W::default();
    fnl.u16(1)
        .u16(0)
        .u32(4)
        .bytes(b"MessageFont.bffnt\0")
        .pad_to(4);

    // material: 1 texture, 1 SRT, 1 coord gen (8 bytes), no TEV
    let mut mat = W::default();
    mat.name("M_Box", 0x1C)
        .bytes(&[10, 20, 30, 0])
        .bytes(&[200, 100, 50, 255]);
    mat.u32(1 | (1 << 2) | (1 << 4));
    mat.u16(0).u8(1).u8(0); // texture 0, wrap s repeat, wrap t clamp
    mat.f32(0.25).f32(0.0).f32(0.0).f32(1.0).f32(1.0);
    mat.u8(0).u8(0).u16(0).u32(0);
    let mut mat1 = W::default();
    mat1.u16(1).u16(0).u32(8 + 4 + 4); // offset from section start
    mat1.bytes(&mat.0);

    let mut root = W::default();
    pane_block(&mut root, "RootPane", 0, [0.0, 0.0], [1280.0, 720.0]);
    let mut pic = W::default();
    pane_block(&mut pic, "P_Box", 0, [100.0, -50.0], [64.0, 32.0]);
    for _ in 0..4 {
        pic.bytes(&[255, 255, 255, 255]);
    }
    pic.u16(0).u8(1).u8(0);
    for uv in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
        pic.f32(uv[0]).f32(uv[1]);
    }
    let mut txt = W::default();
    pane_block(&mut txt, "T_Label", 5, [0.0, 0.0], [300.0, 40.0]);
    let text: Vec<u8> = "Hi".encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
    txt.u16(6)
        .u16(4)
        .u16(0)
        .u16(0)
        .u8(5)
        .u8(0)
        .u8(0)
        .u8(0)
        .f32(0.0);
    txt.u32(0x54 + 0x50); // text offset (from section start)
    txt.bytes(&[255, 255, 255, 255]).bytes(&[255, 0, 0, 255]);
    txt.f32(24.0).f32(28.0).f32(1.0).f32(2.0);
    txt.u32(0x54 + 0x50 + 8); // text id offset
    txt.f32(1.0).f32(1.0).f32(1.0).f32(1.0);
    txt.bytes(&[0, 0, 0, 255])
        .bytes(&[0, 0, 0, 255])
        .f32(0.0)
        .u32(0);
    txt.bytes(&text).u16(0).u16(0).bytes(b"T_Label\0").pad_to(4);

    let mut grp = W::default();
    grp.name("G_All", 0x20).u32(1).name("P_Box", 24);

    let mut usd = W::default();
    usd.u16(1)
        .u16(0)
        .u32(0x10)
        .u32(0xC)
        .u16(1)
        .u8(1)
        .u8(0)
        .u32(80);
    usd.bytes(b"Scalable\0").pad_to(4);

    file(
        FLYT,
        0x0502_0000,
        &[
            section(b"lyt1", &lyt.0),
            section(b"txl1", &txl.0),
            section(b"fnl1", &fnl.0),
            section(b"mat1", &mat1.0),
            section(b"pan1", &root.0),
            section(b"pas1", &[]),
            section(b"pic1", &pic.0),
            section(b"usd1", &usd.0),
            section(b"txt1", &txt.0),
            section(b"pae1", &[]),
            section(b"grp1", &grp.0),
        ],
    )
}

/// Synthetic animation "Move": P_Box TX hermite 0→100 over 10 frames, visibility step off
/// at frame 5; material M_Box pattern step to texture 1 at frame 4.
pub fn sample_bflan() -> Vec<u8> {
    let mut pat = W::default();
    pat.u16(0)
        .u16(1)
        .u32(0x1C)
        .u32(0x24)
        .u16(0)
        .u16(10)
        .u8(0)
        .bytes(&[0; 3]);
    pat.bytes(b"Move\0\0\0\0").name("G_All", 0x24);

    // pai1 body (offsets are from the section start = body - 8)
    let mut pai = W::default();
    pai.u16(10).u8(0).u8(0).u16(2).u16(2);
    let table_pos = pai.len();
    pai.u32(0); // patched below
    // texture table at body+12
    let tex_table = pai.len();
    pai.u32(8).u32(8 + 6);
    pai.bytes(b"Tex0\0\0Tex1\0\0");
    pai.pad_to(4);
    let content_table = pai.len();
    pai.u32(0).u32(0);
    // content 0: pane P_Box with FLPA + FLVI
    let c0 = pai.len();
    pai.name("P_Box", 28).u8(2).u8(0).u16(0);
    let c0_tags = pai.len();
    pai.u32(0).u32(0);
    let tag0 = pai.len();
    pai.bytes(b"FLPA").u8(1).bytes(&[0; 3]).u32(12);
    pai.u8(0).u8(0).u8(2).u8(0).u16(2).u16(0).u32(12);
    pai.f32(0.0).f32(0.0).f32(0.0).f32(10.0).f32(100.0).f32(0.0);
    let tag1 = pai.len();
    pai.bytes(b"FLVI").u8(1).bytes(&[0; 3]).u32(12);
    pai.u8(0).u8(0).u8(1).u8(0).u16(2).u16(0).u32(12);
    pai.f32(0.0).u16(1).u16(0).f32(5.0).u16(0).u16(0);
    // content 1: material M_Box with FLTP
    let c1 = pai.len();
    pai.name("M_Box", 28).u8(1).u8(1).u16(0);
    pai.u32(36);
    pai.bytes(b"FLTP").u8(1).bytes(&[0; 3]).u32(12);
    pai.u8(0).u8(0).u8(1).u8(0).u16(2).u16(0).u32(12);
    pai.f32(0.0).u16(0).u16(0).f32(4.0).u16(1).u16(0);
    let mut b = pai.0;
    let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_be_bytes());
    put(&mut b, table_pos, (content_table + 8) as u32);
    put(&mut b, content_table, (c0 + 8) as u32);
    put(&mut b, content_table + 4, (c1 + 8) as u32);
    put(&mut b, c0_tags, (tag0 - c0) as u32);
    put(&mut b, c0_tags + 4, (tag1 - c0) as u32);
    let _ = tex_table;
    file(
        FLAN,
        0x0502_0000,
        &[section(b"pat1", &pat.0), section(b"pai1", &b)],
    )
}

/// A synthetic font: one 64×32 A8 sheet (linear), cells 7×9, glyphs for 'A' 'B' (table)
/// and '0'..'2' (direct), and one scan entry for '!'.
pub fn sample_font() -> Vec<u8> {
    let sheet: Vec<u8> = (0..64 * 32).map(|i| (i % 251) as u8).collect();
    let mut w = W::default();
    // header (patched size)
    w.bytes(b"FFNT")
        .u16(0xFEFF)
        .u16(0x14)
        .u32(0x0300_0000)
        .u32(0)
        .u16(4)
        .u16(0);
    // FINF @0x14
    let finf = w.len();
    w.bytes(b"FINF").u32(0x20);
    w.u8(1)
        .u8(9)
        .u8(7)
        .u8(8)
        .u16(10)
        .u16(0)
        .u8(0)
        .u8(5)
        .u8(6)
        .u8(1);
    let ptrs = w.len();
    w.u32(0).u32(0).u32(0);
    // TGLP
    let tglp = w.len();
    w.bytes(b"TGLP").u32(0x20);
    w.u8(7)
        .u8(9)
        .u8(1)
        .u8(7)
        .u32(sheet.len() as u32)
        .u16(8)
        .u16(0x8008);
    w.u16(8).u16(3).u16(64).u16(32);
    let data_ptr = w.len();
    w.u32(0);
    // CWDH: glyphs 0..=4
    let cwdh = w.len();
    w.bytes(b"CWDH").u32(8 + 8 + 15 + 1).u16(0).u16(4).u32(0);
    for g in 0..5u8 {
        w.u8(g).u8(5 + g).u8(6 + g);
    }
    w.pad_to(4);
    // CMAP 1: table 'A'..='B' → 0, 1 ; CMAP 2: direct '0'..='2' → 2.. ; CMAP 3: scan '!' → 4
    let cmap1 = w.len();
    w.bytes(b"CMAP")
        .u32(8 + 12 + 4)
        .u16(b'A' as u16)
        .u16(b'B' as u16)
        .u16(1)
        .u16(0);
    let next1 = w.len();
    w.u32(0).u16(0).u16(1);
    let cmap2 = w.len();
    w.bytes(b"CMAP")
        .u32(8 + 12 + 4)
        .u16(b'0' as u16)
        .u16(b'2' as u16)
        .u16(0)
        .u16(0);
    let next2 = w.len();
    w.u32(0).u16(2).u16(0);
    let cmap3 = w.len();
    w.bytes(b"CMAP")
        .u32(8 + 12 + 6)
        .u16(0)
        .u16(0xFFFF)
        .u16(2)
        .u16(0)
        .u32(0);
    w.u16(1).u16(b'!' as u16).u16(4);
    w.pad_to(0x100);
    let data = w.len();
    w.bytes(&sheet);
    let mut b = w.0;
    let put = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_be_bytes());
    let total = b.len() as u32;
    put(&mut b, 0x0C, total);
    put(&mut b, ptrs, (tglp + 8) as u32);
    put(&mut b, ptrs + 4, (cwdh + 8) as u32);
    put(&mut b, ptrs + 8, (cmap1 + 8) as u32);
    put(&mut b, data_ptr, data as u32);
    put(&mut b, next1, (cmap2 + 8) as u32);
    put(&mut b, next2, (cmap3 + 8) as u32);
    let _ = finf;
    b
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

fn msbt_section(w: &mut W, magic: &[u8; 4], body: &[u8]) {
    w.bytes(magic)
        .u32(body.len() as u32)
        .bytes(&[0; 8])
        .bytes(body);
    while !w.len().is_multiple_of(16) {
        w.u8(0xAB);
    }
}

pub fn sample_msbt() -> Vec<u8> {
    // labels: "T_Title_00" → 1, "T_Lives_00" → 0 (one bucket)
    let mut lbl = W::default();
    lbl.u32(1).u32(2).u32(12); // 1 bucket: 2 labels at offset 12
    lbl.u8(10).bytes(b"T_Title_00").u32(1);
    lbl.u8(10).bytes(b"T_Lives_00").u32(0);
    // texts: 0 = "x" + tag (group 2, type 2, params 00 02) + close tag + "!",
    // 1 = "Hello\nWorld"
    let t0: Vec<u8> = [
        utf16("x"),
        vec![0, 0x0E, 0, 2, 0, 2, 0, 2, 0, 2],
        vec![0, 0x0F, 0, 2, 0, 2],
        utf16("!"),
        vec![0, 0],
    ]
    .concat();
    let t1: Vec<u8> = [utf16("Hello\nWorld"), vec![0, 0]].concat();
    let mut txt = W::default();
    txt.u32(2).u32(12).u32(12 + t0.len() as u32);
    txt.bytes(&t0).bytes(&t1);

    let mut w = W::default();
    w.bytes(b"MsgStdBn")
        .u16(0xFEFF)
        .u16(0)
        .u8(1)
        .u8(3)
        .u16(2)
        .u16(0)
        .u32(0);
    w.bytes(&[0; 10]);
    msbt_section(&mut w, b"LBL1", &lbl.0);
    msbt_section(&mut w, b"TXT2", &txt.0);
    w.0
}
