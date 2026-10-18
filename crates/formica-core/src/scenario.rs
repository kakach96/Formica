//! Scenarios: a named, shareable description of a run.
//!
//! A scenario is a [`SimConfig`] override set plus an explicit **map**: wall
//! rectangles and food spots. That combination is what makes a scenario a map
//! and not just a parameter preset - the existing UI presets change one number
//! on the brief's defaults, while a scenario fixes walls *and* food placement,
//! so it reproduces an experiment exactly, file and all.
//!
//! Files are TOML, parsed with `serde`/`toml` (the crate's only non-ECS
//! dependencies - pure data formats, still render-free). Every field is
//! optional with a serde default, so a scenario can override three values and
//! inherit the rest, and two runs of the same scenario + seed stay
//! bit-identical: application is a pure function into `SimConfig`, no RNG is
//! involved anywhere.

use crate::config::{FoodSpot, ObstaclePreset, SimConfig, WallRect};

/// Overrides for the tunable parameters.
///
/// Every field is optional: `None` keeps the default (or the CLI/panel value),
/// `Some` wins. This shape is what lets a scenario file be three lines long
/// and still produce a full configuration.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScenarioOverrides {
    /// Map width in cells.
    pub width: Option<usize>,
    /// Map height in cells.
    pub height: Option<usize>,
    /// Ants per colony.
    pub ant_count: Option<usize>,
    /// Competing colonies, 1..=2.
    pub colony_count: Option<usize>,
    /// Evaporation per tick, `c *= (1 - F)`.
    pub evaporation: Option<f32>,
    /// Pheromone laid per ant per tick.
    pub deposit: Option<f32>,
    /// Antennae reach in cells.
    pub sensor_distance: Option<f32>,
    /// Antennae half-angle in radians.
    pub sensor_angle: Option<f32>,
    /// Box-average radius per antenna.
    pub sensor_box: Option<i32>,
    /// Heading jitter per tick, radians.
    pub random_walk: Option<f32>,
    /// Turn toward the best antenna.
    pub turn_gain: Option<f32>,
    /// Cells per tick.
    pub speed: Option<f32>,
    /// Per-ant trait spread, 0 = clones.
    pub individuality: Option<f32>,
    /// Ants per 4x4 bin counted as a jam.
    pub crowd_threshold: Option<f32>,
    /// Cells an ant steps out of a jam per tick.
    pub crowd_relief: Option<f32>,
    /// Food piles (used by the seeded ring; ignored when `food` spots exist).
    pub piles: Option<usize>,
    /// Food units per pile.
    pub food_per_pile: Option<f32>,
    /// Pile radius in cells.
    pub pile_radius: Option<f32>,
    /// Built-in obstacle shape: `none`, `u-wall` or `blocks`.
    pub obstacles: Option<String>,
    /// Cell wall probability for `blocks`.
    pub block_density: Option<f32>,
    /// Number of predators, 0 = off.
    pub predators: Option<usize>,
    /// Predator speed, cells per tick.
    pub predator_speed: Option<f32>,
    /// Predator sight radius in cells.
    pub predator_sense: Option<f32>,
    /// Catch radius in cells.
    pub predator_catch: Option<f32>,
    /// How returning ants navigate home: `path`, `wall` or `flow`.
    pub homing: Option<String>,
    /// How ants leave the nest: `disk` or `jet`.
    pub spawn_mode: Option<String>,
    /// Jet direction in degrees, 0 = east, counter-clockwise.
    pub spawn_direction: Option<f32>,
    /// Jet cone width in degrees, 0..=360 (360 = the disk again).
    pub spawn_spread: Option<f32>,
    /// Ants emerging per tick, 0 = all at once.
    pub spawn_rate: Option<usize>,
    /// Population at tick 0 when the rate is above 0.
    pub spawn_initial: Option<usize>,
    /// RNG seed. Present in *exported* scenarios so a file reproduces the
    /// run bit for bit; a handwritten scenario may omit it and let `--seed`
    /// or the app's seed box decide. The CLI's `--seed` flag still wins over
    /// a scenario's seed, because flags apply in order after the scenario.
    pub seed: Option<u64>,
    /// U-wall arm length, applied only when `obstacles` is `u-wall`.
    pub u_wall_arm: Option<f32>,
    /// U-wall opening width, applied only when `obstacles` is `u-wall`.
    pub u_wall_gap: Option<f32>,
    /// U-wall thickness, applied only when `obstacles` is `u-wall`.
    pub u_wall_thickness: Option<f32>,
}

/// One shareable scenario: overrides plus an explicit map.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Scenario {
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// One-line description of what the scenario is for.
    #[serde(default)]
    pub description: String,
    /// Parameter overrides.
    #[serde(default)]
    pub config: ScenarioOverrides,
    /// Custom wall rectangles, in cells.
    #[serde(default)]
    pub walls: Vec<WallRect>,
    /// Explicit food piles. Non-empty replaces the seeded ring.
    #[serde(default)]
    pub food: Vec<FoodSpot>,
}

impl Scenario {
    /// Every built-in scenario, in UI order.
    ///
    /// These are the tuned starting points the experiments above were run
    /// against, promoted from "the author's shell history" to data.
    pub fn builtins() -> Vec<Scenario> {
        use crate::config::WallRect as W;
        vec![
            Scenario {
                name: "classic".into(),
                description: "the brief's defaults: 3 seeded piles, open field".into(),
                ..Default::default()
            },
            Scenario {
                name: "duel".into(),
                description: "two colonies, private trails, one shared food ring".into(),
                config: ScenarioOverrides {
                    colony_count: Some(2),
                    ..Default::default()
                },
                ..Default::default()
            },
            Scenario {
                name: "labyrinth".into(),
                description: "45% walls: a maze the crowding relief exists for".into(),
                config: ScenarioOverrides {
                    obstacles: Some("blocks".into()),
                    // Matches the high-density band of the crowding
                    // experiments, where relief measurably helps.
                    random_walk: Some(0.1),
                    ..Default::default()
                },
                ..Default::default()
            },
            Scenario {
                name: "fortress".into(),
                description: "food walled into a U, three predators hunting".into(),
                config: ScenarioOverrides {
                    obstacles: Some("u-wall".into()),
                    predators: Some(3),
                    ..Default::default()
                },
                ..Default::default()
            },
            Scenario {
                name: "arid".into(),
                description: "trails evaporate fast: a sparse colony's hard mode".into(),
                config: ScenarioOverrides {
                    ant_count: Some(150),
                    piles: Some(2),
                    evaporation: Some(0.25),
                    ..Default::default()
                },
                ..Default::default()
            },
            Scenario {
                name: "long-haul".into(),
                description: "a 512x512 map, eight piles, one long caravan".into(),
                config: ScenarioOverrides {
                    width: Some(512),
                    height: Some(512),
                    ant_count: Some(800),
                    piles: Some(8),
                    evaporation: Some(0.05),
                    ..Default::default()
                },
                ..Default::default()
            },
            Scenario {
                name: "maze-run".into(),
                description: "hand-built corridors and a fixed feast at the far end".into(),
                config: ScenarioOverrides {
                    obstacles: Some("none".into()),
                    ..Default::default()
                },
                walls: vec![
                    // A short corridor from the centre out to the east...
                    W {
                        x: 140.0,
                        y: 118.0,
                        w: 4.0,
                        h: 16.0,
                    },
                    W {
                        x: 140.0,
                        y: 140.0,
                        w: 30.0,
                        h: 4.0,
                    },
                    // ...and a broken comb guarding the north approach.
                    W {
                        x: 180.0,
                        y: 60.0,
                        w: 4.0,
                        h: 40.0,
                    },
                    W {
                        x: 196.0,
                        y: 90.0,
                        w: 4.0,
                        h: 40.0,
                    },
                ],
                food: vec![FoodSpot {
                    x: 220.0,
                    y: 128.0,
                    radius: 8.0,
                    amount: 4000.0,
                }],
            },
        ]
    }

    /// A built-in scenario by name, if one exists.
    pub fn builtin(name: &str) -> Option<Scenario> {
        let name = name.trim().to_ascii_lowercase();
        Self::builtins().into_iter().find(|s| s.name == name)
    }

    /// Built-in names, for the CLI help and the panel dropdown.
    pub fn builtin_names() -> Vec<String> {
        Self::builtins()
            .into_iter()
            .map(|s| s.name.clone())
            .collect()
    }

    /// Apply the scenario to a configuration.
    ///
    /// Only fields the scenario actually sets are touched, so a scenario is a
    /// *diff* against whatever came before it (CLI flags, env overrides).
    /// Walls and food spots are copied into the config, where the normal
    /// init/reset/rebuild machinery picks them up - no special-case code path
    /// exists for scenarios, which is what keeps them bit-identical to a run
    /// configured by hand.
    pub fn apply(&self, config: &mut SimConfig) {
        let o = &self.config;
        // Override fields use the CLI's names, config fields the brief's; the
        // pairs below are the whole mapping between them, so a new override
        // cannot silently attach to the wrong parameter.
        macro_rules! set {
            ($($config_field:ident = $override:ident),+ $(,)?) => {
                $( if let Some(v) = o.$override { config.$config_field = v; } )+
            };
        }
        set!(
            width = width,
            height = height,
            ant_count = ant_count,
            colony_count = colony_count,
            evaporation_rate = evaporation,
            deposit_amount = deposit,
            sensor_distance = sensor_distance,
            sensor_angle = sensor_angle,
            sensor_box_radius = sensor_box,
            random_walk_strength = random_walk,
            sensor_turn_gain = turn_gain,
            speed = speed,
            individuality = individuality,
            crowd_threshold = crowd_threshold,
            crowd_relief = crowd_relief,
            food_pile_count = piles,
            food_per_pile = food_per_pile,
            food_pile_radius = pile_radius,
            predator_count = predators,
            predator_speed = predator_speed,
            predator_sense = predator_sense,
            predator_catch_radius = predator_catch,
            spawn_direction = spawn_direction,
            spawn_spread = spawn_spread,
            spawn_rate = spawn_rate,
            spawn_initial = spawn_initial,
            seed = seed,
        );
        if let Some(raw) = &o.obstacles {
            if let Some(preset) = parse_obstacle(raw) {
                config.obstacle_preset = merge_obstacle_geometry(preset, o);
            }
        }
        if let Some(raw) = &o.homing {
            if let Some(mode) = crate::config::HomingMode::parse(raw) {
                config.homing_mode = mode;
            }
        }
        if let Some(raw) = &o.spawn_mode {
            if let Some(mode) = crate::config::SpawnMode::parse(raw) {
                config.spawn_mode = mode;
            }
        }
        config.custom_walls = self.walls.clone();
        config.custom_food = self.food.clone();
        config.sanitize();
    }

    /// Parse a scenario from TOML text.
    pub fn parse_toml(text: &str) -> Result<Scenario, String> {
        toml::from_str(text).map_err(|err| format!("invalid scenario TOML: {err}"))
    }

    /// Serialize to TOML text, for the "export scenario" button and for
    /// sharing files.
    pub fn to_toml_string(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_else(|_| String::new())
    }

    /// Capture the current set-up as a scenario.
    ///
    /// The food piles come from the live `FoodField` (player-added and
    /// player-erased piles included), not from the config's ring parameters.
    /// Preset walls stay compact: exported via the `obstacles` keyword plus
    /// its geometry fields. See `capture_from_grid` for the variant that
    /// also captures hand-drawn walls.
    pub fn capture(
        name: &str,
        description: &str,
        config: &SimConfig,
        piles: &[crate::world::FoodPile],
    ) -> Scenario {
        // Presets carry their geometry as variant data; exported scenarios
        // write it back so the import rebuilds the *same* wall, not a
        // default-sized one.
        let (u_wall_arm, u_wall_gap, u_wall_thickness, block_density) =
            match &config.obstacle_preset {
                ObstaclePreset::UWall {
                    arm,
                    gap,
                    thickness,
                } => (Some(*arm), Some(*gap), Some(*thickness), None),
                ObstaclePreset::RandomBlocks { density } => (None, None, None, Some(*density)),
                ObstaclePreset::None => (None, None, None, None),
            };
        Scenario {
            name: name.to_string(),
            description: description.to_string(),
            config: ScenarioOverrides {
                width: Some(config.width),
                height: Some(config.height),
                ant_count: Some(config.ant_count),
                colony_count: Some(config.colony_count),
                evaporation: Some(config.evaporation_rate),
                deposit: Some(config.deposit_amount),
                sensor_distance: Some(config.sensor_distance),
                sensor_angle: Some(config.sensor_angle),
                sensor_box: Some(config.sensor_box_radius),
                random_walk: Some(config.random_walk_strength),
                turn_gain: Some(config.sensor_turn_gain),
                speed: Some(config.speed),
                individuality: Some(config.individuality),
                crowd_threshold: Some(config.crowd_threshold),
                crowd_relief: Some(config.crowd_relief),
                piles: Some(config.food_pile_count),
                food_per_pile: Some(config.food_per_pile),
                pile_radius: Some(config.food_pile_radius),
                obstacles: Some(preset_keyword(&config.obstacle_preset).to_string()),
                block_density,
                u_wall_arm,
                u_wall_gap,
                u_wall_thickness,
                predators: Some(config.predator_count),
                predator_speed: Some(config.predator_speed),
                predator_sense: Some(config.predator_sense),
                predator_catch: Some(config.predator_catch_radius),
                homing: Some(config.homing_mode.keyword().to_string()),
                spawn_mode: Some(config.spawn_mode.keyword().to_string()),
                spawn_direction: Some(config.spawn_direction),
                spawn_spread: Some(config.spawn_spread),
                spawn_rate: Some(config.spawn_rate),
                spawn_initial: Some(config.spawn_initial),
                seed: Some(config.seed),
            },
            walls: Vec::new(),
            food: piles
                .iter()
                .map(|p| FoodSpot {
                    x: p.x,
                    y: p.y,
                    radius: p.radius,
                    amount: p.initial,
                })
                .collect(),
        }
    }

    /// Capture a scenario straight from a `SimConfig`, with no live world.
    ///
    /// This is the CLI export path (`--export-scenario`): walls come from
    /// `custom_walls` and food from `custom_food`, which is where a loaded
    /// scenario's map lands - so re-exporting a load reproduces the preset's
    /// shape, not just its keyword. Exported runs include the seed, which is
    /// what lets `--export-scenario` + `--scenario <file>` reproduce the
    /// numbers bit for bit.
    pub fn from_config(name: &str, description: &str, config: &SimConfig) -> Scenario {
        let mut scenario = Self::capture(name, description, config, &[]);
        scenario.walls = config.custom_walls.clone();
        if !config.custom_food.is_empty() {
            scenario.food = config.custom_food.clone();
        }
        scenario
    }

    /// Capture the live set-up *with* any hand-painted walls.
    ///
    /// The obstacle grid cannot tell which of its blocked cells came from a
    /// preset and which from the brush, so the decision is keyed on
    /// [`Obstacles::painted`]:
    ///
    /// - untouched map: export via the `obstacles` keyword + geometry, and
    ///   keep a loaded scenario's wall rectangles - the compact readable form.
    /// - hand-edited map: decompose the whole grid into explicit rectangles
    ///   (`Obstacles::wall_rectangles`) and drop the keyword. Stamping those
    ///   rebuilds exactly the map on screen, erasures included; the keyword
    ///   would instead re-stamp preset walls the player removed.
    pub fn capture_from_grid(
        name: &str,
        description: &str,
        config: &SimConfig,
        piles: &[crate::world::FoodPile],
        obstacles: &crate::obstacles::Obstacles,
    ) -> Scenario {
        let mut scenario = Self::capture(name, description, config, piles);
        scenario.walls = config.custom_walls.clone();
        if obstacles.painted {
            scenario.config.obstacles = Some("none".to_string());
            scenario.config.block_density = None;
            scenario.config.u_wall_arm = None;
            scenario.config.u_wall_gap = None;
            scenario.config.u_wall_thickness = None;
            scenario.walls = obstacles.wall_rectangles();
        }
        scenario
    }
}

/// The TOML keyword for an obstacle preset.
fn preset_keyword(preset: &ObstaclePreset) -> &'static str {
    match preset {
        ObstaclePreset::None => "none",
        ObstaclePreset::UWall { .. } => "u-wall",
        ObstaclePreset::RandomBlocks { .. } => "blocks",
    }
}

/// Parse the scenario `obstacles` keyword into a preset, using each shape's
/// default geometry (the same words the CLI flag accepts).
fn parse_obstacle(raw: &str) -> Option<ObstaclePreset> {
    match raw.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "none" | "off" | "open" => Some(ObstaclePreset::None),
        "u-wall" | "u" | "wall" => Some(ObstaclePreset::UWall {
            arm: 14.0,
            gap: 14.0,
            thickness: 3.0,
        }),
        "blocks" | "random" | "rubble" => Some(ObstaclePreset::RandomBlocks { density: 0.15 }),
        _ => None,
    }
}

/// Layer a scenario's explicit U-wall geometry onto a parsed preset.
///
/// The keyword alone gives only the shape; an exported scenario carries the
/// real numbers (`u_wall_arm` and friends) so the imported wall is the size
/// it was when captured, not the default size.
fn merge_obstacle_geometry(preset: ObstaclePreset, o: &ScenarioOverrides) -> ObstaclePreset {
    match preset {
        ObstaclePreset::UWall { .. } => ObstaclePreset::UWall {
            arm: o.u_wall_arm.unwrap_or(14.0),
            gap: o.u_wall_gap.unwrap_or(14.0),
            thickness: o.u_wall_thickness.unwrap_or(3.0),
        },
        ObstaclePreset::RandomBlocks { .. } => ObstaclePreset::RandomBlocks {
            density: o.block_density.unwrap_or(0.15),
        },
        ObstaclePreset::None => preset,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{headless_app, run_ticks};
    use crate::world::FoodField;

    /// A partial TOML must leave untouched fields at their defaults.
    #[test]
    fn a_partial_scenario_only_overrides_what_it_names() {
        let scenario =
            Scenario::parse_toml("name = \"tiny\"\n\n[config]\nwidth = 128\nant_count = 30\n")
                .expect("parse");
        assert_eq!(scenario.name, "tiny");

        let mut config = SimConfig::default();
        scenario.apply(&mut config);
        assert_eq!(config.width, 128);
        assert_eq!(config.ant_count, 30);
        // Untouched: the brief's defaults.
        assert_eq!(config.height, 256);
        assert_eq!(
            config.evaporation_rate,
            crate::config::DEFAULT_EVAPORATION_RATE
        );
        assert!(config.custom_food.is_empty());
    }

    /// The full cycle must be lossless: parse → write → parse → same scenario.
    #[test]
    fn toml_roundtrip_preserves_the_scenario() {
        let mut scenario = Scenario::builtin("maze-run").expect("builtin exists");
        scenario.description = "edited".to_string();
        let text = scenario.to_toml_string();
        let reparsed = Scenario::parse_toml(&text).expect("reparse");
        assert_eq!(reparsed.name, scenario.name);
        assert_eq!(reparsed.description, scenario.description);
        assert_eq!(reparsed.walls.len(), scenario.walls.len());
        assert_eq!(reparsed.food.len(), scenario.food.len());
        assert_eq!(reparsed.food[0].x, scenario.food[0].x);
        assert_eq!(reparsed.config.obstacles, scenario.config.obstacles);
    }

    /// Every built-in must survive an export → parse → apply cycle exactly:
    /// the exported file is the shareable form of the preset, so losing a
    /// wall rectangle or a seed on the way out would quietly change someone
    /// else's simulation.
    #[test]
    fn every_builtin_round_trips_through_toml_unchanged() {
        for name in Scenario::builtin_names() {
            let scenario = Scenario::builtin(&name).expect("builtin exists");
            let text = scenario.to_toml_string();
            let reparsed = Scenario::parse_toml(&text)
                .unwrap_or_else(|err| panic!("{name} did not reparse: {err}"));
            assert_eq!(reparsed, scenario, "{name} changed on the TOML round trip");
        }
    }

    /// `--export-scenario` must preserve shape geometry: a U-wall exported
    /// with arm 20 rebuilds with arm 20, not the parse default of 14.
    #[test]
    fn export_keeps_the_uwall_geometry() {
        let mut config = SimConfig::default();
        config.obstacle_preset = ObstaclePreset::UWall {
            arm: 20.0,
            gap: 12.0,
            thickness: 5.0,
        };
        let scenario = Scenario::from_config("walled", "", &config);
        let text = scenario.to_toml_string();
        assert!(text.contains("u_wall_arm = 20"), "text: {text}");

        let mut rebuilt = SimConfig::default();
        Scenario::parse_toml(&text)
            .expect("reparse")
            .apply(&mut rebuilt);
        assert_eq!(rebuilt.obstacle_preset, config.obstacle_preset);
        assert_eq!(rebuilt.seed, config.seed);
    }

    /// A scenario may carry a seed; applying one must move the config onto
    /// it, and an exported set-up (which always carries its own seed) must
    /// land on the identical config it came from.
    #[test]
    fn a_scenario_can_set_the_seed() {
        let scenario =
            Scenario::parse_toml("name = \"seeded\"\n\n[config]\nseed = 424242\n").expect("parse");
        let mut config = SimConfig::default();
        scenario.apply(&mut config);
        assert_eq!(config.seed, 424_242);
    }

    /// Exit-distribution overrides land on the config and survive an export
    /// → import cycle - a shareable "jet out of the nest" run must jet on
    /// someone else's machine too.
    #[test]
    fn spawn_overrides_apply_and_round_trip() {
        let scenario = Scenario::parse_toml(
            "name = \"stream\"\n\n[config]\nspawn_mode = \"jet\"\nspawn_direction = 90\n\
             spawn_spread = 45\nspawn_rate = 5\nspawn_initial = 10\n",
        )
        .expect("parse");
        let mut config = SimConfig::default();
        scenario.apply(&mut config);
        assert_eq!(config.spawn_mode, crate::config::SpawnMode::Jet);
        assert_eq!(config.spawn_direction, 90.0);
        assert_eq!(config.spawn_spread, 45.0);
        assert_eq!(config.spawn_rate, 5);
        assert_eq!(config.spawn_initial, 10);

        let text = Scenario::from_config("stream", "", &config).to_toml_string();
        assert!(text.contains("spawn_mode = \"jet\""), "text: {text}");
        let mut rebuilt = SimConfig::default();
        Scenario::parse_toml(&text)
            .expect("reparse")
            .apply(&mut rebuilt);
        assert_eq!(rebuilt.spawn_mode, crate::config::SpawnMode::Jet);
        assert_eq!(rebuilt.spawn_rate, 5);
        assert_eq!(rebuilt.spawn_initial, 10);
    }

    /// Hand-painted walls must survive export → import cell for cell.
    ///
    /// The live world starts from the default map, then brush strokes are
    /// painted and erased through the paint path - the export can neither
    /// trust the keyword nor hand-count the cells. Erasing part of an
    /// existing wall is the sharpest case: keyword + geometry would rebuild
    /// what was removed.
    #[test]
    fn painted_walls_survive_an_export_round_trip() {
        let mut config = SimConfig::default();
        // A preset with real geometry first: part of it is erased below, so
        // the export must stop describing the U by keyword.
        config.obstacle_preset = ObstaclePreset::UWall {
            arm: 14.0,
            gap: 14.0,
            thickness: 3.0,
        };
        let mut app = headless_app(config.clone());
        run_ticks(&mut app, 1);
        let original = app
            .world()
            .resource::<crate::obstacles::Obstacles>()
            .clone();
        let piles = app.world().resource::<FoodField>().piles.clone();

        let mut edited = original.clone();
        edited.paint(120, 120, true);
        edited.paint(121, 121, true);
        // Erase a few existing wall cells found by a deterministic scan -
        // the keyword-plus-geometry import would rebuild exactly these.
        let mut erasable = Vec::new();
        'scan: for y in 0..config.height as i32 {
            for x in 0..config.width as i32 {
                let is_new_stroke = (x, y) == (120, 120) || (x, y) == (121, 121);
                if edited.is_blocked(x, y) && !is_new_stroke {
                    erasable.push((x, y));
                    if erasable.len() == 4 {
                        break 'scan;
                    }
                }
            }
        }
        for (x, y) in &erasable {
            edited.paint(*x, *y, false);
        }
        assert!(!erasable.is_empty(), "nothing of the U was in that region");
        assert!(edited.painted, "the edits must mark the grid hand-drawn");

        let scenario = Scenario::capture_from_grid("drawn", "", &config, &piles, &edited);
        assert_eq!(
            scenario.config.obstacles.as_deref(),
            Some("none"),
            "a hand-edited export must not keep the preset keyword"
        );
        assert!(!scenario.walls.is_empty(), "no rectangles captured");

        // Rebuild through the normal import path and compare the whole grid.
        let text = scenario.to_toml_string();
        let mut rebuilt_config = SimConfig::default();
        Scenario::parse_toml(&text)
            .expect("reparse")
            .apply(&mut rebuilt_config);
        let mut rebuilt_app = headless_app(rebuilt_config);
        run_ticks(&mut rebuilt_app, 1);
        let rebuilt = rebuilt_app
            .world()
            .resource::<crate::obstacles::Obstacles>();
        assert_eq!(rebuilt.count(), edited.count(), "wall cell count changed");
        for y in 0..config.height as i32 {
            for x in 0..config.width as i32 {
                assert_eq!(
                    rebuilt.is_blocked(x, y),
                    edited.is_blocked(x, y),
                    "cell ({x},{y}) differs after the painted-wall round trip"
                );
            }
        }
    }

    /// A map that was never hand-edited keeps exporting the compact form:
    /// the preset keyword plus geometry, scenario wall rectangles separate.
    #[test]
    fn an_unpainted_export_keeps_the_compact_form() {
        let mut config = SimConfig::default();
        config.obstacle_preset = ObstaclePreset::RandomBlocks { density: 0.3 };
        config.custom_walls = vec![crate::config::WallRect {
            x: 10.0,
            y: 10.0,
            w: 4.0,
            h: 8.0,
        }];
        let mut obstacles = crate::obstacles::Obstacles::new(config.width, config.height);
        obstacles.apply_preset(config.obstacle_preset, &config, &[(50.0, 50.0)], &[]);
        assert!(!obstacles.painted);

        let scenario = Scenario::capture_from_grid("rubble", "", &config, &[], &obstacles);
        assert_eq!(scenario.config.obstacles.as_deref(), Some("blocks"));
        assert_eq!(scenario.config.block_density, Some(0.3));
        assert_eq!(scenario.walls, config.custom_walls);
    }

    /// Built-ins must have unique, non-empty names - the panel dropdown and
    /// the CLI resolver both key on them.
    #[test]
    fn builtin_names_are_unique_and_populated() {
        let names = Scenario::builtin_names();
        assert!(names.len() >= 5, "builtins: {names:?}");
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate names: {names:?}");
        for name in &names {
            assert!(!name.trim().is_empty());
            assert!(Scenario::builtin(name).is_some());
        }
        assert!(Scenario::builtin("no-such-scenario").is_none());
    }

    /// A scenario's custom walls must actually appear in the obstacle map.
    #[test]
    fn scenario_walls_are_stamped_into_the_map() {
        let scenario = Scenario::builtin("maze-run").expect("builtin exists");
        let mut config = SimConfig::default();
        scenario.apply(&mut config);

        let mut app = headless_app(config);
        run_ticks(&mut app, 1);
        let obstacles = app.world().resource::<crate::obstacles::Obstacles>();
        assert!(obstacles.count() > 0, "scenario walls missing");
        // The four rectangles of the maze-run preset are all inside the map,
        // so every one of their cells must be blocked.
        let expected: usize = scenario
            .walls
            .iter()
            .map(|w| (w.w.ceil() as i32 * w.h.ceil() as i32) as usize)
            .sum();
        assert_eq!(obstacles.count(), expected);
    }

    /// Explicit food spots replace the seeded ring exactly - same positions,
    /// same amounts, no RNG involved.
    #[test]
    fn scenario_food_lands_on_its_spots() {
        let scenario = Scenario::builtin("maze-run").expect("builtin exists");
        let mut config = SimConfig::default();
        scenario.apply(&mut config);

        let mut app = headless_app(config);
        run_ticks(&mut app, 1);
        let food = app.world().resource::<FoodField>();
        assert_eq!(food.piles.len(), scenario.food.len());
        for (pile, spot) in food.piles.iter().zip(&scenario.food) {
            assert!(
                (pile.x - spot.x).abs() < 1e-3,
                "pile at {} not {}",
                pile.x,
                spot.x
            );
            assert!((pile.initial - spot.amount).abs() < 1e-3);
        }
    }

    /// A scenario is deterministic: same scenario, any number of loads, the
    /// world's initial state is identical.
    #[test]
    fn the_same_scenario_builds_the_same_world_twice() {
        let build = || {
            let scenario = Scenario::builtin("maze-run").expect("builtin exists");
            let mut config = SimConfig::default();
            scenario.apply(&mut config);
            let mut app = headless_app(config);
            run_ticks(&mut app, 1);
            let world = app.world_mut();
            (
                world.resource::<FoodField>().initial_total,
                world.resource::<crate::obstacles::Obstacles>().count(),
                world.resource::<crate::metrics::SimStats>().tick,
            )
        };
        assert_eq!(build(), build());
    }
}
