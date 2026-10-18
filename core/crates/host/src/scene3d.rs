//! The 3D styles' scene setup, shared by the portable shell and the Swift app: the per-theme
//! row of the style's SceneDB, the two cameras the game draws a course with (the play field and
//! the background scene, "DV"), the background model, and the environment (lights and fogs).
//!
//! Matrices are column-major 4×4 (`m[col * 4 + row]`), from world pixels (y up, the field at
//! z = 0) to clip space with depth 0 (near) … 1 (far), as WGSL and Metal expect.
//!
//! What the game does (research/specs/render-3d.md, v1.0.0 addresses):
//! - Play field (layer-camera update in FUN_7100aced20): an orthographic camera over the 384×216
//!   view for every style except 3D World, which uses a perspective camera of fovy
//!   0.4° + 12.1°·t whose distance 260.735·tan 22.5° / tan(fovy/2) keeps the z = 0 plane exactly
//!   216 px tall, raised 35·t px and looking at a point 5·t px above the view centre; t = 1 while
//!   playing. Near / far: distance − 259.735 / distance + 739.265.
//! - Background (same function, style 3 / 4 branch): perspective of fovy `DV_Play_Fovy`, near
//!   100, far 80000, eye at (view centre x, view centre y + `DV_CamMoveY` · view bottom, 1000)
//!   looking down −z; frustum offset y `DV_ProjOffsetY` + (`DV_ProjMoveY` − `DV_CamMoveY`) ·
//!   view bottom / 1080. The offset is in units of the frustum size (sead perspective matrix,
//!   FUN_71014e1560).

use std::path::Path;

use mmk_assets::Cache;
use serde_json::Value;

pub type Mat4 = [f32; 16];

/// View size in world pixels.
pub const VIEW: (f32, f32) = (384.0, 216.0);

/// One theme's row of `tables/<style>_SceneDB.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneRow {
    /// Background model file (`3W_DV_plain`, `WU_DV_plain_D`).
    pub dv_model: String,
    /// Environment of the background (`env/<name>.json`: `dv_plain3w`, `dv_plain_d`).
    pub env_dv: String,
    /// Environment of the play field (`plain3w`, `castle_n`).
    pub env_model: String,
    /// Background field of view (degrees).
    pub fovy: f32,
    pub proj_offset_y: f32,
    pub proj_move_y: f32,
    pub cam_move_y: f32,
    /// NSMBU edge-light colour (rim), RGB.
    pub edge_light: Option<[f32; 3]>,
}

/// `3W_plain` → `plain3w`, `DV_plain_D` → `dv_plain_d`: the cache's env file names.
pub fn env_file(name: &str) -> String {
    match name.strip_prefix("3W_") {
        Some(rest) => format!("{}3w", rest.to_ascii_lowercase()),
        None => name.to_ascii_lowercase(),
    }
}

/// The SceneDB row of a theme (rows 0–9 by theme; NSMBU's night rows follow at 10–19).
pub fn scene_row(
    cache: &Cache,
    style: &str,
    theme: u8,
    night: bool,
    vertical: bool,
) -> Option<SceneRow> {
    let table = cache.table(&format!("{style}_SceneDB")).ok()?;
    let rows = table.as_array()?;
    let index = theme as usize + if night && rows.len() >= 20 { 10 } else { 0 };
    let r = rows.get(index)?;
    let f = |k: &str| r.get(k).and_then(Value::as_f64).map(|v| v as f32);
    let s = |k: &str| r.get(k).and_then(Value::as_str).map(String::from);
    let v = if vertical { "DV_V_" } else { "DV_" };
    let mut dv_model = s("DVModel")?;
    if vertical {
        dv_model.push_str("_V");
    }
    Some(SceneRow {
        dv_model,
        env_dv: env_file(&s(if vertical { "Env_DV_V" } else { "Env_DV" })?),
        env_model: env_file(&s("Env_Model")?),
        fovy: f("DV_Play_Fovy").unwrap_or(20.0),
        proj_offset_y: f(&format!("{v}ProjOffsetY")).unwrap_or(0.0),
        proj_move_y: f(&format!("{v}ProjMoveY")).unwrap_or(1.0),
        cam_move_y: f(&format!("{v}CamMoveY")).unwrap_or(0.0),
        edge_light: match (
            f("EdgeLightColor_R"),
            f("EdgeLightColor_G"),
            f("EdgeLightColor_B"),
        ) {
            (Some(r), Some(g), Some(b)) => Some([r, g, b]),
            _ => None,
        },
    })
}

// --- environment ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DirLight {
    /// Direction the light travels (normalised).
    pub dir: [f32; 3],
    /// Colours times intensity.
    pub diffuse: [f32; 3],
    pub backside: [f32; 3],
    pub specular: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hemi {
    pub sky: [f32; 3],
    pub ground: [f32; 3],
    pub dir: [f32; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fog {
    /// Height fog (along world y) rather than distance fog (along the view).
    pub height: bool,
    pub start: f32,
    pub end: f32,
    pub damp: f32,
    /// RGB and the fog's strength at full density.
    pub color: [f32; 4],
    pub dir: [f32; 3],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Env {
    pub dirs: Vec<DirLight>,
    pub hemi: Option<Hemi>,
    pub ambient: [f32; 3],
    pub fogs: Vec<Fog>,
    /// The sky material's six constant colours (background scenes with a sky).
    pub sky: Option<[[f32; 4]; 6]>,
}

fn v3(v: &Value) -> [f32; 3] {
    let a = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    [a(0), a(1), a(2)]
}

fn scaled(v: &Value, k: f32) -> [f32; 3] {
    v3(v).map(|x| x * k)
}

fn normalised(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-6 {
        v.map(|x| x / l)
    } else {
        [0.0, -1.0, 0.0]
    }
}

/// `env/<name>.json` of the cache.
pub fn load_env(root: &Path, name: &str) -> Option<Env> {
    let text = std::fs::read(root.join(format!("env/{name}.json"))).ok()?;
    let j: Value = serde_json::from_slice(&text).ok()?;
    let list = |k: &str| {
        j.get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|o| o.get("enable").and_then(Value::as_bool).unwrap_or(true))
            .collect::<Vec<_>>()
    };
    let num = |o: &Value, k: &str, d: f32| o.get(k).and_then(Value::as_f64).map_or(d, |v| v as f32);
    let dirs = list("directional")
        .iter()
        .map(|o| {
            let i = num(o, "intensity", 1.0);
            DirLight {
                dir: normalised(v3(&o["direction"])),
                diffuse: scaled(&o["diffuse"], i),
                backside: scaled(&o["backside"], i),
                specular: scaled(&o["specular"], i),
            }
        })
        .collect();
    let hemi = list("hemisphere").first().map(|o| {
        let i = num(o, "intensity", 1.0);
        Hemi {
            sky: scaled(&o["sky"], i),
            ground: scaled(&o["ground"], i),
            dir: normalised(v3(&o["direction"])),
        }
    });
    let ambient = list("ambient").iter().fold([0.0; 3], |a, o| {
        let c = scaled(&o["color"], num(o, "intensity", 1.0));
        [a[0] + c[0], a[1] + c[1], a[2] + c[2]]
    });
    let fogs = list("fog")
        .iter()
        .map(|o| {
            let dir = normalised(v3(&o["direction"]));
            let c = &o["color"];
            Fog {
                height: dir[1].abs() > 0.5,
                start: num(o, "start", 0.0),
                end: num(o, "end", 1.0),
                damp: num(o, "damp", 1.0),
                color: [
                    v3(c)[0],
                    v3(c)[1],
                    v3(c)[2],
                    c.get(3).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                ],
                dir,
            }
        })
        .collect();
    Some(Env {
        dirs,
        hemi,
        ambient,
        fogs,
        sky: None,
    })
}

/// Rows of 4 floats in [`Env::uniform`].
pub const LIGHT_ROWS: usize = 30;

/// Scale of the lit colour (row 23) [inf: calibrated against reference frames].
pub const EXPOSURE: f32 = 1.0;

impl Env {
    /// The environment as the shaders' uniform block (`LIGHT_ROWS` × vec4):
    /// 0 eye xyz, w = 1 if lit at all; 1–9 three directional lights (direction xyz, –;
    /// diffuse rgb, –; backside rgb, –); 10 hemisphere sky rgb, w = 1 if present; 11 ground;
    /// 12 hemisphere direction; 13 ambient; 14–22 three fogs — the distance fog, the second
    /// distance fog, the height fog — (start, end, damp, 1 = height fog; colour rgba; direction
    /// xyz, w = 1 if enabled); 23 exposure; 24–29 the sky's constant colours (w = 1 if set).
    pub fn uniform(&self, eye: [f32; 3]) -> [[f32; 4]; LIGHT_ROWS] {
        let mut u = [[0.0f32; 4]; LIGHT_ROWS];
        u[0] = [eye[0], eye[1], eye[2], 1.0];
        for (i, d) in self.dirs.iter().take(3).enumerate() {
            u[1 + 3 * i] = [d.dir[0], d.dir[1], d.dir[2], 1.0];
            u[2 + 3 * i] = [d.diffuse[0], d.diffuse[1], d.diffuse[2], 0.0];
            u[3 + 3 * i] = [d.backside[0], d.backside[1], d.backside[2], 0.0];
        }
        if let Some(h) = &self.hemi {
            u[10] = [h.sky[0], h.sky[1], h.sky[2], 1.0];
            u[11] = [h.ground[0], h.ground[1], h.ground[2], 0.0];
            u[12] = [h.dir[0], h.dir[1], h.dir[2], 0.0];
        }
        u[13] = [self.ambient[0], self.ambient[1], self.ambient[2], 0.0];
        if self.dirs.is_empty() && self.hemi.is_none() && self.ambient == [0.0; 3] {
            // no environment: plain white light
            u[13] = [1.0, 1.0, 1.0, 0.0];
        }
        u[23] = [EXPOSURE, 0.0, 0.0, 0.0];
        if let Some(sky) = &self.sky {
            for (i, c) in sky.iter().enumerate() {
                u[24 + i] = [c[0], c[1], c[2], 1.0];
            }
        }
        // fog slots as the materials switch them: 0 the distance fog (`enable_fog`), 1 the second
        // distance fog (`enable_fog1`), 2 the height fog (`enable_fog_y`)
        let mut slots: [Option<&Fog>; 3] = [None; 3];
        for f in &self.fogs {
            let slot = if f.height {
                2
            } else if slots[0].is_none() {
                0
            } else {
                1
            };
            if slots[slot].is_none() {
                slots[slot] = Some(f);
            }
        }
        for (i, f) in slots.iter().enumerate() {
            let Some(f) = f else { continue };
            u[14 + 3 * i] = [f.start, f.end, f.damp, f.height as u32 as f32];
            u[15 + 3 * i] = f.color;
            u[16 + 3 * i] = [f.dir[0], f.dir[1], f.dir[2], 1.0];
        }
        u
    }
}

// --- cameras ---------------------------------------------------------------------------------

fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [0.0f32; 16];
    for c in 0..4 {
        for r in 0..4 {
            m[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    m
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// View matrix (right-handed, looking down −z in view space).
pub fn look_at(eye: [f32; 3], at: [f32; 3], up: [f32; 3]) -> Mat4 {
    let f = normalised(sub(at, eye));
    let s = normalised(cross(f, up));
    let u = cross(s, f);
    [
        s[0],
        u[0],
        -f[0],
        0.0, //
        s[1],
        u[1],
        -f[1],
        0.0, //
        s[2],
        u[2],
        -f[2],
        0.0, //
        -dot(s, eye),
        -dot(u, eye),
        dot(f, eye),
        1.0,
    ]
}

/// Perspective projection as the game builds it (fovy radians; `offset` moves the frustum by
/// that fraction of its width / height), depth 0 … 1.
pub fn perspective(fovy: f32, aspect: f32, near: f32, far: f32, offset: [f32; 2]) -> Mat4 {
    let h = 2.0 * near * (fovy * 0.5).tan();
    let w = h * aspect;
    let (cx, cy) = (w * offset[0], h * offset[1]);
    let (l, r, b, t) = (cx - w * 0.5, cx + w * 0.5, cy - h * 0.5, cy + h * 0.5);
    let mut m = [0.0f32; 16];
    m[0] = 2.0 * near / (r - l);
    m[5] = 2.0 * near / (t - b);
    m[8] = (r + l) / (r - l);
    m[9] = (t + b) / (t - b);
    m[10] = far / (near - far);
    m[11] = -1.0;
    m[14] = near * far / (near - far);
    m
}

/// Orthographic projection, depth 0 … 1 over z = −near … −far in view space.
pub fn ortho(l: f32, r: f32, b: f32, t: f32, near: f32, far: f32) -> Mat4 {
    let mut m = [0.0f32; 16];
    m[0] = 2.0 / (r - l);
    m[5] = 2.0 / (t - b);
    m[10] = 1.0 / (near - far);
    m[12] = -(r + l) / (r - l);
    m[13] = -(t + b) / (t - b);
    m[14] = near / (near - far);
    m[15] = 1.0;
    m
}

/// A camera: world → clip, and where it is (for lighting and fog).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub view_proj: Mat4,
    pub eye: [f32; 3],
    /// Perspective (for view-depth fog and specular); false for the orthographic field.
    pub perspective: bool,
}

/// 3D World's play-field perspective in play mode (t = 1).
pub const W3_FOVY_DEG: f32 = 12.5;
pub const W3_RAISE: f32 = 35.0;
pub const W3_LOOK_RAISE: f32 = 5.0;

/// The play-field camera for a view whose bottom-left corner is `cam` (world px).
pub fn field_camera(style: &str, cam: (f32, f32)) -> Camera {
    let (cx, cy) = (cam.0 + VIEW.0 * 0.5, cam.1 + VIEW.1 * 0.5);
    if style == "3W" {
        let fovy = W3_FOVY_DEG.to_radians();
        let dist = 260.735 * 22.5f32.to_radians().tan() / (fovy * 0.5).tan();
        let eye = [cx, cy + W3_RAISE, dist];
        let view = look_at(eye, [cx, cy + W3_LOOK_RAISE, -1.0], [0.0, 1.0, 0.0]);
        let proj = perspective(
            fovy,
            VIEW.0 / VIEW.1,
            dist - 260.735 + 1.0,
            dist - 260.735 + 1000.0,
            [0.0, 0.0],
        );
        Camera {
            view_proj: mul(&proj, &view),
            eye,
            perspective: true,
        }
    } else {
        // z = +500 (near, towards the viewer) … −500 (far)
        let eye = [cx, cy, 500.0];
        let view = look_at(eye, [cx, cy, 0.0], [0.0, 1.0, 0.0]);
        let proj = ortho(
            -VIEW.0 * 0.5,
            VIEW.0 * 0.5,
            -VIEW.1 * 0.5,
            VIEW.1 * 0.5,
            0.0,
            1000.0,
        );
        Camera {
            view_proj: mul(&proj, &view),
            eye,
            perspective: false,
        }
    }
}

/// The background camera of a theme for a view whose bottom-left corner is `cam`.
pub fn background_camera(row: &SceneRow, cam: (f32, f32)) -> Camera {
    let (cx, cy) = (cam.0 + VIEW.0 * 0.5, cam.1 + VIEW.1 * 0.5);
    let eye = [cx, cy + row.cam_move_y * cam.1, 1000.0];
    let view = look_at(eye, [eye[0], eye[1], eye[2] - 1.0], [0.0, 1.0, 0.0]);
    let offset_y = row.proj_offset_y + (row.proj_move_y - row.cam_move_y) * cam.1 / 1080.0;
    let proj = perspective(
        row.fovy.to_radians(),
        VIEW.0 / VIEW.1,
        100.0,
        80000.0,
        [0.0, offset_y],
    );
    Camera {
        view_proj: mul(&proj, &view),
        eye,
        perspective: true,
    }
}

/// Clip-space position of a world point.
pub fn project(m: &Mat4, p: [f32; 3]) -> [f32; 4] {
    [0, 1, 2, 3].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ndc(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
        let c = project(m, p);
        [c[0] / c[3], c[1] / c[3], c[2] / c[3]]
    }

    #[test]
    fn field_plane_fills_the_view() {
        // orthographic: the view's corners land on the NDC corners at any depth
        let c = field_camera("WU", (100.0, 50.0));
        let a = ndc(&c.view_proj, [100.0, 50.0, 0.0]);
        let b = ndc(&c.view_proj, [484.0, 266.0, 0.0]);
        assert!(
            (a[0] + 1.0).abs() < 1e-4 && (a[1] + 1.0).abs() < 1e-4,
            "{a:?}"
        );
        assert!(
            (b[0] - 1.0).abs() < 1e-4 && (b[1] - 1.0).abs() < 1e-4,
            "{b:?}"
        );
        assert!(a[2] > 0.0 && a[2] < 1.0);
        // 3D World: the z = 0 plane is (nearly) the view — the camera tilts by ~1.7°
        let c = field_camera("3W", (0.0, 0.0));
        let l = ndc(&c.view_proj, [0.0, 108.0, 0.0]);
        let r = ndc(&c.view_proj, [384.0, 108.0, 0.0]);
        assert!(
            (l[0] + 1.0).abs() < 0.02 && (r[0] - 1.0).abs() < 0.02,
            "{l:?} {r:?}"
        );
        // the front face of a block (z = +8) is a little bigger than the plane
        let near = ndc(&c.view_proj, [384.0, 108.0, 8.0]);
        assert!(near[0] > r[0]);
        assert!((0.0..1.0).contains(&r[2]));
    }

    #[test]
    fn background_offset_moves_the_scene() {
        let row = SceneRow {
            dv_model: String::new(),
            env_dv: String::new(),
            env_model: String::new(),
            fovy: 15.0,
            proj_offset_y: -0.25,
            proj_move_y: 0.0,
            cam_move_y: 0.0,
            edge_light: None,
        };
        let c = background_camera(&row, (0.0, 0.0));
        // a point straight ahead appears a quarter of the view height above the centre... in
        // NDC: frustum moved down by 0.25 h → the point rises by 0.5
        let p = ndc(&c.view_proj, [192.0, 108.0, -2000.0]);
        assert!((p[1] - 0.5).abs() < 1e-3, "{p:?}");
        assert!((0.0..1.0).contains(&p[2]));
        assert_eq!(env_file("3W_DV_plain"), "dv_plain3w");
        assert_eq!(env_file("DV_plain_D"), "dv_plain_d");
        assert_eq!(env_file("castle_n"), "castle_n");
    }
}
