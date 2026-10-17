//! A session plus what drawing it needs: the asset cache, the sprite resolver and the HUD text.

use std::sync::Arc;

use mmk_assets::Cache;
use mmk_host::{DrawSprite, SpriteResolver};

use crate::hud::HudText;
use crate::render::Scene;
use crate::session::Session;
use crate::source::{self, AssetSource};

pub struct Game {
    pub session: Session,
    /// Shared with the renderer's texture upload.
    pub cache: Arc<Cache>,
    /// No game art: sprites are drawn as placeholder boxes.
    pub placeholder: bool,
    resolver: SpriteResolver,
    sprites: Vec<DrawSprite>,
    hud: HudText,
}

impl Game {
    /// Open the cache through `assets` (None: placeholder mode) and load `course` (None: the
    /// built-in demo strip). Course bytes may be raw, zlib-compressed or an encrypted `.bcd`.
    pub fn load(assets: Option<&dyn AssetSource>, course: Option<&[u8]>) -> Result<Self, String> {
        let cache = match assets {
            Some(a) => source::open_cache(a)?,
            None => Cache::default(),
        };
        let course = match course {
            Some(bytes) => Some(mmk_host::course::decode(bytes, || {
                source::key_table(assets)
            })?),
            None => None,
        };
        Ok(Self {
            session: Session::new(course),
            placeholder: assets.is_none(),
            cache: Arc::new(cache),
            resolver: SpriteResolver::default(),
            sprites: Vec::new(),
            hud: HudText::default(),
        })
    }

    /// What to draw now.
    pub fn scene(&mut self) -> Scene<'_> {
        let w = &self.session.world;
        self.resolver.resolve(w, &self.cache, &mut self.sprites);
        self.hud = HudText::of(w, self.session.phase);
        Scene {
            sprites: &self.sprites,
            camera: (w.camera_x, w.camera_y),
            upside_down: w.upside_down(),
            clear: mmk_host::clear_color(w, &self.cache),
            hud: &self.hud,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::MemorySource;

    #[test]
    fn placeholder_demo_and_blank_course_build_scenes() {
        let mut g = Game::load(None, None).unwrap();
        let s = g.scene();
        assert!(!s.sprites.is_empty());
        assert_eq!(s.clear, 0x0000_00ff);
        assert!(s.hud.strip.contains("TIME"));

        let course = mmk_formats::smm2::Course::blank("M1").to_bytes().unwrap();
        let mut m = MemorySource::new("mem");
        m.insert("manifest.json", b"{}".to_vec());
        m.insert("textures/index.json", b"{}".to_vec());
        let mut g = Game::load(Some(&m), Some(&course)).unwrap();
        assert!(!g.placeholder);
        assert!(g.scene().sprites.iter().all(|s| s.texture.is_none()));
        assert!(Game::load(None, Some(b"not a course")).is_err());
    }
}
