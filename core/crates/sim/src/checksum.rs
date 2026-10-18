//! A hash of the world's state for determinism checks: online peers compare it at confirmed
//! frames (`mmk-netplay`), and `mmk simulate --checksum` compares runs across processes. It
//! covers the tick, score, coins, every player's and actor's position, speed, facing and timer —
//! enough that a divergence shows within a frame or two of happening.

use crate::world::World;

/// FNV-1a over 64-bit words.
fn fnv(h: u64, x: u64) -> u64 {
    x.to_le_bytes()
        .iter()
        .fold(h, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

impl World {
    pub fn checksum(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325;
        h = fnv(h, self.tick);
        h = fnv(h, (self.score as u64) << 32 | self.coins as u64);
        for p in std::iter::once(&self.player).chain(&self.others) {
            for v in [p.x, p.y, p.vx, p.vy] {
                h = fnv(h, v.to_bits() as u64);
            }
            h = fnv(h, p.facing as u64);
        }
        h = fnv(h, self.actors.len() as u64);
        for a in &self.actors {
            for v in [a.x, a.y, a.vx, a.vy] {
                h = fnv(h, v.to_bits() as u64);
            }
            h = fnv(h, (a.timer as u32 as u64) << 8 | (a.dir as u8 as u64));
        }
        h
    }
}
