//! Simulation configuration.
//!
//! Everything that is interesting to tune lives in [`SimConfig`]. It is a
//! `Resource`, so *every* system reads it fresh on every tick: mutating the
//! resource at runtime (the Phase 3 egui panel does exactly this) hot-reloads
//! the parameter with no restart and no bookkeeping.

use bevy_ecs::prelude::*;

use crate::grid::MAX_COLONIES;

/// Default tuning values from the project brief (Phase 1, "第一版就用这些").
pub const DEFAULT_ANT_COUNT: usize = 500;
pub const DEFAULT_MAP_SIZE: usize = 256;
pub const DEFAULT_EVAPORATION_RATE: f32 = 0.1;
pub const DEFAULT_SENSOR_RADIUS: f32 = 3.0;
pub const DEFAULT_RANDOM_WALK_STRENGTH: f32 = 0.3;
pub const DEFAULT_SPEED: f32 = 2.0;
pub const DEFAULT_DEPOSIT_AMOUNT: f32 = 1.0;
/// Default spread of the per-ant behaviour traits.
///
/// 0.6 is a deliberate choice rather than a round number. At this sigma roughly
/// 2% of a 500-ant colony are near-ballistic scouts (wander > 2) and 2% are
/// house ants (wander < 0.4), which is enough to break the colony's radial
/// symmetry within a few dozen ticks without the population falling apart into
/// a handful of individuals. Measured: the radial profile stops being an
/// annulus by tick ~30, where `individuality = 0.0` still shows a hollow ring
/// at tick 60.
pub const DEFAULT_INDIVIDUALITY: f32 = 0.6;
/// Default RNG seed. Any `u64` works; this one is just easy to type.
pub const DEFAULT_SEED: u64 = 20_240_517;

/// A shape to stamp into the [`crate::obstacles::Obstacles`] map.
///
/// The geometry lives *inside* the variants so that a UI slider changing an arm
/// length produces a different value, which the sync system notices and
/// re-rasterises - no explicit "apply" plumbing anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ObstaclePreset {
    /// An empty map.
    #[default]
    None,
    /// A U-shaped wall wrapped around the first food pile, closed side facing
    /// the colony so the food cannot be reached in a straight line.
    UWall {
        /// Length of each arm, in cells.
        arm: f32,
        /// Clear width between the arms, in cells.
        gap: f32,
        /// Wall thickness, in cells.
        thickness: f32,
    },
    /// Scattered wall blocks - the "rubble" map.
    RandomBlocks {
        /// Probability that any given cell is wall.
        density: f32,
    },
}

impl ObstaclePreset {
    /// Short label for the UI (English).
    pub fn label(self) -> &'static str {
        match self {
            ObstaclePreset::None => "open field",
            ObstaclePreset::UWall { .. } => "U-shaped wall",
            ObstaclePreset::RandomBlocks { .. } => "scattered blocks",
        }
    }

    /// [`ObstaclePreset::label`] in the language of the given string table.
    pub fn label_for(self, s: &crate::i18n::Strings) -> &'static str {
        match self {
            ObstaclePreset::None => s.obstacle_none_label,
            ObstaclePreset::UWall { .. } => s.obstacle_u_wall_label,
            ObstaclePreset::RandomBlocks { .. } => s.obstacle_blocks_label,
        }
    }
}

/// How a food-carrying ant finds its way back to the nest.
///
/// The brief's rule is *path integration*: a returning ant steers straight at
/// its nest. On an open map that is perfect - and exactly what the grid
/// ants do, since the math is the same as an ant integrating its own
/// outbound steps. But a wall between the ant and the nest turns the direct
/// course into a jam against it (see the U-wall preset), so two richer
/// strategies exist as options:
///
/// * migrating from "straight line" to "follow the wall" - whiskers sense the
///   wall before contact and steer away, which is the jam-escape toolkit, and
/// * a BFS flow field to the nest ( [`crate::flow::NestFlow`] ), which is
///   "the colony knows the building", top-down navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomingMode {
    /// Direct homing: steer at the nest, clamped to `max_turn_rate`. Brief
    /// behaviour, bit-identical to historical runs.
    #[default]
    PathIntegration,
    /// Direct homing plus wall whiskers: sense walls a few cells ahead with
    /// the antennae arc, turn away before touching, and hug open sides - the
    /// stuck-escape family.
    WallSense,
    /// Steer along the BFS flow field towards the nest. Works around any
    /// obstacle, at the cost of a braid-truth (a map the ant did not explore).
    FlowField,
}

impl HomingMode {
    /// The keyword used by the CLI flag and scenario TOML field.
    pub fn keyword(self) -> &'static str {
        match self {
            HomingMode::PathIntegration => "path",
            HomingMode::WallSense => "wall",
            HomingMode::FlowField => "flow",
        }
    }

    /// Parse a CLI/TOML keyword; `parse` accepts the spellings users type.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "path" | "path-integration" | "direct" | "pi" => Some(HomingMode::PathIntegration),
            "wall" | "wall-sense" | "whiskers" | "escape" | "thigmotaxis" => {
                Some(HomingMode::WallSense)
            }
            "flow" | "flow-field" | "bfs" | "field" => Some(HomingMode::FlowField),
            _ => None,
        }
    }
}

/// One axis-aligned wall rectangle, in cells.
///
/// A scenario's custom map is a list of these, stamped on top of the preset.
/// Rectangles (rather than per-cell lists) keep the TOML human-editable: a
/// corridor is one entry, not four hundred cells.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub struct WallRect {
    /// Lower-left corner x, in cells.
    pub x: f32,
    /// Lower-left corner y, in cells.
    pub y: f32,
    /// Width in cells.
    pub w: f32,
    /// Height in cells.
    pub h: f32,
}

/// One explicitly placed food pile, in cells and food units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FoodSpot {
    /// Centre x in cells.
    pub x: f32,
    /// Centre y in cells.
    pub y: f32,
    /// Pile radius in cells.
    pub radius: f32,
    /// Food units in the pile.
    pub amount: f32,
}

/// Tunable simulation parameters.
#[derive(Resource, Debug, Clone)]
pub struct SimConfig {
    // ---- world geometry -------------------------------------------------
    /// Grid width in cells.
    pub width: usize,
    /// Grid height in cells.
    pub height: usize,
    /// Ants **per colony**. With two colonies the population doubles.
    pub ant_count: usize,
    /// Number of competing colonies, 1..=[`MAX_COLONIES`].
    pub colony_count: usize,
    /// Nest centre for a single colony. `None` means "centre of the map".
    pub nest_pos: Option<(f32, f32)>,
    /// Distance from the map centre to each nest when there are two colonies.
    /// The nests sit opposite each other so the contest starts fair.
    pub nest_orbit: f32,
    /// An ant inside this radius of its nest is considered "home".
    pub nest_radius: f32,
    /// Radius of the disc ants are scattered in when they spawn.
    pub spawn_radius: f32,
    /// Spread of the per-ant behaviour traits, as the sigma of a log-normal.
    ///
    /// `0.0` makes every ant identical, which is what makes a colony expand as
    /// a hollow circle: an ensemble of identical random walkers is radially
    /// symmetric by construction, and no other parameter can break that. Above
    /// zero, a thin tail of bold scouts - fast, near-ballistic - appears
    /// alongside a bulk of timid foragers, and the colony develops structure
    /// instead of a ring.
    pub individuality: f32,

    // ---- pheromones -----------------------------------------------------
    /// Fraction of every pheromone cell that decays each tick:
    /// `concentration *= (1 - evaporation_rate)`.
    pub evaporation_rate: f32,
    /// Amount of pheromone an ant lays per tick (total mass, spread over the
    /// deposit kernel below).
    pub deposit_amount: f32,
    /// Radius (in cells) of the pheromone deposit kernel. `0` = single cell,
    /// `1` = 3x3 weighted kernel (smoother trails, easier to sense).
    pub deposit_radius: i32,
    /// Hard ceiling on a single cell's concentration.
    pub max_pheromone: f32,

    // ---- sensing / steering --------------------------------------------
    /// How far ahead the three antennae sample, in cells.
    pub sensor_distance: f32,
    /// Half-angle between the centre antenna and the two side antennae.
    pub sensor_angle: f32,
    /// Radius of the box averaged around each antenna sample point.
    pub sensor_box_radius: i32,
    /// Magnitude of the per-tick random heading jitter (radians).
    pub random_walk_strength: f32,
    /// How hard an ant turns toward the antenna that smells the most
    /// pheromone (radians). `0.0` disables trail following entirely, which is
    /// the cleanest way to demonstrate that the trails are doing the work.
    pub sensor_turn_gain: f32,
    /// Maximum heading change per tick when homing (radians).
    pub max_turn_rate: f32,
    /// Weight of home-pheromone steering for returning ants.
    ///
    /// `0.0` (the default) is the behaviour specified in the brief: returning
    /// ants steer straight at the nest. Raising it makes them follow the
    /// home-pheromone gradient instead.
    pub home_pheromone_weight: f32,
    /// Sensor readings below this are treated as "no signal".
    pub pheromone_deadzone: f32,
    /// Navigation strategy of returning ants. See [`HomingMode`].
    ///
    /// The default (`PathIntegration`) is the brief's direct homing and is
    /// bit-identical to every historical run; the other two are the answers to
    /// "what if the drawn wall sits between an ant and its nest".
    pub homing_mode: HomingMode,

    // ---- movement -------------------------------------------------------
    /// Cells travelled per tick.
    pub speed: f32,

    // ---- crowding -------------------------------------------------------
    /// Ants per 4x4-cell bin above which an ant is treated as jammed.
    ///
    /// Calibrated against measurement, not taste: a 4x4 bin is 16 cells and an
    /// ant sprite is ~2 cells across, so ~32 ants is a bin packed solid, and a
    /// *busy trail* - the densest thing an open map produces - measures about
    /// 10. 24 therefore sits comfortably above normal traffic and below a real
    /// jam, which is why the relief system is inert on an open map.
    pub crowd_threshold: f32,
    /// Distance an ant steps out of a jam per tick, in cells.
    ///
    /// Small on purpose. This is a jostle, not a teleport: `0.6` is a third of
    /// the default step length, so relief nudges an ant without ever dominating
    /// its pheromone-driven movement. `0.0` disables relief entirely.
    pub crowd_relief: f32,

    // ---- food -----------------------------------------------------------
    /// Number of food piles, 1..=8.
    pub food_pile_count: usize,
    /// Food units stored in each pile at t=0.
    pub food_per_pile: f32,
    /// Radius of each food pile in cells.
    pub food_pile_radius: f32,
    /// Minimum distance from the map centre to a food pile.
    pub food_distance_min: f32,
    /// Maximum distance from the map centre to a food pile.
    pub food_distance_max: f32,
    /// Load capacity of a single ant.
    pub ant_capacity: f32,

    // ---- obstacles ------------------------------------------------------
    /// Which obstacle shape to stamp out. See [`ObstaclePreset`].
    pub obstacle_preset: ObstaclePreset,
    /// Scenario walls stamped on top of the preset, in cells.
    ///
    /// Empty for an ordinary run. A scenario carries explicit rectangles so a
    /// custom map is shareable text rather than mouse strokes.
    pub custom_walls: Vec<WallRect>,
    /// Scenario food placement. Empty means the usual seeded ring; non-empty
    /// replaces generation with these exact piles, which is what makes a
    /// scenario a *map* and not just a parameter set.
    pub custom_food: Vec<FoodSpot>,

    // ---- predators ------------------------------------------------------
    /// Number of predators. `0` disables them entirely.
    pub predator_count: usize,
    /// Predator movement speed, in cells per tick.
    pub predator_speed: f32,
    /// How far a predator can see an ant, in cells.
    pub predator_sense: f32,
    /// Maximum heading change per tick for a predator.
    pub predator_turn_rate: f32,
    /// An ant inside this radius of a predator is eaten.
    pub predator_catch_radius: f32,
    /// How far a predator wanders per tick when it cannot see anything.
    pub predator_wander: f32,

    // ---- plumbing -------------------------------------------------------
    /// RNG seed. Same seed + same parameters = bit-identical run.
    pub seed: u64,
    /// How often (in ticks) a stats sample is recorded for the curve.
    pub sample_interval: u64,

    // ---- interactive-app only -------------------------------------------
    /// Simulation ticks executed per rendered frame.
    pub ticks_per_frame: u32,
    /// When true the tick schedule is skipped entirely.
    pub paused: bool,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            width: DEFAULT_MAP_SIZE,
            height: DEFAULT_MAP_SIZE,
            ant_count: DEFAULT_ANT_COUNT,
            colony_count: 1,
            nest_pos: None,
            nest_orbit: 55.0,
            nest_radius: 4.0,
            spawn_radius: 2.0,
            individuality: DEFAULT_INDIVIDUALITY,

            evaporation_rate: DEFAULT_EVAPORATION_RATE,
            deposit_amount: DEFAULT_DEPOSIT_AMOUNT,
            deposit_radius: 1,
            max_pheromone: 50.0,

            sensor_distance: DEFAULT_SENSOR_RADIUS,
            sensor_angle: 0.6,
            sensor_box_radius: 1,
            random_walk_strength: DEFAULT_RANDOM_WALK_STRENGTH,
            sensor_turn_gain: 0.6,
            max_turn_rate: 0.8,
            home_pheromone_weight: 0.0,
            pheromone_deadzone: 1.0e-4,
            homing_mode: HomingMode::PathIntegration,

            speed: DEFAULT_SPEED,

            crowd_threshold: 24.0,
            crowd_relief: 0.6,

            food_pile_count: 3,
            food_per_pile: 3_000.0,
            food_pile_radius: 6.0,
            food_distance_min: 70.0,
            food_distance_max: 115.0,
            ant_capacity: 1.0,

            obstacle_preset: ObstaclePreset::None,
            custom_walls: Vec::new(),
            custom_food: Vec::new(),

            predator_count: 0,
            predator_speed: 1.6,
            predator_sense: 26.0,
            predator_turn_rate: 0.35,
            predator_catch_radius: 3.0,
            predator_wander: 0.5,

            seed: DEFAULT_SEED,
            sample_interval: 10,

            ticks_per_frame: 1,
            paused: false,
        }
    }
}

impl SimConfig {
    /// Number of cells in the pheromone grid.
    #[inline]
    pub fn cell_count(&self) -> usize {
        self.width * self.height
    }

    /// Centre of the map.
    #[inline]
    pub fn map_center(&self) -> (f32, f32) {
        (self.width as f32 * 0.5, self.height as f32 * 0.5)
    }

    /// Nest centre for a single colony, defaulting to the middle of the map.
    #[inline]
    pub fn nest_center(&self) -> (f32, f32) {
        self.nest_pos.unwrap_or_else(|| self.map_center())
    }

    /// Number of colonies, clamped to the supported range.
    #[inline]
    pub fn colonies(&self) -> usize {
        self.colony_count.clamp(1, MAX_COLONIES)
    }

    /// Total ants across every colony.
    #[inline]
    pub fn total_ants(&self) -> usize {
        self.ant_count.saturating_mul(self.colonies())
    }

    /// Largest pile radius that fits the current map.
    #[inline]
    pub fn max_pile_radius(&self) -> f32 {
        (self.width.min(self.height) as f32 * 0.25).max(1.0)
    }

    /// Clamp every field into a range the simulation can actually run with.
    ///
    /// Called once at startup and again whenever the UI applies user edits, so
    /// a slider can never drive the world into a degenerate state.
    pub fn sanitize(&mut self) {
        self.width = self.width.clamp(32, 4096);
        self.height = self.height.clamp(32, 4096);
        self.ant_count = self.ant_count.clamp(1, 20_000);
        self.colony_count = self.colony_count.clamp(1, MAX_COLONIES);
        self.nest_radius = self.nest_radius.clamp(0.5, 64.0);
        self.spawn_radius = self.spawn_radius.clamp(0.0, 64.0);
        self.individuality = self.individuality.clamp(0.0, 3.0);
        // Two nests must both fit, and must not sit on top of each other.
        let max_orbit = self.width.min(self.height) as f32 * 0.5 - self.nest_radius - 2.0;
        self.nest_orbit = self
            .nest_orbit
            .clamp(self.nest_radius * 2.0 + 4.0, max_orbit.max(1.0));

        self.evaporation_rate = self.evaporation_rate.clamp(0.0, 1.0);
        self.deposit_amount = self.deposit_amount.clamp(0.0, 1_000.0);
        self.deposit_radius = self.deposit_radius.clamp(0, 3);
        self.max_pheromone = self.max_pheromone.max(1.0e-3);

        self.sensor_distance = self.sensor_distance.clamp(0.0, 64.0);
        self.sensor_angle = self.sensor_angle.clamp(0.0, std::f32::consts::FRAC_PI_2);
        self.sensor_box_radius = self.sensor_box_radius.clamp(0, 6);
        self.random_walk_strength = self.random_walk_strength.clamp(0.0, 3.0);
        self.sensor_turn_gain = self.sensor_turn_gain.clamp(0.0, 2.0);
        self.max_turn_rate = self.max_turn_rate.clamp(0.0, std::f32::consts::PI);
        self.home_pheromone_weight = self.home_pheromone_weight.clamp(0.0, 1.0);
        self.pheromone_deadzone = self.pheromone_deadzone.max(0.0);

        self.speed = self.speed.clamp(0.0, 32.0);

        self.crowd_threshold = self.crowd_threshold.clamp(1.0, 10_000.0);
        self.crowd_relief = self.crowd_relief.clamp(0.0, 8.0);

        self.food_pile_count = self.food_pile_count.clamp(1, 8);
        self.food_per_pile = self.food_per_pile.clamp(0.0, 1.0e9);
        self.food_pile_radius = self.food_pile_radius.clamp(1.0, self.max_pile_radius());
        self.food_distance_min = self.food_distance_min.max(1.0);
        self.food_distance_max = self.food_distance_max.max(self.food_distance_min);
        self.ant_capacity = self.ant_capacity.clamp(1.0e-3, 1.0e6);

        // Scenario map content is clamped, not dropped: a hand-written TOML
        // rectangle that pokes past an edge should build the part that fits,
        // the same way every other clamp behaves.
        for wall in &mut self.custom_walls {
            wall.w = wall.w.max(0.0);
            wall.h = wall.h.max(0.0);
        }
        let (w, h) = (self.width as f32, self.height as f32);
        let max_radius = self.max_pile_radius();
        for spot in &mut self.custom_food {
            let margin = self.food_pile_radius.max(1.0);
            spot.x = spot.x.clamp(margin, (w - margin).max(margin));
            spot.y = spot.y.clamp(margin, (h - margin).max(margin));
            spot.radius = spot.radius.clamp(1.0, max_radius);
            spot.amount = spot.amount.clamp(0.0, 1.0e9);
        }

        self.predator_count = self.predator_count.min(200);
        self.predator_speed = self.predator_speed.clamp(0.0, 32.0);
        self.predator_sense = self.predator_sense.clamp(0.0, 512.0);
        self.predator_turn_rate = self.predator_turn_rate.clamp(0.0, std::f32::consts::PI);
        self.predator_catch_radius = self.predator_catch_radius.clamp(0.0, 64.0);
        self.predator_wander = self.predator_wander.clamp(0.0, 3.0);

        self.sample_interval = self.sample_interval.clamp(1, 100_000);
        self.ticks_per_frame = self.ticks_per_frame.min(64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_clamps_absurd_configs() {
        let mut config = SimConfig {
            ant_count: 0,
            colony_count: 99,
            evaporation_rate: 5.0,
            food_pile_count: 99,
            sensor_box_radius: -4,
            predator_count: 10_000,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.ant_count, 1);
        assert_eq!(config.colony_count, MAX_COLONIES);
        assert_eq!(config.evaporation_rate, 1.0);
        assert_eq!(config.food_pile_count, 8);
        assert_eq!(config.sensor_box_radius, 0);
        assert_eq!(config.predator_count, 200);
    }

    #[test]
    fn total_ants_scales_with_the_colony_count() {
        let mut config = SimConfig {
            ant_count: 100,
            colony_count: 2,
            ..Default::default()
        };
        assert_eq!(config.total_ants(), 200);
        config.colony_count = 1;
        assert_eq!(config.total_ants(), 100);
    }

    #[test]
    fn nest_orbit_always_leaves_room_for_both_nests() {
        let mut config = SimConfig {
            width: 64,
            height: 64,
            nest_radius: 4.0,
            nest_orbit: 1_000.0,
            ..Default::default()
        };
        config.sanitize();
        // Must fit inside the map...
        assert!(config.nest_orbit + config.nest_radius < 32.0);
        // ...and must keep the two nests apart.
        assert!(config.nest_orbit > config.nest_radius * 2.0);
    }

    #[test]
    fn pile_radius_is_capped_by_the_map_size() {
        let mut config = SimConfig {
            width: 64,
            height: 64,
            food_pile_radius: 500.0,
            ..Default::default()
        };
        config.sanitize();
        assert!(config.food_pile_radius <= 16.0);
    }

    #[test]
    fn obstacle_presets_carry_their_geometry() {
        // The sync system relies on this: a changed slider must compare
        // unequal, or the wall would never rebuild.
        let a = ObstaclePreset::UWall {
            arm: 10.0,
            gap: 10.0,
            thickness: 2.0,
        };
        let b = ObstaclePreset::UWall {
            arm: 11.0,
            gap: 10.0,
            thickness: 2.0,
        };
        assert_ne!(a, b);
        assert_eq!(a, a);
        assert_ne!(a, ObstaclePreset::None);
    }

    /// Homing modes round-trip through their keywords, and the default stays
    /// the brief's behaviour - historical runs must remain bit-identical.
    #[test]
    fn homing_modes_roundtrip_through_their_keywords() {
        assert_eq!(HomingMode::default(), HomingMode::PathIntegration);
        assert_eq!(HomingMode::parse("path"), Some(HomingMode::PathIntegration));
        assert_eq!(
            HomingMode::parse("path-integration"),
            Some(HomingMode::PathIntegration)
        );
        assert_eq!(
            HomingMode::parse("DIRECT"),
            Some(HomingMode::PathIntegration)
        );
        assert_eq!(HomingMode::parse("wall-sense"), Some(HomingMode::WallSense));
        assert_eq!(HomingMode::parse("WHISKERS"), Some(HomingMode::WallSense));
        assert_eq!(HomingMode::parse("flow_field"), Some(HomingMode::FlowField));
        assert_eq!(HomingMode::parse("BFS"), Some(HomingMode::FlowField));
        assert_eq!(HomingMode::parse("nonsense"), None);
        // Keywords match the CLI flag spellings one to one.
        for mode in [
            HomingMode::PathIntegration,
            HomingMode::WallSense,
            HomingMode::FlowField,
        ] {
            assert_eq!(
                HomingMode::parse(mode.keyword()),
                Some(mode),
                "{mode:?} keyword round-trip"
            );
        }
    }
}
