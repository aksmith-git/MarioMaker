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
    background_frame: crate::mesh::MeshFrame,
    field_camera: Option<mmk_host::scene3d::Mat4>,
    lights: [[[f32; 4]; mmk_host::scene3d::LIGHT_ROWS]; 2],
    /// Called with the replay of each clear (`Session::record_replays`), e.g. to send it to the
    /// course server as a verified time.
    pub on_clear: Option<Box<dyn FnMut(mmk_modes::replay::Replay)>>,
    ghost_sprites: Vec<DrawSprite>,
    /// No placeholder HUD strip (the front end draws the SMM2 HUD layout over the course).
    pub no_hud: bool,
    /// The cache's effects (empty: none imported) and this frame's quads.
    fx_lib: Arc<mmk_host::effects::EffectLibrary>,
    fx_frame: crate::effects::EffectFrame,
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
        #[cfg(not(target_arch = "wasm32"))]
        let fx_lib = Arc::new(if placeholder {
            Default::default()
        } else {
            mmk_host::effects::EffectLibrary::load(&cache.root)
        });
        #[cfg(target_arch = "wasm32")]
        let fx_lib = Arc::new(mmk_host::effects::EffectLibrary::default());
        let mut session = Session::new(course);
        session.effects = Some(mmk_host::effects::EffectDirector::new(fx_lib.clone()));
        Self {
            fx_lib,
            fx_frame: Default::default(),
            maker: None,
            editing: false,
            session,
            placeholder,
            cache,
            meshes,
            planner: Default::default(),
            mesh_draws: Vec::new(),
            mesh_frame: Default::default(),
            background_frame: Default::default(),
            field_camera: None,
            lights: [[[0.0; 4]; mmk_host::scene3d::LIGHT_ROWS]; 2],
            resolver: SpriteResolver::default(),
            sprites: Vec::new(),
            hud: HudText::default(),
            on_clear: None,
            ghost_sprites: Vec::new(),
            no_hud: false,
        }
    }

    /// Play `course` (None: the demo strip) from its start, keeping the sound output.
    pub fn set_course(&mut self, course: Option<Course>) {
        let audio = self.session.audio.clone();
        self.session = Session::new(course);
        self.session.set_audio(audio);
        self.resolver.reset();
        self.planner.reset();
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
            self.session.effects =
                Some(mmk_host::effects::EffectDirector::new(self.fx_lib.clone()));
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

    /// Skin this frame's play-field and background meshes through their cameras (after
    /// `planner.frame`). `edit`: the Course Maker's flat view (3D World's perspective only comes
    /// in when playing).
    fn build_meshes(&mut self, cam: (f32, f32), upside_down: bool, edit: bool) {
        let scene = self.planner.scene.clone();
        self.field_camera = None;
        let Some(store) = self.meshes.as_mut() else {
            self.mesh_frame.clear();
            self.background_frame.clear();
            return;
        };
        let field = match &scene {
            Some(s) if !edit => s.field,
            _ => mmk_host::scene3d::field_camera("", cam),
        };
        if scene.is_some() {
            self.field_camera = Some(field.view_proj);
        }
        crate::mesh::build(
            store,
            &self.cache,
            &self.mesh_draws,
            &mut self.mesh_frame,
            &crate::mesh::Projection {
                view_proj: &field.view_proj,
                layered: true,
                flip_y: upside_down,
            },
        );
        match scene.as_ref().and_then(|s| s.background) {
            Some(bg) => crate::mesh::build(
                store,
                &self.cache,
                &self.planner.background,
                &mut self.background_frame,
                &crate::mesh::Projection {
                    view_proj: &bg.view_proj,
                    layered: false,
                    flip_y: upside_down,
                },
            ),
            None => self.background_frame.clear(),
        }
        if let Some(s) = &scene {
            self.lights = [s.field_light, s.background_light];
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
            let cam = (w.camera_x, w.camera_y);
            self.build_meshes(cam, false, true);
            let w = self.maker.as_mut().expect("editing").world();
            self.hud = HudText {
                strip: status,
                banner: None,
            };
            self.fx_frame.quads.clear();
            return Scene {
                sprites: &self.sprites,
                camera: (w.camera_x, w.camera_y),
                upside_down: false,
                clear: mmk_host::clear_color(w, &self.cache),
                hud: &self.hud,
                meshes: &self.mesh_frame,
                background: &self.background_frame,
                field_camera: self.field_camera,
                lights: &self.lights,
                ghost: &[],
                effects: &self.fx_frame,
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
        let (cam, upside_down) = ((w.camera_x, w.camera_y), w.upside_down());
        self.build_meshes(cam, upside_down, false);
        let w = &self.session.world;
        self.ghost_sprites.clear();
        if let Some(g) = &self.session.ghost
            && let Some(sp) = g.world.player_sprite()
        {
            let style = g.world.level.style.clone();
            let d = self.resolver.resolve_one(&style, &sp, &self.cache);
            self.ghost_sprites.push(d);
        }
        self.hud = if self.no_hud {
            HudText {
                strip: String::new(),
                banner: None,
            }
        } else {
            HudText::of(w, self.session.phase)
        };
        match &mut self.session.effects {
            Some(fx) => {
                fx.quads(&mut self.fx_frame.quads);
                self.fx_frame
                    .resolve_textures(fx.player.texture_names(), &self.cache);
            }
            None => self.fx_frame.quads.clear(),
        }
        Scene {
            effects: &self.fx_frame,
            sprites: &self.sprites,
            camera: (w.camera_x, w.camera_y),
            upside_down: w.upside_down(),
            clear: mmk_host::clear_color(w, &self.cache),
            hud: &self.hud,
            meshes: &self.mesh_frame,
            background: &self.background_frame,
            field_camera: self.field_camera,
            lights: &self.lights,
            ghost: &self.ghost_sprites,
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
