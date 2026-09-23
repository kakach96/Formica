//! Predator systems: keep the population right, hunt, and eat.

use std::cmp::Ordering;

use bevy_ecs::prelude::*;

use crate::ant::{AntColony, AntMarker, AntPos};
use crate::config::SimConfig;
use crate::metrics::SimStats;
use crate::motion;
use crate::obstacles::Obstacles;
use crate::predator::{
    newborn_predator, PredatorAlert, PredatorHeading, PredatorMarker, PredatorPos,
};
use crate::rng::SimRng;
use crate::world::Nests;

/// How far from every nest a predator is allowed to spawn.
const SPAWN_CLEARANCE: f32 = 45.0;

/// Spawn or despawn predators until the population matches the config.
pub fn sync_predator_count(
    config: Res<SimConfig>,
    nests: Res<Nests>,
    mut rng: ResMut<SimRng>,
    mut commands: Commands,
    predators: Query<Entity, With<PredatorMarker>>,
) {
    let target = config.predator_count;
    let current = predators.iter().count();

    match current.cmp(&target) {
        Ordering::Equal => {}
        Ordering::Less => {
            for _ in 0..(target - current) {
                commands.spawn(newborn_predator(
                    &mut rng,
                    config.width,
                    config.height,
                    &nests,
                    SPAWN_CLEARANCE,
                ));
            }
        }
        Ordering::Greater => {
            for entity in predators.iter().take(current - target) {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Steer each predator at the nearest ant it can see, then move it.
///
/// The search is a linear scan over every ant, which is `predators × ants` per
/// tick - 5 × 500 in the default configuration, i.e. nothing next to the
/// 131 072-cell evaporation sweep. A spatial index would be premature here.
pub fn move_predators(
    config: Res<SimConfig>,
    obstacles: Res<Obstacles>,
    mut rng: ResMut<SimRng>,
    ants: Query<&AntPos, With<AntMarker>>,
    mut predators: Query<(&mut PredatorPos, &mut PredatorHeading, &mut PredatorAlert)>,
) {
    if config.predator_count == 0 {
        return;
    }
    let sense_sq = config.predator_sense * config.predator_sense;
    let field = motion::MotionField {
        width: config.width,
        height: config.height,
        obstacles: &obstacles,
    };

    for (mut pos, mut heading, mut alert) in predators.iter_mut() {
        let mut nearest: Option<(f32, f32, f32)> = None;
        for ant in ants.iter() {
            let (dx, dy) = (ant.x - pos.x, ant.y - pos.y);
            let d2 = dx * dx + dy * dy;
            if d2 <= sense_sq && nearest.is_none_or(|best| d2 < best.0) {
                nearest = Some((d2, dx, dy));
            }
        }

        match nearest {
            Some((_, dx, dy)) => {
                let desired = dy.atan2(dx);
                let steer = motion::wrap_angle(desired - heading.angle)
                    .clamp(-config.predator_turn_rate, config.predator_turn_rate);
                heading.angle = motion::wrap_angle(heading.angle + steer);
                alert.0 = true;
            }
            None => {
                // Nothing in sight: a lazy random walk, so predators sweep new
                // ground instead of orbiting one spot.
                heading.angle =
                    motion::wrap_angle(heading.angle + rng.signed() * config.predator_wander);
                alert.0 = false;
            }
        }

        let result = motion::step(
            pos.x,
            pos.y,
            heading.angle,
            config.predator_speed,
            field,
            &mut rng,
        );
        pos.x = result.x;
        pos.y = result.y;
        heading.angle = result.angle;
    }
}

/// Eat every ant standing too close to a predator.
///
/// The victim is simply despawned. It is **not** gone for good: the colony's
/// population is a configuration value, so `sync_ant_count` spawns a
/// replacement at the nest on the next tick. Predation therefore costs the
/// colony time and cargo rather than permanently shrinking it, which keeps a
/// predator run comparable with a predator-free one.
///
/// A nest is a refuge - ants standing inside their own nest are immune. Without
/// that rule a predator that follows the trail home parks on the nest and eats
/// each replacement on the tick it appears, which is not predation, it is a
/// black hole.
pub fn predators_catch_ants(
    config: Res<SimConfig>,
    nests: Res<Nests>,
    mut commands: Commands,
    mut stats: ResMut<SimStats>,
    predators: Query<&PredatorPos, With<PredatorMarker>>,
    ants: Query<(Entity, &AntColony, &AntPos), With<AntMarker>>,
) {
    if config.predator_count == 0 {
        return;
    }
    let catch_sq = config.predator_catch_radius * config.predator_catch_radius;
    let mut eaten = 0u64;

    for (entity, colony, ant) in ants.iter() {
        if nests.get(colony.index()).contains(ant.x, ant.y) {
            continue;
        }
        let caught = predators.iter().any(|predator| {
            (predator.x - ant.x).powi(2) + (predator.y - ant.y).powi(2) <= catch_sq
        });
        if caught {
            commands.entity(entity).despawn();
            eaten += 1;
        }
    }

    stats.ants_eaten += eaten;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ant::AntState;
    use crate::predator::PredatorMarker;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    fn base_world(config: SimConfig) -> World {
        let mut world = World::new();
        world.insert_resource(Nests::from_config(&config));
        world.insert_resource(Obstacles::new(config.width, config.height));
        world.insert_resource(SimRng::new(4));
        world.insert_resource(config);
        world.insert_resource(SimStats::default());
        world
    }

    #[test]
    fn predator_population_tracks_the_config() {
        let mut world = base_world(SimConfig {
            predator_count: 5,
            ..Default::default()
        });
        world.run_system_once(sync_predator_count).unwrap();
        let mut q = world.query::<&PredatorMarker>();
        assert_eq!(q.iter(&world).count(), 5);

        world.resource_mut::<SimConfig>().predator_count = 2;
        world.run_system_once(sync_predator_count).unwrap();
        assert_eq!(q.iter(&world).count(), 2);

        world.resource_mut::<SimConfig>().predator_count = 0;
        world.run_system_once(sync_predator_count).unwrap();
        assert_eq!(q.iter(&world).count(), 0);
    }

    #[test]
    fn a_predator_turns_toward_an_ant_in_range() {
        let mut world = base_world(SimConfig {
            predator_count: 1,
            predator_sense: 100.0,
            predator_turn_rate: 3.0,
            predator_speed: 0.0,
            ..Default::default()
        });
        // Predator at (10,10) facing +x; ant straight above it so the desired
        // heading is +pi/2.
        world.spawn((
            PredatorMarker,
            PredatorPos { x: 10.0, y: 10.0 },
            PredatorHeading { angle: 0.0 },
            PredatorAlert(false),
        ));
        world.spawn((
            AntMarker,
            AntColony(0),
            AntPos { x: 10.0, y: 40.0 },
            AntState::Searching,
        ));

        world.run_system_once(move_predators).unwrap();

        let mut q = world.query::<(&PredatorHeading, &PredatorAlert)>();
        let (heading, alert) = q.single(&world).unwrap();
        assert!(alert.0, "predator did not notice the ant");
        assert!(
            (heading.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-3,
            "heading {} is not toward the ant",
            heading.angle
        );
    }

    #[test]
    fn a_predator_ignores_ants_beyond_its_sense_radius() {
        let mut world = base_world(SimConfig {
            predator_count: 1,
            predator_sense: 5.0,
            predator_wander: 0.0,
            predator_speed: 0.0,
            ..Default::default()
        });
        world.spawn((
            PredatorMarker,
            PredatorPos { x: 10.0, y: 10.0 },
            PredatorHeading { angle: 0.0 },
            PredatorAlert(false),
        ));
        world.spawn((
            AntMarker,
            AntColony(0),
            AntPos { x: 10.0, y: 90.0 },
            AntState::Searching,
        ));

        world.run_system_once(move_predators).unwrap();

        let mut q = world.query::<&PredatorAlert>();
        assert!(
            !q.single(&world).unwrap().0,
            "sensed an ant that was too far"
        );
    }

    #[test]
    fn predation_removes_the_ants_in_reach() {
        let config = SimConfig {
            predator_count: 1,
            predator_catch_radius: 3.0,
            ..Default::default()
        };
        let mut world = base_world(config);
        world.spawn((
            PredatorMarker,
            PredatorPos { x: 100.0, y: 100.0 },
            PredatorHeading { angle: 0.0 },
            PredatorAlert(true),
        ));
        // Two ants inside the catch radius, one well outside.
        world.spawn((
            AntMarker,
            AntColony(0),
            AntPos { x: 101.0, y: 100.0 },
            AntState::Searching,
        ));
        world.spawn((
            AntMarker,
            AntColony(0),
            AntPos { x: 100.0, y: 102.0 },
            AntState::Searching,
        ));
        world.spawn((
            AntMarker,
            AntColony(0),
            AntPos { x: 150.0, y: 150.0 },
            AntState::Searching,
        ));

        world.run_system_once(predators_catch_ants).unwrap();

        let mut q = world.query::<&AntMarker>();
        assert_eq!(q.iter(&world).count(), 1, "wrong number of survivors");
        assert_eq!(world.resource::<SimStats>().ants_eaten, 2);
    }

    /// The nest must be a refuge, or a predator parked on it turns the colony
    /// into an all-you-can-eat buffet.
    #[test]
    fn ants_inside_their_own_nest_are_immune() {
        let config = SimConfig {
            predator_count: 1,
            predator_catch_radius: 30.0,
            ..Default::default()
        };
        let mut world = base_world(config);
        let nest = *world.resource::<Nests>().get(0);
        world.spawn((
            PredatorMarker,
            PredatorPos {
                x: nest.x,
                y: nest.y,
            },
            PredatorHeading { angle: 0.0 },
            PredatorAlert(true),
        ));
        world.spawn((
            AntMarker,
            AntColony(0),
            AntPos {
                x: nest.x,
                y: nest.y,
            },
            AntState::Searching,
        ));

        world.run_system_once(predators_catch_ants).unwrap();

        let mut q = world.query::<&AntMarker>();
        assert_eq!(
            q.iter(&world).count(),
            1,
            "an ant was eaten inside its nest"
        );
        assert_eq!(world.resource::<SimStats>().ants_eaten, 0);
    }

    #[test]
    fn no_predators_means_no_predation_work() {
        let mut world = base_world(SimConfig {
            predator_count: 0,
            predator_catch_radius: 100.0,
            ..Default::default()
        });
        world.spawn((
            AntMarker,
            AntColony(0),
            AntPos { x: 5.0, y: 5.0 },
            AntState::Searching,
        ));
        world.run_system_once(predators_catch_ants).unwrap();
        let mut q = world.query::<&AntMarker>();
        assert_eq!(q.iter(&world).count(), 1);
        assert_eq!(world.resource::<SimStats>().ants_eaten, 0);
    }

    #[test]
    fn predators_do_not_walk_through_walls() {
        let config = SimConfig {
            width: 64,
            height: 64,
            predator_count: 1,
            predator_speed: 4.0,
            predator_sense: 0.0,
            predator_wander: 0.5,
            ..Default::default()
        };
        let mut world = base_world(config);
        for y in 0..64 {
            world.resource_mut::<Obstacles>().set(32, y, true);
        }
        let blocked = world.resource::<Obstacles>().clone();
        world.spawn((
            PredatorMarker,
            PredatorPos { x: 30.0, y: 32.0 },
            PredatorHeading { angle: 0.0 },
            PredatorAlert(false),
        ));

        for _ in 0..200 {
            world.run_system_once(move_predators).unwrap();
        }

        let mut q = world.query::<&PredatorPos>();
        let pos = q.single(&world).unwrap();
        assert!(
            !blocked.is_blocked_at(pos.x, pos.y),
            "predator ended up inside a wall at ({}, {})",
            pos.x,
            pos.y
        );
    }
}
