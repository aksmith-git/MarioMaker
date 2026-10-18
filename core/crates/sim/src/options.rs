//! Engine options: extras from the original Super Mario Maker (Wii U) layered on SMM2 play
//! (Phase 11). Every option is off by default, and with all of them off the simulation is the
//! SMM2 behaviour, bit for bit.
//!
//! Hosts keep one `Options` value and hand it to every `World` they make
//! ([`crate::World::set_options`]); a world keeps its options across deaths and restarts. Hosts
//! address options by name ([`Options::set`] / [`Options::get`], the FFI and the command lines
//! use the same names), so a new option needs no new entry point.

/// What Super Mushrooms become in SMB1-style courses (SMM1 shook the Super Mushroom into these
/// SMB1-only items).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MushroomSwap {
    /// SMM2: Super Mushrooms stay Super Mushrooms.
    #[default]
    Off,
    /// Mystery Mushrooms: a costume.
    Mystery,
    /// Weird Mushrooms: Lanky Mario.
    Weird,
}

impl MushroomSwap {
    pub fn code(self) -> i32 {
        match self {
            MushroomSwap::Off => 0,
            MushroomSwap::Mystery => 1,
            MushroomSwap::Weird => 2,
        }
    }

    pub fn from_code(v: i32) -> Option<Self> {
        match v {
            0 => Some(MushroomSwap::Off),
            1 => Some(MushroomSwap::Mystery),
            2 => Some(MushroomSwap::Weird),
            _ => None,
        }
    }

    /// `off` / `mystery` / `weird` (or the numeric code).
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "off" | "normal" | "none" => Some(MushroomSwap::Off),
            "mystery" | "costume" => Some(MushroomSwap::Mystery),
            "weird" | "lanky" => Some(MushroomSwap::Weird),
            n => n.parse().ok().and_then(Self::from_code),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// SMM1 shell physics: the mid-air shell kick and the shell jump (see `World::touch_shell`).
    pub smm1_shell_kicks: bool,
    /// SMM1 draw order (z ladder, see `World::smm1_layer`): the player passing through a pipe is
    /// in front of blocks but behind the pipes, coins sit behind blocks, tracks in front of
    /// pipes; in normal play the player stays in front of pipes, as in SMM2.
    pub smm1_layering: bool,
    /// What Super Mushrooms become in SMB1-style courses.
    pub smb1_mushroom: MushroomSwap,
    /// The costume a Mystery Mushroom gives: None = a random costume at each pickup, Some(id) =
    /// always that costume (an SMM1 costume id, 0 … COSTUMES − 1).
    pub costume: Option<u16>,
    /// The costumes a random pick draws from (bit per costume id; SMM1 draws among the owned
    /// ones): hosts set the costumes they have art for ([`Options::set_costume_pool`]). Empty =
    /// ids 0 … 99. Not a user setting, so not among [`Options::NAMES`].
    pub costume_pool: [u32; 7],
}

impl Options {
    /// Option names, as the FFI and the command lines spell them.
    pub const NAMES: [&'static str; 4] = [
        "smm1_shell_kicks",
        "smm1_layering",
        "smb1_mushroom",
        "costume",
    ];

    /// Whether these options change how a course plays (not just how it looks). Replays and
    /// online play assume SMM2 rules, so hosts skip replays and turn these off online.
    pub fn affects_play(&self) -> bool {
        self.smm1_shell_kicks || self.smb1_mushroom != MushroomSwap::Off
    }

    /// The same options with only the look-only ones kept (for replays and online play).
    pub fn look_only(&self) -> Self {
        Self {
            smm1_shell_kicks: false,
            smb1_mushroom: MushroomSwap::Off,
            ..*self
        }
    }

    /// Draw random costumes from `ids` (ids ≥ COSTUMES are ignored).
    pub fn set_costume_pool(&mut self, ids: impl IntoIterator<Item = u16>) {
        self.costume_pool = [0; 7];
        for id in ids {
            if (id as usize) < crate::player::COSTUMES {
                self.costume_pool[id as usize / 32] |= 1 << (id % 32);
            }
        }
    }

    /// The costume ids a random pick draws from.
    pub fn costume_ids(&self) -> Vec<u16> {
        if self.costume_pool == [0; 7] {
            return (0..100).collect();
        }
        (0..crate::player::COSTUMES as u16)
            .filter(|&i| self.costume_pool[i as usize / 32] & (1 << (i % 32)) != 0)
            .collect()
    }

    /// Set an option by name. Booleans take 0/1, `smb1_mushroom` 0 off / 1 mystery / 2 weird,
    /// `costume` −1 random or a costume id. Returns false for an unknown name or value.
    pub fn set(&mut self, name: &str, value: i32) -> bool {
        match name {
            "smm1_shell_kicks" => self.smm1_shell_kicks = value != 0,
            "smm1_layering" => self.smm1_layering = value != 0,
            "smb1_mushroom" => match MushroomSwap::from_code(value) {
                Some(m) => self.smb1_mushroom = m,
                None => return false,
            },
            "costume" => {
                if value < 0 {
                    self.costume = None;
                } else if (value as usize) < crate::player::COSTUMES {
                    self.costume = Some(value as u16);
                } else {
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    /// An option's value by name (the encoding of [`Options::set`]); None for an unknown name.
    pub fn get(&self, name: &str) -> Option<i32> {
        Some(match name {
            "smm1_shell_kicks" => self.smm1_shell_kicks as i32,
            "smm1_layering" => self.smm1_layering as i32,
            "smb1_mushroom" => self.smb1_mushroom.code(),
            "costume" => self.costume.map_or(-1, i32::from),
            _ => return None,
        })
    }

    /// Parse `name=value` (value as in [`Options::set`]; `smb1_mushroom` also takes
    /// off/mystery/weird and `costume` random). A bare name sets a boolean option.
    pub fn apply(&mut self, assignment: &str) -> Result<(), String> {
        let (name, value) = assignment
            .split_once('=')
            .map(|(n, v)| (n.trim(), v.trim()))
            .unwrap_or((assignment.trim(), "1"));
        let name = name.replace('-', "_");
        let v = match (name.as_str(), value) {
            ("smb1_mushroom", v) => MushroomSwap::parse(v).map(MushroomSwap::code),
            ("costume", "random") => Some(-1),
            (_, "on" | "true" | "yes") => Some(1),
            (_, "off" | "false" | "no") => Some(0),
            (_, v) => v.parse().ok(),
        };
        match v {
            Some(v) if self.set(&name, v) => Ok(()),
            _ => Err(format!(
                "bad option `{assignment}` (options: {})",
                Self::NAMES.join(", ")
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_smm2() {
        let o = Options::default();
        assert!(!o.smm1_shell_kicks && !o.smm1_layering);
        assert_eq!(o.smb1_mushroom, MushroomSwap::Off);
        assert_eq!(o.costume, None);
        for n in Options::NAMES {
            assert!(o.get(n).is_some_and(|v| v <= 0), "{n}");
        }
    }

    #[test]
    fn set_get_and_parse_round_trip() {
        let mut o = Options::default();
        assert!(o.set("smm1_shell_kicks", 1));
        assert!(o.set("smb1_mushroom", 2));
        assert!(o.set("costume", 7));
        assert_eq!(o.get("smm1_shell_kicks"), Some(1));
        assert_eq!(o.get("smb1_mushroom"), Some(2));
        assert_eq!(o.get("costume"), Some(7));
        assert!(!o.set("costume", 100_000));
        assert!(!o.set("smb1_mushroom", 9));
        assert!(!o.set("nonsense", 1));
        assert_eq!(o.get("nonsense"), None);
        let mut p = Options::default();
        p.apply("smm1-layering").unwrap();
        p.apply("smb1_mushroom=mystery").unwrap();
        p.apply("costume=random").unwrap();
        assert!(p.smm1_layering);
        assert_eq!(p.smb1_mushroom, MushroomSwap::Mystery);
        assert_eq!(p.costume, None);
        assert!(p.apply("smb1_mushroom=purple").is_err());
    }
}
