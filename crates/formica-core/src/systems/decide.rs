//! Stage 2 of an ant's tick: turn the sensor readings into a new heading.

use bevy_ecs::prelude::*;

use crate::ant::{wrap_angle, AntColony, AntHeading, AntPos, AntState, AntVigor, PheromoneSensors};
use crate::config::SimConfig;
use crate::rng::SimRng;
use crate::world::Nests;

/// Everything this system reads and writes about one ant.
///
/// `AntVigor` is optional so that a hand-built ant tuple - which every unit test
/// in this file uses - behaves as the population mean instead of failing to
/// match the query at all.
type Forager<'a> = (
    &'a AntColony,
    &'a AntState,
    &'a AntPos,
    &'a PheromoneSensors,
    &'a mut AntHeading,
    Option<&'a AntVigor>,
);

/// Steer every ant for this tick.
///
/// **Searching** ants do a correlated random walk (`heading += noise`) with a
/// bias toward whichever antenna smells the most food pheromone. That is the
/// entire "intelligence" of the colony: no ant knows where the food is, they
/// only know which way smells stronger, and the trail is a side effect of
/// other ants' success.
///
/// **Returning** ants steer straight at *their own* nest, clamped to
/// `max_turn_rate` so the turn looks like an ant rather than a teleport. The
/// brief is explicit that homing is direct; `home_pheromone_weight` is the knob
/// that instead blends in the home-pheromone gradient (default 0.0 = brief
/// behaviour).
///
/// Both branches scale the jitter by the ant's own [`AntVigor::wander`]. That
/// is the only per-individual difference in the whole simulation, and it is
/// load-bearing: without it every ant traces the same random walk and the
/// colony is a featureless expanding disc.
pub fn decide_heading(
    config: Res<SimConfig>,
    nests: Res<Nests>,
    mut rng: ResMut<SimRng>,
    mut ants: Query<Forager>,
) {
    let deadzone = config.pheromone_deadzone;

    for (colony, state, pos, sensors, mut heading, vigor) in ants.iter_mut() {
        // A hand-spawned ant with no `AntVigor` behaves as the population mean,
        // which keeps every unit test that builds a bare ant tuple valid.
        let wander = vigor.copied().unwrap_or_default().wander;

        // A trail is only worth following if it is actually above the noise
        // floor; otherwise the ant is doing a pure random walk.
        let sensed = sensors.strongest() > deadzone;
        let trail_turn = if sensed {
            sensors.preferred_side() * config.sensor_turn_gain
        } else {
            0.0
        };

        match state {
            AntState::Searching => {
                let jitter = rng.signed() * config.random_walk_strength * wander;
                heading.turn(trail_turn + jitter);
            }
            AntState::Returning => {
                let nest = nests.get(colony.index());
                let desired = (nest.y - pos.y).atan2(nest.x - pos.x);
                let mut steer = wrap_angle(desired - heading.angle)
                    .clamp(-config.max_turn_rate, config.max_turn_rate);
                if config.home_pheromone_weight > 0.0 {
                    steer = steer * (1.0 - config.home_pheromone_weight)
                        + trail_turn * config.home_pheromone_weight;
                }
                // Even a homing ant wanders a little, which is what keeps the
                // returning trail a corridor instead of a razor-thin line.
                let jitter = rng.signed() * config.random_walk_strength * 0.3 * wander;
                heading.turn(steer + jitter);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;
    use crate::world::Nest;

    fn world_with(config: SimConfig) -> World {
        let mut world = World::new();
        world.insert_resource(Nests::from_config(&config));
        world.insert_resource(config);
        world.insert_resource(SimRng::new(1));
        world
    }

    /// A hand-placed pair of nests, so the geometry in the assertions is exact.
    fn two_nest_world(config: SimConfig, a: Nest, b: Nest) -> World {
        let mut world = World::new();
        world.insert_resource(Nests(vec![a, b]));
        world.insert_resource(config);
        world.insert_resource(SimRng::new(1));
        world
    }

    #[test]
    fn returning_ant_turns_toward_the_nest() {
        let mut world = world_with(SimConfig {
            random_walk_strength: 0.0,
            // Larger than pi so the clamp does not bind and the assertion can
            // be about the *direction* rather than the turn budget.
            max_turn_rate: 3.5,
            ..Default::default()
        });
        let nest = *world.resource::<Nests>().get(0);
        // Sit due-east of the nest so the desired heading is exactly -x.
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(nest.x + 10.0, nest.y),
            PheromoneSensors::default(),
            AntHeading::new(0.0),
        ));
        world.run_system_once(decide_heading).unwrap();
        let mut q = world.query::<&AntHeading>();
        let angle = q.single(&world).unwrap().angle;
        assert!(
            (angle.abs() - std::f32::consts::PI).abs() < 1e-3,
            "expected to face pi, got {angle}"
        );
    }

    /// Each colony homes to its own nest, not to whichever one is nearest.
    #[test]
    fn each_colony_homes_to_its_own_nest() {
        let config = SimConfig {
            random_walk_strength: 0.0,
            max_turn_rate: 3.5,
            ..Default::default()
        };
        let mut world = two_nest_world(
            config,
            Nest {
                x: 10.0,
                y: 10.0,
                radius: 2.0,
            },
            Nest {
                x: 90.0,
                y: 10.0,
                radius: 2.0,
            },
        );
        // Both ants sit at (50,10), equidistant. Colony 0 must head west,
        // colony 1 east - a nearest-nest rule would send both the same way.
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(50.0, 10.0),
            PheromoneSensors::default(),
            AntHeading::new(0.0),
        ));
        world.spawn((
            AntColony(1),
            AntState::Returning,
            AntPos::new(50.0, 10.0),
            PheromoneSensors::default(),
            AntHeading::new(0.0),
        ));

        world.run_system_once(decide_heading).unwrap();

        let mut q = world.query::<(&AntColony, &AntHeading)>();
        for (colony, heading) in q.iter(&world) {
            let facing_east = heading.angle.cos() > 0.9;
            let facing_west = heading.angle.cos() < -0.9;
            match colony.index() {
                0 => assert!(facing_west, "colony 0 faces {}", heading.angle),
                _ => assert!(facing_east, "colony 1 faces {}", heading.angle),
            }
        }
    }

    #[test]
    fn homing_turn_is_limited_by_max_turn_rate() {
        let mut world = world_with(SimConfig {
            random_walk_strength: 0.0,
            max_turn_rate: 0.5,
            ..Default::default()
        });
        let nest = *world.resource::<Nests>().get(0);
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(nest.x + 10.0, nest.y),
            PheromoneSensors::default(),
            AntHeading::new(0.0),
        ));
        world.run_system_once(decide_heading).unwrap();
        let mut q = world.query::<&AntHeading>();
        let angle = q.single(&world).unwrap().angle;
        // The nest is at angle +pi, which wraps to -pi; either sign is the same
        // direction, so the turn budget is what matters.
        assert!(
            (angle.abs() - 0.5).abs() < 1e-5,
            "turn was not clamped to 0.5: {angle}"
        );
    }

    #[test]
    fn searching_ant_follows_the_stronger_antenna() {
        let mut world = world_with(SimConfig {
            random_walk_strength: 0.0,
            sensor_turn_gain: 0.5,
            pheromone_deadzone: 1e-4,
            ..Default::default()
        });
        world.spawn((
            AntColony(0),
            AntState::Searching,
            AntPos::new(0.0, 0.0),
            PheromoneSensors {
                left: 0.0,
                center: 0.0,
                right: 5.0,
            },
            AntHeading::new(0.0),
        ));
        world.run_system_once(decide_heading).unwrap();
        let mut q = world.query::<&AntHeading>();
        assert!(
            q.single(&world).unwrap().angle > 0.0,
            "should have turned right (positive)"
        );
    }

    #[test]
    fn below_the_deadzone_ants_just_wander() {
        let mut world = world_with(SimConfig {
            random_walk_strength: 0.0,
            pheromone_deadzone: 1.0,
            ..Default::default()
        });
        world.spawn((
            AntColony(0),
            AntState::Searching,
            AntPos::new(0.0, 0.0),
            PheromoneSensors {
                left: 0.0,
                center: 0.1,
                right: 0.5,
            },
            AntHeading::new(0.0),
        ));
        world.run_system_once(decide_heading).unwrap();
        let mut q = world.query::<&AntHeading>();
        assert_eq!(q.single(&world).unwrap().angle, 0.0);
    }

    /// Guards the documented default: returning ants ignore pheromone entirely.
    #[test]
    fn home_pheromone_weight_zero_ignores_the_home_trail() {
        let mut world = world_with(SimConfig {
            random_walk_strength: 0.0,
            max_turn_rate: 3.0,
            home_pheromone_weight: 0.0,
            ..Default::default()
        });
        let nest = *world.resource::<Nests>().get(0);
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(nest.x + 10.0, nest.y),
            PheromoneSensors {
                left: 100.0,
                center: 0.0,
                right: 0.0,
            },
            AntHeading::new(std::f32::consts::PI),
        ));
        world.run_system_once(decide_heading).unwrap();
        let mut q = world.query::<&AntHeading>();
        let angle = q.single(&world).unwrap().angle;
        assert!(
            angle.abs() > 3.0,
            "trail pulled the ant off course: {angle}"
        );
    }
}
