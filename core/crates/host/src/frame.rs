//! Sim sprites → textured rectangles, the way every renderer draws them.

use std::collections::HashMap;

use mmk_assets::{Cache, MtexFormat, Quad};
use mmk_sim::{Sprite, World};

/// One rectangle to draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawSprite {
    /// Texture index in the asset cache, or None when the cache lacks the art (placeholder mode,
    /// or not imported yet): the renderer may draw a stand-in shape.
    pub texture: Option<u32>,
    /// Bottom-left corner in world pixels (y up) and size in pixels.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Texture coordinates, (0,0) = top-left of the texture.
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub layer: u32,
    /// Mirror the image horizontally (swap u0/u1 when drawing).
    pub flip_x: bool,
}

/// Screen rectangle and texture coordinates of a sim sprite: [x, y, w, h, u0, v0, u1, v1].
///
/// Tile sprites (`rect` set) draw that part of the texture into the sim's rectangle. Model
/// sprites (`rect` None) follow the convention that the sim's (x + w/2, y) is the model origin
/// and (w, h) the texture size times the sim's scale; when the BFRES quad of the texture is known
/// it is drawn at origin + quad (scaled), mirrored around the origin for `flip_x`, with the
/// quad's texture coordinates. Without a quad the texture fills the sim's rectangle.
/// `flip_y` flips the image in place. Horizontal flipping of the image itself is left to the
/// renderer (`DrawSprite::flip_x` / FFI `flags` bit 0 swaps u0/u1).
pub fn place(s: &Sprite, tex_w: f32, tex_h: f32, quad: Option<Quad>) -> [f32; 8] {
    let mut out = match (s.image.rect, quad) {
        (Some((x, y, w, h)), _) => [
            s.x,
            s.y,
            s.w,
            s.h,
            x / tex_w,
            y / tex_h,
            (x + w) / tex_w,
            (y + h) / tex_h,
        ],
        (None, Some(q)) => {
            let (sx, sy) = (s.w / tex_w, s.h / tex_h);
            let (ox, oy) = (s.x + s.w / 2.0, s.y);
            let (mut x0, mut x1) = (q.x0 * sx, q.x1 * sx);
            if s.flip_x {
                (x0, x1) = (-x1, -x0);
            }
            [
                ox + x0,
                oy + q.y0 * sy,
                x1 - x0,
                q.height() * sy,
                q.u0,
                q.v0,
                q.u1,
                q.v1,
            ]
        }
        (None, None) => [s.x, s.y, s.w, s.h, 0.0, 0.0, 1.0, 1.0],
    };
    if s.flip_y {
        out.swap(5, 7);
    }
    out
}

/// Size, in the sim's atlas pixels, of a texture a tile sprite cuts its `rect` from. The sim
/// addresses every field atlas in SMB1 pixels (16 × 48 cells of 16 px, 256 × 768); NSMBU's atlas
/// has the same grid at 64 px a cell (1024 × 3072), so its rects scale with it.
pub fn atlas_units(s: &Sprite, tex_w: u32, tex_h: u32) -> (f32, f32) {
    const SIM_ATLAS_W: u32 = 256;
    if s.image.rect.is_some()
        && s.image.model.contains("_Field_")
        && tex_w > SIM_ATLAS_W
        && tex_w.is_multiple_of(SIM_ATLAS_W)
    {
        let k = (tex_w / SIM_ATLAS_W) as f32;
        (tex_w as f32 / k, tex_h as f32 / k)
    } else {
        (tex_w as f32, tex_h as f32)
    }
}

/// Texture index and BFRES quad of a (model, texture) name, and whether it is a stand-in drawn
/// at its own size (an SMM1 costume without its art: small Mario); None if the cache lacks it.
type Resolved = Option<(u32, Option<Quad>, bool)>;

/// Model name prefix of the SMM1 costumes (`SMM1_Costume_<id>`, the importer's costume cache).
pub const COSTUME_PREFIX: &str = "SMM1_Costume_";

/// Small Mario's frame for a costume frame (costumes have a few poses he lacks).
fn costume_stand_in(frame: &str) -> &str {
    match frame.split_once('.') {
        Some(("b_dash", n)) => ["walk.0", "walk.1", "walk.2"][n.parse::<usize>().unwrap_or(0) % 3],
        Some(("jump_fall", _)) => "jump.0",
        Some(("pole", _)) => "climb.0",
        Some(("appeal" | "shoes_wait", _)) => "wait.0",
        _ => frame,
    }
}

/// Resolves the sim's (model, texture) names against an asset cache, once per name. Clear it
/// (`reset`) when the cache changes.
#[derive(Debug, Default)]
pub struct SpriteResolver {
    lookup: HashMap<(&'static str, &'static str), Resolved>,
}

impl SpriteResolver {
    pub fn reset(&mut self) {
        self.lookup.clear();
    }

    /// The world's sprites in draw order (stable sort by layer), placed for drawing.
    pub fn resolve(&mut self, world: &World, cache: &Cache, out: &mut Vec<DrawSprite>) {
        let mut sprites = world.sprites();
        sprites.sort_by_key(|s| s.layer);
        out.clear();
        out.reserve(sprites.len());
        let style = world.level.style.as_str();
        for s in sprites {
            out.push(self.resolve_one(style, &s, cache));
        }
    }

    /// One sprite of a course in `style`, placed for drawing.
    pub fn resolve_one(&mut self, style: &str, s: &Sprite, cache: &Cache) -> DrawSprite {
        let key = (s.image.model, s.image.texture);
        let found = *self.lookup.entry(key).or_insert_with(|| {
            let resolve_tex = |model: &str, tex: &str| {
                let i = cache.texture_index(model, tex)?;
                Some((i as u32, cache.sprite_quad(model, tex), false))
            };
            // the other SMM1 extras without their art: the nearest SMM2 art
            let smm1_stand_in = match key.0 {
                "M1_Player_SlimMarioMdl" => Some(("M1_Player_SpMarioMdl", key.1)),
                // the 3D-only update forms drawn as sprites (no meshes): Super Mario's art
                "M1_Player_SquirrelMarioMdl" | "M1_Player_BoomerangMarioMdl" => {
                    Some(("M1_Player_SpMarioMdl", key.1))
                }
                "M1_Item_kinokofunny" | "M1_Item_CharaKinoko" => Some(("M1_Item_kinoko", "wait.0")),
                _ => None,
            };
            if let Some((model, tex)) = smm1_stand_in {
                return resolve_tex(key.0, key.1).or_else(|| {
                    resolve_tex(model, tex)
                        .or_else(|| resolve_tex(model, "wait.0"))
                        .map(|(i, q, _)| (i, q, true))
                });
            }
            if key.0.starts_with(COSTUME_PREFIX) {
                // an SMM1 costume: its own art, else small Mario standing in
                return resolve_tex(key.0, key.1).or_else(|| {
                    let model = "M1_Player_MarioMdl";
                    resolve_tex(model, costume_stand_in(key.1))
                        .or_else(|| resolve_tex(model, "wait.0"))
                        .map(|(i, q, _)| (i, q, true))
                });
            }
            // the sim names SMB1 models; other styles use the same names with their own prefix
            // (M3_, MW_, …) and fall back to the SMB1 art where a style has no such model
            let styled = key
                .0
                .strip_prefix("M1_")
                .filter(|_| style != "M1")
                .map(|rest| format!("{style}_{rest}"));
            // a style with fewer animation frames: use its last lower frame ("walk.2" →
            // "walk.1") rather than mixing in SMB1 art
            let lower_frame = |model: &str| {
                let (stem, n) = key.1.rsplit_once('.')?;
                let n: u32 = n.parse().ok()?;
                (0..n)
                    .rev()
                    .find_map(|k| resolve_tex(model, &format!("{stem}.{k}")))
            };
            // 3D World tiles without a model (pipes, note blocks, …): the NSMBU atlas
            let wu_field = key
                .0
                .strip_prefix("3W_Field_")
                .map(|t| format!("WU_Field_{t}"));
            styled
                .as_deref()
                .and_then(|m| resolve_tex(m, key.1).or_else(|| lower_frame(m)))
                .or_else(|| resolve_tex(key.0, key.1))
                .or_else(|| wu_field.as_deref().and_then(|m| resolve_tex(m, m)))
        });
        let (texture, p) = match found {
            Some((texture, quad, stand_in)) => {
                let t = &cache.textures[texture as usize];
                let (tw, th) = (t.width as f32, t.height as f32);
                let p = if stand_in {
                    // drawn at its own size around the same origin
                    let mut own = *s;
                    own.x = s.x + s.w / 2.0 - tw / 2.0;
                    (own.w, own.h) = (tw, th);
                    place(&own, tw, th, quad)
                } else {
                    let (w, h) = atlas_units(s, t.width, t.height);
                    place(s, w, h, quad)
                };
                (Some(texture), p)
            }
            None => (None, place(s, s.w.max(1.0), s.h.max(1.0), None)),
        };
        DrawSprite {
            texture,
            x: p[0],
            y: p[1],
            w: p[2],
            h: p[3],
            u0: p[4],
            v0: p[5],
            u1: p[6],
            v1: p[7],
            layer: s.layer,
            flip_x: s.flip_x,
        }
    }
}

/// Background clear colour (0xRRGGBBAA): the theme background's top-left pixel when the cache has
/// it as RGBA8, black otherwise (underground, castle, placeholder art).
pub fn clear_color(world: &World, cache: &Cache) -> u32 {
    const BLACK: u32 = 0x0000_00ff;
    let Some((model, tex)) = world.background() else {
        return BLACK;
    };
    let Some(i) = cache.texture_index(model, tex) else {
        return BLACK;
    };
    let Some(t) = cache.texture(i) else {
        return BLACK;
    };
    if t.format != MtexFormat::Rgba8 || t.data.len() < 4 {
        return BLACK;
    }
    u32::from_be_bytes([t.data[0], t.data[1], t.data[2], 0xff])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite(rect: Option<(f32, f32, f32, f32)>, x: f32, y: f32, w: f32, h: f32) -> Sprite {
        Sprite {
            image: mmk_sim::render::SpriteImage {
                model: "M",
                texture: "t",
                rect,
            },
            x,
            y,
            w,
            h,
            flip_x: false,
            flip_y: false,
            layer: 0,
        }
    }

    const GOOMBA: Quad = Quad {
        x0: -8.0,
        y0: -16.0,
        x1: 8.0,
        y1: 16.0,
        u0: 0.0,
        v0: 0.0,
        u1: 1.0,
        v1: 1.0,
    };

    #[test]
    fn model_sprite_uses_quad_around_origin() {
        // origin (100, 32): sim passes x = origin - w/2, y = origin
        let s = sprite(None, 92.0, 32.0, 16.0, 32.0);
        assert_eq!(
            place(&s, 16.0, 32.0, Some(GOOMBA)),
            [92.0, 16.0, 16.0, 32.0, 0.0, 0.0, 1.0, 1.0]
        );
        // big (×2): the sim's size is twice the texture size
        let s = sprite(None, 84.0, 32.0, 32.0, 64.0);
        assert_eq!(
            place(&s, 16.0, 32.0, Some(GOOMBA))[..4],
            [84.0, 0.0, 32.0, 64.0]
        );
        // no quad: fill the sim rectangle
        let s = sprite(None, 92.0, 32.0, 16.0, 32.0);
        assert_eq!(place(&s, 16.0, 32.0, None)[..4], [92.0, 32.0, 16.0, 32.0]);
    }

    #[test]
    fn flips_mirror_around_origin_and_in_place() {
        let q = Quad {
            x0: 0.0,
            x1: 40.0,
            y0: 0.0,
            y1: 40.0,
            ..GOOMBA
        };
        let mut s = sprite(None, 92.0, 0.0, 40.0, 40.0); // origin x = 112
        s.flip_x = true;
        assert_eq!(place(&s, 40.0, 40.0, Some(q))[..4], [72.0, 0.0, 40.0, 40.0]);
        s.flip_x = false;
        s.flip_y = true;
        let p = place(&s, 40.0, 40.0, Some(q));
        assert_eq!((p[5], p[7]), (1.0, 0.0));
    }

    #[test]
    fn tile_sprite_uses_atlas_rect() {
        let s = sprite(Some((16.0, 0.0, 16.0, 16.0)), 5.0, 6.0, 16.0, 16.0);
        assert_eq!(
            place(&s, 256.0, 256.0, Some(GOOMBA)),
            [5.0, 6.0, 16.0, 16.0, 0.0625, 0.0, 0.125, 0.0625]
        );
    }

    #[test]
    fn empty_cache_reports_missing_art_in_layer_order() {
        let w = World::new();
        let mut out = Vec::new();
        let mut r = SpriteResolver::default();
        r.resolve(&w, &Cache::default(), &mut out);
        assert!(!out.is_empty());
        assert!(out.iter().all(|s| s.texture.is_none()));
        assert!(out.windows(2).all(|p| p[0].layer <= p[1].layer));
        assert_eq!(clear_color(&w, &Cache::default()), 0x0000_00ff);
    }

    /// Without its art an SMM1 costume is drawn as small Mario at his own size (16 × 16 on his
    /// quad), centred where the 24 × 24 costume would stand.
    #[test]
    fn costume_without_art_stands_in_as_small_mario() {
        let px = mmk_assets::Mtex {
            format: MtexFormat::Rgba8,
            variant: mmk_assets::MtexVariant::Unorm,
            width: 16,
            height: 16,
            channel_select: [2, 3, 4, 5],
            data: vec![0; 16 * 16 * 4],
        }
        .to_bytes();
        let files: std::collections::HashMap<String, Vec<u8>> = [
            ("/c/manifest.json".to_string(), b"{}".to_vec()),
            (
                "/c/textures/index.json".to_string(),
                br#"{"M1_Player_MarioMdl":{"wait.0":{"w":16,"h":16},"walk.1":{"w":16,"h":16}}}"#
                    .to_vec(),
            ),
            (
                "/c/textures/M1_Player_MarioMdl/wait.0.mtex".to_string(),
                px.clone(),
            ),
            ("/c/textures/M1_Player_MarioMdl/walk.1.mtex".to_string(), px),
            (
                "/c/sprites.json".to_string(),
                br#"{"version":1,"models":{"M1_Player_MarioMdl":{"quads":{
                    "wait.0":{"rect":[-8,0,8,16],"uv":[0,0,1,1]},
                    "walk.1":{"rect":[-8,0,8,16],"uv":[0,0,1,1]}}}}}"#
                    .to_vec(),
            ),
        ]
        .into();
        let read = |p: &std::path::Path| {
            files
                .get(p.to_str().unwrap())
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        };
        let cache = Cache::load("/c", &read).unwrap();
        let mut s = sprite(None, 88.0, 32.0, 24.0, 24.0);
        s.image.model = "SMM1_Costume_045";
        s.image.texture = "b_dash.1";
        let mut r = SpriteResolver::default();
        let d = r.resolve_one("M1", &s, &cache);
        assert_eq!(
            d.texture,
            cache
                .texture_index("M1_Player_MarioMdl", "walk.1")
                .map(|i| i as u32)
        );
        assert_eq!((d.x, d.y, d.w, d.h), (92.0, 32.0, 16.0, 16.0));
    }
}
