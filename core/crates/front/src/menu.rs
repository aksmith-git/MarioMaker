//! Pieces the SMM2 menu screens share: the abstract menu input, the game's selection frame and
//! scripted debug input (the Swift app's `SMM2Menus.swift`).

use crate::layout::{Layout, Ui};
use crate::{DrawItem, push_layout};

/// Menu buttons (a bit set).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons(pub u8);

impl Buttons {
    pub const UP: Buttons = Buttons(1);
    pub const DOWN: Buttons = Buttons(2);
    pub const CONFIRM: Buttons = Buttons(4);
    pub const BACK: Buttons = Buttons(8);
    pub const LEFT: Buttons = Buttons(16);
    pub const RIGHT: Buttons = Buttons(32);
    pub const NONE: Buttons = Buttons(0);

    pub fn has(self, b: Buttons) -> bool {
        self.0 & b.0 != 0
    }

    pub fn any(self, b: Buttons) -> bool {
        self.has(b)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn union(self, b: Buttons) -> Buttons {
        Buttons(self.0 | b.0)
    }

    pub fn minus(self, b: Buttons) -> Buttons {
        Buttons(self.0 & !b.0)
    }

    /// The direction pressed (layout units, y up), if any.
    pub fn direction(self) -> Option<[f32; 2]> {
        if self.has(Self::UP) {
            Some([0.0, 1.0])
        } else if self.has(Self::DOWN) {
            Some([0.0, -1.0])
        } else if self.has(Self::LEFT) {
            Some([-1.0, 0.0])
        } else if self.has(Self::RIGHT) {
            Some([1.0, 0.0])
        } else {
            None
        }
    }

    /// From a script name (confirm, back, up, down, left, right).
    pub fn from_name(s: &str) -> Option<Buttons> {
        Some(match s {
            "confirm" => Self::CONFIRM,
            "back" => Self::BACK,
            "up" => Self::UP,
            "down" => Self::DOWN,
            "left" => Self::LEFT,
            "right" => Self::RIGHT,
            _ => return None,
        })
    }
}

impl std::ops::BitOr for Buttons {
    type Output = Buttons;
    fn bitor(self, b: Buttons) -> Buttons {
        self.union(b)
    }
}

/// Menu input for one 60 Hz tick: `pressed` went down this tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct UiInput {
    pub pressed: Buttons,
    pub held: Buttons,
    /// The skip key (100 Mario: hold to skip a course) is down.
    pub skip: bool,
    /// A click / tap this tick, in layout units (1280 × 720, origin centre, y up).
    pub click: Option<[f32; 2]>,
    /// A key typed into an open text field (see `FrontInput::text`).
    pub text: Option<char>,
}

/// The game's selection frame (`Cmn_Cursor_00`): `W_SelectFrame_00` sized to the selected
/// button's bounding pane, faded with it [inf: as on the title].
pub struct MenuCursor {
    pub layout: Option<Layout>,
    pub shown: bool,
}

impl MenuCursor {
    pub fn new(ui: &Ui) -> Self {
        let mut layout = ui.layout("Cmn_Cursor_00");
        if let Some(l) = &mut layout {
            l.play_loop("Loop", 0);
        }
        Self {
            layout,
            shown: false,
        }
    }

    /// Frame pane `pane` of `l` (evaluated at `tick`); hidden when it isn't drawn.
    pub fn place(&mut self, l: &mut Layout, pane: &str, tick: i32, alpha: f32) -> bool {
        self.shown = false;
        let Some(c) = &mut self.layout else {
            return false;
        };
        l.apply(tick);
        let Some(r) = l.rect(pane) else {
            return false;
        };
        if !r.visible || r.alpha <= 0.01 {
            return false;
        }
        c.set_pane(
            "W_SelectFrame_00",
            mmk_ui::PaneOverride {
                translate: Some(r.center),
                size: Some(r.size),
                ..Default::default()
            },
        );
        c.set_alpha("N_All_00", 255.0 * r.alpha * alpha);
        self.shown = true;
        true
    }

    pub fn hide(&mut self) {
        self.shown = false;
    }

    pub fn draw(&mut self, tick: i32, out: &mut Vec<DrawItem>) {
        if self.shown
            && let Some(l) = &mut self.layout
        {
            push_layout(out, l, tick);
        }
    }
}

/// Debug captures across screens: `<tick>:<button>[,…]` presses menu buttons at global UI ticks
/// (every SMM2 menu screen's step counts one), like the Swift app's `MMK_UI_SCRIPT`.
#[derive(Clone, Debug, Default)]
pub struct Script {
    pub tick: i32,
    presses: Vec<(i32, Buttons)>,
}

impl Script {
    pub fn parse(s: &str) -> Self {
        let mut presses = Vec::new();
        for item in s.split(',') {
            if let Some((t, b)) = item.split_once(':')
                && let (Ok(t), Some(b)) = (t.trim().parse::<i32>(), Buttons::from_name(b.trim()))
            {
                presses.push((t, b));
            }
        }
        Self { tick: 0, presses }
    }

    /// One menu step: the scripted presses due now.
    pub fn step(&mut self) -> Buttons {
        self.tick += 1;
        self.at(self.tick)
    }

    /// The presses scripted for tick `t` (without counting a step).
    pub fn at(&self, t: i32) -> Buttons {
        self.presses
            .iter()
            .filter(|(at, _)| *at == t)
            .fold(Buttons::NONE, |a, (_, b)| a | *b)
    }
}

/// What a screen's loading part (`Cmn_LoadingIcon_01`) shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Loading {
    Hidden,
    /// The spinning icon with the screen's "please wait" (`<icon>-T_LoadText_00`).
    Waiting,
    /// A message instead (an empty list, an error), in the waiting line without the icon: the
    /// part's own no-data text uses an outlined font style the layer doesn't draw [inf].
    Message(String),
}

/// Show `state` in loading part `icon` of `l` (from `tick`).
pub fn set_loading(l: &mut Layout, icon: &str, state: &Loading, tick: i32) {
    if *state == Loading::Hidden {
        l.show(icon, false);
        return;
    }
    l.visible(icon, None);
    l.show(&format!("{icon}/N_NoData_00"), false);
    l.stop(&format!("{icon}/NoData"));
    l.visible(&format!("{icon}/N_Loading_00"), None);
    let text = format!("{icon}/T_LoadText_00");
    match state {
        Loading::Waiting => {
            l.visible(&format!("{icon}/N_Icon_00"), None);
            let label = format!("{}-T_LoadText_00", icon.replace('/', "-"));
            l.text_from(&text, &label);
            l.play_loop(&format!("{icon}/Loop"), tick);
        }
        Loading::Message(m) => {
            l.show(&format!("{icon}/N_Icon_00"), false);
            l.stop(&format!("{icon}/Loop"));
            l.text(&text, m);
        }
        Loading::Hidden => {}
    }
}

/// The first of `panes` (bounding panes, `[Part/…/]Pane`) drawn under the click `p` (layout
/// units, origin centre, y up) at `tick`.
pub fn hit(l: &mut Layout, tick: i32, panes: &[String], p: [f32; 2]) -> Option<usize> {
    l.apply(tick);
    panes.iter().position(|b| {
        l.rect(b)
            .is_some_and(|r| r.visible && r.alpha > 0.01 && r.contains(p))
    })
}

/// The nearest of `targets` (centres) from `from` in the pressed direction, for menus laid out
/// on a grid: candidates must lie in that direction (within 60° of it).
pub fn nearest(from: [f32; 2], dir: [f32; 2], targets: &[(usize, [f32; 2])]) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for &(i, c) in targets {
        let d = [c[0] - from[0], c[1] - from[1]];
        let along = d[0] * dir[0] + d[1] * dir[1];
        if along <= 1.0 {
            continue;
        }
        let across = (d[0] * dir[1] - d[1] * dir[0]).abs();
        if across > along * 1.8 {
            continue;
        }
        let cost = along + across * 2.0;
        if best.is_none_or(|b| cost < b.1) {
            best = Some((i, cost));
        }
    }
    best.map(|b| b.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_and_nearest() {
        let mut s = Script::parse("2:confirm,2:up,5:back,x:y");
        assert_eq!(s.step(), Buttons::NONE);
        assert_eq!(s.step(), Buttons::CONFIRM | Buttons::UP);
        let t = [(0, [0.0, 0.0]), (1, [100.0, 10.0]), (2, [0.0, 100.0])];
        assert_eq!(nearest([0.0, 0.0], [1.0, 0.0], &t), Some(1));
        assert_eq!(nearest([0.0, 0.0], [0.0, 1.0], &t), Some(2));
        assert_eq!(nearest([0.0, 0.0], [-1.0, 0.0], &t), None);
    }
}
