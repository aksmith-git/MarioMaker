//! Block paths: the routes of snake blocks (84), track blocks (85), Piranha Creepers (107) and
//! ! blocks (119), stored in the area's slot-indexed path arrays.
//!
//! Encoding [corp: all 175 paths of the sample courses]: a path is one node per tile step; a
//! node's code is the step's direction (1 left, 2 right, 3 down, 4 up) or, where the route turns,
//! a corner code naming the old and new direction (5 left→down, 6 down→left, 7 left→up,
//! 8 up→left, 9 right→down, 10 down→right, 11 right→up, 12 up→right); a last node closes it with
//! the final direction (13 right, 14 left, 15 up, 16 down). The part's extended data is its slot
//! plus 1 (snake blocks: in the low nibble, bit 0x10 = start from the other end). Snake records
//! are {slot, count, 1, 0} then {node number, code, 0x100} per node; the others {1, slot,
//! count, 0} then {1, code}.

use mmk_formats::record::Record;
use mmk_formats::smm2::course::{
    BlockPathHead, CourseObject, PathNode, PathRecord, PiranhaCreeperHead, SnakeBlockHead,
    SnakeNode,
};

use crate::{EditError, Editor};

/// A path step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Left = 1,
    Right = 2,
    Down = 3,
    Up = 4,
}

impl Step {
    fn opposite(self) -> Step {
        match self {
            Step::Left => Step::Right,
            Step::Right => Step::Left,
            Step::Down => Step::Up,
            Step::Up => Step::Down,
        }
    }
}

/// Node codes for a route that starts heading `heading` and takes `steps`.
pub fn encode(heading: Step, steps: &[Step]) -> Result<Vec<u8>, EditError> {
    use Step::*;
    let mut out = Vec::with_capacity(steps.len() + 1);
    let mut dir = heading;
    for &s in steps {
        let code = if s == dir {
            s as u8
        } else {
            match (dir, s) {
                (Left, Down) => 5,
                (Down, Left) => 6,
                (Left, Up) => 7,
                (Up, Left) => 8,
                (Right, Down) => 9,
                (Down, Right) => 10,
                (Right, Up) => 11,
                (Up, Right) => 12,
                _ => return Err(EditError::NotAllowed("a path can't turn back on itself")),
            }
        };
        out.push(code);
        dir = s;
    }
    out.push(match dir {
        Right => 13,
        Left => 14,
        Up => 15,
        Down => 16,
    });
    Ok(out)
}

/// The steps of encoded nodes (inverse of `encode`).
pub fn decode(codes: &[u8]) -> Vec<Step> {
    use Step::*;
    codes
        .iter()
        .filter_map(|c| match c {
            1 | 6 | 8 => Some(Left),
            2 | 10 | 12 => Some(Right),
            3 | 5 | 9 => Some(Down),
            4 | 7 | 11 => Some(Up),
            _ => None,
        })
        .collect()
}

/// The heading and steps of encoded nodes (`encode`'s input).
pub fn route_of(codes: &[u8]) -> (Step, Vec<Step>) {
    use Step::*;
    let heading = match codes.first() {
        Some(1 | 14) => Left,
        Some(3 | 16) => Down,
        Some(4 | 15) => Up,
        Some(5 | 7) => Left,
        Some(6 | 10) => Down,
        Some(8 | 12) => Up,
        _ => Right,
    };
    (heading, decode(codes))
}

/// Parts with a path record.
pub fn has_path(part: i16) -> bool {
    capacity(part).is_some()
}

/// (slots, nodes) per path kind.
fn capacity(part: i16) -> Option<(usize, usize)> {
    match part {
        84 => Some((5, 120)),
        85 | 119 => Some((10, 10)),
        107 => Some((10, 20)),
        _ => None,
    }
}

/// The route a freshly placed part gets.
pub fn default_route(part: i16) -> Option<(Step, Vec<Step>)> {
    match part {
        84 => Some((Step::Right, vec![Step::Right; 3])),
        85 | 119 => Some((Step::Right, vec![Step::Right; 2])),
        107 => Some((Step::Up, vec![Step::Up; 2])),
        _ => None,
    }
}

fn slot_of(o: &CourseObject) -> Option<usize> {
    let v = if o.id == 84 {
        o.extended_data & 0xF
    } else {
        o.extended_data
    };
    (v > 0).then(|| v as usize - 1)
}

fn block_record(slot: usize, codes: &[u8]) -> PathRecord<BlockPathHead> {
    let mut r = PathRecord::<BlockPathHead>::blank();
    r.head.unk_0 = 1;
    r.head.index = slot as u8;
    for &c in codes {
        let mut n = PathNode::blank();
        n.unk_0 = 1;
        n.direction = c;
        r.nodes.items.push(n);
    }
    r
}

impl Editor {
    /// Give a path part a route (`heading` is where it faces before the first step: snake
    /// blocks move right unless they start from the other end, creepers grow up).
    pub fn set_path(
        &mut self,
        index: usize,
        heading: Step,
        steps: &[Step],
    ) -> Result<(), EditError> {
        let Some(o) = self.object(index).cloned() else {
            return Err(EditError::Missing);
        };
        let Some((slots, max_nodes)) = capacity(o.id) else {
            return Err(EditError::NotAllowed("this part has no path"));
        };
        // a route that starts by reversing just starts the other way
        let heading = match steps.first() {
            Some(&s) if s == heading.opposite() => s,
            _ => heading,
        };
        let codes = encode(heading, steps)?;
        if codes.len() > max_nodes {
            return Err(EditError::Limit("path length"));
        }
        let slot = match slot_of(&o) {
            Some(s) if self.path_slot_used(o.id, s) => s,
            _ => (0..slots)
                .find(|&s| !self.path_slot_used(o.id, s))
                .ok_or(EditError::Limit("paths"))?,
        };
        let a = self.area_mut();
        match o.id {
            84 => {
                let mut r = PathRecord::<SnakeBlockHead>::blank();
                r.head.index = slot as u8;
                r.head.unk_2 = 1;
                for (i, &c) in codes.iter().enumerate() {
                    let mut n = SnakeNode::blank();
                    n.index = i as u16;
                    n.direction = c as u16;
                    n.unk_4 = 0x100;
                    r.nodes.items.push(n);
                }
                a.snake_blocks.slots.insert(slot, r);
            }
            85 => {
                a.track_blocks
                    .slots
                    .insert(slot, block_record(slot, &codes));
            }
            119 => {
                a.exclamation_blocks
                    .slots
                    .insert(slot, block_record(slot, &codes));
            }
            _ => {
                let mut r = PathRecord::<PiranhaCreeperHead>::blank();
                r.head.unk_0 = 1;
                r.head.index = slot as u8;
                for &c in &codes {
                    let mut n = PathNode::blank();
                    n.unk_0 = 1;
                    n.direction = c;
                    r.nodes.items.push(n);
                }
                a.piranha_creepers.slots.insert(slot, r);
            }
        }
        let o = &mut self.area_mut().objects.items[index];
        o.extended_data = if o.id == 84 {
            (o.extended_data & 0x10) | (slot as u32 + 1)
        } else {
            slot as u32 + 1
        };
        Ok(())
    }

    fn path_slot_used(&self, part: i16, slot: usize) -> bool {
        let a = self.area();
        match part {
            84 => a.snake_blocks.slots.contains_key(&slot),
            85 => a.track_blocks.slots.contains_key(&slot),
            119 => a.exclamation_blocks.slots.contains_key(&slot),
            107 => a.piranha_creepers.slots.contains_key(&slot),
            _ => false,
        }
    }

    /// The node codes of a part's path.
    pub fn path(&self, index: usize) -> Option<Vec<u8>> {
        let o = self.object(index)?;
        let slot = slot_of(o)?;
        let a = self.area();
        match o.id {
            84 => a
                .snake_blocks
                .slots
                .get(&slot)
                .map(|r| r.nodes.iter().map(|n| n.direction as u8).collect()),
            85 => a
                .track_blocks
                .slots
                .get(&slot)
                .map(|r| r.nodes.iter().map(|n| n.direction).collect()),
            119 => a
                .exclamation_blocks
                .slots
                .get(&slot)
                .map(|r| r.nodes.iter().map(|n| n.direction).collect()),
            107 => a
                .piranha_creepers
                .slots
                .get(&slot)
                .map(|r| r.nodes.iter().map(|n| n.direction).collect()),
            _ => None,
        }
    }

    /// Room for one more path of this part's kind?
    pub(crate) fn path_slot_free(&self, part: i16) -> bool {
        capacity(part).is_none_or(|(n, _)| (0..n).any(|s| !self.path_slot_used(part, s)))
    }

    /// Before a part goes: free its path slot.
    pub(crate) fn free_path(&mut self, o: &CourseObject) {
        let Some(slot) = slot_of(o) else { return };
        let a = self.area_mut();
        match o.id {
            84 => drop(a.snake_blocks.slots.remove(&slot)),
            85 => drop(a.track_blocks.slots.remove(&slot)),
            119 => drop(a.exclamation_blocks.slots.remove(&slot)),
            107 => drop(a.piranha_creepers.slots.remove(&slot)),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::*;
    use Step::*;

    #[test]
    fn codes_match_course_files() {
        // a sample snake path: right ×10, up ×3, right, end
        let mut steps = vec![Right; 10];
        steps.extend([Up, Up, Up, Right]);
        let c = encode(Right, &steps).unwrap();
        assert_eq!(&c[9..], &[2, 11, 4, 4, 12, 13]);
        assert_eq!(
            encode(Up, &[]).unwrap(),
            vec![15],
            "a path can be just its end"
        );
        assert!(encode(Right, &[Left]).is_err());
        assert_eq!(decode(&c), steps);
    }

    #[test]
    fn path_parts_get_slots_and_play() {
        let mut e = flat("3W");
        let x = e.place(119, 10, 4).unwrap();
        e.set_path(x, Right, &[Up, Up, Right]).unwrap();
        assert_eq!(e.path(x), Some(vec![11, 4, 12, 13]));
        assert_eq!(e.object(x).unwrap().extended_data, 1);
        let c = e.place(107, 20, 2).unwrap();
        e.set_path(c, Up, &[Up, Up, Right, Right]).unwrap();
        let t = e.place(85, 30, 4).unwrap();
        assert!(e.path(t).is_some(), "placed parts start with a route");
        let back = round_trip(&e);
        let lvl = mmk_sim::level::Level::from_course(&back, 0);
        assert_eq!(lvl.exclamations[0].1, vec![11, 4, 12, 13]);
        assert!(
            lvl.creepers
                .iter()
                .any(|(_, n)| n == &vec![4, 4, 12, 2, 13])
        );
        // erasing frees the slot
        e.erase_object(x);
        assert!(e.area().exclamation_blocks.is_empty());
    }

    #[test]
    fn snake_paths() {
        let mut e = flat("M1");
        let s = e.place(84, 10, 6).unwrap();
        e.set_path(s, Right, &[Right, Down, Down, Right]).unwrap();
        assert_eq!(e.path(s), Some(vec![2, 9, 3, 10, 13]));
        let back = round_trip(&e);
        // the sim finds the path the block names (extended data = slot + 1)
        let w = mmk_sim::World::from_course(&back);
        assert_eq!(w.level.snakes.len(), 1);
        assert_eq!(
            w.level.snakes[0].5,
            vec![2, 9, 3, 10, 13],
            "the block's own path"
        );
        let raw = back.main.snake_blocks.slots[&0].to_bytes().unwrap();
        assert_eq!(&raw[..4], &[0, 5, 1, 0], "slot, count, in use");
        assert_eq!(&raw[4..12], &[0, 0, 2, 0, 0, 1, 0, 0]);
        for _ in 0..4 {
            let i = e
                .place(84, 10, 10 + e.area().snake_blocks.len() as i32 * 2)
                .unwrap();
            assert!(e.path(i).is_some());
        }
        assert_eq!(
            e.place(84, 30, 6),
            Err(EditError::Limit("snake blocks")),
            "5 snake blocks per area"
        );
    }
}
