//! SMM2 decrypted course data (0x5BFC0 bytes, little-endian). See docs/formats/smm2-course.md.

use serde_json::{Map, Value, json};

use crate::error::{Error, Result};
use crate::record;
use crate::record::{Ascii, Endian::Little, Record, RecordArray, SlotArray, Utf16};

pub const COURSE_SIZE: usize = 0x5BFC0;
pub const HEADER_SIZE: usize = 0x200;
pub const AREA_SIZE: usize = 0x2DEE0;
pub const AREA_OFFSETS: [usize; 2] = [0x200, 0x2E0E0];

record! {
    pub struct CourseHeader, HEADER_SIZE, Little, {
        start_y: u8 = 0x00,
        goal_y: u8 = 0x01,
        goal_x: i16 = 0x02,
        timer: i16 = 0x04,
        clear_condition_magnitude: i16 = 0x06,
        year: i16 = 0x08,
        month: i8 = 0x0A,
        day: i8 = 0x0B,
        hour: i8 = 0x0C,
        minute: i8 = 0x0D,
        autoscroll_speed: u8 = 0x0E,
        clear_condition_category: u8 = 0x0F,
        clear_condition: u32 = 0x10,
        game_version: u32 = 0x14,
        management_flags: u32 = 0x18,
        clear_check_attempts: u32 = 0x1C,
        clear_check_time: u32 = 0x20,
        creation_id: u32 = 0x24,
        upload_id: u64 = 0x28,
        clear_check_game_version: u32 = 0x30,
        unk_f0: u8 = 0xF0,
        game_style: Ascii<3> = 0xF1,
        name: Utf16<0x21> = 0xF4,
        description: Utf16<0x65> = 0x136,
    }
}

record! {
    pub struct AreaHeader, 0x48, Little, {
        theme: u8 = 0x00,
        autoscroll_type: u8 = 0x01,
        boundary_type: u8 = 0x02,
        orientation: u8 = 0x03,
        liquid_end_height: u8 = 0x04,
        liquid_mode: u8 = 0x05,
        liquid_speed: u8 = 0x06,
        liquid_start_height: u8 = 0x07,
        boundary_right: i32 = 0x08,
        boundary_top: i32 = 0x0C,
        boundary_left: i32 = 0x10,
        boundary_bottom: i32 = 0x14,
        flags: u32 = 0x18,
        object_count: i32 = 0x1C,
        sound_effect_count: i32 = 0x20,
        snake_block_count: i32 = 0x24,
        clear_pipe_count: i32 = 0x28,
        piranha_creeper_count: i32 = 0x2C,
        exclamation_block_count: i32 = 0x30,
        track_block_count: i32 = 0x34,
        ground_count: i32 = 0x3C,
        track_count: i32 = 0x40,
        icicle_count: i32 = 0x44,
    }
}

impl AreaHeader {
    /// Night mode is bit 1 of the area flags (Phase 3, research/specs/themes.md).
    pub fn is_night(&self) -> bool {
        self.flags & 2 != 0
    }
}

record! {
    /// Positions are in 1/160-tile units, centre-anchored; y increases upward.
    pub struct CourseObject, 0x20, Little, {
        x: i32 = 0x00,
        y: i32 = 0x04,
        width: u8 = 0x0A,
        height: u8 = 0x0B,
        flags: u32 = 0x0C,
        child_flags: u32 = 0x10,
        extended_data: u32 = 0x14,
        id: i16 = 0x18,
        child_id: i16 = 0x1A,
        link_id: i16 = 0x1C,
        sound_effect_id: i16 = 0x1E,
    }
}

impl CourseObject {
    pub fn tile_x(&self) -> f64 {
        (self.x as f64 - 80.0) / 160.0
    }
    pub fn tile_y(&self) -> f64 {
        (self.y as f64 - 80.0) / 160.0
    }
    /// Object size field (flags bits 13–14); 0x4000 = big.
    pub fn is_big(&self) -> bool {
        self.flags & 0x6000 == 0x4000
    }
}

record! { pub struct SoundEffect, 4, Little, { id: u8 = 0, x: u8 = 1, y: u8 = 2 } }
record! { pub struct GroundTile, 4, Little, { x: u8 = 0, y: u8 = 1, id: u8 = 2, background_id: u8 = 3 } }
record! {
    pub struct Track, 0x0C, Little, {
        unk_0: u16 = 0x0, flags: u8 = 0x2, x: u8 = 0x3, y: u8 = 0x4, r#type: u8 = 0x5,
        link_id: u16 = 0x6, k0: u16 = 0x8, k1: u16 = 0xA,
    }
}
record! { pub struct Icicle, 4, Little, { x: u8 = 0, y: u8 = 1, r#type: u8 = 2 } }

// --- path records: a header followed by a fixed-capacity node array ----------------------------

record! { pub struct SnakeNode, 8, Little, { index: u16 = 0, direction: u16 = 2, unk_4: u16 = 4 } }
record! {
    pub struct ClearPipeNode, 8, Little, {
        r#type: u8 = 0, index: u8 = 1, x: u8 = 2, y: u8 = 3, width: u8 = 4, height: u8 = 5,
        unk_6: u8 = 6, direction: u8 = 7,
    }
}
record! { pub struct PathNode, 4, Little, { unk_0: u8 = 0, direction: u8 = 1 } }

record! { pub struct SnakeBlockHead, 0x3C4, Little, { index: u8 = 0, node_count: u8 = 1, unk_2: u8 = 2 } }
record! { pub struct ClearPipeHead, 0x124, Little, { index: u8 = 0, node_count: u8 = 1, unk_2: u8 = 2 } }
record! { pub struct PiranhaCreeperHead, 0x54, Little, { unk_0: u8 = 0, index: u8 = 1, node_count: u8 = 2 } }
record! { pub struct BlockPathHead, 0x2C, Little, { unk_0: u8 = 0, index: u8 = 1, node_count: u8 = 2 } }

/// Header record of a path type: knows its node array.
pub trait PathHead: Record {
    type Node: Record;
    const NODE_CAPACITY: usize;
    fn node_count(&self) -> u8;
    fn set_node_count(&mut self, n: u8);
    fn index(&self) -> u8;
}

macro_rules! path_head {
    ($head:ty, $node:ty, $cap:expr) => {
        impl PathHead for $head {
            type Node = $node;
            const NODE_CAPACITY: usize = $cap;
            fn node_count(&self) -> u8 {
                self.node_count
            }
            fn set_node_count(&mut self, n: u8) {
                self.node_count = n;
            }
            fn index(&self) -> u8 {
                self.index
            }
        }
    };
}
path_head!(SnakeBlockHead, SnakeNode, 120);
path_head!(ClearPipeHead, ClearPipeNode, 36);
path_head!(PiranhaCreeperHead, PathNode, 20);
path_head!(BlockPathHead, PathNode, 10);

const NODE_OFFSET: usize = 4;

#[derive(Clone, Debug)]
pub struct PathRecord<H: PathHead> {
    pub head: H,
    pub nodes: RecordArray<H::Node>,
}

impl<H: PathHead> Record for PathRecord<H> {
    const SIZE: usize = H::SIZE;
    fn parse(raw: &[u8]) -> Self {
        let head = H::parse(raw);
        let n = <H::Node as Record>::SIZE * H::NODE_CAPACITY;
        let nodes = RecordArray::parse(
            &raw[NODE_OFFSET..NODE_OFFSET + n],
            H::NODE_CAPACITY,
            head.node_count() as i64,
        );
        Self { head, nodes }
    }
    fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut head = self.head.clone();
        if self.nodes.changed_length() {
            head.set_node_count(self.nodes.len() as u8);
        }
        let mut buf = head.to_bytes()?;
        let data = self.nodes.to_bytes()?;
        buf[NODE_OFFSET..NODE_OFFSET + data.len()].copy_from_slice(&data);
        Ok(buf)
    }
    fn fields_json(&self) -> Map<String, Value> {
        let mut m = self.head.fields_json();
        m.insert(
            "nodes".into(),
            Value::Array(
                self.nodes
                    .iter()
                    .map(|n| Value::Object(n.fields_json()))
                    .collect(),
            ),
        );
        m
    }
}

pub type SnakeBlock = PathRecord<SnakeBlockHead>;
pub type ClearPipe = PathRecord<ClearPipeHead>;
pub type PiranhaCreeper = PathRecord<PiranhaCreeperHead>;
pub type ExclamationBlock = PathRecord<BlockPathHead>;
pub type TrackBlock = PathRecord<BlockPathHead>;

// --- areas -----------------------------------------------------------------------------------

// Area-relative offsets and capacities. Path arrays are slot-indexed (slot i holds the entry whose
// index is i, possibly sparse); the count is the number of live slots.
const OBJECTS: (usize, usize) = (0x48, 2600);
const SOUND_EFFECTS: (usize, usize) = (0x14548, 300);
const SNAKE_BLOCKS: (usize, usize) = (0x149F8, 5);
const CLEAR_PIPES: (usize, usize) = (0x15CCC, 200);
const PIRANHA_CREEPERS: (usize, usize) = (0x240EC, 10);
const EXCLAMATION_BLOCKS: (usize, usize) = (0x24434, 10);
const TRACK_BLOCKS: (usize, usize) = (0x245EC, 10);
const GROUND: (usize, usize) = (0x247A4, 4000);
const TRACKS: (usize, usize) = (0x28624, 1500);
const ICICLES: (usize, usize) = (0x2CC74, 300);

fn chunk<T: Record>(raw: &[u8], (off, cap): (usize, usize)) -> &[u8] {
    &raw[off..off + T::SIZE * cap]
}

#[derive(Clone, Debug)]
pub struct Area {
    raw: Vec<u8>,
    pub header: AreaHeader,
    pub objects: RecordArray<CourseObject>,
    pub sound_effects: RecordArray<SoundEffect>,
    pub snake_blocks: SlotArray<SnakeBlock>,
    pub clear_pipes: SlotArray<ClearPipe>,
    pub piranha_creepers: SlotArray<PiranhaCreeper>,
    pub exclamation_blocks: SlotArray<ExclamationBlock>,
    pub track_blocks: SlotArray<TrackBlock>,
    pub ground: RecordArray<GroundTile>,
    pub tracks: RecordArray<Track>,
    pub icicles: RecordArray<Icicle>,
}

impl Area {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() != AREA_SIZE {
            return Err(Error::Size {
                what: "area",
                expected: AREA_SIZE,
                got: raw.len(),
            });
        }
        let h = AreaHeader::from_slice(raw, 0);
        Ok(Self {
            objects: RecordArray::parse(
                chunk::<CourseObject>(raw, OBJECTS),
                OBJECTS.1,
                h.object_count as i64,
            ),
            sound_effects: RecordArray::parse(
                chunk::<SoundEffect>(raw, SOUND_EFFECTS),
                SOUND_EFFECTS.1,
                h.sound_effect_count as i64,
            ),
            snake_blocks: SlotArray::parse(chunk::<SnakeBlock>(raw, SNAKE_BLOCKS), SNAKE_BLOCKS.1),
            clear_pipes: SlotArray::parse(chunk::<ClearPipe>(raw, CLEAR_PIPES), CLEAR_PIPES.1),
            piranha_creepers: SlotArray::parse(
                chunk::<PiranhaCreeper>(raw, PIRANHA_CREEPERS),
                PIRANHA_CREEPERS.1,
            ),
            exclamation_blocks: SlotArray::parse(
                chunk::<ExclamationBlock>(raw, EXCLAMATION_BLOCKS),
                EXCLAMATION_BLOCKS.1,
            ),
            track_blocks: SlotArray::parse(chunk::<TrackBlock>(raw, TRACK_BLOCKS), TRACK_BLOCKS.1),
            ground: RecordArray::parse(
                chunk::<GroundTile>(raw, GROUND),
                GROUND.1,
                h.ground_count as i64,
            ),
            tracks: RecordArray::parse(chunk::<Track>(raw, TRACKS), TRACKS.1, h.track_count as i64),
            icicles: RecordArray::parse(
                chunk::<Icicle>(raw, ICICLES),
                ICICLES.1,
                h.icicle_count as i64,
            ),
            header: h,
            raw: raw.to_vec(),
        })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut buf = self.raw.clone();
        let mut h = self.header.clone();
        macro_rules! put {
            ($arr:expr, $at:expr, $count:ident) => {{
                if $arr.changed_length() {
                    h.$count = $arr.len() as i32;
                }
                let data = $arr.to_bytes()?;
                buf[$at.0..$at.0 + data.len()].copy_from_slice(&data);
            }};
        }
        put!(self.objects, OBJECTS, object_count);
        put!(self.sound_effects, SOUND_EFFECTS, sound_effect_count);
        put!(self.snake_blocks, SNAKE_BLOCKS, snake_block_count);
        put!(self.clear_pipes, CLEAR_PIPES, clear_pipe_count);
        put!(
            self.piranha_creepers,
            PIRANHA_CREEPERS,
            piranha_creeper_count
        );
        put!(
            self.exclamation_blocks,
            EXCLAMATION_BLOCKS,
            exclamation_block_count
        );
        put!(self.track_blocks, TRACK_BLOCKS, track_block_count);
        put!(self.ground, GROUND, ground_count);
        put!(self.tracks, TRACKS, track_count);
        put!(self.icicles, ICICLES, icicle_count);
        buf[..AreaHeader::SIZE].copy_from_slice(&h.to_bytes()?);
        Ok(buf)
    }

    pub fn to_json(&self) -> Value {
        fn list<T: Record>(a: &RecordArray<T>) -> Value {
            Value::Array(a.iter().map(|r| Value::Object(r.fields_json())).collect())
        }
        fn slots<T: Record>(a: &SlotArray<T>) -> Value {
            Value::Object(
                a.slots
                    .iter()
                    .map(|(i, r)| (i.to_string(), Value::Object(r.fields_json())))
                    .collect(),
            )
        }
        json!({
            "header": Value::Object(self.header.fields_json()),
            "objects": list(&self.objects),
            "sound_effects": list(&self.sound_effects),
            "snake_blocks": slots(&self.snake_blocks),
            "clear_pipes": slots(&self.clear_pipes),
            "piranha_creepers": slots(&self.piranha_creepers),
            "exclamation_blocks": slots(&self.exclamation_blocks),
            "track_blocks": slots(&self.track_blocks),
            "ground": list(&self.ground),
            "tracks": list(&self.tracks),
            "icicles": list(&self.icicles),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Course {
    pub header: CourseHeader,
    pub main: Area,
    pub sub: Area,
}

/// Two courses are equal when they write the same bytes.
impl PartialEq for Course {
    fn eq(&self, other: &Self) -> bool {
        match (self.to_bytes(), other.to_bytes()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

impl Course {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() != COURSE_SIZE {
            return Err(Error::Size {
                what: "SMM2 course data",
                expected: COURSE_SIZE,
                got: data.len(),
            });
        }
        Ok(Self {
            header: CourseHeader::from_slice(data, 0),
            main: Area::parse(&data[AREA_OFFSETS[0]..AREA_OFFSETS[0] + AREA_SIZE])?,
            sub: Area::parse(&data[AREA_OFFSETS[1]..AREA_OFFSETS[1] + AREA_SIZE])?,
        })
    }

    /// An empty course, laid out like `smmfmt.smm2.Course.blank`.
    pub fn blank(style: &str) -> Self {
        let mut c = Self::parse(&vec![0u8; COURSE_SIZE]).expect("blank course");
        c.header.timer = 300;
        c.header.game_style.set(style);
        c.header.unk_f0 = 0xFF;
        c.header.start_y = 1;
        c.header.goal_y = 1;
        c.header.goal_x = 2400;
        for a in [&mut c.main, &mut c.sub] {
            a.header.boundary_top = 0x1B0;
            a.header.boundary_right = 0xF00;
        }
        c
    }

    pub fn areas(&self) -> [&Area; 2] {
        [&self.main, &self.sub]
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut out = vec![0u8; COURSE_SIZE];
        out[..HEADER_SIZE].copy_from_slice(&self.header.to_bytes()?);
        for (off, area) in AREA_OFFSETS.iter().zip(self.areas()) {
            out[*off..off + AREA_SIZE].copy_from_slice(&area.to_bytes()?);
        }
        Ok(out)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "game": "SMM2",
            "header": Value::Object(self.header.fields_json()),
            "areas": [self.main.to_json(), self.sub.to_json()],
        })
    }
}
