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

use crate::config::SimConfig;
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

/// Per-ant behavioural traits: the reason the colony does not spread as a
/// circle.
///
/// # Why this component has to exist
///
/// Every ant in a colony obeys the same rules, so an *ensemble* of identical
/// ants is radially symmetric: they all leave the nest at the same speed with
/// the same heading jitter, and the population becomes an expanding annulus
/// with a hollow middle. That is not a tuning problem - it is what averaging
/// identical random walks does. No parameter value fixes it, because there is
/// nothing to break the symmetry *with*.
///
/// The fix is heterogeneity *between* ants, which is also what a real colony
/// has: a few bold scouts that travel far in near-straight lines, and a bulk of
/// timid foragers that mill about close to home. The two traits are drawn from
/// one heavy-tailed distribution, so an ant that wanders further also walks
/// faster - a single "boldness" axis rather than two independent knobs.
///
/// Both multipliers are centred on 1.0, so the configured `speed` and
/// `random_walk_strength` remain the population means and every parameter sweep
/// in the README still means what it says.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct AntVigor {
    /// Multiplier on the per-tick random-walk jitter.
    pub wander: f32,
    /// Multiplier on travel speed.
    pub speed: f32,
}

/// Timid end of the wander distribution. An ant below this is effectively a
/// house ant that never leaves the nest's neighbourhood.
pub const MIN_WANDER: f32 = 0.3;
/// Bold end of the wander distribution: near-ballistic scouts.
pub const MAX_WANDER: f32 = 3.0;
/// Slowest and fastest an ant may travel, as multipliers of `config.speed`.
pub const MIN_SPEED: f32 = 0.7;
pub const MAX_SPEED: f32 = 1.4;

impl Default for AntVigor {
    /// The traitless ant - identical to every other, which is exactly the
    /// behaviour before individuality existed. Tests that spawn a bare ant
    /// tuple get this and keep their old expectations.
    fn default() -> Self {
        Self {
            wander: 1.0,
            speed: 1.0,
        }
    }
}

impl AntVigor {
    /// Draw one ant's traits.
    ///
    /// `individuality` is the log-normal sigma; `0.0` makes every ant average.
    /// The speed half uses a smaller sigma (0.35x) because speed variation is
    /// multiplicative on distance travelled, so the same sigma would produce a
    /// far wider spread in position than in turning.
    pub fn sample(rng: &mut SimRng, individuality: f32) -> Self {
        if individuality <= 0.0 {
            return Self::default();
        }
        let boldness = rng.normal();
        Self {
            wander: (individuality * boldness)
                .exp()
                .clamp(MIN_WANDER, MAX_WANDER),
            speed: (individuality * 0.35 * boldness)
                .exp()
                .clamp(MIN_SPEED, MAX_SPEED),
        }
    }
}

/// Build the component tuple for a fresh ant at its nest.
///
/// Returned as a tuple (not a `Bundle` struct) so it can be spawned through
/// either `Commands` or a `&mut World` without duplicating the setup.
///
/// The *positions* are a plain uniform-in-area disc, and deliberately so: at
/// the default `spawn_radius` of 2 cells the whole nest chamber is four cells
/// across, so any structure finer than that is invisible. The irregularity the
/// viewer sees comes from what the ants do next - see [`AntVigor`] - not from
/// where they started.
pub fn newborn_ant(
    colony: AntColony,
    nest: &Nest,
    rng: &mut SimRng,
    config: &SimConfig,
) -> (
    AntMarker,
    AntColony,
    AntPos,
    AntHeading,
    AntState,
    Carrying,
    PheromoneSensors,
    AntVigor,
) {
    let radius = config.spawn_radius.max(0.0);
    let angle = rng.range(0.0, std::f32::consts::TAU);
    // `sqrt` is what makes the scatter uniform over the *area* of the disc
    // rather than piling every ant onto the centre cell.
    let r = radius * rng.unit().sqrt();
    let x = nest.x + angle.cos() * r;
    let y = nest.y + angle.sin() * r;

    // Face away from the nest, but not *exactly* away. A radial heading for
    // every ant is half of why the colony used to expand as a hollow ring: with
    // one shared speed and one shared direction the disc becomes an annulus,
    // and stays one until the heading jitter has had time to decorrelate it.
    let outward = if r > 1.0e-3 {
        angle
    } else {
        rng.range(0.0, std::f32::consts::TAU)
    };

    (
        AntMarker,
        colony,
        AntPos::new(x, y),
        AntHeading::new(outward + rng.range(-1.0, 1.0)),
        AntState::Searching,
        Carrying::default(),
        PheromoneSensors::default(),
        AntVigor::sample(rng, config.individuality),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SimConfig {
        SimConfig::default()
    }

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
        let (_, colony, pos, _, state, cargo, _, _) =
            newborn_ant(AntColony(1), &nest, &mut rng, &config());
        assert_eq!(colony.index(), 1);
        assert_eq!(state, AntState::Searching);
        assert_eq!(cargo.amount, 0.0);
        let d = ((pos.x - 10.0).powi(2) + (pos.y - 10.0).powi(2)).sqrt();
        assert!(d <= config().spawn_radius + 1e-4, "spawned {d} away");
    }

    /// Zero individuality must reproduce the old, traitless ant exactly.
    #[test]
    fn zero_individuality_gives_every_ant_average_traits() {
        let mut rng = SimRng::new(4);
        for _ in 0..500 {
            let vigor = AntVigor::sample(&mut rng, 0.0);
            assert_eq!(vigor, AntVigor::default());
        }
    }

    /// Heterogeneity is the whole point of the component: the distribution has
    /// to reach both a timid tail and a bold one, or the colony is still an
    /// ensemble of identical agents.
    #[test]
    fn individuality_produces_a_spread_of_traits() {
        let mut rng = SimRng::new(5);
        let traits: Vec<AntVigor> = (0..20_000)
            .map(|_| AntVigor::sample(&mut rng, SimConfig::default().individuality))
            .collect();

        let mean = traits.iter().map(|v| v.wander as f64).sum::<f64>() / traits.len() as f64;
        assert!(
            (0.9..1.2).contains(&mean),
            "wander should stay centred on 1.0, got {mean}"
        );

        let timid = traits.iter().filter(|v| v.wander < 0.6).count();
        let bold = traits.iter().filter(|v| v.wander > 1.7).count();
        assert!(timid > 100, "only {timid} timid ants: no lower tail");
        assert!(bold > 100, "only {bold} bold ants: no upper tail");

        // Bold ants walk faster - one boldness axis, not two loose knobs.
        let bold_speed = traits
            .iter()
            .filter(|v| v.wander > 1.7)
            .map(|v| v.speed as f64)
            .sum::<f64>()
            / bold.max(1) as f64;
        assert!(bold_speed > 1.05, "bold ants are not faster: {bold_speed}");

        for v in &traits {
            assert!((MIN_WANDER..=MAX_WANDER).contains(&v.wander), "{v:?}");
            assert!((MIN_SPEED..=MAX_SPEED).contains(&v.speed), "{v:?}");
        }
    }

    /// The initial occupancy is uniform over the disc's *area*, not piled onto
    /// the centre cell - and still inside `spawn_radius`.
    #[test]
    fn spawns_fill_the_disc_without_escaping_it() {
        let nest = Nest {
            x: 50.0,
            y: 50.0,
            radius: 4.0,
        };
        let config = SimConfig {
            spawn_radius: 20.0,
            ..config()
        };
        let mut rng = SimRng::new(7);

        let mut inner = 0;
        let n = 4_000;
        for _ in 0..n {
            let (_, _, pos, _, _, _, _, _) = newborn_ant(AntColony(0), &nest, &mut rng, &config);
            let d = (pos.x - nest.x).hypot(pos.y - nest.y);
            assert!(d <= config.spawn_radius + 1e-3, "spawned {d} out");
            if d < config.spawn_radius * 0.5 {
                inner += 1;
            }
        }
        // Half the radius is a quarter of the area, so a uniform-in-area disc
        // puts ~25% of the ants there; a centre-heavy one puts ~50%.
        let fraction = inner as f64 / n as f64;
        assert!(
            (0.18..0.33).contains(&fraction),
            "{fraction} of ants landed in the inner quarter of the disc"
        );
    }

    /// The heading must not be exactly radial, or the colony still expands as
    /// an annulus even with lumpy spawn positions.
    #[test]
    fn departure_headings_are_not_exactly_radial() {
        let nest = Nest {
            x: 50.0,
            y: 50.0,
            radius: 4.0,
        };
        let config = SimConfig {
            spawn_radius: 20.0,
            individuality: 0.0,
            ..config()
        };
        let mut rng = SimRng::new(11);

        let mut exactly_radial = 0;
        let n = 2_000;
        for _ in 0..n {
            let (_, _, pos, heading, _, _, _, _) =
                newborn_ant(AntColony(0), &nest, &mut rng, &config);
            let outward = (pos.y - nest.y).atan2(pos.x - nest.x);
            if wrap_angle(heading.angle - outward).abs() < 1.0e-3 {
                exactly_radial += 1;
            }
        }
        assert_eq!(
            exactly_radial, 0,
            "{exactly_radial}/{n} ants left dead straight"
        );
    }
}
