//! JSON form of layouts and animations, as stored in the engine's asset cache.
//!
//! `to_json` and `from_json` are exact inverses for every field the parser keeps (checked by the
//! round-trip tests), so the cache can be read without the original files.

use serde_json::{Map, Value, json};

use super::bflan::{AnimContent, AnimEntry, AnimKey, AnimTag, Animation, CurveType};
use super::bflyt::*;
use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "layout JSON",
        detail: detail.into(),
    }
}

// --- small readers -------------------------------------------------------------------------

fn get<'a>(v: &'a Value, k: &str) -> Result<&'a Value> {
    v.get(k).ok_or_else(|| bad(format!("missing '{k}'")))
}
fn f(v: &Value) -> Result<f32> {
    v.as_f64()
        .map(|x| x as f32)
        .ok_or_else(|| bad("expected number"))
}
fn u(v: &Value) -> Result<u64> {
    v.as_u64().ok_or_else(|| bad("expected unsigned integer"))
}
fn i(v: &Value) -> Result<i64> {
    v.as_i64().ok_or_else(|| bad("expected integer"))
}
fn s(v: &Value) -> Result<String> {
    v.as_str()
        .map(str::to_string)
        .ok_or_else(|| bad("expected string"))
}
fn arr(v: &Value) -> Result<&Vec<Value>> {
    v.as_array().ok_or_else(|| bad("expected array"))
}
fn gf(v: &Value, k: &str) -> Result<f32> {
    f(get(v, k)?)
}
fn gu8(v: &Value, k: &str) -> Result<u8> {
    Ok(u(get(v, k)?)? as u8)
}
fn gu16(v: &Value, k: &str) -> Result<u16> {
    Ok(u(get(v, k)?)? as u16)
}
fn gs(v: &Value, k: &str) -> Result<String> {
    s(get(v, k)?)
}
fn fixed_f<const N: usize>(v: &Value) -> Result<[f32; N]> {
    let a = arr(v)?;
    if a.len() != N {
        return Err(bad(format!("expected {N} numbers")));
    }
    let mut out = [0f32; N];
    for (o, x) in out.iter_mut().zip(a) {
        *o = f(x)?;
    }
    Ok(out)
}
fn gfa<const N: usize>(v: &Value, k: &str) -> Result<[f32; N]> {
    fixed_f(get(v, k)?)
}
fn rgba(v: &Value) -> Result<[u8; 4]> {
    let a = arr(v)?;
    if a.len() != 4 {
        return Err(bad("expected 4 colour channels"));
    }
    let mut out = [0u8; 4];
    for (o, x) in out.iter_mut().zip(a) {
        *o = u(x)? as u8;
    }
    Ok(out)
}
fn grgba(v: &Value, k: &str) -> Result<[u8; 4]> {
    rgba(get(v, k)?)
}
fn opt<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
    v.get(k).filter(|x| !x.is_null())
}
fn strings(v: &Value) -> Result<Vec<String>> {
    arr(v)?.iter().map(s).collect()
}

// --- layout --------------------------------------------------------------------------------

pub fn layout_to_json(l: &Layout) -> Value {
    json!({
        "name": l.name,
        "version": l.version,
        "origin_type": l.origin_type,
        "width": l.width,
        "height": l.height,
        "textures": l.textures,
        "fonts": l.fonts,
        "materials": l.materials.iter().map(material_to_json).collect::<Vec<_>>(),
        "root": pane_to_json(&l.root),
        "groups": l.groups.iter().map(group_to_json).collect::<Vec<_>>(),
        "user_data": user_data_to_json(&l.user_data),
    })
}

pub fn layout_from_json(v: &Value) -> Result<Layout> {
    Ok(Layout {
        name: gs(v, "name")?,
        version: u(get(v, "version")?)? as u32,
        origin_type: gu8(v, "origin_type")?,
        width: gf(v, "width")?,
        height: gf(v, "height")?,
        textures: strings(get(v, "textures")?)?,
        fonts: strings(get(v, "fonts")?)?,
        materials: arr(get(v, "materials")?)?
            .iter()
            .map(material_from_json)
            .collect::<Result<_>>()?,
        root: pane_from_json(get(v, "root")?)?,
        groups: arr(get(v, "groups")?)?
            .iter()
            .map(group_from_json)
            .collect::<Result<_>>()?,
        user_data: user_data_from_json(get(v, "user_data")?)?,
    })
}

fn blend_json(b: &Option<BlendMode>) -> Value {
    match b {
        Some(b) => json!([b.op, b.src, b.dst, b.logic]),
        None => Value::Null,
    }
}
fn blend_from(v: Option<&Value>) -> Result<Option<BlendMode>> {
    v.map(|v| {
        let a = arr(v)?;
        let g = |i: usize| a.get(i).map(u).transpose().map(|x| x.unwrap_or(0) as u8);
        Ok(BlendMode {
            op: g(0)?,
            src: g(1)?,
            dst: g(2)?,
            logic: g(3)?,
        })
    })
    .transpose()
}

fn material_to_json(m: &Material) -> Value {
    json!({
        "name": m.name,
        "black": m.black,
        "white": m.white,
        "tex_maps": m.tex_maps.iter().map(|t| json!({
            "texture": t.texture, "wrap": [t.wrap_s, t.wrap_t], "filter": [t.min_filter, t.mag_filter],
        })).collect::<Vec<_>>(),
        "tex_srts": m.tex_srts.iter().map(|t| json!({ "t": t.translate, "r": t.rotate, "s": t.scale })).collect::<Vec<_>>(),
        "tex_coord_gens": m.tex_coord_gens.iter().map(|g| json!([g.matrix, g.source])).collect::<Vec<_>>(),
        "tev_stages": m.tev_stages.iter().map(|t| json!([t.color_mode, t.alpha_mode])).collect::<Vec<_>>(),
        "alpha_compare": m.alpha_compare.map(|(func, r)| json!([func, r])),
        "blend": blend_json(&m.blend),
        "blend_logic": blend_json(&m.blend_logic),
        "texture_only": m.texture_only,
        "proj_tex_gens": m.proj_tex_gens.iter().map(|p| json!({ "t": p.translate, "s": p.scale, "flags": p.flags })).collect::<Vec<_>>(),
        "font_shadow": m.font_shadow.map(|(b, w)| json!([b, w])),
        "flags": m.raw_flags,
    })
}

fn material_from_json(v: &Value) -> Result<Material> {
    let pair = |x: &Value| -> Result<(u8, u8)> {
        let a = arr(x)?;
        Ok((
            u(a.first().ok_or_else(|| bad("pair"))?)? as u8,
            u(a.get(1).ok_or_else(|| bad("pair"))?)? as u8,
        ))
    };
    Ok(Material {
        name: gs(v, "name")?,
        black: grgba(v, "black")?,
        white: grgba(v, "white")?,
        tex_maps: arr(get(v, "tex_maps")?)?
            .iter()
            .map(|t| {
                let (ws, wt) = pair(get(t, "wrap")?)?;
                let (mi, ma) = pair(get(t, "filter")?)?;
                Ok(TexMap {
                    texture: gu16(t, "texture")?,
                    wrap_s: ws,
                    wrap_t: wt,
                    min_filter: mi,
                    mag_filter: ma,
                })
            })
            .collect::<Result<_>>()?,
        tex_srts: arr(get(v, "tex_srts")?)?
            .iter()
            .map(|t| {
                Ok(TexSrt {
                    translate: gfa(t, "t")?,
                    rotate: gf(t, "r")?,
                    scale: gfa(t, "s")?,
                })
            })
            .collect::<Result<_>>()?,
        tex_coord_gens: arr(get(v, "tex_coord_gens")?)?
            .iter()
            .map(|g| pair(g).map(|(matrix, source)| TexCoordGen { matrix, source }))
            .collect::<Result<_>>()?,
        tev_stages: arr(get(v, "tev_stages")?)?
            .iter()
            .map(|g| {
                pair(g).map(|(color_mode, alpha_mode)| TevStage {
                    color_mode,
                    alpha_mode,
                })
            })
            .collect::<Result<_>>()?,
        alpha_compare: opt(v, "alpha_compare")
            .map(|a| -> Result<(u8, f32)> {
                let a = arr(a)?;
                Ok((
                    u(a.first().ok_or_else(|| bad("alpha_compare"))?)? as u8,
                    f(a.get(1).ok_or_else(|| bad("alpha_compare"))?)?,
                ))
            })
            .transpose()?,
        blend: blend_from(opt(v, "blend"))?,
        blend_logic: blend_from(opt(v, "blend_logic"))?,
        texture_only: get(v, "texture_only")?.as_bool().unwrap_or(false),
        proj_tex_gens: arr(get(v, "proj_tex_gens")?)?
            .iter()
            .map(|p| {
                Ok(ProjTexGen {
                    translate: gfa(p, "t")?,
                    scale: gfa(p, "s")?,
                    flags: gu8(p, "flags")?,
                })
            })
            .collect::<Result<_>>()?,
        font_shadow: opt(v, "font_shadow")
            .map(|fs| -> Result<_> {
                let a = arr(fs)?;
                Ok((
                    rgba(a.first().ok_or_else(|| bad("font_shadow"))?)?,
                    rgba(a.get(1).ok_or_else(|| bad("font_shadow"))?)?,
                ))
            })
            .transpose()?,
        raw_flags: u(get(v, "flags")?)? as u32,
    })
}

fn picture_to_json(p: &Picture) -> Value {
    json!({
        "colors": p.vertex_colors,
        "material": p.material,
        "uv": p.tex_coords,
    })
}

fn picture_from_json(v: &Value) -> Result<Picture> {
    let colors = arr(get(v, "colors")?)?;
    let mut vertex_colors = [[0u8; 4]; 4];
    for (o, c) in vertex_colors.iter_mut().zip(colors) {
        *o = rgba(c)?;
    }
    let tex_coords = arr(get(v, "uv")?)?
        .iter()
        .map(|set| {
            let set = arr(set)?;
            let mut out = [[0f32; 2]; 4];
            for (o, uv) in out.iter_mut().zip(set) {
                *o = fixed_f(uv)?;
            }
            Ok(out)
        })
        .collect::<Result<_>>()?;
    Ok(Picture {
        vertex_colors,
        material: gu16(v, "material")?,
        tex_coords,
    })
}

fn user_data_to_json(ud: &[UserData]) -> Value {
    Value::Array(
        ud.iter()
            .map(|u| {
                let value = match &u.value {
                    UserDataValue::String(s) => json!({ "string": s }),
                    UserDataValue::Ints(v) => json!({ "ints": v }),
                    UserDataValue::Floats(v) => json!({ "floats": v }),
                    UserDataValue::Other(k) => json!({ "other": k }),
                };
                json!({ "name": u.name, "value": value })
            })
            .collect(),
    )
}

fn user_data_from_json(v: &Value) -> Result<Vec<UserData>> {
    arr(v)?
        .iter()
        .map(|e| {
            let val = get(e, "value")?;
            let value = if let Some(s) = val.get("string") {
                UserDataValue::String(self::s(s)?)
            } else if let Some(a) = val.get("ints") {
                UserDataValue::Ints(
                    arr(a)?
                        .iter()
                        .map(|x| i(x).map(|n| n as i32))
                        .collect::<Result<_>>()?,
                )
            } else if let Some(a) = val.get("floats") {
                UserDataValue::Floats(arr(a)?.iter().map(f).collect::<Result<_>>()?)
            } else {
                UserDataValue::Other(gu8(val, "other")?)
            };
            Ok(UserData {
                name: gs(e, "name")?,
                value,
            })
        })
        .collect()
}

pub fn pane_to_json(p: &Pane) -> Value {
    let mut m = Map::new();
    m.insert("kind".into(), json!(p.kind.tag()));
    m.insert("name".into(), json!(p.name));
    m.insert("user_info".into(), json!(p.user_info));
    m.insert("flags".into(), json!(p.flags));
    m.insert("origin".into(), json!(p.origin));
    m.insert("alpha".into(), json!(p.alpha));
    m.insert("mag_flags".into(), json!(p.mag_flags));
    m.insert("t".into(), json!(p.translate));
    m.insert("r".into(), json!(p.rotate));
    m.insert("s".into(), json!(p.scale));
    m.insert("size".into(), json!(p.size));
    if !p.user_data.is_empty() {
        m.insert("user_data".into(), user_data_to_json(&p.user_data));
    }
    match &p.kind {
        PaneKind::Picture(pic) => {
            m.insert("picture".into(), picture_to_json(pic));
        }
        PaneKind::Text(t) => {
            m.insert(
                "text".into(),
                json!({
                    "max_bytes": t.max_bytes, "material": t.material, "font": t.font,
                    "position": t.position, "line_alignment": t.line_alignment, "flags": t.flags,
                    "italic": t.italic, "top_color": t.top_color, "bottom_color": t.bottom_color,
                    "font_size": t.font_size, "char_space": t.char_space, "line_space": t.line_space,
                    "text_id": t.text_id, "text": t.text,
                    "shadow_offset": t.shadow_offset, "shadow_scale": t.shadow_scale,
                    "shadow_top": t.shadow_top, "shadow_bottom": t.shadow_bottom,
                    "shadow_italic": t.shadow_italic,
                }),
            );
        }
        PaneKind::Window(w) => {
            m.insert(
                "window".into(),
                json!({
                    "inflation": w.inflation, "frame_size": w.frame_size, "flags": w.flags,
                    "content": picture_to_json(&w.content),
                    "frames": w.frames.iter().map(|f| json!([f.material, f.flip])).collect::<Vec<_>>(),
                }),
            );
        }
        PaneKind::Part(pt) => {
            m.insert(
                "part".into(),
                json!({
                    "layout": pt.layout, "magnify": pt.magnify,
                    "properties": pt.properties.iter().map(|p| json!({
                        "name": p.name, "usage": p.usage, "basic_usage": p.basic_usage,
                        "material_usage": p.material_usage,
                        "pane": p.pane.as_ref().map(|x| pane_to_json(x)),
                        "basic": p.basic.as_ref().map(|b| json!({
                            "user_info": b.user_info, "t": b.translate, "r": b.rotate,
                            "s": b.scale, "size": b.size, "alpha": b.alpha,
                        })),
                    })).collect::<Vec<_>>(),
                }),
            );
        }
        PaneKind::Null | PaneKind::Bounding => {}
    }
    if !p.children.is_empty() {
        m.insert(
            "children".into(),
            Value::Array(p.children.iter().map(pane_to_json).collect()),
        );
    }
    Value::Object(m)
}

pub fn pane_from_json(v: &Value) -> Result<Pane> {
    let kind = match gs(v, "kind")?.as_str() {
        "pic1" => PaneKind::Picture(picture_from_json(get(v, "picture")?)?),
        "txt1" => {
            let t = get(v, "text")?;
            PaneKind::Text(Text {
                max_bytes: gu16(t, "max_bytes")?,
                material: gu16(t, "material")?,
                font: gu16(t, "font")?,
                position: gu8(t, "position")?,
                line_alignment: gu8(t, "line_alignment")?,
                flags: gu8(t, "flags")?,
                italic: gf(t, "italic")?,
                top_color: grgba(t, "top_color")?,
                bottom_color: grgba(t, "bottom_color")?,
                font_size: gfa(t, "font_size")?,
                char_space: gf(t, "char_space")?,
                line_space: gf(t, "line_space")?,
                text_id: gs(t, "text_id")?,
                text: gs(t, "text")?,
                shadow_offset: gfa(t, "shadow_offset")?,
                shadow_scale: gfa(t, "shadow_scale")?,
                shadow_top: grgba(t, "shadow_top")?,
                shadow_bottom: grgba(t, "shadow_bottom")?,
                shadow_italic: gf(t, "shadow_italic")?,
            })
        }
        "wnd1" => {
            let w = get(v, "window")?;
            let mut inflation = [0i16; 4];
            for (o, x) in inflation.iter_mut().zip(arr(get(w, "inflation")?)?) {
                *o = i(x)? as i16;
            }
            let mut frame_size = [0u16; 4];
            for (o, x) in frame_size.iter_mut().zip(arr(get(w, "frame_size")?)?) {
                *o = u(x)? as u16;
            }
            PaneKind::Window(Window {
                inflation,
                frame_size,
                flags: gu8(w, "flags")?,
                content: picture_from_json(get(w, "content")?)?,
                frames: arr(get(w, "frames")?)?
                    .iter()
                    .map(|fr| {
                        let a = arr(fr)?;
                        Ok(WindowFrame {
                            material: u(a.first().ok_or_else(|| bad("frame"))?)? as u16,
                            flip: u(a.get(1).ok_or_else(|| bad("frame"))?)? as u8,
                        })
                    })
                    .collect::<Result<_>>()?,
            })
        }
        "prt1" => {
            let p = get(v, "part")?;
            PaneKind::Part(Part {
                layout: gs(p, "layout")?,
                magnify: gfa(p, "magnify")?,
                properties: arr(get(p, "properties")?)?
                    .iter()
                    .map(|e| {
                        Ok(PartProperty {
                            name: gs(e, "name")?,
                            usage: gu8(e, "usage")?,
                            basic_usage: gu8(e, "basic_usage")?,
                            material_usage: gu8(e, "material_usage")?,
                            pane: opt(e, "pane")
                                .map(|x| pane_from_json(x).map(Box::new))
                                .transpose()?,
                            // absent in imports made before it was parsed
                            basic: opt(e, "basic")
                                .map(|b| -> Result<PartBasicInfo> {
                                    Ok(PartBasicInfo {
                                        user_info: gs(b, "user_info")?,
                                        translate: gfa(b, "t")?,
                                        rotate: gfa(b, "r")?,
                                        scale: gfa(b, "s")?,
                                        size: gfa(b, "size")?,
                                        alpha: gu8(b, "alpha")?,
                                    })
                                })
                                .transpose()?,
                        })
                    })
                    .collect::<Result<_>>()?,
            })
        }
        "bnd1" => PaneKind::Bounding,
        _ => PaneKind::Null,
    };
    Ok(Pane {
        kind,
        name: gs(v, "name")?,
        user_info: gs(v, "user_info")?,
        flags: gu8(v, "flags")?,
        origin: gu8(v, "origin")?,
        alpha: gu8(v, "alpha")?,
        mag_flags: gu8(v, "mag_flags")?,
        translate: gfa(v, "t")?,
        rotate: gfa(v, "r")?,
        scale: gfa(v, "s")?,
        size: gfa(v, "size")?,
        user_data: match v.get("user_data") {
            Some(u) => user_data_from_json(u)?,
            None => Vec::new(),
        },
        children: match v.get("children") {
            Some(c) => arr(c)?.iter().map(pane_from_json).collect::<Result<_>>()?,
            None => Vec::new(),
        },
    })
}

fn group_to_json(g: &Group) -> Value {
    json!({
        "name": g.name,
        "panes": g.panes,
        "children": g.children.iter().map(group_to_json).collect::<Vec<_>>(),
    })
}

fn group_from_json(v: &Value) -> Result<Group> {
    Ok(Group {
        name: gs(v, "name")?,
        panes: strings(get(v, "panes")?)?,
        children: arr(get(v, "children")?)?
            .iter()
            .map(group_from_json)
            .collect::<Result<_>>()?,
    })
}

// --- animation -----------------------------------------------------------------------------

pub fn animation_to_json(a: &Animation) -> Value {
    json!({
        "name": a.name,
        "version": a.version,
        "frame_size": a.frame_size,
        "loop": a.looping,
        "start": a.start_frame,
        "end": a.end_frame,
        "groups": a.groups,
        "textures": a.textures,
        "contents": a.contents.iter().map(|c| json!({
            "name": c.name,
            "kind": c.kind,
            "tags": c.tags.iter().map(|t| json!({
                "tag": t.magic,
                "entries": t.entries.iter().map(|e| json!({
                    "index": e.index,
                    "target": e.target,
                    "curve": e.curve.code(),
                    // [frame, value, slope] triples
                    "keys": e.keys.iter().map(|k| json!([k.frame, k.value, k.slope])).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

pub fn animation_from_json(v: &Value) -> Result<Animation> {
    Ok(Animation {
        name: gs(v, "name")?,
        version: u(get(v, "version")?)? as u32,
        frame_size: gu16(v, "frame_size")?,
        looping: get(v, "loop")?.as_bool().unwrap_or(false),
        start_frame: i(get(v, "start")?)? as i16,
        end_frame: i(get(v, "end")?)? as i16,
        groups: strings(get(v, "groups")?)?,
        textures: strings(get(v, "textures")?)?,
        contents: arr(get(v, "contents")?)?
            .iter()
            .map(|c| {
                Ok(AnimContent {
                    name: gs(c, "name")?,
                    kind: gu8(c, "kind")?,
                    tags: arr(get(c, "tags")?)?
                        .iter()
                        .map(|t| {
                            Ok(AnimTag {
                                magic: gs(t, "tag")?,
                                entries: arr(get(t, "entries")?)?
                                    .iter()
                                    .map(|e| {
                                        Ok(AnimEntry {
                                            index: gu8(e, "index")?,
                                            target: gu8(e, "target")?,
                                            curve: CurveType::from_code(gu8(e, "curve")?),
                                            keys: arr(get(e, "keys")?)?
                                                .iter()
                                                .map(|k| {
                                                    let [frame, value, slope] = fixed_f::<3>(k)?;
                                                    Ok(AnimKey {
                                                        frame,
                                                        value,
                                                        slope,
                                                    })
                                                })
                                                .collect::<Result<_>>()?,
                                        })
                                    })
                                    .collect::<Result<_>>()?,
                            })
                        })
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{sample_bflan, sample_bflyt};

    #[test]
    fn layout_round_trip() {
        let l = Layout::parse(&sample_bflyt()).unwrap();
        let text = serde_json::to_string(&layout_to_json(&l)).unwrap();
        let back = layout_from_json(&serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(back, l);
    }

    #[test]
    fn animation_round_trip() {
        let a = Animation::parse(&sample_bflan()).unwrap();
        let text = serde_json::to_string(&animation_to_json(&a)).unwrap();
        let back = animation_from_json(&serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn missing_fields_are_errors() {
        assert!(layout_from_json(&json!({ "name": "x" })).is_err());
        assert!(animation_from_json(&json!([])).is_err());
    }
}
