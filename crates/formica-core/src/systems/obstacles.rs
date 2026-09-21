//! Obstacle bookkeeping for the simulation side.

use bevy_ecs::prelude::*;

use crate::grid::{Pheromone, PheromoneGrid};
use crate::obstacles::Obstacles;

/// Erase any pheromone left standing inside a wall.
///
/// Walls can appear under an existing trail - the player draws them, and the
/// presets re-stamp them. A trail inside a wall is wrong twice over: ants
/// cannot reach it, so it can never be reinforced or followed, and the renderer
/// paints walls *over* the pheromone field, so it would be invisible anyway.
/// Erasing it keeps the drawn field and the simulated field the same thing.
pub fn scrub_obstacle_pheromones(obstacles: Res<Obstacles>, mut grid: ResMut<PheromoneGrid>) {
    if !obstacles.is_changed() {
        return;
    }
    for y in 0..obstacles.height as i32 {
        for x in 0..obstacles.width as i32 {
            if !obstacles.is_blocked(x, y) {
                continue;
            }
            for colony in 0..grid.colonies {
                grid.set(colony, Pheromone::Food, x, y, 0.0);
                grid.set(colony, Pheromone::Home, x, y, 0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    #[test]
    fn pheromone_inside_a_new_wall_is_erased_everywhere() {
        let mut world = World::new();
        let mut obstacles = Obstacles::new(16, 16);
        let mut grid = PheromoneGrid::new(16, 16, 2);
        grid.set(0, Pheromone::Food, 4, 4, 7.0);
        grid.set(0, Pheromone::Home, 4, 4, 7.0);
        grid.set(1, Pheromone::Food, 4, 4, 7.0);
        grid.set(1, Pheromone::Home, 4, 4, 7.0);
        // A cell that will stay open, to prove the scrub is targeted.
        grid.set(0, Pheromone::Food, 9, 9, 3.0);

        // Dirty the resource first so `is_changed` is already true, then let
        // change detection fire on the wall being added.
        world.insert_resource(obstacles.clone());
        world.insert_resource(grid);
        world.run_system_once(scrub_obstacle_pheromones).unwrap();

        obstacles.set(4, 4, true);
        world.insert_resource(obstacles);
        world.run_system_once(scrub_obstacle_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        for colony in 0..2 {
            assert_eq!(grid.get(colony, Pheromone::Food, 4, 4), 0.0);
            assert_eq!(grid.get(colony, Pheromone::Home, 4, 4), 0.0);
        }
        assert_eq!(
            grid.get(0, Pheromone::Food, 9, 9),
            3.0,
            "the scrub wiped an open cell"
        );
    }

    /// The scrub must be targeted: an open map has nothing to erase, and a map
    /// with one wall must not disturb the rest of the field.
    ///
    /// (An earlier version of this test tried to assert that the
    /// `is_changed` guard *skips* the sweep on an idle tick. That cannot be
    /// observed through `run_system_once`, which builds a fresh system each
    /// call and therefore always sees the resource as changed - the premise
    /// was wrong, not the code.)
    #[test]
    fn an_open_map_is_left_completely_alone() {
        let mut world = World::new();
        world.insert_resource(Obstacles::new(8, 8));
        let mut grid = PheromoneGrid::new(8, 8, 2);
        for colony in 0..2 {
            grid.set(colony, Pheromone::Food, 2, 2, 5.0);
            grid.set(colony, Pheromone::Home, 3, 3, 4.0);
        }
        world.insert_resource(grid);

        world.run_system_once(scrub_obstacle_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        for colony in 0..2 {
            assert_eq!(grid.get(colony, Pheromone::Food, 2, 2), 5.0);
            assert_eq!(grid.get(colony, Pheromone::Home, 3, 3), 4.0);
        }
    }

    #[test]
    fn the_scrub_touches_only_walled_cells() {
        let mut world = World::new();
        let mut obstacles = Obstacles::new(8, 8);
        obstacles.set(5, 5, true);
        world.insert_resource(obstacles);
        let mut grid = PheromoneGrid::new(8, 8, 2);
        for colony in 0..2 {
            grid.set(colony, Pheromone::Food, 5, 5, 9.0);
            grid.set(colony, Pheromone::Food, 5, 6, 9.0);
        }
        world.insert_resource(grid);

        world.run_system_once(scrub_obstacle_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        for colony in 0..2 {
            assert_eq!(grid.get(colony, Pheromone::Food, 5, 5), 0.0);
            assert_eq!(
                grid.get(colony, Pheromone::Food, 5, 6),
                9.0,
                "the scrub reached a cell that is not a wall"
            );
        }
    }
}
