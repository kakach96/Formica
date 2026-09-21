//! Stage 3 of an ant's tick: advance the ant along its heading.
//!
//! The rules themselves live in [`crate::motion`], because predators move under
//! exactly the same bounds-and-obstacle logic. This system is only the ECS
//! adapter: it reads `AntPos`/`AntHeading`, calls `motion::step`, writes back.

use bevy_ecs::prelude::*;

use crate::ant::{AntHeading, AntPos};
use crate::config::SimConfig;
use crate::motion::{self, MotionField};
use crate::obstacles::Obstacles;
use crate::rng::SimRng;

/// Integrate every ant's position.
pub fn move_ants(
    config: Res<SimConfig>,
    obstacles: Res<Obstacles>,
    mut rng: ResMut<SimRng>,
    mut ants: Query<(&mut AntPos, &mut AntHeading)>,
) {
    let field = MotionField {
        width: config.width,
        height: config.height,
        obstacles: &obstacles,
    };
    let speed = config.speed;
    for (mut pos, mut heading) in ants.iter_mut() {
        let result = motion::step(pos.x, pos.y, heading.angle, speed, field, &mut rng);
        pos.x = result.x;
        pos.y = result.y;
        heading.angle = result.angle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    fn run(config: SimConfig, setter: impl FnOnce(&mut Obstacles), x: f32, y: f32, angle: f32) -> (AntPos, AntHeading) {
        let mut world = World::new();
        let (w, h) = (config.width, config.height);
        world.insert_resource(config);
        world.insert_resource(SimRng::new(1));
        let mut obstacles = Obstacles::new(w, h);
        setter(&mut obstacles);
        world.insert_resource(obstacles);

        let entity = world.spawn((AntPos::new(x, y), AntHeading::new(angle))).id();
        world.run_system_once(move_ants).unwrap();
        let mut q = world.query::<(&AntPos, &AntHeading)>();
        let (p, h) = q.get(&world, entity).unwrap();
        (*p, *h)
    }

    fn default_config() -> SimConfig {
        SimConfig::default()
    }

    #[test]
    fn free_flight_advances_by_speed() {
        let (pos, _) = run(default_config(), |_| {}, 50.0, 50.0, 0.0);
        assert!((pos.x - 52.0).abs() < 1e-4);
        assert!((pos.y - 50.0).abs() < 1e-4);
    }

    #[test]
    fn left_wall_reflects_position_and_heading() {
        let (pos, heading) = run(default_config(), |_| {}, 1.0, 50.0, std::f32::consts::PI);
        assert!(pos.x >= 0.0, "escaped the map: {}", pos.x);
        assert!(
            heading.angle.cos() > 0.5,
            "did not turn around: {}",
            heading.angle
        );
    }

    #[test]
    fn top_wall_reflects_position_and_heading() {
        let (pos, heading) = run(
            SimConfig {
                speed: 4.0,
                ..default_config()
            },
            |_| {},
            50.0,
            1.0,
            -std::f32::consts::FRAC_PI_2,
        );
        assert!(pos.y >= 0.0, "escaped the map: {}", pos.y);
        assert!(
            heading.angle.sin() > 0.5,
            "did not turn around: {}",
            heading.angle
        );
    }

    #[test]
    fn ants_never_leave_the_map_even_at_absurd_speed() {
        let config = SimConfig {
            width: 32,
            height: 32,
            speed: 32.0,
            ..default_config()
        };
        for step in 0..64 {
            let angle = step as f32 * 0.7;
            let (pos, _) = run(config.clone(), |_| {}, 16.0, 16.0, angle);
            assert!(
                (0.0..=31.0).contains(&pos.x) && (0.0..=31.0).contains(&pos.y),
                "escaped to {pos:?} at angle {angle}"
            );
        }
    }

    #[test]
    fn ants_do_not_walk_through_walls() {
        // A solid vertical wall across the middle of the map.
        let config = SimConfig {
            width: 64,
            height: 64,
            ..default_config()
        };
        let wall = 32;
        let (pos, _) = run(
            config,
            move |o| {
                for y in 0..64 {
                    o.set(wall, y, true);
                }
            },
            30.0,
            32.0,
            0.0,
        );
        assert!(pos.x < wall as f32, "walked through the wall to {}", pos.x);
    }

    #[test]
    fn every_ant_stays_out_of_the_walls_over_a_long_run() {
        let config = SimConfig {
            width: 64,
            height: 64,
            ..default_config()
        };
        let (w, h) = (config.width, config.height);
        let mut world = World::new();
        world.insert_resource(config);
        world.insert_resource(SimRng::new(7));
        let mut obstacles = Obstacles::new(w, h);
        // A picket fence.
        for y in 0..h {
            if y % 4 != 0 {
                obstacles.set(32, y as i32, true);
            }
        }
        let blocked = obstacles.clone();
        world.insert_resource(obstacles);

        for i in 0..40 {
            let angle = i as f32 * 0.31;
            world.spawn((AntPos::new(20.0, 32.0), AntHeading::new(angle)));
        }

        for _ in 0..200 {
            world.run_system_once(move_ants).unwrap();
        }

        let mut q = world.query::<&AntPos>();
        for pos in q.iter(&world) {
            assert!(
                !blocked.is_blocked_at(pos.x, pos.y),
                "an ant ended up inside a wall at ({}, {})",
                pos.x,
                pos.y
            );
        }
    }
}
