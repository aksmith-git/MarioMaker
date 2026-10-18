use std::collections::{BTreeMap, HashMap};

use mmk_assets::{Mtex, MtexFormat, MtexVariant};
use mmk_formats::bffnt::Font;
use mmk_formats::fixtures::{sample_bflan, sample_bflyt, sample_font};
use mmk_formats::lyt::{
    Animation, Layout, Material, Pane, PaneKind, Part, Picture, TexMap, Window, WindowFrame,
};

use super::*;
use crate::cache::{LayoutSource, UiCache};

fn tex(w: u32, h: u32) -> Mtex {
    Mtex {
        format: MtexFormat::Rgba8,
        variant: MtexVariant::Unorm,
        width: w,
        height: h,
        channel_select: [2, 3, 4, 5],
        data: vec![255; (w * h * 4) as usize],
    }
}

fn sample_cache() -> UiCache {
    let mut c = UiCache::in_memory();
    let font = Font::parse(&sample_font()).unwrap();
    c.add_font("MessageFont", font, vec![tex(64, 32)]);
    let layout = Layout::parse(&sample_bflyt()).unwrap();
    let mut animations = BTreeMap::new();
    animations.insert("Move".into(), Animation::parse(&sample_bflan()).unwrap());
    c.add_layout(LayoutSource {
        layout,
        animations,
        textures: vec![("Box^s".into(), tex(8, 8)), ("Tex1".into(), tex(4, 4))],
    });
    c
}

fn pictures(q: &[Quad]) -> Vec<&Quad> {
    // glyph quads use the font sheet (texture 0 in the sample cache)
    q.iter().filter(|q| q.tex0 != 0).collect()
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

#[test]
fn base_layout_draws_picture_and_text() {
    let mut c = sample_cache();
    let p = LayoutPlayer::new(c.layout("Sample").unwrap());
    let q = p.draw_list();
    let pics = pictures(&q);
    assert_eq!(pics.len(), 1);
    let box_q = pics[0];
    // P_Box: centre origin, 64×32 at (100, −50)
    assert!(close(box_q.pos[0], [68.0, -34.0]));
    assert!(close(box_q.pos[3], [132.0, -66.0]));
    // material colours and the texture SRT (translate 0.25)
    assert_eq!(box_q.white, [200, 100, 50, 255]);
    assert!(close(box_q.uv0[0], [0.25, 0.0]));
    assert_eq!(box_q.flags & 3, 1); // wrap s = repeat
    assert_eq!(box_q.tex0, 1); // Box^s, registered after the font sheet
    // "Hi" with no 'H'/'i' in the font: two replacement glyphs
    let glyphs: Vec<&Quad> = q.iter().filter(|q| q.tex0 == 0).collect();
    assert_eq!(glyphs.len(), 2);
    // T_Label: left-top origin and text position, at (0, 0); bottom colour on the lower corners
    assert!(close(glyphs[0].pos[0], [0.0, 0.0]));
    assert_eq!(glyphs[0].color[2], [255, 0, 0, 255]);
}

#[test]
fn animation_moves_hides_and_swaps_texture() {
    let mut c = sample_cache();
    let mut p = LayoutPlayer::new(c.layout("Sample").unwrap());
    assert!(p.play("Move", 0.0));
    assert!(!p.play("Nope", 0.0));
    assert_eq!(p.animation_length("Move"), Some(10.0));
    let at = |p: &mut LayoutPlayer, f: f32| {
        p.play("Move", f);
        pictures(&p.draw_list()).first().map(|q| (q.pos[0], q.tex0))
    };
    let (pos0, tex0) = at(&mut p, 0.0).unwrap();
    assert!(close(pos0, [-32.0, -34.0])); // TX keyed to 0
    assert_eq!(tex0, 1);
    let (pos4, tex4) = at(&mut p, 4.0).unwrap();
    assert!(pos4[0] > -32.0 && pos4[0] < 18.0);
    assert_eq!(tex4, 2); // pattern key 1 → "Tex1"
    assert!(at(&mut p, 5.0).is_none()); // FLVI off from frame 5
    // unbinding restores the base pose (translate 100, visible)
    p.stop(Some("Move"));
    let q = p.draw_list();
    let pic = pictures(&q)[0];
    assert!(close(pic.pos[0], [68.0, -34.0]));
    assert_eq!(pic.tex0, 1);
}

#[test]
fn deterministic_output() {
    let mut c = sample_cache();
    let mut p = LayoutPlayer::new(c.layout("Sample").unwrap());
    p.play("Move", 3.25);
    assert_eq!(p.draw_list(), p.draw_list());
    let mut q = LayoutPlayer::new(c.layout("Sample").unwrap());
    q.play("Move", 3.25);
    assert_eq!(p.draw_list(), q.draw_list());
}

#[test]
fn text_override_and_messages() {
    let mut c = sample_cache();
    let mut msgs = HashMap::new();
    msgs.insert("T_Label".to_string(), "A".to_string());
    c.add_messages("Sample", msgs);
    let mut p = LayoutPlayer::new(c.layout("Sample").unwrap());
    let glyphs = |p: &LayoutPlayer| -> Vec<Quad> {
        p.draw_list().into_iter().filter(|q| q.tex0 == 0).collect()
    };
    assert_eq!(glyphs(&p).len(), 1); // message text wins over the stored "Hi"
    assert!(p.set_text("T_Label", "AB 0"));
    assert!(!p.set_text("P_Box", "x"));
    let g = glyphs(&p);
    assert_eq!(g.len(), 3); // the space draws nothing
    // second glyph starts one advance (6 px · 24/7) + char space (1) after the first
    let sx = 24.0 / 7.0;
    assert!((g[1].pos[0][0] - (6.0 * sx + 1.0 + 1.0 * sx)).abs() < 1e-3);
}

#[test]
fn missing_characters_come_from_the_fallback_font() {
    let mut c = sample_cache();
    let mut font = Font::parse(&sample_font()).unwrap();
    font.cmap.remove(&('A' as u32));
    // the primary font lacks 'A'; give it a fallback (the sample font again, on texture 99)
    let fallback = std::sync::Arc::new(crate::FontRes::from_font(
        "Fallback",
        Font::parse(&sample_font()).unwrap(),
        vec![99],
    ));
    let mut primary = crate::FontRes::from_font("MessageFont", font, vec![0]);
    primary.fallback = Some(fallback);
    assert_eq!(primary.for_char('A').name, "Fallback");
    assert_eq!(primary.for_char('B').name, "MessageFont");
    assert_eq!(primary.for_char('?').name, "MessageFont"); // nobody has it: replacement glyph
    let res = c.layout("Sample").unwrap();
    let mut res2 = (*res).clone();
    res2.fonts = vec![Some(std::sync::Arc::new(primary))];
    let mut p = LayoutPlayer::new(std::sync::Arc::new(res2));
    p.set_text("T_Label", "AB");
    let tex: Vec<u32> = p
        .draw_list()
        .iter()
        .filter(|q| q.tex0 == 0 || q.tex0 == 99)
        .map(|q| q.tex0)
        .collect();
    assert_eq!(tex, [99, 0]);
}

fn picture(name: &str, mat: u16, t: [f32; 2], size: [f32; 2], children: Vec<Pane>) -> Pane {
    Pane {
        kind: PaneKind::Picture(Picture {
            vertex_colors: [[255; 4]; 4],
            material: mat,
            tex_coords: vec![[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]]],
        }),
        name: name.into(),
        translate: [t[0], t[1], 0.0],
        size,
        children,
        ..Default::default()
    }
}

fn plain_material(name: &str) -> Material {
    Material {
        name: name.into(),
        white: [255; 4],
        tex_maps: vec![TexMap::default()],
        ..Default::default()
    }
}

fn layout(name: &str, root_children: Vec<Pane>) -> Layout {
    Layout {
        name: name.into(),
        width: 1280.0,
        height: 720.0,
        textures: vec!["T.bflim".into()],
        materials: vec![plain_material("M")],
        root: Pane {
            name: "RootPane".into(),
            size: [1280.0, 720.0],
            children: root_children,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn add(c: &mut UiCache, l: Layout) {
    c.add_layout(LayoutSource {
        layout: l,
        animations: BTreeMap::new(),
        textures: vec![("T".into(), tex(16, 16))],
    });
}

#[test]
fn alpha_inheritance_and_anchor() {
    let mut c = UiCache::in_memory();
    let child = picture("P_Child", 0, [0.0, 0.0], [10.0, 10.0], vec![]);
    let mut parent = picture("P_Parent", 0, [0.0, 0.0], [100.0, 100.0], vec![child]);
    parent.alpha = 128;
    parent.origin = 5; // left-top: the rect spans x 0..100, y −100..0
    add(&mut c, layout("A", vec![parent.clone()]));
    let q = LayoutPlayer::new(c.layout("A").unwrap()).draw_list();
    assert_eq!(q.len(), 2);
    assert_eq!(q[0].color[0][3], 128);
    assert_eq!(q[1].color[0][3], 255); // parent does not pass its alpha on
    // child without anchor bits sits on the parent's origin point (its top-left corner)
    assert!(close(q[1].pos[0], [-5.0, 5.0]));
    parent.flags |= 2; // influenced alpha
    // explicit anchor bits: left (1) / top (1), i.e. 0x50
    parent.children[0].origin = 0x50;
    add(&mut c, layout("B", vec![parent]));
    let q = LayoutPlayer::new(c.layout("B").unwrap()).draw_list();
    assert_eq!(q[1].color[0][3], 128);
    // the parent's left-top point is its origin point: offset 0
    assert!(close(q[1].pos[0], [-5.0, 5.0]));
    // anchor centre of a bigger parent: bits right (2) / bottom (2) = 0xA0 → (100, −100)
    let child = Pane {
        origin: 0xA0,
        ..picture("P_C", 0, [0.0, 0.0], [10.0, 10.0], vec![])
    };
    let mut p2 = picture("P_P", 0, [0.0, 0.0], [100.0, 100.0], vec![child]);
    p2.origin = 5;
    add(&mut c, layout("C", vec![p2]));
    let q = LayoutPlayer::new(c.layout("C").unwrap()).draw_list();
    assert!(close(q[1].pos[0], [95.0, -95.0]));
}

#[test]
fn rotation_and_scale() {
    let m = Mtx::srt([10.0, 0.0, 0.0], [0.0, 0.0, 90.0], [2.0, 1.0]);
    let p = m.apply(1.0, 0.0);
    assert!(close(p, [10.0, 2.0]));
    let mut c = UiCache::in_memory();
    let mut pic = picture("P", 0, [0.0, 0.0], [20.0, 10.0], vec![]);
    pic.rotate[2] = 90.0;
    add(&mut c, layout("R", vec![pic]));
    let q = LayoutPlayer::new(c.layout("R").unwrap()).draw_list();
    // top-left corner (−10, 5) rotated by 90° → (−5, −10)
    assert!(close(q[0].pos[0], [-5.0, -10.0]));
}

#[test]
fn part_panes_draw_their_layout_and_take_animations() {
    let mut c = UiCache::in_memory();
    add(
        &mut c,
        layout(
            "Sub",
            vec![picture("P_In", 0, [1.0, 2.0], [4.0, 4.0], vec![])],
        ),
    );
    let part = Pane {
        kind: PaneKind::Part(Part {
            layout: "Sub".into(),
            magnify: [1.0, 1.0],
            properties: vec![],
        }),
        name: "L_Sub_00".into(),
        translate: [100.0, 200.0, 0.0],
        size: [4.0, 4.0],
        ..Default::default()
    };
    add(&mut c, layout("Main", vec![part]));
    let mut p = LayoutPlayer::new(c.layout("Main").unwrap());
    let q = p.draw_list();
    assert_eq!(q.len(), 1);
    assert!(close(q[0].pos[0], [99.0, 204.0])); // (100+1−2, 200+2+2)
    assert!(!p.play("L_Sub_00/Move", 0.0)); // no such animation in the part
    assert!(!p.play("L_Missing/Move", 0.0));
}

#[test]
fn part_text_ids_resolve_in_the_screen_message_table() {
    use mmk_formats::lyt::{PartProperty, Text};
    let mut c = sample_cache(); // has MessageFont
    let text_pane = |name: &str, id: &str, stored: &str| Pane {
        kind: PaneKind::Text(Text {
            font_size: [7.0, 9.0],
            top_color: [255; 4],
            bottom_color: [255; 4],
            text_id: id.into(),
            text: stored.into(),
            ..Default::default()
        }),
        name: name.into(),
        size: [100.0, 20.0],
        ..Default::default()
    };
    let part = |name: &str, layout: &str, props: Vec<PartProperty>| Pane {
        kind: PaneKind::Part(Part {
            layout: layout.into(),
            magnify: [1.0, 1.0],
            properties: props,
        }),
        name: name.into(),
        size: [10.0, 10.0],
        ..Default::default()
    };
    let mut btn = layout("Btn", vec![text_pane("T_Btn_00", "", "X")]);
    btn.fonts = vec!["MessageFont.bffnt".into()];
    add(&mut c, btn);
    // the middle layout overrides the button's text with message id "L_Sel_10"…
    let prop = PartProperty {
        name: "T_Btn_00".into(),
        pane: Some(Box::new(text_pane("T_Btn_00", "L_Sel_10", "?"))),
        ..Default::default()
    };
    add(
        &mut c,
        layout("Mid", vec![part("L_Btn_00", "Btn", vec![prop])]),
    );
    // …which only the screen's table has
    add(
        &mut c,
        layout("Screen", vec![part("L_Mid_00", "Mid", vec![])]),
    );
    let mut msgs = HashMap::new();
    msgs.insert("L_Sel_10".to_string(), "AB".to_string());
    c.add_messages("Screen", msgs);
    let glyphs = |name: &str, c: &mut UiCache| {
        LayoutPlayer::new(c.layout(name).unwrap())
            .draw_list()
            .iter()
            .filter(|q| q.tex0 == 0)
            .count()
    };
    assert_eq!(glyphs("Screen", &mut c), 2); // "AB"
    assert_eq!(glyphs("Mid", &mut c), 1); // stored override text "?"
    assert_eq!(glyphs("Btn", &mut c), 1); // stored "X"
}

#[test]
fn pane_projection_maps_the_origin_to_the_texture_centre() {
    use mmk_formats::lyt::{ProjTexGen, TexCoordGen, TexSrt};
    let mut c = UiCache::in_memory();
    let mut l = layout("P", vec![]);
    // left-bottom origin, pane-projected texture shifted by (−0.5, +0.5) as the game authors it
    let mut pic = picture("P_Goomba", 0, [0.0, 0.0], [64.0, 64.0], vec![]);
    pic.origin = 9;
    l.root.children.push(pic);
    let m = &mut l.materials[0];
    m.tex_coord_gens = vec![TexCoordGen {
        matrix: 0,
        source: 4,
    }];
    m.proj_tex_gens = vec![ProjTexGen {
        translate: [0.0, 0.0],
        scale: [1.0, 1.0],
        flags: 0,
    }];
    m.tex_srts = vec![TexSrt {
        translate: [-0.5, 0.5],
        ..Default::default()
    }];
    add(&mut c, l);
    let q = LayoutPlayer::new(c.layout("P").unwrap()).draw_list();
    for (got, want) in q[0].uv0.iter().zip(UNIT_UV) {
        assert!(close(*got, want), "{got:?} vs {want:?}");
    }
}

#[test]
fn window_with_one_frame_draws_content_corners_and_edges() {
    let mut c = UiCache::in_memory();
    let w = Pane {
        kind: PaneKind::Window(Window {
            frame_size: [8, 8, 8, 8],
            flags: 3,
            content: Picture {
                vertex_colors: [[255; 4]; 4],
                material: 0,
                tex_coords: vec![],
            },
            frames: vec![WindowFrame {
                material: 0,
                flip: 0,
            }],
            ..Default::default()
        }),
        name: "W".into(),
        size: [100.0, 50.0],
        ..Default::default()
    };
    add(&mut c, layout("W", vec![w]));
    let q = LayoutPlayer::new(c.layout("W").unwrap()).draw_list();
    assert_eq!(q.len(), 9);
    // content inset by the frame, TR corner mirrored horizontally
    assert!(close(q[0].pos[0], [-42.0, 17.0]));
    assert!(close(q[2].uv0[0], [1.0, 0.0]));
    assert!(close(q[2].pos[0], [42.0, 25.0]));
}

fn part_pane(name: &str, sub: &str, props: Vec<mmk_formats::lyt::PartProperty>) -> Pane {
    Pane {
        kind: PaneKind::Part(Part {
            layout: sub.into(),
            magnify: [1.0, 1.0],
            properties: props,
        }),
        name: name.into(),
        size: [4.0, 4.0],
        ..Default::default()
    }
}

#[test]
fn influenced_alpha_fades_the_whole_subtree_and_parts() {
    let mut c = UiCache::in_memory();
    add(
        &mut c,
        layout(
            "Leaf",
            vec![picture("P_Leaf", 0, [0.0, 0.0], [4.0, 4.0], vec![])],
        ),
    );
    let grandchild = picture("P_Grand", 0, [0.0, 0.0], [4.0, 4.0], vec![]);
    let middle = Pane {
        name: "N_Mid".into(),
        flags: 1, // visible, not influenced
        size: [4.0, 4.0],
        children: vec![grandchild, part_pane("L_Leaf_00", "Leaf", vec![])],
        ..Default::default()
    };
    let top = Pane {
        name: "N_Top".into(),
        flags: 3, // influenced alpha
        alpha: 0,
        size: [4.0, 4.0],
        children: vec![middle],
        ..Default::default()
    };
    add(&mut c, layout("Fade", vec![top]));
    let q = LayoutPlayer::new(c.layout("Fade").unwrap()).draw_list();
    assert_eq!(q.len(), 2);
    assert!(
        q.iter().all(|q| q.color[0][3] == 0),
        "{:?}",
        q.iter().map(|q| q.color[0]).collect::<Vec<_>>()
    );
}

#[test]
fn caller_visibility_and_pane_info() {
    let mut c = UiCache::in_memory();
    add(
        &mut c,
        layout("V", vec![picture("P_A", 0, [3.0, 4.0], [8.0, 8.0], vec![])]),
    );
    let mut p = LayoutPlayer::new(c.layout("V").unwrap());
    assert_eq!(p.draw_list().len(), 1);
    assert!(p.set_visible("P_A", Some(false)));
    assert!(p.draw_list().is_empty());
    let info = p.pane_info("P_A").unwrap();
    assert_eq!(info.translate, [3.0, 4.0, 0.0]);
    assert!(!info.visible);
    assert!(p.set_visible("P_A", None));
    assert_eq!(p.draw_list().len(), 1);
    assert!(!p.set_visible("P_Missing", Some(true)));
    assert!(p.pane_info("P_Missing").is_none());
}

#[test]
fn part_properties_override_basic_values_and_picture_material() {
    use mmk_formats::lyt::{PartBasicInfo, PartProperty};
    let mut c = UiCache::in_memory();
    add(
        &mut c,
        layout(
            "Dot",
            vec![
                picture("P_Num", 0, [0.0, 0.0], [10.0, 10.0], vec![]),
                picture("P_Hide", 0, [0.0, 0.0], [10.0, 10.0], vec![]),
            ],
        ),
    );
    // the screen has a second material with its own texture, used by the dot's P_Num
    let mut screen = layout(
        "Map",
        vec![part_pane(
            "L_Dot_00",
            "Dot",
            vec![
                PartProperty {
                    name: "P_Num".into(),
                    basic_usage: 16, // size
                    basic: Some(PartBasicInfo {
                        size: [20.0, 6.0],
                        ..Default::default()
                    }),
                    pane: Some(Box::new(picture(
                        "P_Num",
                        1,
                        [0.0, 0.0],
                        [1.0, 1.0],
                        vec![],
                    ))),
                    ..Default::default()
                },
                PartProperty {
                    name: "P_Hide".into(),
                    basic_usage: 1, // visibility override, value 0
                    ..Default::default()
                },
            ],
        )],
    );
    screen.textures.push("U.bflim".into());
    let mut m = plain_material("M2");
    m.tex_maps[0].texture = 1;
    screen.materials.push(m);
    c.add_layout(LayoutSource {
        layout: screen,
        animations: BTreeMap::new(),
        textures: vec![("T".into(), tex(16, 16)), ("U".into(), tex(2, 2))],
    });
    let p = LayoutPlayer::new(c.layout("Map").unwrap());
    let q = p.draw_list();
    assert_eq!(q.len(), 1, "P_Hide is hidden by the property");
    let screen_u = c.layout("Map").unwrap().textures[1].unwrap().id;
    assert_eq!(q[0].tex0, screen_u);
    assert!(close(q[0].pos[0], [-10.0, 3.0]));
    let info = p.pane_info("L_Dot_00/P_Num").unwrap();
    assert_eq!(info.size, [20.0, 6.0]);
}

#[test]
fn messages_by_label() {
    let mut c = sample_cache();
    let mut msgs = HashMap::new();
    msgs.insert("T_A".to_string(), "plain".to_string());
    c.add_messages("Sample", msgs);
    let p = LayoutPlayer::new(c.layout("Sample").unwrap());
    assert_eq!(p.message("T_A").as_deref(), Some("plain"));
    assert!(p.message("T_None").is_none());
}

/// Screen → L_Btn_00 / L_Btn_01 (part "Btn" with one text pane T_Btn_00), as SMM2 builds menus.
fn smm2_menu(c: &mut UiCache) {
    use mmk_formats::lyt::{PartProperty, Text};
    let text = Pane {
        kind: PaneKind::Text(Text {
            font_size: [7.0, 9.0],
            top_color: [255; 4],
            bottom_color: [255; 4],
            text: "JP".into(),
            ..Default::default()
        }),
        name: "T_Btn_00".into(),
        size: [100.0, 20.0],
        ..Default::default()
    };
    let mut btn = layout(
        "Btn",
        vec![
            picture("B_Btn_00", 0, [0.0, 0.0], [40.0, 20.0], vec![]),
            text.clone(),
        ],
    );
    btn.fonts = vec!["MessageFont.fcpx".into()];
    add(c, btn);
    // the screen's part properties carry a text pane with the Japanese placeholder, as SMM2's do
    let prop = PartProperty {
        name: "T_Btn_00".into(),
        usage: 1,
        pane: Some(Box::new(text)),
        ..Default::default()
    };
    let part = |name: &str, y: f32| Pane {
        kind: PaneKind::Part(Part {
            layout: "Btn".into(),
            magnify: [1.0, 1.0],
            properties: vec![prop.clone()],
        }),
        name: name.into(),
        translate: [0.0, y, 0.0],
        size: [40.0, 20.0],
        ..Default::default()
    };
    let group = Pane {
        name: "N_List_00".into(),
        translate: [10.0, 0.0, 0.0],
        children: vec![part("L_Btn_00", 50.0), part("L_Btn_01", 0.0)],
        ..Default::default()
    };
    add(c, layout("Menu", vec![group]));
    let mut msgs = HashMap::new();
    msgs.insert("L_Btn_00-T_Btn_00".to_string(), "AB".to_string());
    msgs.insert("L_Btn_01-T_Btn_00".to_string(), "ABAB".to_string());
    c.add_messages("Menu", msgs);
}

#[test]
fn smm2_part_texts_use_part_pane_labels() {
    let mut c = sample_cache();
    smm2_menu(&mut c);
    let p = LayoutPlayer::new(c.layout("Menu").unwrap());
    let glyphs = p.draw_list().iter().filter(|q| q.tex0 == 0).count();
    assert_eq!(glyphs, 6); // "AB" + "ABAB", not the stored "JP"
}

#[test]
fn clones_follow_their_source_and_have_their_own_instance() {
    let mut c = sample_cache();
    smm2_menu(&mut c);
    let mut p = LayoutPlayer::new(c.layout("Menu").unwrap());
    assert!(p.add_clone("L_Btn_01", "L_Btn_02", [0.0, -50.0]));
    assert!(!p.add_clone("L_Btn_01", "L_Btn_00", [0.0, 0.0])); // name taken
    assert!(!p.add_clone("N_List_00", "X", [0.0, 0.0])); // not a part
    assert!(p.set_text("L_Btn_02/T_Btn_00", "A"));
    let q = p.draw_list();
    assert_eq!(q.iter().filter(|q| q.tex0 == 0).count(), 7);
    // the clone's bounding picture sits 50 below the source's, inside the moved group
    let g = p.pane_global("L_Btn_02/B_Btn_00").unwrap();
    assert!(close(g.corners[0], [-10.0, -40.0]));
    let src = p.pane_global("L_Btn_01/B_Btn_00").unwrap();
    assert!(close(src.corners[3], [30.0, -10.0]));
    assert!(g.visible && (g.alpha - 1.0).abs() < 1e-6);
    // hiding the group hides the clone too
    p.set_visible("N_List_00", Some(false));
    assert!(!p.pane_global("L_Btn_02/B_Btn_00").unwrap().visible);
    assert_eq!(p.draw_list().len(), 0);
}

#[test]
fn caller_pane_overrides_win_over_animations() {
    let mut c = sample_cache();
    let mut p = LayoutPlayer::new(c.layout("Sample").unwrap());
    p.play("Move", 10.0); // moves P_Box to x 100 → 200
    let o = PaneOverride {
        translate: Some([0.0, 0.0]),
        size: Some([10.0, 20.0]),
        alpha: Some(51.0),
        ..Default::default()
    };
    assert!(p.set_pane("P_Box", o));
    let g = p.pane_global("P_Box").unwrap();
    assert!(close(g.corners[0], [-5.0, 10.0]));
    assert!((g.alpha - 0.2).abs() < 1e-6);
    let info = p.pane_info("P_Box").unwrap();
    assert_eq!(info.size, [10.0, 20.0]);
    assert!(p.set_pane("P_Box", PaneOverride::default())); // removes it
    assert!(!p.set_pane("P_None", o));
}

#[test]
fn caller_picture_texture_material_colour_and_text_width() {
    let mut c = sample_cache();
    let icon = c.add_texture_file("Icons", "Star", tex(16, 16));
    assert_eq!(c.texture_file("Icons", "Star"), Some(icon));
    assert_eq!(c.texture_file("Icons", "Missing"), None); // in memory: no files
    let mut p = LayoutPlayer::new(c.layout("Sample").unwrap());
    let pic = |p: &LayoutPlayer| *pictures(&p.draw_list())[0];
    // a texture of the caller's choice wins over the material's (and its pattern animation)
    let own = pic(&p).tex0;
    assert!(p.set_picture_texture("P_Box", Some(icon)));
    assert_eq!(pic(&p).tex0, icon.id);
    p.play("Move", 4.0);
    assert_eq!(pic(&p).tex0, icon.id);
    p.stop(None);
    assert!(p.set_picture_texture("P_Box", None));
    assert_eq!(pic(&p).tex0, own);
    assert!(!p.set_picture_texture("T_Label", Some(icon)));
    // material colours
    assert!(p.set_material_color("P_Box", None, Some([1, 2, 3, 4])));
    assert_eq!(pic(&p).white, [1, 2, 3, 4]);
    assert_eq!(p.material_color("P_Box"), Some((None, Some([1, 2, 3, 4]))));
    assert!(p.set_material_color("P_Box", None, None));
    assert_eq!(pic(&p).white, [200, 100, 50, 255]);
    assert!(!p.set_material_color("T_Label", None, Some([0; 4])));
    // text width: grows with the text, none for a picture
    p.set_text("T_Label", "A");
    let one = p.text_width("T_Label").unwrap();
    p.set_text("T_Label", "AA\nA");
    let two = p.text_width("T_Label").unwrap();
    assert!(one > 0.0 && two > one);
    assert_eq!(p.text_width("P_Box"), None);
}

#[test]
fn shared_message_tables() {
    let mut c = sample_cache();
    let mut msgs = HashMap::new();
    msgs.insert("Goal".to_string(), "Reach it".to_string());
    c.add_messages("GoalTable", msgs);
    assert_eq!(
        c.table_message("GoalTable", "Goal").as_deref(),
        Some("Reach it")
    );
    assert_eq!(c.table_message("GoalTable", "Nope"), None);
    assert_eq!(c.table_message("NoTable", "Goal"), None);
}
