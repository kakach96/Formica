//! Stage 4 of an ant's tick: leave a trail.
//!
//! This is the only place ants *write* to the world, and the rule is tiny:
//! a searching ant lays **home** pheromone (so it, or a friend, can find the
//! way back), a returning ant lays **food** pheromone (so others can find what
//! it found). No ant ever writes the trail it follows - and no ant ever writes
//! into the *other* colony's fields.

use bevy_ecs::prelude::*;

use crate::ant::{AntColony, AntPos, AntState};
use crate::config::SimConfig;
use crate::grid::{Deposit, Pheromone, PheromoneGrid};
use crate::obstacles::Obstacles;

/// Deposit pheromone under every ant.
pub fn deposit_pheromones(
    config: Res<SimConfig>,
    obstacles: Res<Obstacles>,
    mut grid: ResMut<PheromoneGrid>,
    ants: Query<(&AntColony, &AntState, &AntPos)>,
) {
    let amount = config.deposit_amount;
    let radius = config.deposit_radius;
    let cap = config.max_pheromone;

    for (colony, state, pos) in ants.iter() {
        // Never lay a trail inside a wall, and never let the deposit *kernel*
        // spill into one either: a 3x3 kernel reaches a cell further than the
        // ant stands, so an ant walking beside a wall would otherwise paint
        // pheromone onto it. That pheromone is unreachable, cannot be
        // reinforced, and is invisible in the renderer - it is pure noise in
        // the field and in the books.
        if obstacles.is_blocked_at(pos.x, pos.y) {
            continue;
        }
        let kind = match state {
            AntState::Searching => Pheromone::Home,
            AntState::Returning => Pheromone::Food,
        };
        grid.deposit_kernel(
            Deposit::new(
                colony.index(),
                kind,
                pos.x,
                pos.y,
                amount,
                radius,
                cap,
            ),
            |x, y| obstacles.is_blocked(x, y),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    fn world() -> World {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            deposit_radius: 0,
            ..Default::default()
        });
        world.insert_resource(Obstacles::new(16, 16));
        world.insert_resource(PheromoneGrid::new(16, 16, 2));
        world
    }

    #[test]
    fn searching_lays_home_and_returning_lays_food() {
        let mut world = world();
        world.spawn((AntColony(0), AntState::Searching, AntPos::new(4.0, 4.0)));
        world.spawn((AntColony(0), AntState::Returning, AntPos::new(9.0, 9.0)));

        world.run_system_once(deposit_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        assert!(
            grid.get(0, Pheromone::Home, 4, 4) > 0.0,
            "searcher laid no home trail"
        );
        assert_eq!(grid.get(0, Pheromone::Food, 4, 4), 0.0);
        assert!(
            grid.get(0, Pheromone::Food, 9, 9) > 0.0,
            "returner laid no food trail"
        );
        assert_eq!(grid.get(0, Pheromone::Home, 9, 9), 0.0);
    }

    #[test]
    fn deposits_land_only_in_the_ants_own_colony() {
        let mut world = world();
        world.spawn((AntColony(0), AntState::Searching, AntPos::new(4.0, 4.0)));
        world.spawn((AntColony(1), AntState::Searching, AntPos::new(9.0, 9.0)));

        world.run_system_once(deposit_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        assert!(grid.get(0, Pheromone::Home, 4, 4) > 0.0);
        assert_eq!(
            grid.get(1, Pheromone::Home, 4, 4),
            0.0,
            "colony 1 gained a trail from colony 0's ant"
        );
        assert!(grid.get(1, Pheromone::Home, 9, 9) > 0.0);
        assert_eq!(grid.get(0, Pheromone::Home, 9, 9), 0.0);
    }

    #[test]
    fn deposits_are_capped() {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            deposit_radius: 0,
            deposit_amount: 100.0,
            max_pheromone: 5.0,
            ..Default::default()
        });
        world.insert_resource(Obstacles::new(16, 16));
        world.insert_resource(PheromoneGrid::new(16, 16, 1));
        for _ in 0..10 {
            world.spawn((AntColony(0), AntState::Searching, AntPos::new(4.0, 4.0)));
        }
        world.run_system_once(deposit_pheromones).unwrap();
        let grid = world.resource::<PheromoneGrid>();
        assert!(grid.get(0, Pheromone::Home, 4, 4) <= 5.0 + 1e-5);
    }

    /// A wall is not a surface: no trail may be painted on it, **including** by
    /// the spread of a deposit kernel from an ant standing beside it.
    #[test]
    fn no_pheromone_is_laid_inside_a_wall() {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            deposit_radius: 1,
            ..Default::default()
        });
        world.insert_resource(Obstacles::new(16, 16));
        world.insert_resource(PheromoneGrid::new(16, 16, 2));

        // A wall column at x = 8.
        for y in 0..16 {
            world.resource_mut::<Obstacles>().set(8, y, true);
        }
        // An ant standing immediately to its left: its 3x3 kernel would reach
        // into the wall without the mask.
        world.spawn((AntColony(0), AntState::Searching, AntPos::new(7.0, 8.0)));
        // And one on the wall itself, which cannot happen normally but must not
        // paint either.
        world.spawn((AntColony(0), AntState::Returning, AntPos::new(8.0, 8.0)));

        world.run_system_once(deposit_pheromones).unwrap();

        let grid = world.resource::<PheromoneGrid>();
        for y in 0..16 {
            assert_eq!(
                grid.get(0, Pheromone::Food, 8, y),
                0.0,
                "food trail leaked into the wall at y = {y}"
            );
            assert_eq!(
                grid.get(0, Pheromone::Home, 8, y),
                0.0,
                "home trail leaked into the wall at y = {y}"
            );
        }
        // The ant beside the wall still laid a trail on the open side.
        assert!(
            grid.get(0, Pheromone::Home, 7, 8) > 0.0,
            "the mask swallowed the whole deposit"
        );
    }
}
