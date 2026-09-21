//! Assembly: the `SimulationTick` schedule, world bootstrap and reset.
//!
//! The simulation is *not* wired into `Update` or `FixedUpdate`. It lives in
//! its own schedule, [`SimulationTick`], which means:
//!
//! * the headless runner can execute exactly N ticks with zero engine
//!   overhead and no wall-clock dependence (`world.run_schedule`), and
//! * the rendered app can execute N ticks per frame, so "fast forward" is a
//!   counter rather than a change to the physics.
//!
//! Same schedule, same result, both ways - which is what makes the Phase 1
//! numbers a valid prediction of what Phase 2 will draw.

use bevy_app::{App, Plugin};
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ScheduleLabel;

use crate::ant::{newborn_ant, AntColony, AntMarker, Carrying};
use crate::config::SimConfig;
use crate::crowd::{CrowdGrid, CROWD_BIN};
use crate::grid::{PheromoneGrid, MAX_COLONIES};
use crate::metrics::SimStats;
use crate::obstacles::{sync_obstacles, Obstacles};
use crate::rng::SimRng;
use crate::systems::{
    decide_heading, deliver_food, deposit_pheromones, evaporate_pheromones, move_ants,
    move_predators, pickup_food, predators_catch_ants, relieve_crowding, scrub_obstacle_pheromones,
    sense_pheromones, sync_ant_count, sync_nests, sync_predator_count, tally_crowding, update_stats,
};
use crate::world::{FoodField, Nests};

/// Schedule containing exactly one tick of the simulation.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SimulationTick;

/// Installs the headless simulation: resources, the ant population and the
/// [`SimulationTick`] schedule.
#[derive(Debug, Clone, Default)]
pub struct SimPlugin {
    /// Configuration to start from. It is sanitized before use.
    pub config: SimConfig,
}

impl SimPlugin {
    /// Build the plugin with an explicit configuration.
    pub fn new(config: SimConfig) -> Self {
        Self { config }
    }
}

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        let mut config = self.config.clone();
        config.sanitize();
        app.insert_resource(config);

        init_simulation(app.world_mut());

        app.init_schedule(SimulationTick);
        app.add_systems(
            SimulationTick,
            (
                sync_nests,
                sync_obstacles,
                scrub_obstacle_pheromones,
                sync_predator_count,
                evaporate_pheromones,
                sense_pheromones,
                decide_heading,
                move_ants,
                move_predators,
                // Order matters: the tally must see final positions, and the
                // relief must land before the trail is laid so a pushed ant
                // writes pheromone where it actually ended up.
                tally_crowding,
                relieve_crowding,
                deposit_pheromones,
                pickup_food,
                deliver_food,
                predators_catch_ants,
                // After predation, not before: this way every victim is
                // replaced within the same tick it died, so the population at
                // a tick boundary is always exactly what the configuration
                // asks for. Running it earlier leaves the colony short by
                // however many ants the predators ate, which is invisible in
                // the totals but makes "population == config" unassertable.
                sync_ant_count,
                update_stats,
            )
                .chain(),
        );
    }
}

/// Create every resource the simulation needs and spawn the initial colony.
///
/// Requires [`SimConfig`] to already be present in `world`.
pub fn init_simulation(world: &mut World) {
    let config = world.resource::<SimConfig>().clone();
    let nests = Nests::from_config(&config);
    let mut rng = SimRng::new(config.seed);
    let food = FoodField::generate(&config, &mut rng);
    let initial_food = food.initial_total;

    for index in 0..config.colonies() {
        let nest = *nests.get(index);
        for _ in 0..config.ant_count {
            let ant = newborn_ant(AntColony(index), &nest, &mut rng, &config);
            world.spawn(ant);
        }
    }

    world.insert_resource(nests);
    world.insert_resource(food);
    world.insert_resource(rng);
    world.insert_resource(PheromoneGrid::new(
        config.width,
        config.height,
        MAX_COLONIES,
    ));
    world.insert_resource(Obstacles::new(config.width, config.height));
    world.insert_resource(CrowdGrid::new(config.width, config.height, CROWD_BIN));
    world.insert_resource(SimStats {
        food_initial: initial_food,
        food_remaining: initial_food,
        ..Default::default()
    });
}

/// Rebuild the world from scratch from the current [`SimConfig`].
///
/// Used by the app's "reset" button and whenever the map is resized: fresh food
/// piles, an empty pheromone grid, zeroed statistics and a new colony standing
/// on each nest.
pub fn reset_simulation(world: &mut World) {
    let ants: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<AntMarker>>();
        query.iter(world).collect()
    };
    for entity in ants {
        let _ = world.despawn(entity);
    }
    let predators: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<crate::predator::PredatorMarker>>();
        query.iter(world).collect()
    };
    for entity in predators {
        let _ = world.despawn(entity);
    }

    let config = world.resource::<SimConfig>().clone();
    let nests = Nests::from_config(&config);
    let mut rng = SimRng::new(config.seed);
    let food = FoodField::generate(&config, &mut rng);
    let initial_food = food.initial_total;

    for index in 0..config.colonies() {
        let nest = *nests.get(index);
        for _ in 0..config.ant_count {
            let ant = newborn_ant(AntColony(index), &nest, &mut rng, &config);
            world.spawn(ant);
        }
    }

    world.insert_resource(nests);
    world.insert_resource(food);
    world.insert_resource(rng);
    world.insert_resource(PheromoneGrid::new(
        config.width,
        config.height,
        MAX_COLONIES,
    ));
    // A fresh obstacle map: `sync_obstacles` re-stamps the preset on the next
    // tick, and the revision bump makes the renderer and the pheromone scrubber
    // notice.
    world.insert_resource(Obstacles::new(config.width, config.height));
    world.insert_resource(CrowdGrid::new(config.width, config.height, CROWD_BIN));
    world.insert_resource(SimStats {
        food_initial: initial_food,
        food_remaining: initial_food,
        ..Default::default()
    });
}

/// A headless [`App`] with the simulation installed and ready to tick.
pub fn headless_app(config: SimConfig) -> App {
    let mut app = App::new();
    app.add_plugins(SimPlugin::new(config));
    app
}

/// Food currently held by ants: picked up, not yet delivered.
///
/// The third term in the conservation law. A run that ends with ants still in
/// the field is not "missing" food - this is where it is.
pub fn food_in_transit(world: &mut World) -> f64 {
    let mut query = world.query::<&Carrying>();
    query.iter(world).map(|cargo| cargo.amount as f64).sum()
}

/// Advance a headless app by exactly `ticks` simulation ticks.
///
/// Deterministic and wall-clock independent: nothing here consults `Time`.
pub fn run_ticks(app: &mut App, ticks: u64) {
    for _ in 0..ticks {
        app.world_mut().run_schedule(SimulationTick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ant::{AntHeading, AntPos, AntState};
    use crate::grid::Pheromone;
    use crate::predator::PredatorMarker;

    #[test]
    fn plugin_populates_the_world() {
        let mut app = headless_app(SimConfig {
            ant_count: 25,
            ..Default::default()
        });
        {
            let world = app.world();
            let grid = world.resource::<PheromoneGrid>();
            assert_eq!(grid.width, 256);
            assert_eq!(grid.height, 256);
            // Capacity, not the active colony count: see `PheromoneGrid`.
            assert_eq!(grid.colonies, MAX_COLONIES);
            assert_eq!(world.resource::<FoodField>().piles.len(), 3);
            assert_eq!(world.resource::<Nests>().len(), 1);
            assert_eq!(world.resource::<Obstacles>().count(), 0);
            assert_eq!(
                world.resource::<SimStats>().food_initial,
                world.resource::<FoodField>().initial_total
            );
        }
        let mut q = app.world_mut().query::<&AntState>();
        assert_eq!(q.iter(app.world()).count(), 25);
    }

    #[test]
    fn two_colonies_get_two_nests_and_two_field_sets() {
        let mut app = headless_app(SimConfig {
            ant_count: 20,
            colony_count: 2,
            ..Default::default()
        });
        {
            let world = app.world();
            assert_eq!(world.resource::<Nests>().len(), 2);
            assert_eq!(world.resource::<PheromoneGrid>().colonies, 2);
        }
        let mut q = app.world_mut().query::<&AntColony>();
        assert_eq!(q.iter(app.world()).count(), 40);
    }

    #[test]
    fn ticks_advance_the_clock() {
        let mut app = headless_app(SimConfig {
            ant_count: 5,
            ..Default::default()
        });
        run_ticks(&mut app, 37);
        assert_eq!(app.world().resource::<SimStats>().tick, 37);
    }

    #[test]
    fn simulation_is_deterministic_for_a_seed() {
        let config = SimConfig {
            ant_count: 40,
            width: 96,
            height: 96,
            ..Default::default()
        };
        let mut a = headless_app(config.clone());
        let mut b = headless_app(config);
        run_ticks(&mut a, 400);
        run_ticks(&mut b, 400);

        {
            let sa = a.world().resource::<SimStats>();
            let sb = b.world().resource::<SimStats>();
            assert_eq!(sa.delivered_total, sb.delivered_total);
            assert_eq!(sa.deliveries, sb.deliveries);
            assert_eq!(sa.samples.len(), sb.samples.len());
        }

        // Determinism has to hold per-ant, not just in aggregate.
        let positions = |app: &mut App| -> Vec<(f32, f32, f32)> {
            let mut q = app.world_mut().query::<(&AntPos, &AntHeading)>();
            let mut v: Vec<(f32, f32, f32)> = q
                .iter(app.world())
                .map(|(p, h)| (p.x, p.y, h.angle))
                .collect();
            v.sort_by(|l, r| l.partial_cmp(r).unwrap());
            v
        };
        assert_eq!(positions(&mut a), positions(&mut b));
    }

    #[test]
    fn different_seeds_diverge() {
        let base = SimConfig {
            ant_count: 40,
            width: 96,
            height: 96,
            ..Default::default()
        };
        let mut a = headless_app(base.clone());
        let mut b = headless_app(SimConfig {
            seed: base.seed ^ 0xDEAD_BEEF,
            ..base
        });
        run_ticks(&mut a, 300);
        run_ticks(&mut b, 300);
        let sa = a.world().resource::<SimStats>();
        let sb = b.world().resource::<SimStats>();
        assert!(
            sa.delivered_total != sb.delivered_total || sa.deliveries != sb.deliveries,
            "two different seeds produced identical results"
        );
    }

    #[test]
    fn reset_restores_the_initial_state() {
        let config = SimConfig {
            ant_count: 30,
            width: 96,
            height: 96,
            predator_count: 3,
            ..Default::default()
        };
        let mut app = headless_app(config);
        run_ticks(&mut app, 300);
        assert_eq!(app.world().resource::<SimStats>().tick, 300);

        reset_simulation(app.world_mut());

        {
            let world = app.world();
            let stats = world.resource::<SimStats>();
            assert_eq!(stats.tick, 0);
            assert_eq!(stats.delivered_total, 0.0);
            assert_eq!(stats.ants_eaten, 0);
            assert!(stats.samples.is_empty());
            assert_eq!(
                world.resource::<PheromoneGrid>().total_all(Pheromone::Food),
                0.0
            );
            assert_eq!(world.resource::<Obstacles>().count(), 0);
        }
        // Both populations come back.
        let mut ants = app.world_mut().query::<&AntPos>();
        assert_eq!(ants.iter(app.world()).count(), 30);
        let mut predators = app.world_mut().query::<&PredatorMarker>();
        assert_eq!(predators.iter(app.world()).count(), 0);
    }

    /// A reset must pick up a changed map size, however the size got there.
    #[test]
    fn reset_follows_a_changed_map_size() {
        let mut app = headless_app(SimConfig {
            ant_count: 10,
            ..Default::default()
        });
        app.world_mut().resource_mut::<SimConfig>().width = 64;
        app.world_mut().resource_mut::<SimConfig>().height = 48;

        reset_simulation(app.world_mut());

        let grid = app.world().resource::<PheromoneGrid>();
        assert_eq!((grid.width, grid.height), (64, 48));
        let obstacles = app.world().resource::<Obstacles>();
        assert_eq!((obstacles.width, obstacles.height), (64, 48));
        let food = app.world().resource::<FoodField>();
        assert_eq!((food.width, food.height), (64, 48));
    }
}
