//! SMM1 extras (Phase 11, `crate::options::Options`): rules from the original Super Mario Maker
//! that hosts can switch on over SMM2 play. Behaviour specs: research/specs/smm1-extras.md.

use super::World;

impl World {}

#[cfg(test)]
mod tests {
    use crate::level::{Level, PipeDir};
    use crate::world::World;

    fn pipe_layers(w: &World) -> Vec<u32> {
        w.sprites()
            .iter()
            .filter(|s| s.image.rect.is_some() && s.layer != 12 && s.layer != 20)
            .filter(|s| {
                w.level
                    .tiles
                    .iter()
                    .any(|t| t.pipe && (t.x as f32 * 16.0, t.y as f32 * 16.0) == (s.x, s.y))
            })
            .map(|s| s.layer)
            .collect()
    }

    #[test]
    fn pipes_drawn_over_the_player_only_with_the_option() {
        let mut lvl = Level::flat(60);
        lvl.add_pipe(6, 2, 3, PipeDir::Up);
        let mut w = World::with_level(lvl);
        let player = w.player_sprite().unwrap().layer;
        let layers = pipe_layers(&w);
        assert_eq!(layers.len(), 6, "2 × 3 pipe tiles in view");
        assert!(
            layers.iter().all(|&l| l < player),
            "SMM2: the player is in front"
        );
        let mut o = w.options;
        o.pipes_over_player = true;
        w.set_options(o);
        assert!(
            pipe_layers(&w).iter().all(|&l| l > player),
            "SMM1: pipes in front"
        );
        // ground tiles keep their layer
        assert!(
            w.sprites()
                .iter()
                .any(|s| s.image.rect.is_some() && s.layer == 10)
        );
    }
}
