//! What the simulation asks the renderer to draw. Names, not GPU handles: the FFI layer resolves
//! (model, texture) to the renderer's texture indices.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteImage {
    pub model: &'static str,
    pub texture: &'static str,
    /// Sub-rectangle of the texture in pixels (x, y, w, h); None = whole texture.
    pub rect: Option<(f32, f32, f32, f32)>,
}

/// Placement convention:
/// - Tile sprites (`image.rect` set): (x, y) is the bottom-left corner and (w, h) the size of
///   the drawn rectangle.
/// - Model sprites (`image.rect` None): (x + w/2, y) is the **model origin** (the actor's origin,
///   feet centre for walkers) and (w, h) the texture size times the sprite's scale. The front end
///   draws the texture on the quad the game's model gives it (`mmk_assets::Cache::sprite_quad`,
///   from the BFRES), so offsets of the art inside the texture are not the sim's concern; only
///   without quad data does it fill (x, y, w, h).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    pub image: SpriteImage,
    /// World position (px, y up) and size (px); see the placement convention above.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub flip_x: bool,
    pub flip_y: bool,
    /// Draw order: lower first.
    pub layer: u32,
}
