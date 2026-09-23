//! Crowding: measure it, then relieve it.
//!
//! # The failure this exists to fix
//!
//! Put food behind a wall so that the only way in is a gap a few cells wide,
//! and the colony does something it never does on an open map: it stops being
//! spread out and collapses into a single spot. The mechanism is a queue with
//! nowhere to go. The trail through the gap is genuinely the best trail - it is
//! the only one that leads to food - so every searching ant within sensing
//! range turns onto it, and the gap admits a couple of ants per tick. The rest
//! pile up on it.
//!
//! Nothing in the tick loop is *wrong* here. Sense, decide, move, deposit all
//! behave exactly as specified; the swarm simply has no notion of "that cell
//! already has twenty ants on it". Two systems supply that notion:
//!
//! * [`tally_crowding`] builds [`CrowdGrid`] from the post-movement positions.
//! * [`relieve_crowding`] nudges an ant that is standing in an over-full bin
//!   toward the emptiest adjacent bin.
//!
//! # Why the nudge is positional and not a steering change
//!
//! Steering is where the ant's *intent* lives, and the intent - "follow the
//! food trail" - is correct even in a jam; that is the trail that leads to
//! food. Overriding it would break the mechanism the whole simulation is built
//! on, and would make a busy trail repel the ants that are supposed to reinforce
//! it. Crowding is a *physical* problem, so it gets a physical answer: ants do
//! not stand inside one another. Real ants in a bottleneck squeeze, jostle and
//! spread sideways, and that is exactly what a small positional push reproduces.

use bevy_ecs::prelude::*;

use crate::ant::{AntHeading, AntPos};
use crate::config::SimConfig;
use crate::crowd::CrowdGrid;
use crate::motion::wrap_angle;
use crate::obstacles::Obstacles;
use crate::rng::SimRng;

/// How far the relief push is scattered off the escape direction, in radians.
///
/// Without this the push is a *translation*, not a dispersal: every ant in the
/// bin computes the same escape direction and steps the same distance, so the
/// whole mass slides sideways as a block and lands in the next bin still
/// stacked on top of itself. Measured directly - a 200-ant point mass sat at
/// `max_bin = 200` for forty ticks without this, and unfolds normally with it.
///
/// It is also why the candidate list below falls back to the axis-aligned
/// halves of the push: in a one-cell-wide corridor the scattered direction is
/// usually wall, and the un-rotated component is the only legal move.
const JOSTLE_SPREAD: f32 = 0.9;

/// Count every ant into [`CrowdGrid`].
///
/// Runs straight after the movement systems, so the field describes where the
/// colony *is* this tick rather than where it was last tick.
pub fn tally_crowding(config: Res<SimConfig>, mut grid: ResMut<CrowdGrid>, ants: Query<&AntPos>) {
    if grid.width_mismatch(config.width, config.height) {
        grid.resize(config.width, config.height);
    }
    grid.rebuild(ants.iter().map(|pos| (pos.x, pos.y)));
}

/// Push ants out of over-full bins.
///
/// The push is a small distance toward the emptiest neighbouring bin, applied
/// only when the ant's own bin holds more than `crowd_threshold` ants and some
/// neighbour holds fewer.
///
/// # The threshold is soft
///
/// The push scales with how far over the threshold the bin is: a bin one ant
/// over gets one part in twenty-four of the step, a bin at twice the threshold
/// gets all of it. That matters because the two regimes have to coexist. A busy
/// trail on an open map peaks at 25 ants in a 4x4 bin - measured, over 1800
/// ticks - and a labyrinth at 45% wall density runs at a *mean* of 55 with
/// spikes past 120. A hard threshold has to sit between those, which leaves it
/// either too loose to catch a real jam or tight enough to shove ants off a
/// perfectly healthy trail. A soft one is negligible at 25 and full strength at
/// 55, so it does not have to choose.
///
/// Two details are load-bearing:
///
/// * The push is rejected when it would land in a wall. A bottleneck is
///   normally *made* of wall, so a relief rule that ignored obstacles would
///   shove half the colony into the rock.
/// * The ant's heading is turned to match the push. Moving an ant sideways
///   while it still believes it is walking north means the next tick's step
///   undoes the displacement, and the relief degenerates into a vibration.
pub fn relieve_crowding(
    config: Res<SimConfig>,
    obstacles: Res<Obstacles>,
    grid: Res<CrowdGrid>,
    mut rng: ResMut<SimRng>,
    mut ants: Query<(&mut AntPos, &mut AntHeading)>,
) {
    if config.crowd_relief <= 0.0 {
        return;
    }
    let threshold = config.crowd_threshold.max(1.0);
    // Max coordinates: `motion` treats the map edge as a wall, so the push must
    // respect it for the same reason it respects obstacles.
    let max_x = (config.width as f32 - 1.0).max(0.0);
    let max_y = (config.height as f32 - 1.0).max(0.0);

    for (mut pos, mut heading) in ants.iter_mut() {
        let crowded = grid.at(pos.x, pos.y) as f32;
        let over = crowded - threshold;
        if over <= 0.0 {
            continue;
        }
        let push = config.crowd_relief * (over / threshold).clamp(0.0, 1.0);
        let Some((ex, ey)) = grid.escape_direction(pos.x, pos.y) else {
            // Every neighbour is at least as crowded: the colony is spread
            // evenly and there is nothing to relieve.
            continue;
        };

        // Scatter the push so that ants sharing a bin fan out instead of
        // marching off together. Drawn before the candidate test, not after, so
        // the RNG stream does not depend on which candidate happened to be
        // legal.
        let jostle = rng.signed() * JOSTLE_SPREAD;
        let (sin, cos) = jostle.sin_cos();
        let (dx, dy) = (ex * cos - ey * sin, ex * sin + ey * cos);

        // Try the full push, then the two axis-aligned components of it, then
        // give up. Shrinking the step rather than rejecting it outright matters
        // at a bottleneck: the diagonal neighbour is usually wall, so a rule
        // that only accepted the exact escape direction would never fire there.
        let candidates = [
            (pos.x + dx * push, pos.y + dy * push),
            (pos.x + dx * push, pos.y),
            (pos.x, pos.y + dy * push),
        ];
        let target = candidates.into_iter().find(|(x, y)| {
            !obstacles.is_blocked_at(*x, *y)
                && (0.0..=max_x).contains(x)
                && (0.0..=max_y).contains(y)
        });
        let Some((nx, ny)) = target else {
            continue;
        };

        // Turn to face the displacement only if the ant was actually moved; a
        // zero-length push would otherwise snap its heading to +x.
        let (mx, my) = (nx - pos.x, ny - pos.y);
        if mx * mx + my * my > 1.0e-12 {
            pos.x = nx;
            pos.y = ny;
            heading.angle = wrap_angle(my.atan2(mx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crowd::CROWD_BIN;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    fn world(mut config: SimConfig, width: usize, height: usize) -> World {
        config.width = width;
        config.height = height;
        let mut world = World::new();
        world.insert_resource(CrowdGrid::new(width, height, CROWD_BIN));
        world.insert_resource(Obstacles::new(width, height));
        world.insert_resource(SimRng::new(1));
        world.insert_resource(config);
        world
    }

    fn open_world(config: SimConfig) -> World {
        world(config, 64, 64)
    }

    #[test]
    fn tally_counts_every_ant() {
        let mut world = open_world(SimConfig::default());
        for i in 0..10 {
            world.spawn(AntPos::new(10.0 + i as f32 * 0.1, 10.0));
        }
        world.spawn(AntPos::new(40.0, 40.0));
        world.run_system_once(tally_crowding).unwrap();

        let grid = world.resource::<CrowdGrid>();
        assert_eq!(grid.total, 11);
        assert_eq!(grid.max_bin, 10);
        assert_eq!(grid.occupied, 2);
    }

    /// The common case: nothing is crowded, so nothing is touched.
    #[test]
    fn a_normal_trail_is_left_alone() {
        let mut world = open_world(SimConfig {
            crowd_relief: 1.0,
            crowd_threshold: 24.0,
            ..Default::default()
        });
        // A realistic busy trail: 60 ants strung out along 120 cells.
        for i in 0..60 {
            world.spawn((
                AntPos::new(10.0 + i as f32 * 2.0, 30.0),
                AntHeading::new(0.0),
            ));
        }
        let before: Vec<(f32, f32)> = {
            let mut q = world.query::<&AntPos>();
            q.iter(&world).map(|p| (p.x, p.y)).collect()
        };

        world.run_system_once(tally_crowding).unwrap();
        world.run_system_once(relieve_crowding).unwrap();

        let mut q = world.query::<&AntPos>();
        let after: Vec<(f32, f32)> = q.iter(&world).map(|p| (p.x, p.y)).collect();
        assert_eq!(before, after, "a spread-out trail was disturbed");
    }

    /// The failure case: everything on one spot, with somewhere to go.
    ///
    /// This is the exact shape of the reported bug - "the ants all gather at a
    /// single point" - reduced to its smallest form. It also pins the reason
    /// the push is scattered by a per-ant jostle: without that, every ant in
    /// the bin computes the same escape direction and the mass translates
    /// sideways rather than unfolding, and this test sits at `max_bin = 200`
    /// for ever.
    #[test]
    fn a_point_mass_spreads_out() {
        let mut world = open_world(SimConfig {
            crowd_relief: 1.0,
            crowd_threshold: 24.0,
            ..Default::default()
        });
        for _ in 0..200 {
            world.spawn((AntPos::new(20.0, 20.0), AntHeading::new(0.0)));
        }

        world.run_system_once(tally_crowding).unwrap();
        let packed = world.resource::<CrowdGrid>().max_bin;
        assert!(packed > 24, "the fixture is not actually crowded: {packed}");

        // One push is bounded by `crowd_relief`; several ticks are what
        // actually unfolds the mass, so run the pair repeatedly.
        for _ in 0..60 {
            world.run_system_once(tally_crowding).unwrap();
            world.run_system_once(relieve_crowding).unwrap();
        }

        world.run_system_once(tally_crowding).unwrap();
        let grid = world.resource::<CrowdGrid>();
        assert!(
            grid.max_bin < packed / 2,
            "the point mass did not spread: {packed} -> {}",
            grid.max_bin
        );
        assert!(grid.occupied > 1, "the ants are still all in one bin");
    }

    /// The dispersal must be real, not a translation: the ants have to end up
    /// on many *different* positions, not the same new one.
    #[test]
    fn a_point_mass_does_not_just_translate() {
        let mut world = open_world(SimConfig {
            crowd_relief: 1.0,
            crowd_threshold: 24.0,
            ..Default::default()
        });
        for _ in 0..100 {
            world.spawn((AntPos::new(20.0, 20.0), AntHeading::new(0.0)));
        }

        for _ in 0..10 {
            world.run_system_once(tally_crowding).unwrap();
            world.run_system_once(relieve_crowding).unwrap();
        }

        let mut q = world.query::<&AntPos>();
        let mut distinct: Vec<(u32, u32)> = q
            .iter(&world)
            .map(|p| ((p.x * 16.0) as u32, (p.y * 16.0) as u32))
            .collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert!(
            distinct.len() > 20,
            "100 ants packed on one point occupy only {} distinct spots after 10 ticks",
            distinct.len()
        );
    }

    /// A relief push must never put an ant inside a wall - the very place a
    /// bottleneck forms.
    #[test]
    fn relief_never_pushes_an_ant_into_a_wall() {
        let mut world = open_world(SimConfig {
            crowd_relief: 1.0,
            crowd_threshold: 4.0,
            ..Default::default()
        });
        // A wall with a single-cell gap at (32, 20): the classic bottleneck.
        for y in 0..64 {
            if y != 20 {
                world.resource_mut::<Obstacles>().set(32, y, true);
            }
        }
        // A pile of ants on the gap and the cells beside it.
        for _ in 0..60 {
            world.spawn((AntPos::new(31.0, 20.0), AntHeading::new(0.0)));
        }

        for _ in 0..30 {
            world.run_system_once(tally_crowding).unwrap();
            world.run_system_once(relieve_crowding).unwrap();
        }

        let blocked = world.resource::<Obstacles>().clone();
        let mut q = world.query::<&AntPos>();
        for pos in q.iter(&world) {
            assert!(
                !blocked.is_blocked_at(pos.x, pos.y),
                "an ant was pushed into the wall at ({}, {})",
                pos.x,
                pos.y
            );
            assert!((0.0..64.0).contains(&pos.x) && (0.0..64.0).contains(&pos.y));
        }
    }

    /// Turning the knob off must restore the old behaviour exactly.
    #[test]
    fn zero_relief_is_a_no_op() {
        let mut world = open_world(SimConfig {
            crowd_relief: 0.0,
            ..Default::default()
        });
        for _ in 0..200 {
            world.spawn((AntPos::new(20.0, 20.0), AntHeading::new(0.0)));
        }
        world.run_system_once(tally_crowding).unwrap();
        world.run_system_once(relieve_crowding).unwrap();

        let mut q = world.query::<(&AntPos, &AntHeading)>();
        for (pos, heading) in q.iter(&world) {
            assert_eq!((pos.x, pos.y), (20.0, 20.0));
            assert_eq!(heading.angle, 0.0);
        }
    }

    #[test]
    fn the_grid_follows_a_map_resize() {
        let mut world = open_world(SimConfig::default());
        world.spawn(AntPos::new(10.0, 10.0));
        world.run_system_once(tally_crowding).unwrap();

        world.resource_mut::<SimConfig>().width = 128;
        world.resource_mut::<SimConfig>().height = 128;
        world.run_system_once(tally_crowding).unwrap();

        let grid = world.resource::<CrowdGrid>();
        assert_eq!((grid.bins_x, grid.bins_y), (32, 32));
        assert_eq!(grid.total, 1);
    }
}
