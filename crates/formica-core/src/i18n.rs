//! Minimal bilingual (English / Simplified Chinese) UI text.
//!
//! The design goal is "zero dependencies, zero macros to learn": every user
//! facing string lives in the [`Strings`] struct as a `&'static str` field, and
//! there are exactly two copies of it - [`EN`] and [`ZH`]. A caller holds a
//! `&'static Strings` (the [`Lang`] enum dereferences to one) and reads fields
//! directly, so IDE completion and the compiler catch a missing translation.
//!
//! Everything that a user can read is routed through here:
//!
//! * the egui control panel (window app),
//! * the CLI help, progress and report text (headless runner),
//! * the labels on shared enums ([`ObstaclePreset`], the presets, the paint
//!   modes) and the curve verdicts in `metrics`.
//!
//! A `SimConfig` carries no language: the language is chosen once per process,
//! from the `FORMICA_LANG` environment variable (`zh` / `en`) or, in the
//! window app, overridden live from the panel. Terminal output only ever uses
//! English by default because the Windows console's code page cannot render
//! CJK reliably; `--lang zh` opts in explicitly.
//!
//! [`ObstaclePreset`]: crate::config::ObstaclePreset

/// Which language the UI speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lang {
    /// English (the default, also the fallback for any untranslated string).
    #[default]
    En,
    /// Simplified Chinese.
    Zh,
}

impl Lang {
    /// Every language, in UI order.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Zh];

    /// Parse a language from a `--lang` flag or `FORMICA_LANG` value.
    ///
    /// Accepts the usual spellings (`zh`, `zh-cn`, `chinese`, `en`, `english`,
    /// case-insensitive). `None` for anything unrecognised - the caller keeps
    /// its default rather than erroring, because a language guess should never
    /// be a hard failure.
    pub fn parse(raw: &str) -> Option<Lang> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "en" | "english" | "en-us" | "en_us" => Some(Lang::En),
            "zh" | "cn" | "chinese" | "zh-cn" | "zh_cn" | "zh-hans" | "simplified-chinese" => {
                Some(Lang::Zh)
            }
            _ => None,
        }
    }

    /// English short code, for display and diagnostics.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
        }
    }

    /// The string table this language selects.
    pub fn strings(self) -> &'static Strings {
        match self {
            Lang::En => &EN,
            Lang::Zh => &ZH,
        }
    }
}

/// One row of the language picker: `(code, display name)`.
pub fn lang_choices(lang: Lang) -> [(&'static str, &'static str); 2] {
    match lang {
        Lang::En => [("en", "English"), ("zh", "中文")],
        // The Chinese UI keeps the native names, which are not translations of
        // each other but the conventional way each language names itself.
        Lang::Zh => [("en", "English"), ("zh", "中文")],
    }
}

/// All user-facing text, in English and Chinese.
///
/// Grouped the way the two consumers are: window-panel strings first, then the
/// headless report, then shared enum labels. Field names match the English
/// meaning; the Chinese values are the translation.
pub struct Strings {
    // ---- window app: header, status grid, curve -------------------------
    /// Panel subtitle under the "Formica" heading.
    pub app_subtitle: &'static str,
    /// Delivery-curve panel title.
    pub curve_title: &'static str,
    /// Placeholder while the curve has too few samples to plot.
    pub curve_collecting: &'static str,
    /// Curve annotation: S-shape score.
    pub curve_s_score: &'static str,
    pub tick: &'static str,
    pub delivered: &'static str,
    pub colony_row: &'static str,
    pub round_trips: &'static str,
    pub ants_per_colony: &'static str,
    pub searching_returning: &'static str,
    pub food_left: &'static str,
    pub ants_eaten: &'static str,
    pub first_delivery: &'static str,
    /// Value of "first delivery" before anything has arrived.
    pub not_yet: &'static str,
    pub fps: &'static str,

    // ---- window app: run controls ---------------------------------------
    pub run_section: &'static str,
    pub pause: &'static str,
    pub pause_hint: &'static str,
    /// Shown as the tooltip of the pause/resume button while paused.
    pub resume: &'static str,
    /// Tooltip detail for the resume state of the pause/resume button.
    pub resume_hint: &'static str,
    pub restart: &'static str,
    pub restart_hint: &'static str,
    pub clear_trails: &'static str,
    pub clear_trails_hint: &'static str,
    pub ticks_per_frame: &'static str,
    pub ticks_per_frame_hint: &'static str,

    // ---- window app: presets ---------------------------------------------
    pub presets_section: &'static str,
    /// One-line "presets vs scenarios" distinction under the section header.
    pub presets_hint: &'static str,
    /// "applied: {name}" - shown under the preset grid after a click.
    pub preset_active_fmt: &'static str,
    pub preset_brief: &'static str,
    pub preset_brief_hint: &'static str,
    pub preset_no_pheromone: &'static str,
    pub preset_no_pheromone_hint: &'static str,
    pub preset_volatile: &'static str,
    pub preset_volatile_hint: &'static str,
    pub preset_persistent: &'static str,
    pub preset_persistent_hint: &'static str,
    pub preset_wall: &'static str,
    pub preset_wall_hint: &'static str,
    pub preset_hunted: &'static str,
    pub preset_hunted_hint: &'static str,
    pub preset_contest: &'static str,
    pub preset_contest_hint: &'static str,

    // ---- window app: world & food ----------------------------------------
    pub world_section: &'static str,
    pub size: &'static str,
    pub make_square: &'static str,
    pub make_square_hint: &'static str,
    pub apply_size: &'static str,
    pub apply_size_hint: &'static str,
    pub starting_piles: &'static str,
    pub starting_piles_hint: &'static str,
    pub food_per_pile: &'static str,
    pub pile_radius: &'static str,
    /// "{units} units on the map" - formatted with the total.
    pub units_on_map: &'static str,

    // ---- window app: mouse -------------------------------------------------
    pub mouse_section: &'static str,
    pub enable_map_editing: &'static str,
    pub brush_radius: &'static str,
    pub mouse_help: &'static str,
    pub mode_place_food: &'static str,
    pub mode_draw_wall: &'static str,
    pub mode_erase: &'static str,

    // ---- window app: obstacles --------------------------------------------
    pub obstacles_section: &'static str,
    /// Label above the preset dropdown.
    pub obstacle_kind: &'static str,
    pub obstacle_none: &'static str,
    pub obstacle_u_wall: &'static str,
    pub obstacle_blocks: &'static str,
    pub obstacle_none_hint: &'static str,
    pub obstacle_u_wall_hint: &'static str,
    pub arm_length: &'static str,
    pub clear_width: &'static str,
    pub thickness: &'static str,
    pub wall_density: &'static str,
    /// "{n} wall cells ({pct}% of the map)"
    pub wall_cells: &'static str,
    pub draw_your_own: &'static str,

    // ---- window app: homing navigation ---------------------------------------
    pub homing_section: &'static str,
    pub homing_mode: &'static str,
    pub homing_path: &'static str,
    pub homing_path_hint: &'static str,
    pub homing_wall: &'static str,
    pub homing_wall_hint: &'static str,
    pub homing_flow: &'static str,
    pub homing_flow_hint: &'static str,

    // ---- window app: colonies ----------------------------------------------
    pub competing_colonies: &'static str,
    pub competing_colonies_hint: &'static str,
    pub nest_spacing: &'static str,
    pub colony_colors_note: &'static str,

    // ---- window app: pheromones ---------------------------------------------
    pub pheromones_section: &'static str,
    pub evaporation: &'static str,
    pub evaporation_hint: &'static str,
    pub deposit_strength: &'static str,
    pub deposit_kernel_radius: &'static str,
    pub cell_ceiling: &'static str,

    // ---- window app: ants -----------------------------------------------------
    pub ants_section: &'static str,
    pub ants_per_colony_slider: &'static str,
    pub ants_per_colony_hint: &'static str,
    pub speed: &'static str,
    pub perception_radius: &'static str,
    pub perception_radius_hint: &'static str,
    pub antenna_spread: &'static str,
    pub antenna_box_radius: &'static str,
    pub random_walk: &'static str,
    pub trail_turn_gain: &'static str,
    pub trail_turn_gain_hint: &'static str,
    pub home_trail_guidance: &'static str,
    pub home_trail_guidance_hint: &'static str,
    pub individuality: &'static str,
    pub individuality_hint: &'static str,

    // ---- window app: exit distribution ----------------------------------------
    /// Section header inside the Ants panel.
    pub exit_section: &'static str,
    /// Line under the section header.
    pub exit_hint: &'static str,
    /// Mode picker label.
    pub exit_mode: &'static str,
    pub exit_disk: &'static str,
    pub exit_disk_hint: &'static str,
    pub exit_jet: &'static str,
    pub exit_jet_hint: &'static str,
    pub exit_direction: &'static str,
    pub exit_direction_hint: &'static str,
    pub exit_spread: &'static str,
    pub exit_spread_hint: &'static str,
    pub exit_rate: &'static str,
    pub exit_rate_hint: &'static str,
    pub exit_initial: &'static str,
    pub exit_initial_hint: &'static str,

    // ---- window app: crowding ---------------------------------------------------
    pub crowding_section: &'static str,
    pub jam_threshold: &'static str,
    pub jam_threshold_hint: &'static str,
    pub relief_step: &'static str,
    pub relief_step_hint: &'static str,
    /// "busiest 4x4 bin: {max} ants (peak {peak}), over {occupied} bins\n
    ///  an evenly spread colony would hold about {uniform:.1} per bin"
    pub busiest_bin_fmt: &'static str,

    // ---- window app: predators ----------------------------------------------------
    pub predators_section: &'static str,
    pub predators: &'static str,
    pub predators_hint: &'static str,
    pub sight_radius: &'static str,
    pub catch_radius: &'static str,
    pub wander: &'static str,
    pub predator_note: &'static str,

    // ---- window app: display ---------------------------------------------------------
    pub display_section: &'static str,
    pub food_trail_gain: &'static str,
    pub home_trail_gain: &'static str,
    pub gain_hint: &'static str,
    pub food_trail: &'static str,
    pub home_trail: &'static str,
    pub color_legend: &'static str,
    /// The actor-colour legend line (ants, predators, walls).
    pub legend_actors: &'static str,

    // ---- window app: language picker -----------------------------------------------------
    pub language: &'static str,

    // ---- window app: scenarios --------------------------------------------
    /// Scenario section header.
    pub scenario_section: &'static str,
    /// Hint under the builtin dropdown.
    pub scenario_builtins_hint: &'static str,
    /// Export button label.
    pub scenario_export: &'static str,
    /// Export button tooltip.
    pub scenario_export_hint: &'static str,
    /// "wrote the current set-up to {path}"
    pub scenario_exported_fmt: &'static str,
    /// "failed to write {path}: {err}"
    pub scenario_failed_fmt: &'static str,
    /// Preset export button label.
    pub scenario_export_preset: &'static str,
    /// Preset export button tooltip.
    pub scenario_export_preset_hint: &'static str,
    /// "wrote preset '{name}' to {path}"
    pub scenario_preset_exported_fmt: &'static str,
    /// Import button label.
    pub scenario_import: &'static str,
    /// Import button tooltip.
    pub scenario_import_hint: &'static str,
    /// "loaded scenario '{name}' from {path} - the world was rebuilt"
    pub scenario_imported_fmt: &'static str,
    /// "failed to load {path}: {err}"
    pub scenario_import_failed_fmt: &'static str,

    // ---- headless: CLI help -----------------------------------------------------------------
    pub help_text: &'static str,
    /// "running {ticks} ticks ({ants} ants x {colonies} colony, {w}x{h}, evaporation {evap:.3})..."
    pub running_fmt: &'static str,
    /// Progress line labels: `(tick, delivered, searching, returning, food left, eaten, busiest bin)`.
    /// The numbers are spliced in by [`Strings::progress_line`].
    pub progress_labels: [&'static str; 7],
    pub report_header: &'static str,
    pub report_world: &'static str,
    pub report_behaviour: &'static str,
    pub report_obstacles: &'static str,
    pub report_predators: &'static str,
    pub report_seed: &'static str,
    pub chart_title: &'static str,
    pub per_colony_title: &'static str,
    pub per_colony_fmt: &'static str,
    /// Milestone / result rows: the numeric part is spliced by the caller.
    pub milestones_title: &'static str,
    pub m_first_delivery: &'static str,
    pub m_10: &'static str,
    pub m_50: &'static str,
    pub m_90: &'static str,
    pub m_peak_rate: &'static str,
    pub result_title: &'static str,
    pub r_delivered: &'static str,
    pub r_recovery: &'static str,
    pub r_removed: &'static str,
    pub r_mean_rate: &'static str,
    pub r_peak_rate: &'static str,
    pub r_sigmoid: &'static str,
    pub r_still_rising: &'static str,
    pub r_yes: &'static str,
    pub r_no: &'static str,
    pub r_ants_at_end: &'static str,
    pub r_ants_eaten: &'static str,
    pub r_concentration: &'static str,
    pub verdict_label: &'static str,
    /// Units suffix shared by the result rows ("units", "units/tick").
    pub units_suffix: &'static str,
    /// "X units in N round trips" - the round-trip count noun.
    pub trips_suffix: &'static str,
    /// "N units left on the map" - the remainder phrase.
    pub map_remaining: &'static str,
    /// "units/tick" rate unit.
    pub rate_unit: &'static str,
    /// "(window of N ticks)" prefix.
    pub window_of: &'static str,
    /// "searching" state noun in the ants-at-end row.
    pub searching_label: &'static str,
    /// "returning" state noun in the ants-at-end row.
    pub returning_label: &'static str,
    /// "ants in the busiest 4x4 bin".
    pub busiest_bin: &'static str,
    /// "peak".
    pub peak_label: &'static str,
    /// "occupied bins".
    pub occupied_label: &'static str,
    /// "wrote {n} samples to {path}"
    pub csv_written_fmt: &'static str,
    /// "failed to write {path}: {err}"
    pub csv_failed_fmt: &'static str,
    /// "error: {msg}"
    pub error_prefix: &'static str,
    /// "{flag} needs a value"
    pub err_needs_value: &'static str,
    /// "{flag}: '{raw}' is not a valid number"
    pub err_not_a_number: &'static str,
    /// "unknown option '{flag}'"
    pub err_unknown_option: &'static str,
    /// "--obstacles: unknown kind '{kind}' (expected none, u-wall or blocks)"
    pub err_unknown_obstacle: &'static str,
    /// "unknown scenario '{name}' (built-ins: {list} or a .toml path)"
    pub err_unknown_scenario: &'static str,
    /// "--homing: unknown mode '{kind}' (expected path, wall or flow)"
    pub err_unknown_homing: &'static str,
    /// "unknown spawn mode '{kind}' for --spawn-mode (expected disk or jet)"
    pub err_unknown_spawn: &'static str,
    /// Report line label for the exit distribution (values are keywords).
    pub report_spawn: &'static str,
    /// Report line prefix for the homing row.
    pub report_homing: &'static str,
    /// Report line prefix for the scenario row.
    pub report_scenario: &'static str,
    pub sweep_header_fmt: &'static str,
    pub sweep_columns: &'static str,

    // ---- metrics: verdicts & chart --------------------------------------------------------------
    pub verdict_none: &'static str,
    pub verdict_strong: &'static str,
    pub verdict_s: &'static str,
    pub verdict_weak: &'static str,
    pub verdict_linear: &'static str,
    pub chart_no_samples: &'static str,

    // ---- enum labels -----------------------------------------------------------------------------
    pub obstacle_none_label: &'static str,
    pub obstacle_u_wall_label: &'static str,
    pub obstacle_blocks_label: &'static str,
}

/// English table.
pub static EN: Strings = Strings {
    app_subtitle: "ant colony foraging - simple rules, emergent trails",
    curve_title: "Delivery curve",
    curve_collecting: "collecting samples...",
    curve_s_score: "S-score",
    tick: "tick",
    delivered: "delivered",
    colony_row: "colony",
    round_trips: "round trips",
    ants_per_colony: "ants / colony",
    searching_returning: "searching / returning",
    food_left: "food left",
    ants_eaten: "ants eaten",
    first_delivery: "first delivery",
    not_yet: "not yet",
    fps: "fps",

    run_section: "Run",
    pause: "pause",
    pause_hint: "stop the tick schedule; the ants hold still",
    resume: "resume",
    resume_hint: "continue the tick schedule from where it stopped",
    restart: "reset",
    restart_hint: "fresh food, empty trails, zeroed statistics",
    clear_trails: "clear trails",
    clear_trails_hint: "zero every pheromone field without disturbing the ants",
    ticks_per_frame: "ticks per frame",
    ticks_per_frame_hint: "fast-forward: the tick schedule is simply run more often per frame",

    presets_section: "Presets & scenarios",
    presets_hint: "presets change behaviour parameters only; scenarios also rebuild the map (walls and food) and can be shared as files",
    preset_active_fmt: "applied: {name}",
    preset_brief: "Brief defaults",
    preset_brief_hint: "evaporation 0.10, the parameters from the project brief",
    preset_no_pheromone: "No pheromone",
    preset_no_pheromone_hint: "deposit 0, turn-gain 0 - pure random walk, for comparison",
    preset_volatile: "Volatile trail",
    preset_volatile_hint: "evaporation 0.30 - trails die faster than they form",
    preset_persistent: "Persistent trail",
    preset_persistent_hint: "evaporation 0.01 - trails barely decay, paths ossify",
    preset_wall: "U-shaped wall",
    preset_wall_hint: "food walled into a U - the straight path is blocked",
    preset_hunted: "Under predation",
    preset_hunted_hint: "predators on the map: time and cargo are lost, lives are not",
    preset_contest: "Two colonies",
    preset_contest_hint: "two colonies, private trails, one shared food supply",

    world_section: "World & food",
    size: "size",
    make_square: "make square",
    make_square_hint: "set both to the larger of the two",
    apply_size: "apply size",
    apply_size_hint: "reallocates the pheromone texture and rebuilds the scene, which discards the current run",
    starting_piles: "starting piles",
    starting_piles_hint: "applied on reset; click the map to add more",
    food_per_pile: "food per pile",
    pile_radius: "pile radius",
    units_on_map: "units on the map",

    mouse_section: "Map edit",
    enable_map_editing: "enable map editing",
    brush_radius: "brush radius",
    mouse_help: "left click applies the mode; right click erases food and wall",
    mode_place_food: "place food",
    mode_draw_wall: "draw wall",
    mode_erase: "erase",

    obstacles_section: "Obstacles",
    obstacle_kind: "obstacle kind",
    obstacle_none: "open field",
    obstacle_u_wall: "U-shaped wall",
    obstacle_blocks: "scattered blocks",
    obstacle_none_hint: "an open field: the baseline everything else is compared against",
    obstacle_u_wall_hint: "wraps the first pile, closed side facing the colony",
    arm_length: "arm length",
    clear_width: "clear width",
    thickness: "thickness",
    wall_density: "wall density",
    wall_cells: "wall cells",
    draw_your_own: "draw your own with the Map edit section above",

    homing_section: "Homing navigation",
    homing_mode: "mode",
    homing_path: "path integration",
    homing_path_hint: "steer straight at the nest (brief behaviour); walls between an ant and home pile it up",
    homing_wall: "wall whiskers",
    homing_wall_hint: "the antennae sense walls a few cells ahead; a felt wall is escaped hard, a nearby wall avoided gently",
    homing_flow: "nest flow field",
    homing_flow_hint: "follow a BFS distance field to the nest - routes around any wall, rebuilds on every map edit",

    competing_colonies: "competing colonies",
    competing_colonies_hint: "each colony has private pheromone fields and its own nest, but they share the same finite food",
    nest_spacing: "nest spacing",
    colony_colors_note: "colony 0 trails are green/violet, colony 1 are amber/cyan. Neither can smell the other.",

    pheromones_section: "Pheromones",
    evaporation: "evaporation /tick",
    evaporation_hint: "c *= (1 - rate) every tick",
    deposit_strength: "deposit strength",
    deposit_kernel_radius: "deposit kernel radius",
    cell_ceiling: "cell ceiling",

    ants_section: "Ants",
    ants_per_colony_slider: "ants per colony",
    ants_per_colony_hint: "applied live: ants are spawned at, or removed from, their nest",
    speed: "speed (cells/tick)",
    perception_radius: "perception radius",
    perception_radius_hint: "how far ahead the antennae sample, in cells",
    antenna_spread: "antenna spread (rad)",
    antenna_box_radius: "antenna box radius",
    random_walk: "random walk (rad)",
    trail_turn_gain: "trail turn gain",
    trail_turn_gain_hint: "0 disables trail following entirely",
    home_trail_guidance: "home-trail guidance",
    home_trail_guidance_hint: "0 = returning ants head straight for the nest (brief behaviour)",
    individuality: "individuality",
    individuality_hint: "spread of per-ant speed and wander. 0 makes every ant identical, which is what makes the colony expand as a hollow ring",

    exit_section: "Exit distribution",
    exit_hint: "every change rebuilds the world so all ants re-emerge with the new settings",
    exit_mode: "exit mode",
    exit_disk: "uniform disk",
    exit_disk_hint: "ants start scattered uniformly over the nest disc, facing outward",
    exit_jet: "directed jet",
    exit_jet_hint: "ants leave on bearings inside a cone around one direction; positions stay on the nest disc",
    exit_direction: "jet direction (°)",
    exit_direction_hint: "0° points east (+x), counter-clockwise; 90° is straight up",
    exit_spread: "jet spread (°)",
    exit_spread_hint: "width of the exit cone; 360° is statistically the uniform disk again",
    exit_rate: "emerge per tick",
    exit_rate_hint: "0 = all ants leave at once. Above 0 the colony opens with the initial count and the rest emerges from the nest at this rate",
    exit_initial: "initial ants",
    exit_initial_hint: "population at tick 0 while emerge-per-tick is on; ignored when the rate is 0",

    crowding_section: "Crowding",
    jam_threshold: "jam threshold",
    jam_threshold_hint: "ants sharing one 4x4-cell bin before relief starts",
    relief_step: "relief step (cells)",
    relief_step_hint: "0 disables crowding relief entirely",
    busiest_bin_fmt: "busiest 4x4 bin: {max} ants  (peak {peak}), over {occupied} bins\nan evenly spread colony would hold about {uniform:.1} per bin",

    predators_section: "Predators",
    predators: "predators",
    predators_hint: "0 turns predation off entirely",
    sight_radius: "sight radius",
    catch_radius: "catch radius",
    wander: "wander",
    predator_note: "eaten ants are replaced at their nest, so predation costs time and cargo rather than lives. Nests are a refuge.",

    display_section: "Display",
    food_trail_gain: "food trail gain",
    home_trail_gain: "home trail gain",
    gain_hint: "drop this to 0 to isolate the food trail, and vice versa",
    food_trail: "food trail",
    home_trail: "home trail",
    color_legend: "colony 0: green / violet    colony 1: amber / cyan",
    legend_actors: "ants: white / orange    predators: red    walls: grey",
    language: "language",

    scenario_section: "Scenario",
    scenario_builtins_hint: "pick one to load it; export saves a set-up as a TOML file, import loads one back",
    scenario_export: "export current set-up…",
    scenario_export_hint: "writes the current set-up to the file named in the box - config, every food pile, and any walls you drew",
    scenario_exported_fmt: "wrote the current set-up to {path}",
    scenario_failed_fmt: "failed to write {path}: {err}",
    scenario_export_preset: "export preset…",
    scenario_export_preset_hint: "writes the built-in currently selected in the dropdown, walls and food included",
    scenario_preset_exported_fmt: "wrote preset '{name}' to {path}",
    scenario_import: "import from file…",
    scenario_import_hint: "loads a scenario TOML (an exported file, for example) and rebuilds the world",
    scenario_imported_fmt: "loaded scenario '{name}' from {path} - the world was rebuilt",
    scenario_import_failed_fmt: "failed to load {path}: {err}",

    help_text: "\
Formica - ant colony foraging simulation (headless)

USAGE:
    formica [OPTIONS]

COLONY
    --ants N              Ants per colony                  [default: 500]
    --colonies N          Competing colonies, 1..=2        [default: 1]
    --ticks N             Simulation ticks to run          [default: 5000]
    --seed N              RNG seed                         [default: 20240517]

WORLD
    --width N             Map width in cells               [default: 256]
    --height N            Map height in cells              [default: 256]
    --piles N             Food piles, 1..=8                [default: 3]
    --food-per-pile F     Food units per pile              [default: 3000]
    --pile-radius F       Food pile radius in cells        [default: 6]
    --nest-radius F       Nest radius in cells             [default: 4]
    --nest-orbit F        Nest distance from centre, 2 colonies  [default: 55]

SPAWN
    --spawn-mode KIND     disk | jet - how ants leave the nest  [default: disk]
    --spawn-direction F   Jet direction in degrees, 0 = east, CCW  [default: 0]
    --spawn-spread F      Jet cone width in degrees, 360 = disk  [default: 60]
    --spawn-rate N        Ants emerging per tick, 0 = all at once  [default: 0]
    --spawn-initial N     Ants at tick 0 when the rate is above 0  [default: 0]

OBSTACLES
    --obstacles KIND      none | u-wall | blocks           [default: none]
    --u-wall-arm F        U-wall arm length in cells       [default: 14]
    --u-wall-gap F        U-wall clear width in cells      [default: 14]
    --u-wall-thickness F  U-wall thickness in cells        [default: 3]
    --block-density F     Cell wall probability, 0..1      [default: 0.15]

PREDATORS
    --predators N         Number of predators, 0 = off     [default: 0]
    --predator-speed F    Cells per tick                   [default: 1.6]
    --predator-sense F    Sight radius in cells            [default: 26]
    --predator-catch F    Catch radius in cells            [default: 3]

PHEROMONES AND BEHAVIOUR
    --evaporation F       Decay per tick, c *= (1 - F)     [default: 0.1]
    --deposit F           Pheromone laid per ant per tick  [default: 1.0]
    --deposit-radius N    Deposit kernel radius, 0..=3     [default: 1]
    --sensor-distance F   Antenna reach in cells           [default: 3.0]
    --sensor-angle F      Antenna half-angle in radians    [default: 0.6]
    --sensor-box N        Box average radius per antenna   [default: 1]
    --random-walk F       Heading jitter per tick, radians [default: 0.3]
    --turn-gain F         Turn toward the best antenna     [default: 0.6]
    --speed F             Cells per tick                   [default: 2.0]
    --individuality F     Per-ant trait spread, 0 = clones [default: 0.6]

CROWDING
    --crowd-threshold F   Ants per 4x4 bin counted as a jam [default: 24]
    --crowd-relief F      Cells an ant steps out of a jam  [default: 0.6]

HOMING
    --homing MODE         path | wall | flow - how returning ants navigate
                          home when walls block the way  [default: path]

OUTPUT
    --sample-interval N   Ticks between curve samples      [default: 10]
    --report-every N      Progress line interval, 0=off    [default: 500]
    --csv PATH            Write the curve to a CSV file
    --quiet               Only print the final report
    --chart-width N       ASCII chart width                [default: 78]
    --chart-height N      ASCII chart height               [default: 16]

SCENARIOS
    --scenario S          Built-in name or a TOML file; the rest of the
                          command line still applies on top
    --export-scenario PATH  Write the parsed set-up (a loaded scenario's
                          walls and food included) to a TOML and exit

EXPERIMENTS
    --sweep-evaporation L  Comma-separated values; runs each and compares
    --sweep-random-walk L  Comma-separated values; runs each and compares
    --sweep-predators L    Comma-separated predator counts; runs each
    --sweep-individuality L  Comma-separated trait-spread values; runs each

    -h, --help            Show this help

EXIT CODE
    0 if any colony delivered food, 1 if none did.
",
    running_fmt: "running {ticks} ticks ({ants} ants x {colonies} colony, {w}x{h}, evaporation {evap:.3})...",
    progress_labels: ["tick", "delivered", "searching", "returning", "food left", "eaten", "busiest bin"],
    report_header: "=== Formica colony foraging run ===",
    report_world: "world",
    report_behaviour: "behaviour",
    report_obstacles: "obstacles",
    report_predators: "predators",
    report_seed: "seed",
    chart_title: "--- cumulative food delivered vs tick (all colonies) ---",
    per_colony_title: "--- per colony ---",
    per_colony_fmt: "  colony {index}: delivered {:>8.1} ({:>5.1}% of the haul) in {:>5} trips, 50% at {:>6}, peak rate {:>6}",
    milestones_title: "--- milestones (tick at which N% of the final total was reached) ---",
    m_first_delivery: "first delivery",
    m_10: "10% delivered",
    m_50: "50% delivered",
    m_90: "90% delivered",
    m_peak_rate: "peak delivery rate",
    result_title: "--- result ---",
    r_delivered: "delivered",
    r_recovery: "recovery",
    r_removed: "removed by the player",
    r_mean_rate: "mean rate",
    r_peak_rate: "peak interval rate",
    r_sigmoid: "sigmoid score",
    r_still_rising: "still rising at end",
    r_yes: "yes",
    r_no: "no",
    r_ants_at_end: "ants at end",
    r_ants_eaten: "ants eaten",
    r_concentration: "concentration",
    verdict_label: "VERDICT",
    units_suffix: "units",
    trips_suffix: "round trips",
    map_remaining: "units left on the map:",
    rate_unit: "units/tick",
    window_of: "window of",
    searching_label: "searching",
    returning_label: "returning",
    busiest_bin: "ants in the busiest 4x4 bin",
    peak_label: "peak",
    occupied_label: "over",
    csv_written_fmt: "wrote {n} samples to {path}",
    csv_failed_fmt: "failed to write {path}: {err}",
    error_prefix: "error",
    err_needs_value: "needs a value",
    err_not_a_number: "'{raw}' is not a valid number",
    err_unknown_option: "unknown option '{flag}'",
    err_unknown_obstacle: "--obstacles: unknown kind '{kind}' (expected none, u-wall or blocks)",
    err_unknown_scenario: "unknown scenario '{name}' (built-ins: {list}, or a .toml file path)",
    err_unknown_homing: "--homing: unknown mode '{kind}' (expected path, wall or flow)",
    err_unknown_spawn: "--spawn-mode: unknown mode '{kind}' (expected disk or jet)",
    report_homing: "homing",
    report_spawn: "exit",
    report_scenario: "scenario",
    sweep_header_fmt: "=== sweep: {name} ({ticks} ticks, {ants} ants x {colonies} colony, {w}x{h}) ===",
    sweep_columns: "delivered", // column headers below are assembled per-language; see `sweep_table`
    verdict_none: "NO FORAGING - the colony never delivered any food",
    verdict_strong: "STRONG S-CURVE - clear lag, steep growth phase, plateau",
    verdict_s: "S-CURVE - growth clearly accelerates then flattens",
    verdict_weak: "WEAK S - some acceleration, mostly steady accumulation",
    verdict_linear: "LINEAR - constant rate, no positive feedback visible",
    chart_no_samples: "(no samples recorded)\n",

    obstacle_none_label: "open field",
    obstacle_u_wall_label: "U-shaped wall",
    obstacle_blocks_label: "scattered blocks",
};

/// Simplified Chinese table.
///
/// Console note: Windows terminals default to a code page that cannot render
/// CJK, so the headless runner only prints Chinese when asked (`--lang zh` or
/// `FORMICA_LANG=zh`); the window app switches freely because egui loads a
/// CJK-capable font.
pub static ZH: Strings = Strings {
    app_subtitle: "蚁群觅食模拟 - 简单规则,自发的通路",
    curve_title: "运回曲线",
    curve_collecting: "正在采样……",
    curve_s_score: "S 评分",
    tick: "tick",
    delivered: "已运回",
    colony_row: "蚁群",
    round_trips: "往返次数",
    ants_per_colony: "蚂蚁 / 窝",
    searching_returning: "觅食 / 归巢",
    food_left: "剩余食物",
    ants_eaten: "被吃蚂蚁",
    first_delivery: "首次运回",
    not_yet: "还没有",
    fps: "帧率",

    run_section: "运行",
    pause: "暂停",
    pause_hint: "暂停 tick 推进,蚂蚁原地停住",
    resume: "继续",
    resume_hint: "从暂停处继续推进 tick",
    restart: "重置",
    restart_hint: "重新生成食物、清空信息素、统计归零",
    clear_trails: "清空信息素",
    clear_trails_hint: "把所有信息素场清零,蚂蚁不动",
    ticks_per_frame: "每帧 tick 数",
    ticks_per_frame_hint: "快进:tick 调度每帧多跑几次而已",

    presets_section: "预设与场景",
    presets_hint: "预设只调行为参数;场景还会重建地图(墙与食物),并可存成文件分享",
    preset_active_fmt: "已应用:{name}",
    preset_brief: "任务书默认",
    preset_brief_hint: "蒸发率 0.10,项目任务书的参数",
    preset_no_pheromone: "无信息素",
    preset_no_pheromone_hint: "撒放 0、循迹增益 0 - 纯随机游走,作对照",
    preset_volatile: "易挥发痕迹",
    preset_volatile_hint: "蒸发率 0.30 - 痕迹消失得比形成还快",
    preset_persistent: "持久痕迹",
    preset_persistent_hint: "蒸发率 0.01 - 痕迹几乎不衰减,路径僵化",
    preset_wall: "U 形墙",
    preset_wall_hint: "食物被 U 形墙围住 - 直线路径走不通",
    preset_hunted: "捕食压力",
    preset_hunted_hint: "地图上有捕食者:损失的是时间和货物,不是命",
    preset_contest: "双蚁群",
    preset_contest_hint: "两窝蚂蚁、各自的信息素、共享同一批食物",

    world_section: "世界与食物",
    size: "尺寸",
    make_square: "取正方形",
    make_square_hint: "两边都取较大的那个",
    apply_size: "应用尺寸",
    apply_size_hint: "重新分配信息素纹理并重建场景,会丢弃当前这局",
    starting_piles: "初始食物堆",
    starting_piles_hint: "重置后生效;点地图可以加更多",
    food_per_pile: "每堆食物量",
    pile_radius: "食物堆半径",
    units_on_map: "单位食物在地图上",

    mouse_section: "地图编辑",
    enable_map_editing: "启用地图编辑",
    brush_radius: "刷子半径",
    mouse_help: "左键应用当前模式;右键擦除食物和墙",
    mode_place_food: "放食物",
    mode_draw_wall: "画墙",
    mode_erase: "擦除",

    obstacles_section: "障碍物",
    obstacle_kind: "障碍物类型",
    obstacle_none: "空地",
    obstacle_u_wall: "U 形墙",
    obstacle_blocks: "随机方块",
    obstacle_none_hint: "空地:所有其他配置的对照基线",
    obstacle_u_wall_hint: "围住第一个食物堆,实心一面朝向蚁巢",
    arm_length: "臂长",
    clear_width: "开口宽度",
    thickness: "墙厚",
    wall_density: "墙密度",
    wall_cells: "个墙格",
    draw_your_own: "用上面的「地图编辑」区自己画",

    homing_section: "归巢导航",
    homing_mode: "模式",
    homing_path: "航位推算",
    homing_path_hint: "直接朝蚁巢直线飞(基线行为);蚂蚁与巢穴之间有墙时会堆积在墙前",
    homing_wall: "触角避墙",
    homing_wall_hint: "触角提前数格感知墙体;感知到墙即强力转向逃逸,轻微贴墙则温和避让",
    homing_flow: "巢穴导航场",
    homing_flow_hint: "沿到巢穴的 BFS 距离场下山:可绕开任何墙体,每次改墙都会重建",

    competing_colonies: "竞争蚁群数",
    competing_colonies_hint: "每窝有自己的信息素场和巢穴,但共享同一批有限的食物",
    nest_spacing: "巢间距",
    colony_colors_note: "0 号窝的通路是绿/紫,1 号窝是琥珀/青。彼此闻不到对方的气味。",

    pheromones_section: "信息素",
    evaporation: "蒸发率 /tick",
    evaporation_hint: "每 tick 乘 (1 - rate)",
    deposit_strength: "撒放强度",
    deposit_kernel_radius: "撒放核半径",
    cell_ceiling: "单格上限",

    ants_section: "蚂蚁",
    ants_per_colony_slider: "每窝蚂蚁数",
    ants_per_colony_hint: "即时生效:蚂蚁在巢穴处生成或被移除",
    speed: "速度(格/tick)",
    perception_radius: "感知半径",
    perception_radius_hint: "触角向前采样的距离,以格计",
    antenna_spread: "触角张角(弧度)",
    antenna_box_radius: "触角盒平均半径",
    random_walk: "随机游走(弧度)",
    trail_turn_gain: "循迹转向增益",
    trail_turn_gain_hint: "0 = 完全关闭循迹",
    home_trail_guidance: "归巢气味引导",
    home_trail_guidance_hint: "0 = 归巢蚂蚁直线朝巢走(任务书行为)",
    individuality: "个体差异",
    individuality_hint: "每只蚂蚁速度与游走强度的差异幅度。0 = 全部一样,蚁群会扩成空心圆环",

    exit_section: "出巢分布",
    exit_hint: "任何改动都会重建世界,全部蚂蚁按新设置重新出巢",
    exit_mode: "出巢模式",
    exit_disk: "均匀圆盘",
    exit_disk_hint: "蚂蚁均匀散布在巢穴圆盘内,朝向外侧出发",
    exit_jet: "定向喷出",
    exit_jet_hint: "蚂蚁朝一个方向、在一个扇形内的方位出巢;位置仍散布在巢穴圆盘内",
    exit_direction: "喷出方向(°)",
    exit_direction_hint: "0° 指向右侧(+x),逆时针增大;90° 为正上方",
    exit_spread: "喷出扇角(°)",
    exit_spread_hint: "出巢扇形的张角;360° 时在统计上与均匀圆盘分布一致",
    exit_rate: "每tick出巢数",
    exit_rate_hint: "0 = 全部蚂蚁一次出巢。大于 0 时蚁群以初始数量开局,其余按此速率从巢中陆续出巢",
    exit_initial: "初始出巢数",
    exit_initial_hint: "分批出巢时 tick 0 的蚁群数量;每tick出巢数为 0 时不生效",

    crowding_section: "拥挤",
    jam_threshold: "堵塞阈值",
    jam_threshold_hint: "一个 4x4 方块里多少只蚂蚁算堵塞",
    relief_step: "缓解步长(格)",
    relief_step_hint: "0 = 完全关闭拥挤缓解",
    busiest_bin_fmt: "最挤的 4x4 方块:{max} 只蚂蚁(峰值 {peak}),超过 {occupied} 个方块\n均匀摊开的蚁群每个方块约 {uniform:.1} 只",

    predators_section: "捕食者",
    predators: "捕食者数量",
    predators_hint: "0 = 关闭捕食",
    sight_radius: "视野半径",
    catch_radius: "捕获半径",
    wander: "游走强度",
    predator_note: "被吃的蚂蚁会在巢穴补回,所以捕食损失的是时间和货物,不是命。巢穴是避难所。",

    display_section: "显示",
    food_trail_gain: "食物信息素增益",
    home_trail_gain: "归巢信息素增益",
    gain_hint: "调成 0 可以单独隔离另一个场",
    food_trail: "食物信息素",
    home_trail: "归巢信息素",
    color_legend: "0 号窝:绿 / 紫    1 号窝:琥珀 / 青",
    legend_actors: "蚂蚁:白 / 橙    捕食者:红    墙:灰",
    language: "语言",

    scenario_section: "场景",
    scenario_builtins_hint: "选择即加载;导出把设置存成 TOML 文件,导入则把文件加载回来",
    scenario_export: "导出当前设置…",
    scenario_export_hint: "把当前设置写入路径框指定的文件:配置、每堆食物,以及画上去的墙",
    scenario_exported_fmt: "当前设置已写入 {path}",
    scenario_failed_fmt: "写入 {path} 失败:{err}",
    scenario_export_preset: "导出预设场景…",
    scenario_export_preset_hint: "把下拉中选中的内置场景完整导出(含墙体与食物)",
    scenario_preset_exported_fmt: "预设 {name} 已写入 {path}",
    scenario_import: "从文件导入…",
    scenario_import_hint: "读取一个场景 TOML(例如之前导出的)并重建世界",
    scenario_imported_fmt: "已从 {path} 载入场景 {name},世界已重建",
    scenario_import_failed_fmt: "载入 {path} 失败:{err}",

    help_text: "\
Formica - 蚁群觅食模拟(无渲染命令行)

用法:
    formica [选项]

蚁群
    --ants N              每窝蚂蚁数                       [默认: 500]
    --colonies N          竞争蚁群数, 1..=2                [默认: 1]
    --ticks N             运行的仿真 tick 数               [默认: 5000]
    --seed N              随机种子                         [默认: 20240517]

世界
    --width N             地图宽度(格)                    [默认: 256]
    --height N            地图高度(格)                    [默认: 256]
    --piles N             食物堆数, 1..=8                  [默认: 3]
    --food-per-pile F     每堆食物量                       [默认: 3000]
    --pile-radius F       食物堆半径(格)                  [默认: 6]
    --nest-radius F       巢穴半径(格)                    [默认: 4]
    --nest-orbit F        双蚁群时巢穴到中心距离           [默认: 55]

出巢
    --spawn-mode KIND     disk | jet - 蚂蚁如何离开巢穴    [默认: disk]
    --spawn-direction F   喷出方向(度), 0 = 右, 逆时针     [默认: 0]
    --spawn-spread F      喷出扇角(度), 360 = 均匀圆盘     [默认: 60]
    --spawn-rate N        每tick出巢数, 0 = 一次全出       [默认: 0]
    --spawn-initial N     出巢速率大于 0 时 tick 0 的数量  [默认: 0]

障碍物
    --obstacles KIND      none | u-wall | blocks           [默认: none]
    --u-wall-arm F        U 形墙臂长(格)                  [默认: 14]
    --u-wall-gap F        U 形墙开口宽度(格)              [默认: 14]
    --u-wall-thickness F  U 形墙厚度(格)                  [默认: 3]
    --block-density F     格子是墙的概率, 0..1             [默认: 0.15]

捕食者
    --predators N         捕食者数量, 0 = 关闭             [默认: 0]
    --predator-speed F    速度(格/tick)                   [默认: 1.6]
    --predator-sense F    视野半径(格)                    [默认: 26]
    --predator-catch F    捕获半径(格)                    [默认: 3]

信息素与行为
    --evaporation F       每 tick 衰减, c *= (1 - F)       [默认: 0.1]
    --deposit F           每只蚂蚁每 tick 撒放量           [默认: 1.0]
    --deposit-radius N    撒放核半径, 0..=3                [默认: 1]
    --sensor-distance F   触角采样距离(格)                [默认: 3.0]
    --sensor-angle F      触角半张角(弧度)                [默认: 0.6]
    --sensor-box N        每个触角的盒平均半径             [默认: 1]
    --random-walk F       每 tick 朝向抖动(弧度)          [默认: 0.3]
    --turn-gain F         朝最优触角的转向量               [默认: 0.6]
    --speed F             速度(格/tick)                   [默认: 2.0]
    --individuality F     个体差异幅度, 0 = 全部一样       [默认: 0.6]

拥挤
    --crowd-threshold F   每个 4x4 方块多少只算堵塞        [默认: 24]
    --crowd-relief F      挤开时每 tick 挪多少格           [默认: 0.6]

归巢
    --homing MODE         path | wall | flow - 搬运蚂蚁如何绕墙回家
                                                    [默认: path]

输出
    --sample-interval N   曲线采样间隔(tick)              [默认: 10]
    --report-every N      进度行间隔, 0 = 关闭             [默认: 500]
    --csv PATH            把曲线写到 CSV 文件
    --quiet               只打印最终报告
    --chart-width N       ASCII 图宽                       [默认: 78]
    --chart-height N      ASCII 图高                       [默认: 16]

场景
    --scenario S          内置场景名或 TOML 文件;其余命令行参数仍然
                          叠加在其上
    --export-scenario PATH  把解析后的设置(含已加载场景的墙体与食物)
                          写成 TOML 并退出

实验
    --sweep-evaporation L  逗号分隔的值;逐个运行并对比
    --sweep-random-walk L  逗号分隔的值;逐个运行并对比
    --sweep-predators L    逗号分隔的捕食者数量;逐个运行
    --sweep-individuality L  逗号分隔的个体差异值;逐个运行

    -h, --help            显示本帮助

退出码
    运回了食物为 0,一只都没运回为 1。
",
    running_fmt: "正在运行 {ticks} tick({ants} 只蚂蚁 x {colonies} 窝,{w}x{h},蒸发率 {evap:.3})……",
    progress_labels: ["tick", "已运回", "觅食", "归巢", "剩余食物", "被吃", "最挤方块"],
    report_header: "=== Formica 蚁群觅食运行 ===",
    report_world: "世界",
    report_behaviour: "行为",
    report_obstacles: "障碍",
    report_predators: "捕食者",
    report_seed: "种子",
    chart_title: "--- 累计运回食物量 vs tick(全部蚁群)---",
    per_colony_title: "--- 分窝 ---",
    per_colony_fmt: "  蚁群 {index}: 运回 {:>8.1}(占 {:>5.1}%)共 {:>5} 次往返,50% 于 {:>6},峰值速率 {:>6}",
    milestones_title: "--- 里程碑(运回最终总量 N% 的 tick)---",
    m_first_delivery: "首次运回",
    m_10: "10% 运回",
    m_50: "50% 运回",
    m_90: "90% 运回",
    m_peak_rate: "峰值运回速率",
    result_title: "--- 结果 ---",
    r_delivered: "已运回",
    r_recovery: "回收率",
    r_removed: "玩家删除",
    r_mean_rate: "平均速率",
    r_peak_rate: "峰值区间速率",
    r_sigmoid: "S 形评分",
    r_still_rising: "结束时仍上升",
    r_yes: "是",
    r_no: "否",
    r_ants_at_end: "结束时蚂蚁",
    r_ants_eaten: "被吃蚂蚁",
    r_concentration: "聚集度",
    units_suffix: "单位",
    trips_suffix: "次往返",
    map_remaining: "单位剩余在地图上:",
    rate_unit: "单位/tick",
    window_of: "统计窗口",
    searching_label: "觅食",
    returning_label: "归巢",
    busiest_bin: "最挤 4x4 方块内蚂蚁数",
    peak_label: "峰值",
    occupied_label: "占用方块",
    verdict_label: "结论",
    csv_written_fmt: "已写入 {n} 个采样点到 {path}",
    csv_failed_fmt: "写入 {path} 失败:{err}",
    error_prefix: "错误",
    err_needs_value: "需要一个值",
    err_not_a_number: "'{raw}' 不是合法数字",
    err_unknown_option: "未知选项 '{flag}'",
    err_unknown_obstacle: "--obstacles:未知类型 '{kind}'(应为 none、u-wall 或 blocks)",
    err_unknown_scenario: "未知场景 '{name}'(内置:{list},或给一个 .toml 文件路径)",
    err_unknown_homing: "--homing:未知模式 '{kind}'(应为 path、wall 或 flow)",
    err_unknown_spawn: "--spawn-mode:未知模式 '{kind}'(应为 disk 或 jet)",
    report_homing: "归巢方式",
    report_spawn: "出巢",
    report_scenario: "场景",
    sweep_header_fmt: "=== 扫描:{name}({ticks} tick,{ants} 只蚂蚁 x {colonies} 窝,{w}x{h})===",
    sweep_columns: "运回量",
    verdict_none: "没有觅食 - 蚁群从未运回任何食物",
    verdict_strong: "强 S 形 - 滞后期、陡增期、平台期清晰",
    verdict_s: "S 形 - 增长先加速后放缓",
    verdict_weak: "弱 S - 有一定加速,大体仍是稳定累积",
    verdict_linear: "线性 - 恒定速率,看不到正反馈",
    chart_no_samples: "(没有采样记录)\n",

    obstacle_none_label: "空地",
    obstacle_u_wall_label: "U 形墙",
    obstacle_blocks_label: "随机方块",
};

impl Strings {
    /// One progress line of the headless runner, with the numbers spliced in.
    ///
    /// Column widths are fixed so that successive lines line up in a terminal;
    /// the labels themselves are what is localized.
    #[allow(clippy::too_many_arguments)]
    pub fn progress_line(
        &self,
        tick: u64,
        delivered: f64,
        recovered_pct: f64,
        searching: u32,
        returning: u32,
        food_left: f64,
        eaten: u64,
        busiest_bin: u32,
    ) -> String {
        let l = &self.progress_labels;
        format!(
            "  {:<5} {:>6}  {:<9} {:>9.1} ({:>5.1}%)  {:<9} {:>5}  {:<9} {:>5}  {:<9} {:>9.1}  {:<5} {}  {:<11} {}",
            l[0], tick, l[1], delivered, recovered_pct, l[2], searching, l[3], returning,
            l[4], food_left, l[5], eaten, l[6], busiest_bin
        )
    }

    /// Sweep table header: `(value, delivered, recovery, sigmoid, eaten, first tick)`.
    pub fn sweep_table(&self) -> [&'static str; 6] {
        if std::ptr::eq(self, &EN) {
            [
                "value",
                "delivered",
                "recovery",
                "sigmoid",
                "eaten",
                "1st tick",
            ]
        } else {
            ["取值", "运回量", "回收率", "S 评分", "被吃", "首达"]
        }
    }

    /// Dropdown label for a built-in scenario.
    ///
    /// The ASCII name stays the identifier (the CLI's `--scenario` and the
    /// TOML key on it), so in Chinese the localized short name rides beside
    /// it: `classic(经典)`. English keeps the bare keyword, which is also
    /// what an unknown name (a custom file) falls back to.
    pub fn scenario_display(&self, name: &str) -> String {
        if std::ptr::eq(self, &EN) {
            return name.to_string();
        }
        let zh = match name {
            "classic" => "经典",
            "duel" => "对决",
            "labyrinth" => "迷宫",
            "fortress" => "堡垒",
            "arid" => "荒原",
            "long-haul" => "长途",
            "maze-run" => "曲径",
            _ => return name.to_string(),
        };
        format!("{name}({zh})")
    }

    /// Localized description of a built-in scenario, for tooltips and the
    /// headless report.
    ///
    /// Empty outside Chinese and for unknown names: in English the caller
    /// falls back to the scenario's own text (which is the source of truth
    /// in [`crate::scenario::Scenario::builtins`]), and a custom TOML file
    /// keeps whatever the file itself says.
    pub fn scenario_description(&self, name: &str) -> &'static str {
        if std::ptr::eq(self, &EN) {
            return "";
        }
        match name {
            "classic" => "任务书默认:三堆种子食物,空旷场地",
            "duel" => "两窝竞争:各自的信息素通路,共享同一圈食物",
            "labyrinth" => "45% 墙体:拥挤缓解为之而生的迷宫",
            "fortress" => "食物被 U 形墙围住,三只捕食者巡猎",
            "arid" => "信息素蒸发极快:稀疏蚁群的高难模式",
            "long-haul" => "512x512 大地图,八堆食物,一列长途商队",
            "maze-run" => "手工搭建的走廊,尽头是一顿固定的盛宴",
            _ => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every field must be non-empty in both tables - a missing translation
    /// would otherwise surface as an empty label at runtime, not at compile
    /// time.
    #[test]
    fn every_string_is_non_empty_in_both_languages() {
        for lang in Lang::ALL {
            let s = lang.strings();
            macro_rules! check {
                ($($field:ident),+ $(,)?) => {
                    $(
                        assert!(
                            !s.$field.trim().is_empty(),
                            "{:?}.{} is empty",
                            lang,
                            stringify!($field)
                        );
                    )+
                };
            }
            check!(
                app_subtitle,
                curve_title,
                curve_collecting,
                curve_s_score,
                tick,
                delivered,
                colony_row,
                round_trips,
                ants_per_colony,
                searching_returning,
                food_left,
                ants_eaten,
                first_delivery,
                not_yet,
                fps,
                run_section,
                pause,
                resume,
                resume_hint,
                restart,
                restart_hint,
                clear_trails,
                clear_trails_hint,
                ticks_per_frame,
                ticks_per_frame_hint,
                pause,
                pause_hint,
                presets_section,
                presets_hint,
                preset_active_fmt,
                preset_brief,
                preset_brief_hint,
                preset_no_pheromone,
                preset_no_pheromone_hint,
                preset_volatile,
                preset_volatile_hint,
                preset_persistent,
                preset_persistent_hint,
                preset_wall,
                preset_wall_hint,
                preset_hunted,
                preset_hunted_hint,
                preset_contest,
                preset_contest_hint,
                world_section,
                size,
                make_square,
                make_square_hint,
                apply_size,
                apply_size_hint,
                starting_piles,
                starting_piles_hint,
                food_per_pile,
                pile_radius,
                units_on_map,
                mouse_section,
                enable_map_editing,
                brush_radius,
                mouse_help,
                mode_place_food,
                mode_draw_wall,
                mode_erase,
                obstacles_section,
                obstacle_kind,
                obstacle_none,
                obstacle_u_wall,
                obstacle_blocks,
                obstacle_none_hint,
                obstacle_u_wall_hint,
                arm_length,
                clear_width,
                thickness,
                wall_density,
                wall_cells,
                draw_your_own,
                competing_colonies,
                competing_colonies_hint,
                nest_spacing,
                colony_colors_note,
                pheromones_section,
                evaporation,
                evaporation_hint,
                deposit_strength,
                deposit_kernel_radius,
                cell_ceiling,
                ants_section,
                ants_per_colony_slider,
                ants_per_colony_hint,
                speed,
                perception_radius,
                perception_radius_hint,
                antenna_spread,
                antenna_box_radius,
                random_walk,
                trail_turn_gain,
                trail_turn_gain_hint,
                home_trail_guidance,
                home_trail_guidance_hint,
                individuality,
                individuality_hint,
                exit_section,
                exit_hint,
                exit_mode,
                exit_disk,
                exit_disk_hint,
                exit_jet,
                exit_jet_hint,
                exit_direction,
                exit_direction_hint,
                exit_spread,
                exit_spread_hint,
                exit_rate,
                exit_rate_hint,
                exit_initial,
                exit_initial_hint,
                crowding_section,
                jam_threshold,
                jam_threshold_hint,
                relief_step,
                relief_step_hint,
                busiest_bin_fmt,
                predators_section,
                predators,
                predators_hint,
                sight_radius,
                catch_radius,
                wander,
                predator_note,
                display_section,
                food_trail_gain,
                home_trail_gain,
                gain_hint,
                food_trail,
                home_trail,
                color_legend,
                legend_actors,
                language,
                help_text,
                running_fmt,
                report_header,
                report_world,
                report_behaviour,
                report_obstacles,
                report_predators,
                report_seed,
                chart_title,
                per_colony_title,
                per_colony_fmt,
                milestones_title,
                m_first_delivery,
                m_10,
                m_50,
                m_90,
                m_peak_rate,
                result_title,
                r_delivered,
                r_recovery,
                r_removed,
                r_mean_rate,
                r_peak_rate,
                r_sigmoid,
                r_still_rising,
                r_yes,
                r_no,
                r_ants_at_end,
                r_ants_eaten,
                r_concentration,
                verdict_label,
                csv_written_fmt,
                csv_failed_fmt,
                error_prefix,
                err_needs_value,
                err_not_a_number,
                err_unknown_option,
                err_unknown_obstacle,
                sweep_header_fmt,
                verdict_none,
                verdict_strong,
                verdict_s,
                verdict_weak,
                verdict_linear,
                chart_no_samples,
                obstacle_none_label,
                obstacle_u_wall_label,
                obstacle_blocks_label,
                homing_section,
                homing_mode,
                homing_path,
                homing_path_hint,
                homing_wall,
                homing_wall_hint,
                homing_flow,
                homing_flow_hint,
                units_suffix,
                trips_suffix,
                map_remaining,
                rate_unit,
                window_of,
                searching_label,
                returning_label,
                busiest_bin,
                peak_label,
                occupied_label,
                scenario_section,
                scenario_builtins_hint,
                scenario_export,
                scenario_export_hint,
                scenario_exported_fmt,
                scenario_failed_fmt,
                scenario_export_preset,
                scenario_export_preset_hint,
                scenario_preset_exported_fmt,
                scenario_import,
                scenario_import_hint,
                scenario_imported_fmt,
                scenario_import_failed_fmt,
                err_unknown_scenario,
                err_unknown_homing,
                err_unknown_spawn,
                report_homing,
                report_spawn,
                report_scenario,
            );
            for header in s.progress_labels {
                assert!(
                    !header.trim().is_empty(),
                    "{lang:?} progress label is empty"
                );
            }
            for header in s.sweep_table() {
                assert!(!header.trim().is_empty(), "{lang:?} sweep header is empty");
            }
        }
    }

    /// The progress line must splice every value in, in order.
    #[test]
    fn the_progress_line_carries_every_value() {
        let line = EN.progress_line(42, 123.4, 56.7, 10, 20, 30.5, 3, 9);
        assert!(line.contains("42"), "{line}");
        assert!(line.contains("123.4"), "{line}");
        assert!(line.contains("56.7"), "{line}");
        assert!(line.contains("10"), "{line}");
        assert!(line.contains("20"), "{line}");
        assert!(line.contains("30.5"), "{line}");
        assert!(line.contains("3"), "{line}");
        assert!(line.contains("9"), "{line}");
        let zh = ZH.progress_line(1, 1.0, 1.0, 1, 1, 1.0, 1, 1);
        assert!(zh.contains("已运回"), "{zh}");
    }

    /// `parse` must accept the spellings a real user would type, and reject
    /// garbage without panicking.
    #[test]
    fn language_parsing_is_forgiving() {
        assert_eq!(Lang::parse("en"), Some(Lang::En));
        assert_eq!(Lang::parse(" EN "), Some(Lang::En));
        assert_eq!(Lang::parse("English"), Some(Lang::En));
        assert_eq!(Lang::parse("zh"), Some(Lang::Zh));
        assert_eq!(Lang::parse("zh-CN"), Some(Lang::Zh));
        assert_eq!(Lang::parse("chinese"), Some(Lang::Zh));
        assert_eq!(Lang::parse("fr"), None);
        assert_eq!(Lang::parse(""), None);
        // The default is English, so an unparseable value silently keeps it.
        assert_eq!(Lang::default(), Lang::En);
    }

    /// The two tables must stay exhaustive and distinct where it matters: the
    /// Chinese table must not silently be a copy of the English one.
    #[test]
    fn the_two_languages_differ() {
        assert_ne!(EN.app_subtitle, ZH.app_subtitle);
        assert_ne!(EN.preset_brief, ZH.preset_brief);
        assert_ne!(EN.verdict_strong, ZH.verdict_strong);
        assert_ne!(EN.sweep_table()[1], ZH.sweep_table()[1]);
    }

    /// The Chinese help must cover the same flags as the English one, so a
    /// translated help block cannot silently drop an option.
    #[test]
    fn both_help_texts_list_the_same_flags() {
        let flags = |text: &str| -> Vec<String> {
            text.lines()
                .filter_map(|line| line.split_whitespace().next())
                .filter(|word| word.starts_with("--") || *word == "-h,")
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(flags(EN.help_text), flags(ZH.help_text));
    }

    /// Every built-in scenario must carry a Chinese short name and a Chinese
    /// description (the English side keeps the bare keyword and defers to the
    /// scenario's own text), so a new built-in cannot ship untranslated.
    #[test]
    fn builtin_scenarios_are_localized() {
        for name in crate::scenario::Scenario::builtin_names() {
            assert!(
                !ZH.scenario_description(&name).trim().is_empty(),
                "{name} has no Chinese description"
            );
            let display = ZH.scenario_display(&name);
            assert!(display.contains(&name), "{display} lost the keyword");
            assert_ne!(display, name, "{name} has no Chinese short name");
            assert_eq!(EN.scenario_display(&name), name);
            assert_eq!(EN.scenario_description(&name), "");
        }
        // Unknown names (custom TOML files) fall back to the given text.
        assert_eq!(ZH.scenario_display("my-file"), "my-file");
        assert_eq!(ZH.scenario_description("my-file"), "");
    }
}
