//! What the simulation asks the renderer to draw. Names, not GPU handles: the FFI layer resolves
//! (model, texture) to the renderer's texture indices.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteImage {
    pub model: &'static str,
    pub texture: &'static str,
    /// Sub-rectangle of the texture in pixels (x, y, w, h); None = whole texture.
    pub rect: Option<(f32, f32, f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    pub image: SpriteImage,
    /// World position of the bottom-left corner (px, y up) and size (px).
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub flip_x: bool,
    /// Draw order: lower first.
    pub layer: u32,
}
