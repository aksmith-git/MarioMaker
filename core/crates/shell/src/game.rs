//! A session plus what drawing it needs: the asset cache, the sprite resolver and the HUD text.

use std::sync::Arc;

use mmk_assets::Cache;
use mmk_formats::smm2::Course;
use mmk_host::{DrawSprite, SpriteResolver};

use crate::hud::HudText;
use crate::render::Scene;
use crate::session::Session;
use crate::source::{self, AssetSource};

pub struct Game {
    pub session: Session,
    /// Course Maker mode (`--edit`): the editor, and whether it is showing (else test play).
    pub maker: Option<crate::maker::Maker>,
    pub editing: bool,
    /// Shared with the renderer's texture upload.
    pub cache: Arc<Cache>,
    /// No game art: sprites are drawn as placeholder boxes.
    pub placeholder: bool,
    resolver: SpriteResolver,
    sprites: Vec<DrawSprite>,
    hud: HudText,
    /// NSMBU / 3D World models (None without a mesh stage in the cache, or `--no-meshes`).
    pub meshes: Option<mmk_host::mesh::MeshStore>,
    planner: mmk_host::mesh::MeshPlanner,
    mesh_draws: Vec<mmk_host::mesh::MmkMeshDraw>,
    mesh_frame: crate::mesh::MeshFrame,
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
        Ok(Self::with_cache(Arc::new(cache), assets.is_none(), course))
    }

    /// A game on an already opened cache (the web build keeps one for every course it plays).
    pub fn with_cache(cache: Arc<Cache>, placeholder: bool, course: Option<Course>) -> Self {
        let meshes = {
            let store = mmk_host::mesh::MeshStore::new(cache.root.clone());
            (!placeholder && store.available()).then_some(store)
        };
        Self {
            maker: None,
            editing: false,
            session: Session::new(course),
            placeholder,
            cache,
            meshes,
            planner: Default::default(),
            mesh_draws: Vec::new(),
            mesh_frame: Default::default(),
            resolver: SpriteResolver::default(),
            sprites: Vec::new(),
            hud: HudText::default(),
        }
    }

    /// What to draw now.
    /// Test-play the maker's course (Enter in the editor).
    pub fn play_maker_course(&mut self) {
        let Some(bytes) = self.maker.as_ref().and_then(|m| m.course_bytes()) else {
            return;
        };
        if let Ok(c) = mmk_formats::smm2::Course::parse(&bytes) {
            let audio = self.session.audio.clone();
            let options = self.session.options;
            self.session = Session::new(Some(c));
            self.session.set_options(options);
            self.session.set_audio(audio);
            self.editing = false;
            self.resolver.reset();
            self.planner.reset();
        }
    }

    /// Back from test play to the editor.
    pub fn edit_again(&mut self) {
        if self.maker.is_some() {
            self.editing = true;
            self.resolver.reset();
            self.planner.reset();
        }
    }

    pub fn scene(&mut self) -> Scene<'_> {
        if self.editing
            && let Some(m) = self.maker.as_mut()
        {
            let status = m.status();
            let w = m.world();
            self.planner.frame(
                w,
                &self.cache,
                self.meshes.as_mut(),
                &mut self.resolver,
                &mut self.sprites,
                &mut self.mesh_draws,
            );
            match self.meshes.as_mut() {
                Some(store) => {
                    crate::mesh::build(store, &self.cache, &self.mesh_draws, &mut self.mesh_frame)
                }
                None => self.mesh_frame.clear(),
            }
            self.hud = HudText {
                strip: status,
                banner: None,
            };
            return Scene {
                sprites: &self.sprites,
                camera: (w.camera_x, w.camera_y),
                upside_down: false,
                clear: mmk_host::clear_color(w, &self.cache),
                hud: &self.hud,
                meshes: &self.mesh_frame,
            };
        }
        let w = &self.session.world;
        self.planner.frame(
            w,
            &self.cache,
            self.meshes.as_mut(),
            &mut self.resolver,
            &mut self.sprites,
            &mut self.mesh_draws,
        );
        match self.meshes.as_mut() {
            Some(store) => {
                crate::mesh::build(store, &self.cache, &self.mesh_draws, &mut self.mesh_frame)
            }
            None => self.mesh_frame.clear(),
        }
        self.hud = HudText::of(w, self.session.phase);
        Scene {
            sprites: &self.sprites,
            camera: (w.camera_x, w.camera_y),
            upside_down: w.upside_down(),
            clear: mmk_host::clear_color(w, &self.cache),
            hud: &self.hud,
            meshes: &self.mesh_frame,
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
