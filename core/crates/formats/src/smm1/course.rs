//! SMM1 course area file `.cdt` (0x15000 bytes, big-endian). See docs/formats/smm1-course.md.

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::record;
use crate::record::{Ascii, Endian::Big, Record, RecordArray, Utf16};

pub const CDT_SIZE: usize = 0x15000;
pub const CRC_OFFSET: usize = 0x08;
pub const CRC_START: usize = 0x10;
const OBJECTS_OFFSET: usize = 0xF0;
const OBJECT_CAPACITY: usize = 2600;
const SOUNDS_OFFSET: usize = 0x145F0;
const SOUND_CAPACITY: usize = 300;
pub const EMPTY_SOUND: [u8; 8] = [0xFF, 0xFF, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0x00];

record! {
    pub struct CourseHeader, 0xF0, Big, {
        version: u64 = 0x00,
        crc32: u32 = 0x08,
        year: u16 = 0x10,
        month: u8 = 0x12,
        day: u8 = 0x13,
        hour: u8 = 0x14,
        minute: u8 = 0x15,
        official_maker: u8 = 0x17,
        is_downloaded: u8 = 0x20,
        previously_removed: u8 = 0x21,
        physics_version: u8 = 0x27,
        name: Utf16<0x21> = 0x28,
        game_style: Ascii<2> = 0x6A,
        theme: u8 = 0x6D,
        uploaded: u8 = 0x6E,
        clear_check: u8 = 0x6F,
        timer: u16 = 0x70,
        autoscroll: u8 = 0x72,
        width: u32 = 0x74,
        mii_country: i32 = 0xD8,
        object_count: u32 = 0xEC,
    }
}

record! {
    /// x and y are in 1/160-tile units (tile * 160 + 80); z is the draw layer.
    pub struct CourseObject, 0x20, Big, {
        x: u32 = 0x00,
        z: u32 = 0x04,
        y: i16 = 0x08,
        width: i8 = 0x0A,
        height: i8 = 0x0B,
        flags: u32 = 0x0C,
        child_flags: u32 = 0x10,
        extended_data: u32 = 0x14,
        r#type: i8 = 0x18,
        child_type: i8 = 0x19,
        link_id: i16 = 0x1A,
        effect_index: i16 = 0x1C,
        transform_id: i8 = 0x1E,
        child_transform_id: i8 = 0x1F,
    }
}

record! {
    /// Layout only partly known; the raw 8 bytes are always preserved.
    pub struct SoundEffect, 8, Big, { r#type: u8 = 0, variation: u8 = 2, x: u8 = 3, y: u8 = 4 }
}

impl SoundEffect {
    pub fn is_empty(&self) -> bool {
        self.to_bytes().map(|b| b == EMPTY_SOUND).unwrap_or(false)
    }
}

#[derive(Clone, Debug)]
pub struct Course {
    raw: Vec<u8>,
    pub header: CourseHeader,
    pub objects: RecordArray<CourseObject>,
    pub sound_effects: RecordArray<SoundEffect>,
}

impl Course {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() != CDT_SIZE {
            return Err(Error::Size {
                what: ".cdt",
                expected: CDT_SIZE,
                got: raw.len(),
            });
        }
        let header = CourseHeader::from_slice(raw, 0);
        let objects = RecordArray::parse(
            &raw[OBJECTS_OFFSET..OBJECTS_OFFSET + CourseObject::SIZE * OBJECT_CAPACITY],
            OBJECT_CAPACITY,
            header.object_count as i64,
        );
        let sound_effects = RecordArray::parse(
            &raw[SOUNDS_OFFSET..SOUNDS_OFFSET + SoundEffect::SIZE * SOUND_CAPACITY],
            SOUND_CAPACITY,
            SOUND_CAPACITY as i64,
        );
        Ok(Self {
            raw: raw.to_vec(),
            header,
            objects,
            sound_effects,
        })
    }

    pub fn stored_crc_ok(data: &[u8]) -> bool {
        data.len() == CDT_SIZE
            && u32::from_be_bytes(data[CRC_OFFSET..CRC_OFFSET + 4].try_into().unwrap())
                == crc32fast::hash(&data[CRC_START..])
    }

    /// Serialise; the CRC32 is recomputed.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut buf = self.raw.clone();
        let mut h = self.header.clone();
        if self.objects.changed_length() {
            h.object_count = self.objects.len() as u32;
        }
        let objs = self.objects.to_bytes()?;
        buf[OBJECTS_OFFSET..OBJECTS_OFFSET + objs.len()].copy_from_slice(&objs);
        let snd = self.sound_effects.to_bytes()?;
        buf[SOUNDS_OFFSET..SOUNDS_OFFSET + snd.len()].copy_from_slice(&snd);
        buf[..CourseHeader::SIZE].copy_from_slice(&h.to_bytes()?);
        let crc = crc32fast::hash(&buf[CRC_START..]);
        buf[CRC_OFFSET..CRC_OFFSET + 4].copy_from_slice(&crc.to_be_bytes());
        Ok(buf)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "game": "SMM1",
            "header": Value::Object(self.header.fields_json()),
            "objects": self.objects.iter().map(|o| Value::Object(o.fields_json())).collect::<Vec<_>>(),
            "sound_effects": self.sound_effects.iter().map(|s| Value::Object(s.fields_json())).collect::<Vec<_>>(),
        })
    }
}
