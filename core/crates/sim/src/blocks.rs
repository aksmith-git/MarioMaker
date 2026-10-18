//! Blocks that react: brick, ? block, hidden block, empty block, hard block, note block…
//!
//! From research/specs/objects/4-brick.md §B and 5-question-block.md. Plain blocks live in the
//! level grid as solid cells; this map holds what the grid cannot: the block kind, its contents and
//! the bump animation. Positions are tile coordinates (y up); a block's origin is its bottom
//! centre.

use std::collections::BTreeMap;

use crate::level::TILE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Brick,
    Question,
    Hidden,
    /// Used block (contents taken): never bounces again.
    Empty,
    Hard,
    Note,
    Donut,
    Cloud,
    Ice,
    /// ON/OFF switch block: toggles the course-wide ON/OFF state when hit.
    OnOff,
    /// Dotted-line block: solid when its colour matches the ON/OFF state (red = ON).
    Dotted {
        blue: bool,
    },
}

/// What a block releases (the course object's child id, mapped to what the slice supports).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contents {
    Coin,
    Mushroom,
    FireFlower,
    Star,
    OneUp,
    /// The style power-up (44): Super Leaf in SMB3, Cape Feather in SMW (SMB1: mushroom).
    Special,
    Goomba,
    Koopa {
        red: bool,
    },
    /// Yoshi's Egg (SMW / NSMBU style; other styles' 45 is the Goomba's Shoe).
    YoshiEgg,
    /// An update power-up item (`Kind::PowerItem::what`): part 81, the Master Sword (part 20
    /// with flag 0x4 in SMB1), a wearable (127–131).
    Power(u8),
}

impl Contents {
    /// Course part id → contents (child_id of a block object).
    pub fn from_part(id: i16, child_flags: u32) -> Option<Self> {
        Some(match id {
            8 => Contents::Coin,
            // the Super Mushroom's 0x4 form: the Master Sword (SMB1 only; the world turns it
            // back into a mushroom in the other styles) [obs 0x7100d80df0]
            20 if child_flags & 0x4 != 0 => Contents::Power(crate::actors::SWORD),
            20 => Contents::Mushroom,
            44 => Contents::Special,
            34 => Contents::FireFlower,
            35 => Contents::Star,
            33 => Contents::OneUp,
            0 => Contents::Goomba,
            1 => Contents::Koopa {
                red: child_flags & 0x4 != 0,
            },
            45 => Contents::YoshiEgg,
            id => Contents::Power(crate::actors::item_for_part(id)?),
        })
    }

    pub fn is_powerup(self) -> bool {
        matches!(
            self,
            Contents::FireFlower | Contents::Special | Contents::Power(crate::actors::STYLE2)
        )
    }
}

/// Bounce state (B6): v ← v − a (clamped at v_end), d ← d + v; ends when d crosses 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bump {
    pub d: f32,
    pub v: f32,
    pub down: bool,
    a: f32,
    v_end: f32,
    /// Player index that hit the block (contents go to them).
    pub frames: u32,
}

impl Bump {
    pub fn up() -> Self {
        Self {
            d: 0.0,
            v: 2.0,
            down: false,
            a: 0.281,
            v_end: -2.0,
            frames: 0,
        }
    }
    pub fn down() -> Self {
        Self {
            d: 0.0,
            v: -2.0,
            down: true,
            a: 0.281,
            v_end: 2.0,
            frames: 0,
        }
    }
    pub fn note_up() -> Self {
        Self {
            d: 0.0,
            v: 4.5,
            down: false,
            a: 0.64,
            v_end: -4.5,
            frames: 0,
        }
    }
    pub fn note_down() -> Self {
        Self {
            d: 0.0,
            v: -4.3,
            down: true,
            a: 0.64,
            v_end: 4.3,
            frames: 0,
        }
    }

    /// One frame; returns true when the bump has ended.
    pub fn step(&mut self) -> bool {
        if self.down {
            self.v = (self.v + self.a).min(self.v_end);
        } else {
            self.v = (self.v - self.a).max(self.v_end);
        }
        self.d += self.v;
        self.frames += 1;
        if (!self.down && self.d <= 0.0) || (self.down && self.d >= 0.0) {
            self.d = 0.0;
            return true;
        }
        false
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    pub contents: Option<Contents>,
    pub bump: Option<Bump>,
    /// Multi-coin brick: coins given so far and the 280-frame window (B9).
    pub coins_given: u8,
    pub coin_timer: i32,
}

impl Block {
    pub fn new(kind: BlockKind, contents: Option<Contents>) -> Self {
        Self {
            kind,
            contents,
            bump: None,
            coins_given: 0,
            coin_timer: 0,
        }
    }

    /// Vertical draw/collision offset while bouncing.
    pub fn offset(&self) -> f32 {
        self.bump.map_or(0.0, |b| b.d)
    }
}

/// What a hit from below (or above) asks the world to do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitResult {
    Nothing,
    /// Bounced (things on top get a block hit).
    Bumped,
    /// Bounced and popped a coin now.
    Coin,
    /// Broke into debris.
    Broke,
    /// ON/OFF switch hit: bounced and the state flips.
    Switch,
}

/// Events produced when a bump ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Release {
    pub x: i32,
    pub y: i32,
    pub contents: Contents,
    pub down: bool,
}

/// How strong the hitter is (B4: the player's per-side capability levels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitPower {
    /// Small player head: bumps bricks.
    Bump,
    /// Super or bigger head, kicked shell: breaks bricks.
    Break,
}

pub const MULTI_COIN_WINDOW: i32 = 280;
pub const MULTI_COIN_MAX: u8 = 10;

#[derive(Clone, Debug, Default)]
pub struct Blocks {
    pub map: BTreeMap<(i32, i32), Block>,
}

impl Blocks {
    pub fn get(&self, x: i32, y: i32) -> Option<&Block> {
        self.map.get(&(x, y))
    }

    /// A hit on the block at (x, y). `from_below` = the hitter is under the block (side 2);
    /// false = on top (side 3). Returns what happened; the caller clears the grid cell on Broke.
    pub fn hit(&mut self, x: i32, y: i32, from_below: bool, power: HitPower) -> HitResult {
        let Some(b) = self.map.get_mut(&(x, y)) else {
            return HitResult::Nothing;
        };
        if b.bump.is_some() {
            return HitResult::Nothing; // only idle blocks accept a bump (B5)
        }
        let bump = if from_below { Bump::up() } else { Bump::down() };
        match b.kind {
            BlockKind::Empty | BlockKind::Hard | BlockKind::Ice => HitResult::Nothing,
            BlockKind::Brick => {
                if b.contents.is_none() && power == HitPower::Break {
                    self.map.remove(&(x, y));
                    return HitResult::Broke;
                }
                b.bump = Some(bump);
                match b.contents {
                    Some(Contents::Coin) => {
                        // multi-coin block: up to 10 coins within 280 frames of the first hit
                        if b.coins_given == 0 {
                            b.coin_timer = MULTI_COIN_WINDOW;
                        }
                        let last = b.coins_given + 1 >= MULTI_COIN_MAX || b.coin_timer <= 0;
                        b.coins_given += 1;
                        if last {
                            b.contents = None;
                            b.kind = BlockKind::Empty;
                        }
                        HitResult::Coin
                    }
                    _ => HitResult::Bumped,
                }
            }
            BlockKind::Question | BlockKind::Hidden => {
                if b.kind == BlockKind::Hidden && !from_below {
                    return HitResult::Nothing;
                }
                b.kind = BlockKind::Question;
                b.bump = Some(bump);
                let c = b.contents.unwrap_or(Contents::Coin);
                if c == Contents::Coin {
                    // coin pops at bump start; the block is used at bump end
                    b.contents = None;
                    b.coins_given = 1;
                    HitResult::Coin
                } else {
                    b.contents = Some(c);
                    HitResult::Bumped
                }
            }
            BlockKind::Note => {
                b.bump = Some(if from_below {
                    Bump::note_up()
                } else {
                    Bump::note_down()
                });
                HitResult::Bumped
            }
            BlockKind::Donut | BlockKind::Cloud | BlockKind::Dotted { .. } => HitResult::Nothing,
            BlockKind::OnOff => {
                b.bump = Some(bump);
                HitResult::Switch
            }
        }
    }

    /// Advance bumps and multi-coin timers; returns items to spawn (bump end, B9).
    pub fn step(&mut self) -> Vec<Release> {
        let mut out = Vec::new();
        for (&(x, y), b) in self.map.iter_mut() {
            if b.coin_timer > 0 {
                b.coin_timer -= 1;
            }
            if let Some(mut bump) = b.bump {
                if bump.step() {
                    b.bump = None;
                    let down = bump.down;
                    match b.kind {
                        BlockKind::Question => {
                            if let Some(c) = b.contents.take() {
                                out.push(Release {
                                    x,
                                    y,
                                    contents: c,
                                    down,
                                });
                            }
                            b.kind = BlockKind::Empty;
                        }
                        BlockKind::Brick => {
                            if let Some(c) = b.contents
                                && c != Contents::Coin
                            {
                                b.contents = None;
                                b.kind = BlockKind::Empty;
                                out.push(Release {
                                    x,
                                    y,
                                    contents: c,
                                    down,
                                });
                            }
                        }
                        _ => {}
                    }
                } else {
                    b.bump = Some(bump);
                }
            }
        }
        out
    }

    /// Tile coordinates and current pixel offset of every bouncing block (for "hit things on
    /// top").
    pub fn rising(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.map
            .iter()
            .filter(|(_, b)| b.bump.is_some_and(|bp| !bp.down && bp.frames <= 1))
            .map(|(&k, _)| k)
    }
}

/// Bottom-centre pixel position of tile (x, y).
pub fn origin(x: i32, y: i32) -> (f32, f32) {
    (x as f32 * TILE + TILE / 2.0, y as f32 * TILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_matches_spec_curve() {
        // B6: normal up-bump lasts 14 frames, peak 6.13 at frame 7
        let mut b = Bump::up();
        let mut path = Vec::new();
        loop {
            let done = b.step();
            path.push(b.d);
            if done {
                break;
            }
        }
        assert_eq!(path.len(), 14);
        assert!((path[0] - 1.719).abs() < 1e-3);
        assert!((path[6] - 6.132).abs() < 1e-3);
    }

    #[test]
    fn question_block_gives_coin_then_empties() {
        let mut bl = Blocks::default();
        bl.map.insert((3, 4), Block::new(BlockKind::Question, None));
        assert_eq!(bl.hit(3, 4, true, HitPower::Bump), HitResult::Coin);
        for _ in 0..14 {
            bl.step();
        }
        assert_eq!(bl.get(3, 4).unwrap().kind, BlockKind::Empty);
        assert_eq!(bl.hit(3, 4, true, HitPower::Bump), HitResult::Nothing);
    }

    #[test]
    fn item_released_at_bump_end() {
        let mut bl = Blocks::default();
        bl.map.insert(
            (0, 0),
            Block::new(BlockKind::Question, Some(Contents::Mushroom)),
        );
        assert_eq!(bl.hit(0, 0, true, HitPower::Bump), HitResult::Bumped);
        let mut released = None;
        for f in 1..=14 {
            let r = bl.step();
            if !r.is_empty() {
                released = Some(f);
            }
        }
        assert_eq!(released, Some(14));
    }

    #[test]
    fn brick_breaks_only_when_big() {
        let mut bl = Blocks::default();
        bl.map.insert((0, 0), Block::new(BlockKind::Brick, None));
        assert_eq!(bl.hit(0, 0, true, HitPower::Bump), HitResult::Bumped);
        for _ in 0..14 {
            bl.step();
        }
        assert_eq!(bl.hit(0, 0, true, HitPower::Break), HitResult::Broke);
        assert!(bl.get(0, 0).is_none());
    }

    #[test]
    fn multi_coin_brick_gives_ten() {
        let mut bl = Blocks::default();
        bl.map
            .insert((0, 0), Block::new(BlockKind::Brick, Some(Contents::Coin)));
        let mut coins = 0;
        for _ in 0..20 {
            if bl.hit(0, 0, true, HitPower::Break) == HitResult::Coin {
                coins += 1;
            }
            for _ in 0..14 {
                bl.step();
            }
        }
        assert_eq!(coins, 10);
        assert_eq!(bl.get(0, 0).unwrap().kind, BlockKind::Empty);
    }
}
