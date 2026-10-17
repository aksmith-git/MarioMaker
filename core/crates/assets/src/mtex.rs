//! `.mtex`: one GPU-ready texture (mip 0, linear rows, top row first).
//!
//! ```text
//! 0x00  "MTEX"
//! 0x04  u16 version (1)
//! 0x06  u8  format   (MtexFormat)
//! 0x07  u8  variant  (MtexVariant)
//! 0x08  u32 width    (pixels)
//! 0x0C  u32 height   (pixels)
//! 0x10  [u8; 4] channel select R,G,B,A (0 zero, 1 one, 2 red, 3 green, 4 blue, 5 alpha)
//! 0x14  u32 data length
//! 0x18  data
//! ```
//! All integers little-endian. Block-compressed formats store 4×4 blocks row by row.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MtexError {
    #[error("not an mtex file")]
    Magic,
    #[error("unsupported mtex version {0}")]
    Version(u16),
    #[error("unknown format code {0}")]
    Format(u8),
    #[error("truncated mtex file")]
    Truncated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MtexFormat {
    Rgba8 = 1,
    Bc1 = 2,
    Bc2 = 3,
    Bc3 = 4,
    Bc4 = 5,
    Bc5 = 6,
    Bc6h = 7,
    Bc7 = 8,
}

impl MtexFormat {
    pub fn from_code(c: u8) -> Result<Self, MtexError> {
        Ok(match c {
            1 => Self::Rgba8,
            2 => Self::Bc1,
            3 => Self::Bc2,
            4 => Self::Bc3,
            5 => Self::Bc4,
            6 => Self::Bc5,
            7 => Self::Bc6h,
            8 => Self::Bc7,
            c => return Err(MtexError::Format(c)),
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Rgba8 => "rgba8",
            Self::Bc1 => "bc1",
            Self::Bc2 => "bc2",
            Self::Bc3 => "bc3",
            Self::Bc4 => "bc4",
            Self::Bc5 => "bc5",
            Self::Bc6h => "bc6h",
            Self::Bc7 => "bc7",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MtexVariant {
    Unorm = 0,
    Snorm = 1,
    Srgb = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mtex {
    pub format: MtexFormat,
    pub variant: MtexVariant,
    pub width: u32,
    pub height: u32,
    pub channel_select: [u8; 4],
    pub data: Vec<u8>,
}

const MAGIC: &[u8; 4] = b"MTEX";
const VERSION: u16 = 1;

impl Mtex {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(0x18 + self.data.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.push(self.format as u8);
        out.push(self.variant as u8);
        out.extend_from_slice(&self.width.to_le_bytes());
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.channel_select);
        out.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.data);
        out
    }

    pub fn parse(b: &[u8]) -> Result<Self, MtexError> {
        if b.len() < 0x18 {
            return Err(MtexError::Truncated);
        }
        if &b[..4] != MAGIC {
            return Err(MtexError::Magic);
        }
        let version = u16::from_le_bytes([b[4], b[5]]);
        if version != VERSION {
            return Err(MtexError::Version(version));
        }
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let len = u32_at(0x14) as usize;
        let data = b
            .get(0x18..0x18 + len)
            .ok_or(MtexError::Truncated)?
            .to_vec();
        Ok(Self {
            format: MtexFormat::from_code(b[6])?,
            variant: match b[7] {
                1 => MtexVariant::Snorm,
                2 => MtexVariant::Srgb,
                _ => MtexVariant::Unorm,
            },
            width: u32_at(0x08),
            height: u32_at(0x0C),
            channel_select: b[0x10..0x14].try_into().unwrap(),
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let t = Mtex {
            format: MtexFormat::Rgba8,
            variant: MtexVariant::Srgb,
            width: 2,
            height: 1,
            channel_select: [2, 3, 4, 5],
            data: vec![1, 2, 3, 4, 5, 6, 7, 8],
        };
        assert_eq!(Mtex::parse(&t.to_bytes()).unwrap(), t);
        assert_eq!(Mtex::parse(b"nope"), Err(MtexError::Truncated));
    }
}
