//! The SMM2 layouts' own sounds, resolved the way the game's data says (`mmk import-smm2-ui` →
//! `ui/sound/sounds.json` + WAVs; see `mmk_import::smm2_ui_sound`), as the Swift app's
//! `UISound` does:
//! - a layout names its sound user and gives some buttons a sound type (`L_PlayBtn_00` →
//!   `makeplaybtn`); selecting a button plays the user's `UB_Select_<type>` (plain `UB_Select`
//!   without a type), deciding it `UB_Decide_<type>`; an animation with a sound plays
//!   `Anm_<Animation>` (`Anm_<Animation>_<type>` on a typed button);
//! - a container key plays its unconditional children (in sequence, with their delays), or the
//!   one conditional child the context picks (`Select_Key`, `…_backmove_title`, `In_Layout_M1`);
//!   volumes and delays (frames) are the data's.
//!
//! Which context picks which child (the game's conditions are not read) is ours [inf]. The
//! output is a list of [`SoundCue`]s (WAV names) the host plays.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

/// One sound to start: `ui/sound/<name>.wav`, at `volume`, `delay` frames (60 Hz) from now.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundCue {
    pub name: String,
    pub volume: f32,
    pub delay: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Select,
    Decide,
}

#[derive(Clone, Debug, Default)]
struct LayoutInfo {
    user: String,
    buttons: HashMap<String, String>,
    #[allow(dead_code)]
    anims: HashSet<String>,
}

/// `sounds.json`, parsed.
#[derive(Clone, Debug, Default)]
pub struct UiSounds {
    layouts: HashMap<String, LayoutInfo>,
    users: HashMap<String, serde_json::Map<String, Value>>,
}

impl UiSounds {
    pub fn from_json(s: &str) -> Option<Self> {
        let doc: Value = serde_json::from_str(s).ok()?;
        let mut out = UiSounds::default();
        for (name, v) in doc.get("layouts")?.as_object()? {
            let Some(user) = v.get("user").and_then(Value::as_str) else {
                continue;
            };
            let buttons = v
                .get("buttons")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, t)| Some((k.clone(), t.as_str()?.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            let anims = v
                .get("anims")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            out.layouts.insert(
                name.clone(),
                LayoutInfo {
                    user: user.to_string(),
                    buttons,
                    anims,
                },
            );
        }
        if let Some(users) = doc.get("users").and_then(Value::as_object) {
            for (u, v) in users {
                if let Some(m) = v.as_object() {
                    out.users.insert(u.clone(), m.clone());
                }
            }
        }
        Some(out)
    }

    /// The sound type of button `pane` (a part path) in `layout`: its own entry, else the
    /// longest listed prefix of the path (`L_WorldMap_00/L_WorldMapCourse` covers every dot).
    fn button_type<'a>(info: &'a LayoutInfo, pane: &str) -> Option<&'a str> {
        if let Some(t) = info.buttons.get(pane) {
            return Some(t);
        }
        let last = pane.rsplit('/').next().unwrap_or(pane);
        if let Some(t) = info.buttons.get(last) {
            return Some(t);
        }
        info.buttons
            .keys()
            .filter(|k| pane.starts_with(k.as_str()) || last.starts_with(k.as_str()))
            .max_by_key(|k| k.len())
            .and_then(|k| info.buttons.get(k))
            .map(String::as_str)
    }

    /// A button of `layout` was selected / decided; `context` picks a conditional child.
    pub fn button(
        &self,
        layout: &str,
        pane: &str,
        event: Event,
        context: &[&str],
    ) -> Vec<SoundCue> {
        let Some(info) = self.layouts.get(layout) else {
            return Vec::new();
        };
        let verb = match event {
            Event::Select => "UB_Select",
            Event::Decide => "UB_Decide",
        };
        let key = match Self::button_type(info, pane) {
            Some(t) => format!("{verb}_{t}"),
            None => verb.to_string(),
        };
        let has = self
            .users
            .get(&info.user)
            .is_some_and(|u| u.contains_key(&key));
        let mut ctx: Vec<&str> = context.to_vec();
        if event == Event::Select {
            ctx.push("Select_Key");
        }
        self.play(&info.user, if has { &key } else { verb }, &ctx)
    }

    /// A "back" decided on `layout` (the user's backmove, by context: title / normal).
    pub fn back(&self, layout: &str, context: &str) -> Vec<SoundCue> {
        let Some(info) = self.layouts.get(layout) else {
            return Vec::new();
        };
        let c = format!("UB_Decide_backmove_{context}");
        self.play(&info.user, "UB_Decide_backmove", &[&c])
    }

    /// Animation `anim` of `layout` started (of button `pane` when given).
    pub fn anim(
        &self,
        layout: &str,
        anim: &str,
        pane: Option<&str>,
        context: &[&str],
    ) -> Vec<SoundCue> {
        let Some(info) = self.layouts.get(layout) else {
            return Vec::new();
        };
        let mut key = format!("Anm_{anim}");
        if let Some(t) = pane.and_then(|p| Self::button_type(info, p)) {
            key = format!("{key}_{t}");
        }
        self.play(&info.user, &key, context)
    }

    /// Any key of `layout`'s user (e.g. the pause menu's `In_Layout`).
    pub fn key(&self, layout: &str, key: &str, context: &[&str]) -> Vec<SoundCue> {
        let Some(info) = self.layouts.get(layout) else {
            return Vec::new();
        };
        self.play(&info.user, key, context)
    }

    fn play(&self, user: &str, key: &str, context: &[&str]) -> Vec<SoundCue> {
        let mut out = Vec::new();
        if let Some(node) = self.users.get(user).and_then(|u| u.get(key)) {
            Self::node(node, context, 0.0, &mut out);
        }
        out
    }

    fn node(node: &Value, context: &[&str], delay: f32, out: &mut Vec<SoundCue>) {
        let d = delay + node.get("delay").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        if let Some(sound) = node.get("sound").and_then(Value::as_str) {
            if sound != "@Blank" {
                out.push(SoundCue {
                    name: sound.to_string(),
                    volume: node.get("volume").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    delay: d,
                });
            }
            return;
        }
        let Some(children) = node.get("children").and_then(Value::as_array) else {
            return;
        };
        let conditional = |c: &Value| c.get("conditional").and_then(Value::as_bool) == Some(true);
        if let Some(pick) = children.iter().filter(|c| conditional(c)).find(|c| {
            c.get("key")
                .and_then(Value::as_str)
                .is_some_and(|k| context.contains(&k))
        }) {
            Self::node(pick, context, d, out);
            return;
        }
        for c in children.iter().filter(|c| !conditional(c)) {
            Self::node(c, context, d, out);
        }
    }
}

/// Playing the SMM2 import's sounds (cues queue on the [`Ui`] until the host takes them).
impl crate::layout::Ui {
    pub fn sound_button(&self, layout: &str, pane: &str, event: Event) {
        if let Some(s) = self.sounds() {
            self.push_cues(s.button(layout, pane, event, &[]));
        }
    }

    pub fn sound_back(&self, layout: &str, context: &str) {
        if let Some(s) = self.sounds() {
            self.push_cues(s.back(layout, context));
        }
    }

    pub fn sound_anim(&self, layout: &str, anim: &str, pane: Option<&str>) {
        if let Some(s) = self.sounds() {
            self.push_cues(s.anim(layout, anim, pane, &[]));
        }
    }

    pub fn sound_key(&self, layout: &str, key: &str, context: &[&str]) {
        if let Some(s) = self.sounds() {
            self.push_cues(s.key(layout, key, context));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{
      "layouts": {"L": {"user": "U", "buttons": {"L_Btn_00": "okbtn", "L_Map_00/L_Dot": "dot"}, "anims": ["In"]}},
      "users": {"U": {
        "UB_Select": {"children": [
          {"sound": "touch", "key": "Select_Touch", "conditional": true},
          {"sound": "key", "key": "Select_Key", "conditional": true}]},
        "UB_Decide_okbtn": {"sound": "ok", "volume": 0.5, "delay": 10},
        "UB_Select_dot": {"sound": "dot"},
        "Anm_In": {"delay": 2, "children": [{"sound": "a"}, {"sound": "b", "delay": 5}]},
        "UB_Decide_backmove": {"children": [
          {"sound": "t", "key": "UB_Decide_backmove_title", "conditional": true},
          {"sound": "n", "key": "UB_Decide_backmove_normal", "conditional": true}]}
      }}}"#;

    #[test]
    fn resolves_buttons_containers_and_contexts() {
        let s = UiSounds::from_json(DOC).unwrap();
        let names = |v: Vec<SoundCue>| v.into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names(s.button("L", "L_Other", Event::Select, &[])), ["key"]);
        let d = s.button("L", "L_Btn_00", Event::Decide, &[]);
        assert_eq!((d[0].volume, d[0].delay), (0.5, 10.0));
        assert_eq!(
            names(s.button("L", "L_Map_00/L_Dot_03", Event::Select, &[])),
            ["dot"]
        );
        let a = s.anim("L", "In", None, &[]);
        assert_eq!(a.iter().map(|c| c.delay).collect::<Vec<_>>(), [2.0, 7.0]);
        assert_eq!(names(s.back("L", "title")), ["t"]);
        assert!(s.button("Nope", "x", Event::Select, &[]).is_empty());
    }
}
