//! Stage 2 of an ant's tick: turn the sensor readings into a new heading.

use bevy_ecs::prelude::*;

use crate::ant::{wrap_angle, AntColony, AntHeading, AntPos, AntState, AntVigor, PheromoneSensors};
use crate::config::{HomingMode, SimConfig};
use crate::flow::NestFlow;
use crate::obstacles::Obstacles;
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

/// How hard a returning ant turns away from a wall its whiskers felt.
///
/// Measured on the U-wall preset: at the brief's speed (2 cells/tick) an ant
/// with `max_turn_rate` unbound can grind into a wall for hundreds of ticks
/// while sliding only sideways; clamping the whisker turn to the full turn
/// budget makes the escape happen within a couple of ticks, so the colony
/// flows around the wall instead of pooling against it.
const WALL_ESCAPE_GAIN: f32 = 1.0;

/// How much a free-ahead ant biases away from a wall felt at one antenna.
///
/// Small on purpose (a quarter of the turn budget): this is wall *avoidance*,
/// which only has to stop the ant from hugging the wall, not fight the hive
/// vector. The escape branch above is what handles the real jams.
const WALL_AVOID_GAIN: f32 = 0.25;

/// Steer every ant for this tick.
///
/// **Searching** ants do a correlated random walk (`heading += noise`) with a
/// bias toward whichever antenna smells the most food pheromone. That is the
/// entire "intelligence" of the colony: no ant knows where the food is, they
/// only know which way smells stronger, and the trail is a side effect of
/// other ants' success.
///
/// **Returning** ants home by [`HomingMode`]:
///
/// * `PathIntegration` - steer straight at the nest (brief behaviour; the
///   trig equivalent of integrating the outbound steps), clamped to
///   `max_turn_rate`. `home_pheromone_weight` blends in the home-pheromone
///   gradient (default 0.0 = brief behaviour).
/// * `WallSense` - the same nest vector, but whiskers sense the wall map a
///   few cells ahead of the antennae arc; a felt wall is escaped hard, a
///   nearby-but-avoidable one earns a gentle avoidance bias.
/// * `FlowField` - steer along the colony's BFS field to the nest, which is
///   the only mode that always routes *around* a drawn wall. Cells with no
///   field fall back to direct homing.
///
/// Both branches scale the jitter by the ant's own [`AntVigor::wander`]. That
/// is the only per-individual difference in the whole simulation, and it is
/// load-bearing: without it every ant traces the same random walk and the
/// colony is a featureless expanding disc.
pub fn decide_heading(
    config: Res<SimConfig>,
    nests: Res<Nests>,
    obstacles: Res<Obstacles>,
    nest_flow: Res<NestFlow>,
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
                match config.homing_mode {
                    // Brief behaviour, exactly as it has always been.
                    HomingMode::PathIntegration => {}
                    HomingMode::WallSense => {
                        // `desired` is the unclamped nest bearing: the escape
                        // branch picks between two blocked sides with it, so
                        // a stuck ant escapes toward home rather than at
                        // random.
                        steer += wall_whisker_steering(
                            &config,
                            &obstacles,
                            pos.x,
                            pos.y,
                            heading.angle,
                            desired,
                        );
                    }
                    HomingMode::FlowField => {
                        if let Some((vx, vy)) = nest_flow.direction(colony.index(), pos.x, pos.y) {
                            // Blend, rather than replace, so an ant near the
                            // field's seed still glides onto the mound instead
                            // of pivoting on the spot over it.
                            let flow_angle = vy.atan2(vx);
                            let flow_steer = wrap_angle(flow_angle - heading.angle)
                                .clamp(-config.max_turn_rate, config.max_turn_rate);
                            steer = steer * 0.5 + flow_steer * 0.5;
                        }
                    }
                }
                // Even a homing ant wanders a little, which is what keeps the
                // returning trail a corridor instead of a razor-thin line.
                let jitter = rng.signed() * config.random_walk_strength * 0.3 * wander;
                heading.turn(steer + jitter);
            }
        }
    }
}

/// Whisker steering for `WallSense` homing.
///
/// Three whiskers (the antennae arc) feel the wall map a few cells ahead.
/// A wall dead ahead triggers the hard escape turn - toward whichever side
/// still feels open, preferring the rotation that brings the heading closer
/// to the nest bearing. A wall on one side only earns a gentle push to the
/// other, keeping the ant off the wall without fighting the hive vector.
/// Returns the extra turn in radians to add to `steer`.
fn wall_whisker_steering(
    config: &SimConfig,
    obstacles: &Obstacles,
    x: f32,
    y: f32,
    heading: f32,
    desired: f32,
) -> f32 {
    let reach = config.sensor_distance.max(1.0);
    let half = config.sensor_angle.max(0.1);
    let feels =
        |angle: f32| obstacles.is_blocked_at(x + angle.cos() * reach, y + angle.sin() * reach);
    // Positive turns are counter-clockwise, so "ccw" is the whisker at
    // `heading + half` and "cw" the one at `heading - half`.
    let ccw = feels(heading + half);
    let cw = feels(heading - half);

    if !feels(heading) {
        // Nothing dead ahead. A wall felt at one whisker only is still worth
        // steering clear of: quarter-gain away from it. Both clear or both
        // walled read as "no lateral advice".
        if cw && !ccw {
            return -WALL_AVOID_GAIN * config.max_turn_rate;
        }
        if ccw && !cw {
            return WALL_AVOID_GAIN * config.max_turn_rate;
        }
        return 0.0;
    }

    // Dead-ahead wall. Prefer the side whose rotation brings the heading
    // closer to the nest, so the escape never sends the ant on the wrong leg
    // around the obstacle when it does not have to.
    let ccw_score = wrap_angle(heading + half - desired).abs();
    let cw_score = wrap_angle(heading - half - desired).abs();
    let sign = match (ccw, cw) {
        // Counter-clockwise side sealed: turn clockwise (negative).
        (true, false) => -1.0,
        // Clockwise side sealed: turn counter-clockwise.
        (false, true) => 1.0,
        // Both open: take the side that ends nearer the nest bearing.
        (false, false) => {
            if ccw_score <= cw_score {
                1.0
            } else {
                -1.0
            }
        }
        // Sealed on every side: turn hard toward the nest itself - the wall
        // slalom in `motion::step` then converts that into progress.
        (true, true) => wrap_angle(desired - heading).signum(),
    };
    sign * WALL_ESCAPE_GAIN * config.max_turn_rate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Nest;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    fn world_with(config: SimConfig) -> World {
        let mut world = World::new();
        world.insert_resource(Nests::from_config(&config));
        world.insert_resource(config);
        // Navigation resources: an empty wall map and an unbuilt flow field,
        // matching what a fresh world gets (both only matter in one of the
        // three homing modes).
        world.insert_resource(Obstacles::new(256, 256));
        world.insert_resource(NestFlow::new(256, 256));
        world.insert_resource(SimRng::new(1));
        world
    }

    /// A hand-placed pair of nests, so the geometry in the assertions is exact.
    fn two_nest_world(config: SimConfig, a: Nest, b: Nest) -> World {
        let mut world = World::new();
        world.insert_resource(Nests(vec![a, b]));
        world.insert_resource(config);
        world.insert_resource(Obstacles::new(256, 256));
        world.insert_resource(NestFlow::new(256, 256));
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

    /// `WallSense`: a wall dead ahead must trigger the hard escape turn,
    /// while in the default mode the same wall is ignored - the brief's
    /// behaviour does not change unless the mode is selected.
    #[test]
    fn wall_sense_escapes_a_dead_ahead_wall() {
        let mut world = world_with(SimConfig {
            random_walk_strength: 0.0,
            sensor_distance: 3.0,
            sensor_box_radius: 0,
            sensor_angle: 0.6,
            max_turn_rate: 0.8,
            homing_mode: HomingMode::WallSense,
            ..Default::default()
        });
        // A wall three cells due east of the ant: the straight whisker.
        world.resource_mut::<Obstacles>().set(13, 10, true);
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(10.0, 10.0),
            PheromoneSensors::default(),
            AntHeading::new(0.0), // heading due east, into the wall
        ));
        world.run_system_once(decide_heading).unwrap();
        let mut q = world.query::<&AntHeading>();
        let angle = q.single(&world).unwrap().angle;
        // The escape add is on top of the hive steer: the ant's bearing to
        // the nest is exactly +pi/4 from here (the default nest sits on the
        // map diagonal). What this pins is the whisker turn itself: one full
        // turn budget on top of the direct course.
        let relative = wrap_angle(angle - std::f32::consts::FRAC_PI_4).abs();
        assert!(
            (relative - 0.8).abs() < 1e-4,
            "no hard escape turn of the full budget: {angle}"
        );

        // Control: the same wall under the default mode must not steer. The
        // ant here turns toward the nest by pure `max_turn_rate` homing; the
        // whiskers must be a no-op in this mode.
        let mut world = world_with(SimConfig {
            random_walk_strength: 0.0,
            sensor_distance: 3.0,
            sensor_box_radius: 0,
            sensor_angle: 0.6,
            ..Default::default()
        });
        world.resource_mut::<Obstacles>().set(13, 10, true);
        let nest = *world.resource::<Nests>().get(0);
        let direct_bearing = (nest.y - 10.0).atan2(nest.x - 10.0);
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(10.0, 10.0),
            PheromoneSensors::default(),
            AntHeading::new(0.0),
        ));
        world.run_system_once(decide_heading).unwrap();
        let mut q = world.query::<&AntHeading>();
        let angle = q.single(&world).unwrap().angle;
        assert!(
            (wrap_angle(angle - direct_bearing)).abs() < 1e-4,
            "direct homing was perturbed by the wall: {angle} vs {direct_bearing}"
        );
    }

    /// `FlowField`: with a wall between the ant and the nest, the heading
    /// must have been steered along the field's downhill direction - which is
    /// not where the nest is.
    #[test]
    fn flow_field_steers_downhill_around_a_wall() {
        use crate::flow::rebuild_nest_flow;
        let mut world = world_with(SimConfig {
            random_walk_strength: 0.0,
            max_turn_rate: 3.0,
            homing_mode: HomingMode::FlowField,
            ..Default::default()
        });
        let nest = *world.resource::<Nests>().get(0);
        // A wall column east of the nest, with one gap; the ant stands east
        // of the wall, so the nest is hidden behind it.
        {
            let mut obstacles = world.resource_mut::<Obstacles>();
            obstacles.resize(256, 256);
            for y in 0..256 {
                obstacles.set(40, y, true);
            }
            obstacles.set(40, 50, false); // one gap
        }
        {
            let obstacles = world.resource::<Obstacles>().clone();
            let nests = world.resource::<Nests>().clone();
            let mut flow = world.resource_mut::<NestFlow>();
            rebuild_nest_flow(&mut flow, &obstacles, &nests);
        }
        // The ant heads due southwest - the direct bearing to the nest - but
        // the wall sits between them, and the field routes the ant *along*
        // the wall toward the gap instead.
        let heading_before = -3.0 * std::f32::consts::FRAC_PI_4; // southwest
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(nest.x + 30.0, nest.y + 30.0),
            PheromoneSensors::default(),
            AntHeading::new(heading_before),
        ));
        world.run_system_once(decide_heading).unwrap();
        let mut q = world.query::<&AntHeading>();
        let turned = q.single(&world).unwrap().angle;
        // The field's first downhill step from here is due west (slide along
        // the wall), not the direct southwest bearing: the heading must have
        // been pulled visibly off course.
        let moved = wrap_angle(turned - heading_before).abs();
        assert!(
            moved > 0.05,
            "the flow field did not change the direct course: {turned}"
        );
        // ...and toward the wall-hugging leg: within half a lobe of due west.
        assert!(
            (wrap_angle(turned + std::f32::consts::FRAC_PI_2)).abs() < 1.2
                || (wrap_angle(turned - std::f32::consts::PI)).abs() < 1.2,
            "the flow mode turned somewhere else entirely: {turned}"
        );
    }
}
