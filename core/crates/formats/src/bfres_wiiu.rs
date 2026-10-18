//! Wii U BFRES (FRES version 3, big-endian): the embedded textures (FTEX) only — enough for the
//! SMM1 costume art, whose files hold nothing else.
//!
//! Layout (from the public Wii U BFRES descriptions): the header holds 12 relative offsets to
//! index groups (`0x20 + 4·i`) and their entry counts (u16 at `0x50 + 2·i`); group 1 lists the
//! textures. An index group is `u32 size, i32 count`, then `count + 1` 16-byte nodes (the first
//! is the root): `u32 search, u16 left, u16 right, i32 name, i32 data`, the last two relative to
//! their own field. An FTEX starts with its magic and a GX2 surface descriptor (dimension, width,
//! height, depth, mip count, format, AA, use, image size, image pointer, mip size, mip pointer,
//! tile mode, swizzle, alignment, pitch, 13 mip offsets), then the view (first mip, mip count,
//! first slice, slice count), the four channel selectors, 5 registers, a handle, the array
//! length, and relative offsets to the name, the path, the image data and the mip data.

use crate::bflim::{FlimFormat, IDENTITY, Image, decode_surface};
use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BFRES (Wii U)",
        detail: detail.into(),
    }
}

/// One FTEX: the GX2 surface fields needed to decode mip 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Texture {
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// `GX2SurfaceFormat`.
    pub format: u32,
    pub tile_mode: u32,
    pub swizzle: u32,
    /// Channel selectors (GX2: 0 R, 1 G, 2 B, 3 A, 4 zero, 5 one).
    pub comp_sel: [u8; 4],
    /// Mip 0, still tiled.
    pub data: Vec<u8>,
}

/// GX2 surface format → the BFLIM format with the same element layout.
fn flim_format(gx2: u32) -> Option<FlimFormat> {
    use FlimFormat::*;
    Some(match gx2 {
        0x001 => L8,
        0x007 => La8,
        0x008 => Rgb565,
        0x00a => Rgb5a1,
        0x00b => Rgba4,
        0x01a => Rgba8,
        0x41a => Rgba8Srgb,
        0x031 => Bc1,
        0x431 => Bc1Srgb,
        0x032 => Bc2,
        0x432 => Bc2Srgb,
        0x033 => Bc3,
        0x433 => Bc3Srgb,
        0x034 => Bc4L,
        0x035 => Bc5La,
        _ => return None,
    })
}

impl Texture {
    /// Untile and convert mip 0. The channel selectors are applied through
    /// `Image::channel_select` (BC4/BC5 keep their luminance mapping when the selectors are the
    /// identity).
    pub fn decode(&self) -> Result<Image> {
        let fmt = flim_format(self.format)
            .ok_or_else(|| bad(format!("{}: GX2 format {:#x}", self.name, self.format)))?;
        let mut img = decode_surface(
            &self.data,
            self.width,
            self.height,
            fmt,
            self.tile_mode,
            self.swizzle,
        )?;
        if self.comp_sel != [0, 1, 2, 3] {
            // GX2 selector → .mtex selector (0 zero, 1 one, 2 R, 3 G, 4 B, 5 A)
            let sel = self.comp_sel.map(|c| match c {
                0..=3 => c + 2,
                4 => 0,
                _ => 1,
            });
            img.channel_select = if img.channel_select == IDENTITY {
                sel
            } else {
                // compose: output channel i reads the decoded channel `sel[i]`
                sel.map(|s| match s {
                    2..=5 => img.channel_select[(s - 2) as usize],
                    v => v,
                })
            };
        }
        Ok(img)
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn u32(&self, o: usize) -> Result<u32> {
        self.0
            .get(o..o + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .ok_or_else(|| bad(format!("read past end at {o:#x}")))
    }
    fn u16(&self, o: usize) -> Result<u16> {
        self.0
            .get(o..o + 2)
            .map(|b| u16::from_be_bytes(b.try_into().unwrap()))
            .ok_or_else(|| bad(format!("read past end at {o:#x}")))
    }
    /// A self-relative offset (0 = none).
    fn rel(&self, o: usize) -> Result<Option<usize>> {
        let v = self.u32(o)? as i32;
        Ok((v != 0).then(|| (o as i64 + v as i64) as usize))
    }
    fn cstr(&self, o: usize) -> Result<String> {
        let tail = self.0.get(o..).ok_or_else(|| bad("string outside file"))?;
        let n = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        Ok(String::from_utf8_lossy(&tail[..n]).into_owned())
    }
}

/// The textures of a Wii U BFRES, in index-group order.
pub fn textures(file: &[u8]) -> Result<Vec<Texture>> {
    let r = R(file);
    if file.get(..4) != Some(b"FRES") {
        return Err(bad("missing FRES magic"));
    }
    if file.get(4) != Some(&3) {
        return Err(bad(format!("unsupported version {:?}", file.get(4..8))));
    }
    let count = r.u16(0x50 + 2)? as usize;
    let Some(group) = r.rel(0x20 + 4)? else {
        return Ok(Vec::new());
    };
    let n = (r.u32(group + 4)? as usize).min(count.max(r.u32(group + 4)? as usize));
    let mut out = Vec::with_capacity(n);
    for i in 1..=n {
        let node = group + 8 + 16 * i;
        let Some(o) = r.rel(node + 12)? else {
            continue;
        };
        if file.get(o..o + 4) != Some(b"FTEX") {
            return Err(bad(format!("entry {i}: not an FTEX")));
        }
        let f = |k: usize| r.u32(o + 4 + 4 * k);
        let (width, height, format) = (f(1)?, f(2)?, f(5)?);
        let (size, tile_mode, swizzle) = (f(8)? as usize, f(12)?, f(13)?);
        let comp = file
            .get(o + 0x88..o + 0x8c)
            .ok_or_else(|| bad("truncated FTEX"))?;
        let name = match r.rel(o + 0xA8)? {
            Some(p) => r.cstr(p)?,
            None => match r.rel(node + 8)? {
                Some(p) => r.cstr(p)?,
                None => format!("texture{i}"),
            },
        };
        let data_at = r
            .rel(o + 0xB0)?
            .ok_or_else(|| bad(format!("{name}: no image data")))?;
        let data = file
            .get(data_at..data_at + size)
            .ok_or_else(|| bad(format!("{name}: image data outside file")))?
            .to_vec();
        out.push(Texture {
            name,
            width,
            height,
            format,
            tile_mode,
            swizzle,
            comp_sel: [comp[0], comp[1], comp[2], comp[3]],
            data,
        });
    }
    Ok(out)
}

/// Build a texture-only Wii U BFRES (tests and synthetic fixtures): `(name, width, height,
/// RGBA8 pixels)` per texture, stored linear-aligned (tile mode 1).
pub fn build_rgba(textures: &[(&str, u32, u32, Vec<u8>)]) -> Vec<u8> {
    let mut f = vec![0u8; 0x6C];
    f[..4].copy_from_slice(b"FRES");
    f[4..8].copy_from_slice(&[3, 5, 0, 3]);
    f[8..10].copy_from_slice(&[0xFE, 0xFF]);
    f[10..12].copy_from_slice(&0x10u16.to_be_bytes());
    f[0x52..0x54].copy_from_slice(&(textures.len() as u16).to_be_bytes());
    let group = f.len();
    let put = |f: &mut Vec<u8>, at: usize, v: u32| f[at..at + 4].copy_from_slice(&v.to_be_bytes());
    let rel = |at: usize, to: usize| (to as i64 - at as i64) as i32 as u32;
    f.resize(group + 8 + 16 * (textures.len() + 1), 0);
    put(&mut f, 0x24, rel(0x24, group));
    put(&mut f, group, (8 + 16 * (textures.len() + 1)) as u32);
    put(&mut f, group + 4, textures.len() as u32);
    for (i, (name, w, h, px)) in textures.iter().enumerate() {
        let px =
            crate::gx2::tile(px, *w, *h, 32, crate::gx2::TileMode::LinearAligned, 0).expect("tile");
        let o = f.len();
        f.resize(o + 0xC0, 0);
        f[o..o + 4].copy_from_slice(b"FTEX");
        let fields = [
            1,
            *w,
            *h,
            1,
            1,
            0x1a,
            0,
            1,
            px.len() as u32,
            0,
            0,
            0,
            1,
            0,
            1,
            *w,
        ];
        for (k, v) in fields.iter().enumerate() {
            put(&mut f, o + 4 + 4 * k, *v);
        }
        f[o + 0x88..o + 0x8c].copy_from_slice(&[0, 1, 2, 3]);
        let name_at = f.len();
        f.extend_from_slice(name.as_bytes());
        f.push(0);
        while !f.len().is_multiple_of(0x100) {
            f.push(0);
        }
        let data_at = f.len();
        f.extend_from_slice(&px);
        put(&mut f, o + 0xA8, rel(o + 0xA8, name_at));
        put(&mut f, o + 0xB0, rel(o + 0xB0, data_at));
        let node = group + 8 + 16 * (i + 1);
        put(&mut f, node + 8, rel(node + 8, name_at));
        put(&mut f, node + 12, rel(node + 12, o));
    }
    let len = f.len() as u32;
    put(&mut f, 0x0C, len);
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthetic_textures_round_trip() {
        let px: Vec<u8> = (0..8 * 4 * 4).map(|i| (i * 7 % 251) as u8).collect();
        let file = build_rgba(&[("wait.0", 8, 4, px.clone()), ("walk.1", 8, 4, px.clone())]);
        let t = textures(&file).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name, "wait.0");
        assert_eq!(t[1].name, "walk.1");
        assert_eq!((t[1].width, t[1].height, t[1].format), (8, 4, 0x1a));
        let img = t[0].decode().unwrap();
        assert_eq!(img.data, px);
    }

    #[test]
    fn rejects_other_files() {
        assert!(textures(b"FLIM....").is_err());
        let mut f = build_rgba(&[]);
        f[4] = 4;
        assert!(textures(&f).is_err());
        assert!(textures(&build_rgba(&[])).unwrap().is_empty());
    }

    #[test]
    fn selectors_compose() {
        let px = vec![10, 20, 30, 40];
        let mut t = textures(&build_rgba(&[("a", 1, 1, px)])).unwrap().remove(0);
        t.comp_sel = [0, 0, 0, 5];
        assert_eq!(t.decode().unwrap().channel_select, [2, 2, 2, 1]);
    }
}
