//! Stage 0 of an ant's tick: decay the world's memory.

use bevy_ecs::prelude::*;

use crate::config::SimConfig;
use crate::grid::PheromoneGrid;

/// Multiply every pheromone cell by `(1 - evaporation_rate)`.
///
/// Evaporation is what makes the colony *adaptive* rather than merely
/// persistent: a trail to a drained food pile fades because nobody reinforces
/// it, and the swarm re-explores. It is also the counterweight to deposition -
/// without it every cell would saturate at `max_pheromone` and the gradient,
/// which is the only information ants exchange, would vanish.
pub fn evaporate_pheromones(config: Res<SimConfig>, mut grid: ResMut<PheromoneGrid>) {
    grid.evaporate(config.evaporation_rate);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Pheromone;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    #[test]
    fn one_tick_applies_the_configured_rate() {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            evaporation_rate: 0.25,
            ..Default::default()
        });
        let mut grid = PheromoneGrid::new(8, 8, 1);
        grid.set(0, Pheromone::Food, 2, 2, 8.0);
        grid.set(0, Pheromone::Home, 3, 3, 8.0);
        world.insert_resource(grid);

        world.run_system_once(evaporate_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        assert!((grid.get(0, Pheromone::Food, 2, 2) - 6.0).abs() < 1e-5);
        assert!((grid.get(0, Pheromone::Home, 3, 3) - 6.0).abs() < 1e-5);
    }

    #[test]
    fn zero_rate_is_a_no_op() {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            evaporation_rate: 0.0,
            ..Default::default()
        });
        let mut grid = PheromoneGrid::new(8, 8, 1);
        grid.set(0, Pheromone::Food, 2, 2, 3.0);
        world.insert_resource(grid);
        world.run_system_once(evaporate_pheromones).unwrap();
        assert!(
            (world
                .resource::<PheromoneGrid>()
                .get(0, Pheromone::Food, 2, 2)
                - 3.0)
                .abs()
                < 1e-6
        );
    }

    /// One evaporation tick must reach every colony, not just the first.
    #[test]
    fn every_colony_decays_together() {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            evaporation_rate: 0.5,
            colony_count: 2,
            ..Default::default()
        });
        let mut grid = PheromoneGrid::new(8, 8, 2);
        for colony in 0..2 {
            grid.set(colony, Pheromone::Food, 1, 1, 8.0);
            grid.set(colony, Pheromone::Home, 5, 5, 8.0);
        }
        world.insert_resource(grid);

        world.run_system_once(evaporate_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        for colony in 0..2 {
            assert!((grid.get(colony, Pheromone::Food, 1, 1) - 4.0).abs() < 1e-5);
            assert!((grid.get(colony, Pheromone::Home, 5, 5) - 4.0).abs() < 1e-5);
        }
    }
}
