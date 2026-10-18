//! The course-server account, on SMM2's player info screen `Nwk_PlayerInfo_00` (opened from the
//! Course World hub's player card, `L_PlayerBody_00`, over the hub as the game does: the hub's
//! InPlayerInfo fades its buttons, then the screen's In).
//!
//! The game's three buttons (face / name / outfit) become ours [departure: no Miis, no Nintendo
//! account]: signed out, "Create account" (type a name, `register`) and "Sign in with a token"
//! (type the secret token of an existing account, checked with `me`); signed in, "Sign out". The
//! name sits where the game shows the player's name; while typing it shows the text with a
//! blinking caret [inf]. The Mii body is hidden.

use serde_json::{Value, json};

use crate::layout::{Layout, Ui};
use crate::menu::{Buttons, MenuCursor, UiInput, hit};
use crate::online::edit;
use crate::sound::Event;
use crate::{Cx, DrawItem, push_layout};

pub const LAYOUT: &str = "Nwk_PlayerInfo_00";

/// Server limits (`mmk_server::accounts::MAX_NAME`, a token's 64 hex digits).
pub const MAX_NAME: usize = 10;
const MAX_TOKEN: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Token,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Create,
    SignInToken,
    SignOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountEvent {
    Back,
}

pub struct AccountScene {
    ui: Ui,
    pub layout: Layout,
    cursor: MenuCursor,
    pub tick: i32,
    /// The buttons shown (part, action), top first.
    pub buttons: Vec<(&'static str, Action)>,
    pub selection: usize,
    pub editing: Option<(Field, String)>,
    waiting: Option<(u32, Field, String)>,
    closing: Option<i32>,
    busy_until: i32,
}

impl AccountScene {
    pub fn new(ui: &Ui, cx: &mut Cx) -> Option<Self> {
        let mut l = ui.layout(LAYOUT)?;
        l.play("In", 0);
        l.show("L_MiiBody_01", false);
        // the screen's own full-screen fade stays clear (the hub shows under it) [inf]
        l.show("P_Fade_00", false);
        l.show("L_Btn_01", false);
        let busy = l.ticks("In", 1);
        let mut s = Self {
            ui: ui.clone(),
            layout: l,
            cursor: MenuCursor::new(ui),
            tick: 0,
            buttons: Vec::new(),
            selection: 0,
            editing: None,
            waiting: None,
            closing: None,
            busy_until: busy,
        };
        s.refresh(cx);
        Some(s)
    }

    /// Buttons and the name line for the account state.
    fn refresh(&mut self, cx: &Cx) {
        self.buttons = if cx.online.signed_in() {
            vec![("L_Btn_02", Action::SignOut)]
        } else {
            vec![
                ("L_Btn_02", Action::Create),
                ("L_Btn_00", Action::SignInToken),
            ]
        };
        self.selection = self.selection.min(self.buttons.len() - 1);
        let l = &mut self.layout;
        l.show("L_Btn_00", self.buttons.len() > 1);
        for (part, action) in &self.buttons {
            l.text(
                &format!("{part}/T_Btn_00"),
                match action {
                    Action::Create => "Create account",
                    Action::SignInToken => "Sign in with a token",
                    Action::SignOut => "Sign out",
                },
            );
        }
        for (k, (part, _)) in self.buttons.iter().enumerate() {
            l.stop(&format!("{part}/Decide"));
            let a = format!("{part}/Select");
            if k == self.selection {
                l.hold_end(&a);
            } else {
                l.hold(&a, 0.0);
            }
        }
        self.name_line(cx);
    }

    fn name_line(&mut self, cx: &Cx) {
        let text = match (&self.editing, &cx.online.account) {
            (Some((_, t)), _) => {
                // a blinking caret while typing [inf]
                let caret = if self.tick / 30 % 2 == 0 { "_" } else { " " };
                format!("{t}{caret}")
            }
            (None, Some(a)) if !a.name.is_empty() => a.name.clone(),
            (None, Some(_)) => "Signed in".to_string(),
            (None, None) if self.waiting.is_some() => "...".to_string(),
            (None, None) => "Not signed in".to_string(),
        };
        self.layout.text("T_PlayerName_00", &text);
    }

    pub fn typing(&self) -> bool {
        self.editing.is_some()
    }

    pub fn step(&mut self, input: &UiInput, cx: &mut Cx) -> Option<AccountEvent> {
        self.tick += 1;
        if let Some(at) = self.closing {
            return (self.tick >= at).then_some(AccountEvent::Back);
        }
        self.collect(cx);
        if self.editing.is_some() {
            self.type_key(input, cx);
        } else if self.tick >= self.busy_until && self.waiting.is_none() {
            let mut pressed = input.pressed;
            if let Some(p) = input.click {
                let panes: Vec<String> = self
                    .buttons
                    .iter()
                    .map(|(b, _)| format!("{b}/B_Btn_00"))
                    .collect();
                if let Some(i) = hit(&mut self.layout, self.tick, &panes, p) {
                    self.select(i);
                    pressed = pressed | Buttons::CONFIRM;
                }
            }
            if let Some(e) = self.handle(pressed, cx) {
                return Some(e);
            }
        }
        if self.editing.is_some() {
            self.name_line(cx);
        }
        let part = self.buttons[self.selection].0;
        let alpha = if self.editing.is_some() || self.waiting.is_some() {
            0.0
        } else {
            1.0
        };
        self.cursor.place(
            &mut self.layout,
            &format!("{part}/B_Btn_00"),
            self.tick,
            alpha,
        );
        None
    }

    fn select(&mut self, i: usize) {
        if i == self.selection {
            return;
        }
        let part = self.buttons[i].0;
        self.ui.sound_button(LAYOUT, part, Event::Select);
        self.layout
            .hold(&format!("{}/Select", self.buttons[self.selection].0), 0.0);
        self.selection = i;
        self.layout.play(&format!("{part}/Select"), self.tick);
    }

    fn handle(&mut self, pressed: Buttons, cx: &mut Cx) -> Option<AccountEvent> {
        let n = self.buttons.len();
        if pressed.has(Buttons::BACK) {
            self.ui.sound_back(LAYOUT, "normal");
            self.layout.play("Out", self.tick);
            self.closing = Some(self.tick + self.layout.ticks("Out", 1));
            return None;
        }
        if pressed.has(Buttons::UP) && self.selection > 0 {
            self.select(self.selection - 1);
        } else if pressed.has(Buttons::DOWN) && self.selection + 1 < n {
            self.select(self.selection + 1);
        } else if pressed.has(Buttons::CONFIRM) {
            let (part, action) = self.buttons[self.selection];
            self.ui.sound_button(LAYOUT, part, Event::Decide);
            self.layout.play(&format!("{part}/Decide"), self.tick);
            match action {
                Action::Create => self.editing = Some((Field::Name, String::new())),
                Action::SignInToken => self.editing = Some((Field::Token, String::new())),
                Action::SignOut => {
                    cx.online.sign_out(cx.host);
                    cx.toast("Signed out of the course server.");
                    self.selection = 0;
                    self.refresh(cx);
                }
            }
            self.name_line(cx);
        }
        None
    }

    /// A key typed into the open field ('\u{1b}' or back cancels, enter or confirm sends).
    fn type_key(&mut self, input: &UiInput, cx: &mut Cx) {
        let Some((field, text)) = &mut self.editing else {
            return;
        };
        let field = *field;
        let max = if field == Field::Name {
            MAX_NAME
        } else {
            MAX_TOKEN
        };
        let mut done = input.pressed.has(Buttons::CONFIRM);
        let mut cancel = input.pressed.has(Buttons::BACK);
        match input.text {
            Some('\u{1b}') => cancel = true,
            Some(c) => done |= edit(text, c, max),
            None => {}
        }
        if cancel {
            self.editing = None;
            self.name_line(cx);
            return;
        }
        if !done {
            return;
        }
        let text = text.trim().to_string();
        self.editing = None;
        if text.is_empty() {
            self.name_line(cx);
            return;
        }
        let call = match field {
            Field::Name => cx.online.call(cx.host, "register", json!({ "name": text })),
            // the token typed is checked by asking the server who it belongs to
            Field::Token => {
                let saved = cx.online.account.take();
                cx.online.account = Some(crate::online::Account {
                    name: String::new(),
                    maker_id: String::new(),
                    token: text.clone(),
                });
                let id = cx.online.call(cx.host, "me", Value::Null);
                cx.online.account = saved;
                id
            }
        };
        match call {
            Some(id) => self.waiting = Some((id, field, text)),
            None => cx.toast("No course server is set (--server or MMK_SERVER)."),
        }
        self.name_line(cx);
    }

    /// The server's answer to a registration or a token check.
    fn collect(&mut self, cx: &mut Cx) {
        let Some((id, field, text)) = self.waiting.clone() else {
            return;
        };
        let Some(answer) = cx.online.take(id) else {
            return;
        };
        self.waiting = None;
        let signed = match (field, answer) {
            (Field::Name, Ok(v)) => cx.online.sign_in(cx.host, &v),
            (Field::Token, Ok(v)) => cx
                .online
                .sign_in(cx.host, &json!({ "account": v, "token": text })),
            (_, Err(e)) => {
                cx.toast(&format!("The server refused: {e}"));
                false
            }
        };
        if signed {
            let name = cx
                .online
                .account
                .as_ref()
                .map(|a| a.name.clone())
                .unwrap_or_default();
            cx.toast(&format!("Signed in as {name}."));
        }
        self.selection = 0;
        self.refresh(cx);
    }

    pub fn draw(&mut self, out: &mut Vec<DrawItem>) {
        push_layout(out, &mut self.layout, self.tick);
        self.cursor.draw(self.tick, out);
    }
}
