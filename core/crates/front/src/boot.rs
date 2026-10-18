//! The SMM2 opening (`Boot_BootScene_00`) at launch, before the title, as the game's boot page
//! runs it (the Swift app's `SMM2Boot.swift`):
//! - Appear picks one opening: the newest one an update added that hasn't been seen yet
//!   (Koopalings, then Link, then co-op; seeing a newer one marks the older ones seen too), else
//!   at random: 66% InDefault, 8% InCharacter, 8% InKuribo, 2% InCoopMode, 8% InLink,
//!   4% InKoopaling, 4% InKoopa [obs]; it plays at rate 1 to its end;
//! - InMain (200 frames) brings the characters and the logo to the final picture [obs];
//! - Disp holds it; the boot sequence then changes to the title with the black circle [obs].
//!
//! The game reads no input there [obs]. `Loop` (the characters' idle) is not bound by the page;
//! played from the start [inf].

use crate::host::Host;
use crate::layout::{Layout, Ui};
use crate::{DrawItem, push_layout};

/// The openings in the page's order (index = the binary's choice).
pub const OPENINGS: [&str; 7] = [
    "InDefault",
    "InCharacter",
    "InKuribo",
    "InKoopa",
    "InCoopMode",
    "InLink",
    "InKoopaling",
];

/// Save key of the "seen" flags of the update openings (bit 0 co-op, 1 Link, 2 Koopalings).
const SEEN_KEY: &str = "boot.openingsSeen";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Appear,
    InMain,
    Disp,
}

pub struct BootScene {
    ui: Ui,
    pub layout: Layout,
    pub opening: &'static str,
    pub tick: i32,
    pub state: State,
}

impl BootScene {
    /// `forced`: play this opening (captures); None chooses like the game.
    pub fn new(ui: &Ui, forced: Option<&str>, host: &mut dyn Host) -> Option<Self> {
        let mut l = ui.layout("Boot_BootScene_00")?;
        let opening = match forced.and_then(|f| OPENINGS.iter().find(|&&o| o == f)) {
            Some(o) => *o,
            None => OPENINGS[choose(host)],
        };
        l.play_loop("Loop", 0);
        l.play(opening, 0);
        // the opening's own sound sequence (BootScene's Anm_<opening>) [obs data]
        ui.sound_anim("Boot_BootScene_00", opening, None);
        Some(Self {
            ui: ui.clone(),
            layout: l,
            opening,
            tick: 0,
            state: State::Appear,
        })
    }

    /// One tick; true when the final picture is up (the host changes to the title).
    pub fn step(&mut self) -> bool {
        self.tick += 1;
        match self.state {
            State::Appear => {
                if self.layout.finished(self.opening, self.tick) {
                    self.state = State::InMain;
                    self.layout.play("InMain", self.tick);
                    // the title call
                    self.ui.sound_anim("Boot_BootScene_00", "InMain", None);
                }
            }
            State::InMain => {
                if self.layout.finished("InMain", self.tick) {
                    self.state = State::Disp;
                    return true;
                }
            }
            State::Disp => {}
        }
        false
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, self.tick);
    }
}

/// The opening to play, with the update openings' "seen" flags kept by the host like the game's
/// save flags.
pub fn choose(host: &mut dyn Host) -> usize {
    let seen: u32 = host
        .load_value(SEEN_KEY)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let mut set = |v: u32| host.store_value(SEEN_KEY, &v.to_string());
    if seen & 4 == 0 {
        set(seen | 7);
        return 6;
    }
    if seen & 2 == 0 {
        set(seen | 3);
        return 5;
    }
    if seen & 1 == 0 {
        set(seen | 1);
        return 4;
    }
    match host.random() % 100 {
        0..66 => 0,
        66..74 => 1,
        74..82 => 2,
        82..84 => 4,
        84..92 => 5,
        92..96 => 6,
        _ => 3,
    }
}
