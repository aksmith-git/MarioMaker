//! Sprite placement and frame timing (`sprites.json`, written by the importer's sprites stage).
//!
//! ```text
//! { "version": 1, "models": { <model>: {
//!     "quads": { <texture>: { "rect": [x0, y0, x1, y1], "uv": [u0, v0, u1, v1] | null, ... } },
//!     "anims": { <anim>: { "frames": n, "loop": bool, "material": s, "keys": [[frame, texture], ...] } }
//! } } }
//! ```
//! `rect` is in model space (1 unit = 1 px, y up, origin = the actor's origin); `uv` gives the
//! texture coordinates at the top-left (u0, v0) and bottom-right (u1, v1) corners, (0, 0) being
//! the top-left of the texture.

use std::collections::HashMap;

use serde_json::Value;

/// Where a texture is drawn relative to its model's origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

impl Quad {
    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }
    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }
}

/// A texture-pattern animation: which texture is shown on which frame (60 Hz frames at play
/// rate 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Anim {
    pub frames: u32,
    pub looping: bool,
    /// (first frame, texture), sorted by frame.
    pub keys: Vec<(u32, String)>,
}

impl Anim {
    /// Texture shown `t` frames after the animation started (looping animations wrap, others
    /// hold their last frame).
    pub fn texture_at(&self, t: u32) -> Option<&str> {
        let t = match (self.looping, self.frames) {
            (true, n) if n > 0 => t % n,
            _ => t,
        };
        self.keys
            .iter()
            .take_while(|(f, _)| *f <= t)
            .last()
            .or(self.keys.first())
            .map(|(_, s)| s.as_str())
    }

    /// Frames each key is shown for, in key order (the closing key of a loop at frame == length
    /// is dropped).
    pub fn durations(&self) -> Vec<(String, u32)> {
        let keys: Vec<&(u32, String)> = self
            .keys
            .iter()
            .filter(|(f, _)| !(self.looping && *f >= self.frames && self.frames > 0))
            .collect();
        keys.iter()
            .enumerate()
            .map(|(i, (f, t))| {
                let end = keys
                    .get(i + 1)
                    .map(|k| k.0)
                    .unwrap_or(self.frames.max(*f + 1));
                (t.clone(), end - f)
            })
            .collect()
    }
}

#[derive(Debug, Default)]
pub(crate) struct Sprites {
    pub quads: HashMap<(String, String), Quad>,
    pub anims: HashMap<(String, String), Anim>,
}

fn f(v: &Value) -> Option<f32> {
    v.as_f64().map(|x| x as f32)
}

impl Sprites {
    pub fn from_json(v: &Value) -> Self {
        let mut out = Sprites::default();
        let Some(models) = v.get("models").and_then(Value::as_object) else {
            return out;
        };
        for (model, m) in models {
            for (tex, q) in m
                .get("quads")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                let rect = q.get("rect").and_then(Value::as_array);
                let uv = q.get("uv").and_then(Value::as_array);
                let (Some(r), Some(uv)) = (rect, uv) else {
                    continue; // tiled or not linear: no single quad
                };
                let r: Vec<f32> = r.iter().filter_map(f).collect();
                let uv: Vec<f32> = uv.iter().filter_map(f).collect();
                if let ([x0, y0, x1, y1], [u0, v0, u1, v1]) = (&r[..], &uv[..]) {
                    out.quads.insert(
                        (model.clone(), tex.clone()),
                        Quad {
                            x0: *x0,
                            y0: *y0,
                            x1: *x1,
                            y1: *y1,
                            u0: *u0,
                            v0: *v0,
                            u1: *u1,
                            v1: *v1,
                        },
                    );
                }
            }
            for (name, a) in m
                .get("anims")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                let keys = a
                    .get("keys")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|k| {
                        Some((k.get(0)?.as_u64()? as u32, k.get(1)?.as_str()?.to_string()))
                    })
                    .collect();
                out.anims.insert(
                    (model.clone(), name.clone()),
                    Anim {
                        frames: a.get("frames").and_then(Value::as_u64).unwrap_or(0) as u32,
                        looping: a.get("loop").and_then(Value::as_bool).unwrap_or(false),
                        keys,
                    },
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk() -> Anim {
        Anim {
            frames: 15,
            looping: true,
            keys: vec![
                (0, "walk.0".into()),
                (5, "walk.1".into()),
                (10, "walk.2".into()),
                (15, "walk.0".into()),
            ],
        }
    }

    #[test]
    fn anim_texture_at_wraps_and_holds() {
        let a = walk();
        assert_eq!(a.texture_at(0), Some("walk.0"));
        assert_eq!(a.texture_at(4), Some("walk.0"));
        assert_eq!(a.texture_at(5), Some("walk.1"));
        assert_eq!(a.texture_at(14), Some("walk.2"));
        assert_eq!(a.texture_at(15), Some("walk.0"));
        assert_eq!(a.texture_at(20), Some("walk.1"));
        let once = Anim {
            frames: 16,
            looping: false,
            keys: vec![(0, "r.0".into()), (8, "r.1".into()), (16, "r.2".into())],
        };
        assert_eq!(once.texture_at(100), Some("r.2"));
    }

    #[test]
    fn anim_durations() {
        assert_eq!(
            walk().durations(),
            vec![
                ("walk.0".into(), 5),
                ("walk.1".into(), 5),
                ("walk.2".into(), 5)
            ]
        );
    }

    #[test]
    fn parses_json_and_skips_quads_without_uv() {
        let v: Value = serde_json::from_str(
            r#"{"version":1,"models":{"M":{"quads":{
                "a":{"rect":[-8,-16,8,16],"uv":[0,0,1,1]},
                "b":{"rect":[0,0,1,1],"uv":null}},
              "anims":{"walk_ftp":{"frames":20,"loop":true,"keys":[[0,"a"],[10,"b"]]}}}}}"#,
        )
        .unwrap();
        let s = Sprites::from_json(&v);
        let q = s.quads[&("M".into(), "a".into())];
        assert_eq!(
            (q.x0, q.y0, q.width(), q.height()),
            (-8.0, -16.0, 16.0, 32.0)
        );
        assert!(!s.quads.contains_key(&("M".into(), "b".into())));
        let a = &s.anims[&("M".into(), "walk_ftp".into())];
        assert_eq!(a.texture_at(12), Some("b"));
    }
}
