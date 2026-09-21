//! Stage 1 of an ant's tick: look at the world through three antennae.

use bevy_ecs::prelude::*;

use crate::ant::{AntColony, AntHeading, AntPos, AntState, PheromoneSensors};
use crate::config::SimConfig;
use crate::grid::{Pheromone, PheromoneGrid};

/// Sample the pheromone field at three points ahead of each ant.
///
/// Which field is sampled depends on two things: what the ant is doing (a
/// searching ant cares about the *food* trail, a returning ant about the *home*
/// trail) and which colony it belongs to. The colony half matters as much as
/// the state half - two colonies sharing a map cannot smell each other, which
/// is the entire difference between cooperation and competition.
///
/// Each sample averages a small box rather than reading a single cell. That
/// matters: with a 2-cell-per-tick stride and a 3-cell antenna an ant would
/// otherwise step straight over its own trail, and a single-cell trail would
/// read as noise. Box averaging turns the discrete grid into something an ant
/// can actually follow.
pub fn sense_pheromones(
    config: Res<SimConfig>,
    grid: Res<PheromoneGrid>,
    mut ants: Query<(
        &AntColony,
        &AntState,
        &AntPos,
        &AntHeading,
        &mut PheromoneSensors,
    )>,
) {
    let distance = config.sensor_distance;
    let half_angle = config.sensor_angle;
    let box_radius = config.sensor_box_radius;

    for (colony, state, pos, heading, mut sensors) in ants.iter_mut() {
        let kind = match state {
            AntState::Searching => Pheromone::Food,
            AntState::Returning => Pheromone::Home,
        };
        let colony = colony.index();
        let h = heading.angle;
        let (lx, ly) = ((h - half_angle).cos(), (h - half_angle).sin());
        let (cx, cy) = (h.cos(), h.sin());
        let (rx, ry) = ((h + half_angle).cos(), (h + half_angle).sin());

        sensors.left =
            grid.sample_box(colony, kind, pos.x + lx * distance, pos.y + ly * distance, box_radius);
        sensors.center =
            grid.sample_box(colony, kind, pos.x + cx * distance, pos.y + cy * distance, box_radius);
        sensors.right =
            grid.sample_box(colony, kind, pos.x + rx * distance, pos.y + ry * distance, box_radius);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    fn world_with_grid() -> World {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            sensor_distance: 0.0,
            sensor_box_radius: 0,
            ..Default::default()
        });
        let mut grid = PheromoneGrid::new(16, 16, 2);
        // Colony 0 has a food trail and a home trail at (5,5).
        grid.set(0, Pheromone::Food, 5, 5, 9.0);
        grid.set(0, Pheromone::Home, 5, 5, 4.0);
        // Colony 1 put a much stronger trail in the *same* cell.
        grid.set(1, Pheromone::Food, 5, 5, 100.0);
        world.insert_resource(grid);
        world
    }

    #[test]
    fn searching_ants_read_food_pheromone_and_returning_ants_read_home() {
        let mut world = world_with_grid();
        world.spawn((
            AntColony(0),
            AntState::Searching,
            AntPos::new(5.0, 5.0),
            AntHeading::new(0.0),
            PheromoneSensors::default(),
        ));
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(5.0, 5.0),
            AntHeading::new(0.0),
            PheromoneSensors::default(),
        ));

        world.run_system_once(sense_pheromones).unwrap();

        let mut q = world.query::<(&AntState, &PheromoneSensors)>();
        for (state, sensors) in q.iter(&world) {
            match state {
                AntState::Searching => assert_eq!(sensors.center, 9.0),
                AntState::Returning => assert_eq!(sensors.center, 4.0),
            }
        }
    }

    /// The competition rule, tested directly: an ant standing exactly on top of
    /// another colony's enormous trail must read zero.
    #[test]
    fn an_ant_cannot_smell_another_colonys_trail() {
        let mut world = world_with_grid();
        world.spawn((
            AntColony(1),
            AntState::Searching,
            AntPos::new(5.0, 5.0),
            AntHeading::new(0.0),
            PheromoneSensors::default(),
        ));

        world.run_system_once(sense_pheromones).unwrap();

        let mut q = world.query::<&PheromoneSensors>();
        let sensors = q.single(&world).unwrap();
        assert_eq!(
            sensors.center, 100.0,
            "colony 1 should read its own trail, not colony 0's"
        );
        assert_eq!(sensors.left, 100.0);
        assert_eq!(sensors.right, 100.0);

        // And colony 0 sees only its own weaker trail.
        let mut world = world_with_grid();
        world.spawn((
            AntColony(0),
            AntState::Searching,
            AntPos::new(5.0, 5.0),
            AntHeading::new(0.0),
            PheromoneSensors::default(),
        ));
        world.run_system_once(sense_pheromones).unwrap();
        let mut q = world.query::<&PheromoneSensors>();
        assert_eq!(q.single(&world).unwrap().center, 9.0);
    }
}
