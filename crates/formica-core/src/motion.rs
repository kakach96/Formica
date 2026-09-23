//! Shared movement: bounds reflection plus obstacle avoidance.
//!
//! Ants and predators move under the same rules, so the rules live in one
//! place. Duplicating them is how you end up with predators that can walk
//! through walls - or ants that cannot.
//!
//! Two things make this more than a clamped `+=`:
//!
//! * **Bouncing, not wrapping.** The world is a walled box. Wrapping would be
//!   cheaper, but it makes "the direction of the nest" ambiguous for anything
//!   near an edge, and direct homing is a core rule of the brief.
//! * **Escaping a wall you are standing in.** Obstacles can appear underneath
//!   an agent at any moment, because the player can draw one there. Without an
//!   escape path, that agent is entombed and its colony silently loses a
//!   forager for the rest of the run.

use crate::obstacles::Obstacles;
use crate::rng::SimRng;

/// Where a step left the agent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepResult {
    /// New x in cells.
    pub x: f32,
    /// New y in cells.
    pub y: f32,
    /// New heading in radians (wrapped).
    pub angle: f32,
    /// True when a wall (or the map edge) turned the agent this tick.
    pub hit_wall: bool,
}

/// Wrap an angle into `[-pi, pi)`.
#[inline]
pub fn wrap_angle(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    (a + PI).rem_euclid(TAU) - PI
}

/// The world an agent moves through.
///
/// Bundled so `step` takes four meaningful arguments instead of eight, six of
/// which are numbers that could be transposed without a compile error.
#[derive(Debug, Clone, Copy)]
pub struct MotionField<'a> {
    /// Map width in cells.
    pub width: usize,
    /// Map height in cells.
    pub height: usize,
    /// The wall map.
    pub obstacles: &'a Obstacles,
}

/// Advance one agent, respecting map bounds and obstacles.
pub fn step(
    x: f32,
    y: f32,
    angle: f32,
    speed: f32,
    field: MotionField<'_>,
    rng: &mut SimRng,
) -> StepResult {
    use std::f32::consts::PI;

    let MotionField {
        width,
        height,
        obstacles,
    } = field;
    let max_x = (width as f32 - 1.0).max(0.0);
    let max_y = (height as f32 - 1.0).max(0.0);
    let (dx, dy) = (angle.cos(), angle.sin());
    let mut angle = angle;
    let mut nx = x + dx * speed;
    let mut ny = y + dy * speed;
    let mut reflected = false;

    // Reflect off the map edges, mirroring the heading as well as the position
    // so an agent bounces instead of grinding along a wall.
    if nx < 0.0 {
        nx = -nx;
        angle = PI - angle;
        reflected = true;
    } else if nx > max_x {
        nx = 2.0 * max_x - nx;
        angle = PI - angle;
        reflected = true;
    }
    if ny < 0.0 {
        ny = -ny;
        angle = -angle;
        reflected = true;
    } else if ny > max_y {
        ny = 2.0 * max_y - ny;
        angle = -angle;
        reflected = true;
    }
    // A very large speed could tunnel past both edges in one tick.
    nx = nx.clamp(0.0, max_x);
    ny = ny.clamp(0.0, max_y);

    if obstacles.is_blocked_at(x, y) {
        // Something was drawn on top of us. Take the first free candidate,
        // including a full reversal, so we are never entombed.
        let candidates = [
            (nx, ny, angle),
            (nx, y, angle),
            (x, ny, angle),
            (x - dx * speed, y - dy * speed, angle + PI),
        ];
        for (cx, cy, ca) in candidates {
            let (cx, cy) = (cx.clamp(0.0, max_x), cy.clamp(0.0, max_y));
            if !obstacles.is_blocked_at(cx, cy) {
                return StepResult {
                    x: cx,
                    y: cy,
                    angle: wrap_angle(ca),
                    hit_wall: true,
                };
            }
        }
        return StepResult {
            x,
            y,
            angle: wrap_angle(angle + PI),
            hit_wall: true,
        };
    }

    if !obstacles.is_blocked_at(nx, ny) {
        return StepResult {
            x: nx,
            y: ny,
            angle: wrap_angle(angle),
            hit_wall: reflected,
        };
    }

    // Blocked. Slide along whichever axis is still free, so an agent running
    // into a wall keeps making progress instead of stalling against it.
    if !obstacles.is_blocked_at(nx, y) {
        return StepResult {
            x: nx,
            y,
            angle: wrap_angle(angle),
            hit_wall: true,
        };
    }
    if !obstacles.is_blocked_at(x, ny) {
        return StepResult {
            x,
            y: ny,
            angle: wrap_angle(angle),
            hit_wall: true,
        };
    }

    // Fully cornered: stay put and turn away hard. The turn is at least a
    // right angle, so an agent cannot grind into the same wall forever.
    let magnitude = rng.range(PI * 0.5, PI);
    let sign = if rng.unit() < 0.5 { -1.0 } else { 1.0 };
    StepResult {
        x,
        y,
        angle: wrap_angle(angle + magnitude * sign),
        hit_wall: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(width: usize, height: usize) -> Obstacles {
        Obstacles::new(width, height)
    }

    #[test]
    fn free_flight_advances_by_speed() {
        let obstacles = open(64, 64);
        let mut rng = SimRng::new(1);
        let r = step(
            20.0,
            20.0,
            0.0,
            2.0,
            MotionField {
                width: 64,
                height: 64,
                obstacles: &obstacles,
            },
            &mut rng,
        );
        assert!((r.x - 22.0).abs() < 1e-4);
        assert!((r.y - 20.0).abs() < 1e-4);
        assert!(!r.hit_wall);
    }

    #[test]
    fn the_map_edges_reflect_position_and_heading() {
        let obstacles = open(64, 64);
        let mut rng = SimRng::new(1);

        let r = step(
            1.0,
            30.0,
            std::f32::consts::PI,
            2.0,
            MotionField {
                width: 64,
                height: 64,
                obstacles: &obstacles,
            },
            &mut rng,
        );
        assert!(r.x >= 0.0);
        assert!(r.angle.cos() > 0.5, "did not turn around: {}", r.angle);
        assert!(r.hit_wall);

        let r = step(
            30.0,
            1.0,
            -std::f32::consts::FRAC_PI_2,
            4.0,
            MotionField {
                width: 64,
                height: 64,
                obstacles: &obstacles,
            },
            &mut rng,
        );
        assert!(r.y >= 0.0);
        assert!(r.angle.sin() > 0.5, "did not turn around: {}", r.angle);
    }

    #[test]
    fn agents_never_leave_the_map_even_at_absurd_speed() {
        let obstacles = open(32, 32);
        let mut rng = SimRng::new(5);
        for i in 0..128 {
            let angle = i as f32 * 0.7;
            let r = step(
                16.0,
                16.0,
                angle,
                32.0,
                MotionField {
                    width: 32,
                    height: 32,
                    obstacles: &obstacles,
                },
                &mut rng,
            );
            assert!(
                (0.0..=31.0).contains(&r.x) && (0.0..=31.0).contains(&r.y),
                "escaped to ({}, {}) at angle {angle}",
                r.x,
                r.y
            );
        }
    }

    #[test]
    fn a_wall_blocks_the_step() {
        let mut obstacles = open(64, 64);
        // A vertical wall at x = 30.
        for y in 0..64 {
            obstacles.set(30, y, true);
        }
        let mut rng = SimRng::new(1);

        let start = (28.0, 32.0);
        let r = step(
            start.0,
            start.1,
            0.0,
            2.0,
            MotionField {
                width: 64,
                height: 64,
                obstacles: &obstacles,
            },
            &mut rng,
        );
        assert!(
            !obstacles.is_blocked_at(r.x, r.y),
            "walked into the wall at ({}, {})",
            r.x,
            r.y
        );
        assert!(r.hit_wall);
    }

    /// Sliding is what stops agents deadlocking on a wall: an agent hitting a
    /// wall at an angle should still make progress along it.
    #[test]
    fn an_oblique_hit_slides_along_the_wall() {
        let mut obstacles = open(64, 64);
        for y in 0..64 {
            obstacles.set(30, y, true);
        }
        let mut rng = SimRng::new(1);

        // Heading mostly +x but with a +y component, starting just left of the
        // wall: x is blocked at the target, y is free.
        let angle = 0.2;
        let r = step(
            29.0,
            20.0,
            angle,
            2.0,
            MotionField {
                width: 64,
                height: 64,
                obstacles: &obstacles,
            },
            &mut rng,
        );
        assert!(!obstacles.is_blocked_at(r.x, r.y));
        assert!(r.y > 20.0, "did not slide along the wall: y = {}", r.y);
    }

    #[test]
    fn an_agent_dropped_inside_a_wall_escapes() {
        let mut obstacles = open(64, 64);
        // The player drew a wall exactly on top of the agent.
        obstacles.set(31, 30, true);
        assert!(obstacles.is_blocked_at(31.0, 30.0));
        let mut rng = SimRng::new(1);

        let r = step(
            31.0,
            30.0,
            0.0,
            1.0,
            MotionField {
                width: 64,
                height: 64,
                obstacles: &obstacles,
            },
            &mut rng,
        );
        assert!(
            !obstacles.is_blocked_at(r.x, r.y),
            "entombed at ({}, {})",
            r.x,
            r.y
        );
    }

    /// Even with nowhere to go, the agent must not panic, must stay on the map,
    /// and must report the collision.
    #[test]
    fn an_agent_completely_walled_in_stays_sane() {
        let mut obstacles = open(64, 64);
        for y in 29..=31 {
            for x in 29..=31 {
                obstacles.set(x, y, true);
            }
        }
        let mut rng = SimRng::new(1);
        let r = step(
            30.0,
            30.0,
            0.0,
            1.0,
            MotionField {
                width: 64,
                height: 64,
                obstacles: &obstacles,
            },
            &mut rng,
        );
        assert!((0.0..64.0).contains(&r.x) && (0.0..64.0).contains(&r.y));
        assert!(r.hit_wall);
        assert!((-std::f32::consts::PI..std::f32::consts::PI).contains(&r.angle));
    }

    #[test]
    fn wrap_angle_normalises() {
        use std::f32::consts::PI;
        assert!((wrap_angle(0.5) - 0.5).abs() < 1e-6);
        assert!(wrap_angle(0.0).abs() < 1e-6);
        // Every multiple of pi collapses onto +/-pi, which are the same
        // direction; compare magnitudes rather than signs.
        assert!((wrap_angle(PI * 3.0).abs() - PI).abs() < 1e-5);
        assert!((wrap_angle(-PI * 3.0).abs() - PI).abs() < 1e-5);
        assert!(wrap_angle(PI * 4.0).abs() < 1e-5);
        // Somewhere just below -pi must come back as just below +pi. A naive
        // `%` plus comparison leaves this one unwrapped, and reflection off a
        // wall lands exactly here.
        let nudged = wrap_angle(-PI - 0.001);
        assert!((nudged - (PI - 0.001)).abs() < 1e-4, "got {nudged}");
        for i in -100..100 {
            let a = wrap_angle(i as f32 * 0.37);
            assert!((-PI..PI).contains(&a), "{a} out of range");
        }
    }
}
