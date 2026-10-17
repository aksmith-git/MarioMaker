//! One play session of one course, independent of window, GPU and audio device: fixed 60 Hz
//! stepping from the host clock (like the Swift app's `GameSession`), pause, course clear and
//! game over, and the sound director fed after every tick.

use std::sync::Arc;

use mmk_formats::smm2::Course;
use mmk_host::audio::{AudioOut, Director};
use mmk_sim::{Input, World, input};

/// Length of one sim tick in seconds.
pub const TICK: f64 = 1.0 / 60.0;
/// The most host time one frame may catch up on (after a stall, the game slows down instead of
/// running hundreds of ticks at once).
const MAX_CATCH_UP: f64 = 0.25;
/// Frames a result screen ignores input for, so a held jump doesn't skip it.
const RESULT_GRACE: u32 = 30;
/// Music volume while paused.
const PAUSED_MUSIC: f32 = 0.3;

/// Turns host time into a whole number of fixed ticks.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixedStep {
    acc: f64,
}

impl FixedStep {
    /// Add `dt` seconds of host time; returns how many ticks to run now.
    pub fn advance(&mut self, dt: f64) -> u32 {
        self.acc = (self.acc + dt.max(0.0)).min(MAX_CATCH_UP);
        // (the epsilon keeps exact multiples of a tick from rounding down)
        let n = (self.acc / TICK + 1e-9).floor();
        self.acc -= n * TICK;
        n as u32
    }
    pub fn reset(&mut self) {
        self.acc = 0.0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Playing,
    Paused,
    Cleared,
    GameOver,
}

/// Shell actions that are not sim buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Actions {
    /// Restart the course (R key).
    pub restart: bool,
}

pub struct Session {
    pub world: World,
    course: Option<Course>,
    pub phase: Phase,
    step: FixedStep,
    pub(crate) audio: Option<Arc<AudioOut>>,
    director: Option<Director>,
    /// The start button must be released before it pauses again.
    held_start: bool,
    /// Buttons held when a result screen opened (not counted as presses).
    held: u32,
    was_dying: bool,
    phase_frames: u32,
    /// Replaces the player's input (smoke tests); given the tick.
    pub scripted: Option<fn(u64) -> Input>,
}

impl Session {
    /// A session on `course`, or the built-in demo strip.
    pub fn new(course: Option<Course>) -> Self {
        let world = match &course {
            Some(c) => World::from_course(c),
            None => World::new(),
        };
        Self {
            world,
            course,
            phase: Phase::Playing,
            step: FixedStep::default(),
            audio: None,
            director: None,
            held_start: true,
            held: 0,
            was_dying: false,
            phase_frames: 0,
            scripted: None,
        }
    }

    /// Attach sound output (None detaches): the course music starts.
    pub fn set_audio(&mut self, audio: Option<Arc<AudioOut>>) {
        self.director = audio.as_ref().map(|a| {
            let mut d = a.director();
            d.load_music(a, &self.world.level.style, self.world.level.theme);
            d.start_course(&self.world);
            d
        });
        self.audio = audio;
        self.apply_volumes();
    }

    fn apply_volumes(&self) {
        if let Some(a) = &self.audio {
            let music = if self.phase == Phase::Paused {
                PAUSED_MUSIC
            } else {
                1.0
            };
            a.send(mmk_audio::Command::SetVolumes {
                master: 1.0,
                music,
                sfx: 1.0,
            });
        }
    }

    fn set_phase(&mut self, p: Phase, held: u32) {
        self.phase = p;
        self.phase_frames = 0;
        self.held = held;
        self.apply_volumes();
    }

    /// Start the course again (fresh lives, score and timer).
    pub fn restart(&mut self) {
        self.world = match &self.course {
            Some(c) => World::from_course(c),
            None => World::new(),
        };
        if let Some(d) = &mut self.director {
            d.start_course(&self.world);
        }
        self.was_dying = false;
        self.step.reset();
        self.held_start = true;
        self.set_phase(Phase::Playing, 0);
    }

    /// One display frame: `dt` seconds of host time with `live` input held. Returns the number
    /// of sim ticks run.
    pub fn frame(&mut self, dt: f64, live: Input, actions: Actions) -> u32 {
        let live = if self.scripted.is_some() {
            Input::default()
        } else {
            live
        };
        if actions.restart {
            self.restart();
            return 0;
        }
        let start_down = live.held(input::START);
        let start_pressed = start_down && !self.held_start;
        self.held_start = start_down;
        let pressed = live.buttons & !self.held;
        self.held &= live.buttons;
        self.phase_frames = self.phase_frames.saturating_add(1);

        match self.phase {
            Phase::Playing => {
                if start_pressed {
                    self.set_phase(Phase::Paused, live.buttons);
                    return 0;
                }
                let n = self.step.advance(dt);
                let mut ran = 0;
                for _ in 0..n {
                    let mut inp = match self.scripted {
                        Some(f) => f(self.world.tick),
                        None => live,
                    };
                    inp.buttons &= !input::START;
                    self.tick(inp);
                    ran += 1;
                    if self.phase != Phase::Playing {
                        break;
                    }
                }
                ran
            }
            Phase::Paused => {
                if start_pressed {
                    self.step.reset();
                    self.set_phase(Phase::Playing, live.buttons);
                }
                0
            }
            Phase::Cleared | Phase::GameOver => {
                if self.phase_frames > RESULT_GRACE && pressed & (input::JUMP | input::START) != 0 {
                    self.restart();
                }
                0
            }
        }
    }

    /// Exactly one sim tick (also used by the headless screenshot run).
    pub fn tick(&mut self, input: Input) {
        self.world.step(input);
        if let Some(d) = &mut self.director {
            d.after_step(&self.world);
        }
        let h = self.world.hud();
        let dying = !self.world.player.alive();
        if self.was_dying && !dying && h.lives == 0 {
            self.set_phase(Phase::GameOver, input.buttons);
        }
        self.was_dying = dying;
        if h.cleared {
            self.set_phase(Phase::Cleared, input.buttons);
        }
    }
}

/// The smoke-test script, as in the Swift app: run right, jump for 20 ticks every 90.
pub fn scripted_run(tick: u64) -> Input {
    let mut b = input::RIGHT | input::RUN;
    if tick % 90 >= 60 && tick % 90 < 80 {
        b |= input::JUMP;
    }
    Input { buttons: b }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_step_counts_whole_ticks_and_caps_catch_up() {
        let mut s = FixedStep::default();
        assert_eq!(s.advance(TICK * 0.5), 0);
        assert_eq!(s.advance(TICK * 0.6), 1);
        assert_eq!(s.advance(TICK * 2.0), 2);
        // a long stall runs at most a quarter second of ticks
        assert_eq!(s.advance(10.0), 15);
        assert_eq!(s.advance(-1.0), 0);
    }

    #[test]
    fn demo_strip_steps_at_60_hz_and_pauses_on_start() {
        let mut s = Session::new(None);
        let none = Input::default();
        let start = Input {
            buttons: input::START,
        };
        assert_eq!(s.frame(1.0 / 30.0, none, Actions::default()), 2);
        assert_eq!(s.world.tick, 2);
        s.frame(TICK, start, Actions::default());
        assert_eq!(s.phase, Phase::Paused);
        assert_eq!(s.frame(1.0, none, Actions::default()), 0);
        s.frame(TICK, start, Actions::default());
        assert_eq!(s.phase, Phase::Playing);
        let restart = Actions { restart: true };
        s.frame(TICK, none, restart);
        assert_eq!(s.world.tick, 0);
    }

    #[test]
    fn scripted_run_moves_right() {
        let mut s = Session::new(None);
        s.scripted = Some(scripted_run);
        let x0 = s.world.player.x;
        for _ in 0..60 {
            s.frame(TICK, Input::default(), Actions::default());
        }
        assert!(s.world.player.x > x0 + 32.0);
    }
}
