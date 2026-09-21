//! Stage 6 of an ant's tick: bookkeeping.
//!
//! Kept last and kept dumb. It reads the world, never writes it, so it can be
//! dropped from a run without changing the simulation by a single bit.

use bevy_ecs::prelude::*;

use crate::ant::AntState;
use crate::config::SimConfig;
use crate::grid::{Pheromone, PheromoneGrid};
use crate::metrics::SimStats;
use crate::world::FoodField;

/// Refresh counters and, every `sample_interval` ticks, record a curve point.
pub fn update_stats(
    config: Res<SimConfig>,
    food: Res<FoodField>,
    grid: Res<PheromoneGrid>,
    mut stats: ResMut<SimStats>,
    ants: Query<&AntState>,
) {
    stats.tick += 1;

    let mut searching = 0u32;
    let mut returning = 0u32;
    for state in ants.iter() {
        if state.is_returning() {
            returning += 1;
        } else {
            searching += 1;
        }
    }
    stats.searching = searching;
    stats.returning = returning;
    stats.food_remaining = food.remaining_total;
    stats.food_initial = food.initial_total;
    stats.food_removed = food.removed_total;

    if stats.tick.is_multiple_of(config.sample_interval) {
        // Summing every cell of every field is far more expensive than the rest
        // of this system, so only pay for it on the ticks that store it.
        stats.food_pheromone_total = grid.total_all(Pheromone::Food);
        stats.home_pheromone_total = grid.total_all(Pheromone::Home);
        stats.push_sample(config.sample_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::FoodPile;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            sample_interval: 2,
            colony_count: 2,
            ..Default::default()
        });
        world.insert_resource(FoodField::from_piles(
            8,
            8,
            vec![FoodPile {
                x: 4.0,
                y: 4.0,
                radius: 1.0,
                initial: 20.0,
            }],
        ));
        world.insert_resource(PheromoneGrid::new(8, 8, 2));
        world.insert_resource(SimStats::default());
        world
    }

    #[test]
    fn tick_advances_and_splits_the_population_by_state() {
        let mut world = world();
        world.spawn(AntState::Searching);
        world.spawn(AntState::Searching);
        world.spawn(AntState::Returning);

        world.run_system_once(update_stats).unwrap();

        let stats = world.resource::<SimStats>();
        assert_eq!(stats.tick, 1);
        assert_eq!(stats.searching, 2);
        assert_eq!(stats.returning, 1);
        assert_eq!(stats.food_remaining, 20.0);
        assert_eq!(stats.food_initial, 20.0);
        assert!(stats.samples.is_empty());
    }

    #[test]
    fn sampling_happens_on_the_interval() {
        let mut world = world();
        world.spawn(AntState::Searching);
        for _ in 0..4 {
            world.run_system_once(update_stats).unwrap();
        }
        let stats = world.resource::<SimStats>();
        assert_eq!(stats.tick, 4);
        assert_eq!(stats.samples.len(), 2, "expected samples at ticks 2 and 4");
        assert_eq!(stats.samples[0].tick, 2);
        assert_eq!(stats.samples[1].tick, 4);
    }

    #[test]
    fn stats_are_read_only_with_respect_to_the_world() {
        let mut world = world();
        world.spawn(AntState::Searching);
        let before = world.resource::<FoodField>().remaining_total;
        world.run_system_once(update_stats).unwrap();
        assert_eq!(world.resource::<FoodField>().remaining_total, before);
        assert_eq!(
            world.resource::<PheromoneGrid>().total_all(Pheromone::Food),
            0.0
        );
    }

    #[test]
    fn pheromone_totals_cover_every_colony() {
        let mut world = world();
        world.resource_mut::<PheromoneGrid>().set(0, Pheromone::Food, 1, 1, 3.0);
        world.resource_mut::<PheromoneGrid>().set(1, Pheromone::Food, 2, 2, 4.0);
        world.spawn(AntState::Searching);

        for _ in 0..2 {
            world.run_system_once(update_stats).unwrap();
        }

        let stats = world.resource::<SimStats>();
        assert_eq!(stats.samples.len(), 1);
        assert_eq!(
            stats.samples[0].food_pheromone, 7.0,
            "colony 1's trail was left out of the total"
        );
    }
}
