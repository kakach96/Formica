//! Ant components.
//!
//! Deliberately *not* one `struct Ant` living in a `Vec<Ant>`: the brief asks
//! for a data-oriented layout, so each concern is its own `Component` and each
//! simulation stage is its own `System` that touches only the columns it
//! needs. `sense` writes [`PheromoneSensors`], `decide` consumes it and writes
//! [`AntHeading`], `move_ants` writes [`AntPos`], and so on.
//!
//! ```text
//!   sense ──> PheromoneSensors ──> decide ──> AntHeading ──> move ──> AntPos
//!                                                                     │
//!                              deposit <──────────────────────────────┤
//!                          pickup/deliver ──> AntState, Carrying <────┘
//! ```

use bevy_ecs::prelude::*;

use crate::rng::SimRng;
use crate::world::Nest;

// `wrap_angle` is fundamentally about movement, so it lives in `motion`; it is
// re-exported here because `AntHeading` is where callers reach for it.
pub use crate::motion::wrap_angle;

/// Marker for every ant entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct AntMarker;

/// Which colony an ant belongs to.
///
/// Determines **which pheromone fields the ant reads and writes** and which
/// nest it delivers to. Two colonies with the same `AntColony` index share
/// trails; two with different indices cannot smell each other at all, even
/// though they forage over the same map for the same food.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct AntColony(pub usize);

impl AntColony {
    /// Index into [`crate::world::Nests`], clamped by the consumers.
    #[inline]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// The ant's position in continuous world coordinates (cells).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct AntPos {
    /// X in cells.
    pub x: f32,
    /// Y in cells.
    pub y: f32,
}

impl AntPos {
    /// Construct a position.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Direction the ant is facing, in radians.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct AntHeading {
    /// Angle in radians; direction of travel is `(cos, sin)`.
    pub angle: f32,
}

impl AntHeading {
    /// Construct a heading.
    pub const fn new(angle: f32) -> Self {
        Self { angle }
    }

    /// Unit direction vector `(cos, sin)`.
    #[inline]
    pub fn direction(&self) -> (f32, f32) {
        (self.angle.cos(), self.angle.sin())
    }

    /// Rotate, keeping the angle in `(-pi, pi]`.
    #[inline]
    pub fn turn(&mut self, delta: f32) {
        self.angle = wrap_angle(self.angle + delta);
    }
}

/// The two-state behavioural machine from the brief.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum AntState {
    /// Outbound: random walk biased toward food pheromone, deposits home pheromone.
    #[default]
    Searching,
    /// Inbound: heads for the nest, deposits food pheromone.
    Returning,
}

impl AntState {
    /// True when the ant is carrying food home.
    #[inline]
    pub const fn is_returning(self) -> bool {
        matches!(self, AntState::Returning)
    }
}

/// How much food the ant is currently carrying.
///
/// Only meaningful while [`AntState::Returning`]; it is zeroed on delivery.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Carrying {
    /// Carried food units.
    pub amount: f32,
}

/// The three antenna readings written by `sense_pheromones`.
///
/// `left`/`right` are `sensor_angle` off the current heading, `center` is
/// straight ahead - the classic three-sensor ACO arrangement, which lets an
/// ant both *detect* a trail and *align* with it.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct PheromoneSensors {
    /// Reading at the left antenna.
    pub left: f32,
    /// Reading at the centre antenna.
    pub center: f32,
    /// Reading at the right antenna.
    pub right: f32,
}

impl PheromoneSensors {
    /// Strongest reading.
    #[inline]
    pub fn strongest(&self) -> f32 {
        self.left.max(self.center).max(self.right)
    }

    /// Weakest reading.
    #[inline]
    pub fn weakest(&self) -> f32 {
        self.left.min(self.center).min(self.right)
    }

    /// `-1` when the left antenna is best, `+1` when the right is best,
    /// `0` when the centre is (or nothing is sensed).
    #[inline]
    pub fn preferred_side(&self) -> f32 {
        if self.left > self.center && self.left >= self.right {
            -1.0
        } else if self.right > self.center && self.right > self.left {
            1.0
        } else {
            0.0
        }
    }
}

/// Build the component tuple for a fresh ant at its nest.
///
/// Returned as a tuple (not a `Bundle` struct) so it can be spawned through
/// either `Commands` or a `&mut World` without duplicating the setup.
pub fn newborn_ant(
    colony: AntColony,
    nest: &Nest,
    rng: &mut SimRng,
    spawn_radius: f32,
) -> (
    AntMarker,
    AntColony,
    AntPos,
    AntHeading,
    AntState,
    Carrying,
    PheromoneSensors,
) {
    let angle = rng.range(0.0, std::f32::consts::TAU);
    let r = if spawn_radius > 0.0 {
        rng.range(0.0, spawn_radius)
    } else {
        0.0
    };
    let x = nest.x + angle.cos() * r;
    let y = nest.y + angle.sin() * r;
    (
        AntMarker,
        colony,
        AntPos::new(x, y),
        // Face outward from the nest, away from the crowd.
        AntHeading::new(angle),
        AntState::Searching,
        Carrying::default(),
        PheromoneSensors::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferred_side_points_at_the_strongest_antenna() {
        let s = PheromoneSensors {
            left: 5.0,
            center: 1.0,
            right: 2.0,
        };
        assert_eq!(s.preferred_side(), -1.0);
        let s = PheromoneSensors {
            left: 1.0,
            center: 1.0,
            right: 3.0,
        };
        assert_eq!(s.preferred_side(), 1.0);
        let s = PheromoneSensors {
            left: 1.0,
            center: 9.0,
            right: 1.0,
        };
        assert_eq!(s.preferred_side(), 0.0);
    }

    #[test]
    fn newborn_ants_start_searching_and_empty() {
        let nest = Nest {
            x: 10.0,
            y: 10.0,
            radius: 3.0,
        };
        let mut rng = SimRng::new(1);
        let (_, colony, pos, _, state, cargo, _) =
            newborn_ant(AntColony(1), &nest, &mut rng, 2.0);
        assert_eq!(colony.index(), 1);
        assert_eq!(state, AntState::Searching);
        assert_eq!(cargo.amount, 0.0);
        let d = ((pos.x - 10.0).powi(2) + (pos.y - 10.0).powi(2)).sqrt();
        assert!(d <= 2.0 + 1e-4, "spawned {d} away");
    }
}
