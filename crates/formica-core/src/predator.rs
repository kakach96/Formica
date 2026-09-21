//! Predators.
//!
//! A predator is deliberately much simpler than an ant: it has no pheromone, no
//! state machine and no memory. It steers at the nearest ant it can see, and
//! wanders when it cannot see one. That is the whole design.
//!
//! What makes it interesting is what it does to the *colony*. Ants are not
//! killed permanently - an eaten ant is despawned and `sync_ant_count` replaces
//! it at its nest on a later tick - so the population stays constant while the
//! colony keeps losing whichever ants were furthest from home. Predation
//! therefore applies continuous pressure to stay near the trail, and it is a
//! clean way to show that the colony's structure is load-bearing rather than
//! incidental.

use bevy_ecs::prelude::*;

use crate::rng::SimRng;
use crate::world::Nests;

/// Marks a predator entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PredatorMarker;

/// Predator position in continuous world coordinates (cells).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PredatorPos {
    /// X in cells.
    pub x: f32,
    /// Y in cells.
    pub y: f32,
}

/// Predator heading in radians.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PredatorHeading {
    /// Angle in radians; direction of travel is `(cos, sin)`.
    pub angle: f32,
}

/// Whether the predator currently has prey in sight.
///
/// Written by the steering system and read by the renderer, so a hunting
/// predator can be drawn differently from a wandering one.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PredatorAlert(pub bool);

/// Build the component tuple for a new predator.
///
/// Predators spawn away from every nest. Dropping one on top of a colony would
/// delete a large fraction of the foragers in the first few ticks, which is not
/// predation - it is a reset.
pub fn newborn_predator(
    rng: &mut SimRng,
    width: usize,
    height: usize,
    nests: &Nests,
    keep_clear: f32,
) -> (
    PredatorMarker,
    PredatorPos,
    PredatorHeading,
    PredatorAlert,
) {
    let margin = 4.0f32;
    let (mut x, mut y) = (margin, margin);
    for _ in 0..64 {
        let cx = rng.range(margin, (width as f32 - margin).max(margin + 1.0));
        let cy = rng.range(margin, (height as f32 - margin).max(margin + 1.0));
        let clear = nests
            .0
            .iter()
            .all(|n| (cx - n.x).hypot(cy - n.y) > keep_clear + n.radius);
        x = cx;
        y = cy;
        if clear {
            break;
        }
    }
    (
        PredatorMarker,
        PredatorPos { x, y },
        PredatorHeading {
            angle: rng.range(0.0, std::f32::consts::TAU),
        },
        PredatorAlert(false),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SimConfig;

    #[test]
    fn predators_spawn_on_the_map_and_away_from_the_nests() {
        let config = SimConfig::default();
        let nests = Nests::from_config(&config);
        let mut rng = SimRng::new(11);

        for _ in 0..200 {
            let (_, pos, heading, alert) =
                newborn_predator(&mut rng, config.width, config.height, &nests, 40.0);
            assert!((0.0..config.width as f32).contains(&pos.x));
            assert!((0.0..config.height as f32).contains(&pos.y));
            assert!((-std::f32::consts::TAU..std::f32::consts::TAU).contains(&heading.angle));
            assert!(!alert.0, "a fresh predator should not start alert");
            for nest in &nests.0 {
                assert!(
                    (pos.x - nest.x).hypot(pos.y - nest.y) > nest.radius + 40.0,
                    "predator spawned on a nest"
                );
            }
        }
    }

    #[test]
    fn spawn_still_succeeds_when_the_keep_clear_radius_is_impossible() {
        // A clear radius larger than the map: the loop must give up gracefully
        // and still return a usable position rather than looping forever.
        let config = SimConfig::default();
        let nests = Nests::from_config(&config);
        let mut rng = SimRng::new(3);
        let (_, pos, _, _) = newborn_predator(&mut rng, config.width, config.height, &nests, 1.0e6);
        assert!((0.0..config.width as f32).contains(&pos.x));
        assert!((0.0..config.height as f32).contains(&pos.y));
    }
}
