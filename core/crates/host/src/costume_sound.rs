//! SMM1 costume sounds (Phase 11): while the player wears a Mystery Mushroom costume, its own
//! sounds (`costume_<id>_<event>.bwav` of the SMM1 sprite cache's `audio/`) replace the jump,
//! power-up, death and clear sounds, and play its landing, skid and taunt sounds. Both
//! directors load the worn costume's sounds into the course's sound set when it is put on.

use std::sync::Arc;

use mmk_audio::Sound;
use mmk_sim::World;
use mmk_sim::player::Form;

/// Reads `<name>.bwav` of the SMM1 sprite cache's `audio/`.
pub type CostumeReader = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

/// SMM1 costume sound events (SMM1 event enum "down, goal, jump, powerup, appeal, ground,
/// slip"; `jump2` is an alternate take).
pub const COSTUME_EVENTS: [&str; 8] = [
    "jump", "jump2", "powerup", "appeal", "ground", "slip", "down", "goal",
];

/// The worn costume's sounds (ids in the mixer once loaded).
#[derive(Default)]
pub struct CostumeSounds {
    read: Option<CostumeReader>,
    loaded: Option<u16>,
    ids: [Option<u16>; 8],
    jump_alt: bool,
}

impl CostumeSounds {
    pub fn new(read: Option<CostumeReader>) -> Self {
        Self {
            read,
            ..Self::default()
        }
    }

    /// The mixer's sounds were replaced (a new sound set): load again when next worn.
    pub fn forget(&mut self) {
        self.loaded = None;
        self.ids = [None; 8];
    }

    /// Load the worn costume's sounds when it changes; `add` puts a sound into the mixer and
    /// returns its id.
    pub fn sync(&mut self, world: &World, add: &mut dyn FnMut(Sound) -> Option<u16>) {
        let want = (world.player.form == Form::Costume).then_some(world.player.costume);
        if want.is_none() || want == self.loaded {
            return;
        }
        let Some(read) = &self.read else { return };
        let id = want.unwrap_or(0);
        for (k, e) in COSTUME_EVENTS.iter().enumerate() {
            let name = format!("costume_{id:03}_{e}");
            self.ids[k] = read(&name)
                .and_then(|bytes| Sound::from_bwav(&name, &bytes).ok())
                .and_then(&mut *add);
        }
        self.loaded = want;
    }

    /// The worn costume's sound for `event` (None: not costumed, or the costume lacks it).
    pub fn sound(&self, world: &World, event: &str) -> Option<u16> {
        if world.player.form != Form::Costume || self.loaded != Some(world.player.costume) {
            return None;
        }
        let k = COSTUME_EVENTS.iter().position(|e| *e == event)?;
        self.ids[k]
    }

    /// The costume's jump: its second take every other jump.
    pub fn jump(&mut self, world: &World) -> Option<u16> {
        let first = self.sound(world, "jump")?;
        let alt = self.sound(world, "jump2").filter(|_| self.jump_alt);
        self.jump_alt = !self.jump_alt;
        Some(alt.unwrap_or(first))
    }
}
