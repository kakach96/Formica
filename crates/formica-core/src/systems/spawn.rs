//! Ant population management, per colony.
//!
//! Keeps each colony's ant count in step with `SimConfig::ant_count`. In
//! Phase 1 this settles on the first tick and is then a single cheap count per
//! tick; interactively it is what makes the "colony size" slider live, what
//! replaces ants eaten by predators, and what removes a whole colony's ants
//! when the colony count is dropped back to one.
//!
//! The target is [`SimConfig::emerged_per_colony`] rather than `ant_count`
//! directly, which is how batched release works: with `spawn_rate > 0` the
//! target grows by `spawn_rate` per tick, so both the initial trickle out of
//! the nest and post-predation replacements draw from the same release
//! budget, and a colony that has not finished emerging has no spare ants to
//! hand out.

use bevy_ecs::prelude::*;

use crate::ant::{newborn_ant, AntColony, AntMarker};
use crate::config::SimConfig;
use crate::grid::MAX_COLONIES;
use crate::metrics::SimStats;
use crate::rng::SimRng;
use crate::world::Nests;

/// Keep the nest layout in step with the configured colony count.
///
/// The colony count is a live setting, and the nests are plain data built once
/// at startup. Without this, turning a second colony on at runtime would find
/// only one nest in `Nests` - and since `Nests::get` clamps rather than
/// panicking, both colonies would quietly nest in the *same place* and the
/// contest would be ruined in a way nothing else reports.
pub fn sync_nests(config: Res<SimConfig>, mut nests: ResMut<Nests>) {
    if nests.len() == config.colonies() {
        return;
    }
    *nests = Nests::from_config(&config);
}

/// Spawn or despawn ants until every colony matches the release schedule.
///
/// Any ant that is not counted toward a live colony's emerged quota is
/// surplus - including every ant of a colony that no longer exists - so
/// reducing the colony count cleans up after itself with no special case.
///
/// The tick comes from `SimStats` (updated after this system in the chain, so
/// `tick` here is "ticks completed"), which makes the emerged target a pure
/// function of the schedule - no batching state to reset anywhere.
pub fn sync_ant_count(
    config: Res<SimConfig>,
    nests: Res<Nests>,
    stats: Res<SimStats>,
    mut rng: ResMut<SimRng>,
    mut commands: Commands,
    ants: Query<(Entity, &AntColony), With<AntMarker>>,
) {
    let colonies = config.colonies();
    let per_colony = config.emerged_per_colony(stats.tick);
    let mut kept = [0usize; MAX_COLONIES];
    let mut surplus: Vec<Entity> = Vec::new();

    for (entity, colony) in ants.iter() {
        let index = colony.index();
        if index < colonies && kept[index] < per_colony && index < MAX_COLONIES {
            kept[index] += 1;
        } else {
            surplus.push(entity);
        }
    }

    for entity in surplus {
        commands.entity(entity).despawn();
    }

    for (index, already) in kept.iter().enumerate().take(colonies) {
        if *already >= per_colony {
            continue;
        }
        let nest = *nests.get(index);
        for _ in *already..per_colony {
            commands.spawn(newborn_ant(AntColony(index), &nest, &mut rng, &config));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ant::{AntPos, AntState};
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    fn setup(config: SimConfig) -> World {
        let nests = Nests::from_config(&config);
        let mut world = World::new();
        world.insert_resource(nests);
        world.insert_resource(SimRng::new(1));
        world.insert_resource(config);
        // The release schedule reads the tick counter out of the stats.
        world.insert_resource(SimStats::default());
        world
    }

    fn snapshot(world: &mut World) -> (usize, usize) {
        let mut q = world.query::<&AntMarker>();
        let total = q.iter(world).count();
        let mut c = world.query::<&AntColony>();
        let second = c.iter(world).filter(|a| a.index() == 1).count();
        (total, second)
    }

    #[test]
    fn population_grows_to_the_configured_size() {
        let mut world = setup(SimConfig {
            ant_count: 50,
            ..Default::default()
        });
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (50, 0));
        // A second run must be a no-op, not an overshoot.
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (50, 0));
    }

    #[test]
    fn two_colonies_get_their_own_equal_populations() {
        let mut world = setup(SimConfig {
            ant_count: 40,
            colony_count: 2,
            ..Default::default()
        });
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (80, 40));
    }

    #[test]
    fn population_shrinks_when_the_slider_goes_down() {
        let mut world = setup(SimConfig {
            ant_count: 40,
            ..Default::default()
        });
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (40, 0));

        world.resource_mut::<SimConfig>().ant_count = 10;
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (10, 0));
    }

    /// Dropping back to one colony must disband the other entirely.
    #[test]
    fn dropping_the_colony_count_removes_its_ants() {
        let mut world = setup(SimConfig {
            ant_count: 30,
            colony_count: 2,
            ..Default::default()
        });
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (60, 30));

        world.resource_mut::<SimConfig>().colony_count = 1;
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (30, 0), "colony 1 was not disbanded");
    }

    #[test]
    fn replenishment_spawns_at_the_right_nest() {
        let config = SimConfig {
            ant_count: 6,
            colony_count: 2,
            ..Default::default()
        };
        let mut world = setup(config.clone());
        world.run_system_once(sync_ant_count).unwrap();

        let nests = Nests::from_config(&config);
        let mut q = world.query::<(&AntColony, &AntPos, &AntState)>();
        let mut seen = [0usize; 2];
        for (colony, pos, state) in q.iter(&world) {
            assert_eq!(*state, AntState::Searching);
            let nest = nests.get(colony.index());
            let d = nest.distance_sq(pos.x, pos.y).sqrt();
            assert!(d <= config.spawn_radius + 1e-3, "spawned {d} from its nest");
            seen[colony.index()] += 1;
        }
        assert_eq!(seen, [6, 6]);
    }

    /// Batched release: the population starts at `spawn_initial` and grows by
    /// `spawn_rate` per tick until it reaches `ant_count`, then stays put.
    /// The control arm (rate 0) still settles on the first run.
    #[test]
    fn the_population_arrives_at_the_release_rate() {
        let mut world = setup(SimConfig {
            ant_count: 100,
            spawn_rate: 10,
            spawn_initial: 20,
            ..Default::default()
        });
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(
            snapshot(&mut world),
            (20, 0),
            "tick 0 released more than the initial batch"
        );

        world.resource_mut::<SimStats>().tick = 1;
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (30, 0));

        world.resource_mut::<SimStats>().tick = 7;
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (90, 0));

        // Fully emerged: further runs are a no-op, not an overshoot.
        world.resource_mut::<SimStats>().tick = 8;
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (100, 0));
        world.resource_mut::<SimStats>().tick = 100;
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (100, 0));
    }

    /// Replacements for eaten ants also draw from the release budget: a
    /// colony mid-emergence tops out at the emerged target, not the full
    /// configuration.
    #[test]
    fn replacements_cannot_exceed_the_emerged_budget() {
        let mut world = setup(SimConfig {
            ant_count: 50,
            spawn_rate: 10,
            spawn_initial: 5,
            ..Default::default()
        });
        world.resource_mut::<SimStats>().tick = 2;
        world.run_system_once(sync_ant_count).unwrap();
        assert_eq!(snapshot(&mut world), (25, 0));
    }
}
