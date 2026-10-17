//! Switch NSO executables: header, LZ4 segment decompression, flat memory image.

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Text,
    Ro,
    Data,
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub kind: SegmentKind,
    pub mem_offset: usize,
    pub size: usize,
}

#[derive(Clone, Debug)]
pub struct Nso {
    /// 32-byte module id (build id) identifying the exact build.
    pub module_id: [u8; 32],
    pub segments: Vec<Segment>,
    /// Decompressed segments laid out at their memory offsets (bss not included).
    pub image: Vec<u8>,
}

impl Nso {
    pub fn parse(data: &[u8]) -> Result<Self> {
        let bad = |d: &str| Error::Format {
            what: "NSO",
            detail: d.to_string(),
        };
        if data.len() < 0x100 || &data[..4] != b"NSO0" {
            return Err(bad("missing magic"));
        }
        let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap()) as usize;
        let flags = u32_at(0xC);
        let kinds = [SegmentKind::Text, SegmentKind::Ro, SegmentKind::Data];
        let mut segments = Vec::new();
        let mut image = Vec::new();
        for (i, kind) in kinds.into_iter().enumerate() {
            let h = 0x10 + i * 0x10;
            let (file_off, mem_off, size) = (u32_at(h), u32_at(h + 4), u32_at(h + 8));
            let csize = u32_at(0x60 + i * 4);
            let src = data
                .get(file_off..file_off + csize)
                .ok_or_else(|| bad("segment outside file"))?;
            let bytes = if flags & (1 << i) != 0 {
                lz4_flex::block::decompress(src, size).map_err(|e| Error::Format {
                    what: "NSO",
                    detail: e.to_string(),
                })?
            } else {
                src.to_vec()
            };
            if bytes.len() != size {
                return Err(bad("segment size mismatch"));
            }
            if image.len() < mem_off + size {
                image.resize(mem_off + size, 0);
            }
            image[mem_off..mem_off + size].copy_from_slice(&bytes);
            segments.push(Segment {
                kind,
                mem_offset: mem_off,
                size,
            });
        }
        Ok(Self {
            module_id: data[0x40..0x60].try_into().unwrap(),
            segments,
            image,
        })
    }

    pub fn module_id_hex(&self) -> String {
        self.module_id.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn segment(&self, kind: SegmentKind) -> Option<&[u8]> {
        self.segments
            .iter()
            .find(|s| s.kind == kind)
            .map(|s| &self.image[s.mem_offset..s.mem_offset + s.size])
    }
}
