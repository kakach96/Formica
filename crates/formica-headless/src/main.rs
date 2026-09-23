//! Formica - renderer-free runner.
//!
//! Ticks the colony and prints the thing the acceptance criteria are actually
//! about: the cumulative-delivery curve, drawn in the terminal, plus enough
//! milestones and a sigmoid score to judge it without squinting. With two
//! colonies it prints one curve each, so the contest is visible rather than
//! inferred from a total.
//!
//! ```text
//! cargo run -p formica-headless --release -- --ticks 5000
//! cargo run -p formica-headless --release -- --obstacles u-wall
//! cargo run -p formica-headless --release -- --predators 5 --colonies 2
//! cargo run -p formica-headless --release -- --sweep-evaporation 0.01,0.05,0.1,0.2
//! ```
//!
//! The output language follows `FORMICA_LANG` or `--lang en|zh` (English by
//! default, because a Windows console's default code page cannot render CJK).

use std::fmt::Write as _;
use std::process::ExitCode;

use formica_core::i18n::Strings;
use formica_core::obstacles::Obstacles;
use formica_core::prelude::*;
use formica_core::scenario::Scenario;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // The language is resolved before anything prints, so `--help` and error
    // messages come out in the requested tongue. `FORMICA_LANG` covers whole
    // sessions; `--lang` wins when both are given.
    let mut lang = std::env::var("FORMICA_LANG")
        .ok()
        .and_then(|raw| Lang::parse(&raw))
        .unwrap_or_default();
    if let Some(pos) = args.iter().position(|a| a == "--lang") {
        if let Some(raw) = args.get(pos + 1) {
            if let Some(parsed) = Lang::parse(raw) {
                lang = parsed;
            }
        }
    }
    let s = lang.strings();

    let opts = match Options::parse(&args, s) {
        Ok(Some(opts)) => opts,
        Ok(None) => {
            print!("{}", s.help_text);
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("{}: {message}\n", s.error_prefix);
            eprint!("{}", s.help_text);
            return ExitCode::from(2);
        }
    };

    if !opts.evaporation_sweep.is_empty()
        || !opts.random_walk_sweep.is_empty()
        || !opts.predator_sweep.is_empty()
        || !opts.individuality_sweep.is_empty()
    {
        return run_sweeps(&opts);
    }

    let outcome = run_once(&opts);
    print_report(&opts, &outcome);

    if let Some(path) = &opts.csv {
        match write_csv(path, &outcome.stats) {
            Ok(()) => println!(
                "\n{}",
                opts.lang
                    .strings()
                    .csv_written_fmt
                    .replace("{n}", &outcome.stats.samples.len().to_string())
                    .replace("{path}", path)
            ),
            Err(err) => {
                eprintln!(
                    "\n{}",
                    opts.lang
                        .strings()
                        .csv_failed_fmt
                        .replace("{path}", path)
                        .replace("{err}", &err.to_string())
                );
                return ExitCode::from(3);
            }
        }
    }

    if outcome.stats.delivered_total > 0.0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------------------
// options
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Options {
    config: SimConfig,
    /// The scenario applied to `config`, if one was loaded. Kept around so the
    /// report can say which scenario produced the numbers.
    scenario: Option<Scenario>,
    ticks: u64,
    report_every: u64,
    csv: Option<String>,
    quiet: bool,
    chart_width: usize,
    chart_height: usize,
    lang: Lang,
    evaporation_sweep: Vec<f32>,
    random_walk_sweep: Vec<f32>,
    predator_sweep: Vec<f32>,
    individuality_sweep: Vec<f32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            config: SimConfig::default(),
            scenario: None,
            ticks: 5000,
            report_every: 500,
            csv: None,
            quiet: false,
            chart_width: 78,
            chart_height: 16,
            lang: Lang::default(),
            evaporation_sweep: Vec::new(),
            random_walk_sweep: Vec::new(),
            predator_sweep: Vec::new(),
            individuality_sweep: Vec::new(),
        }
    }
}

impl Options {
    /// Returns `Ok(None)` when `--help` was requested.
    ///
    /// `s` is the table the *initial* language resolved to, used for error
    /// messages while parsing; the resolved language is stored on the options
    /// once parsing succeeds.
    fn parse(args: &[String], s: &'static Strings) -> Result<Option<Self>, String> {
        let mut opts = Self::default();
        let mut i = 0;

        fn value<'a>(
            args: &'a [String],
            i: &mut usize,
            flag: &str,
            s: &'static Strings,
        ) -> Result<&'a str, String> {
            *i += 1;
            let message = format!("{flag} {}", s.err_needs_value);
            args.get(*i).map(|s| s.as_str()).ok_or_else(move || message)
        }
        fn number<T: std::str::FromStr>(
            args: &[String],
            i: &mut usize,
            flag: &str,
            s: &'static Strings,
        ) -> Result<T, String> {
            let raw = value(args, i, flag, s)?;
            raw.parse::<T>()
                .map_err(|_| format!("{flag}: {}", s.err_not_a_number.replace("{raw}", raw)))
        }

        while i < args.len() {
            let flag = args[i].as_str();
            match flag {
                "-h" | "--help" => return Ok(None),

                "--ants" => opts.config.ant_count = number(args, &mut i, flag, s)?,
                "--colonies" => opts.config.colony_count = number(args, &mut i, flag, s)?,
                "--ticks" => opts.ticks = number(args, &mut i, flag, s)?,
                "--seed" => opts.config.seed = number(args, &mut i, flag, s)?,

                "--width" => opts.config.width = number(args, &mut i, flag, s)?,
                "--height" => opts.config.height = number(args, &mut i, flag, s)?,
                "--piles" => opts.config.food_pile_count = number(args, &mut i, flag, s)?,
                "--food-per-pile" => opts.config.food_per_pile = number(args, &mut i, flag, s)?,
                "--pile-radius" => opts.config.food_pile_radius = number(args, &mut i, flag, s)?,
                "--nest-radius" => opts.config.nest_radius = number(args, &mut i, flag, s)?,
                "--nest-orbit" => opts.config.nest_orbit = number(args, &mut i, flag, s)?,

                "--obstacles" => {
                    let raw = value(args, &mut i, flag, s)?;
                    // Keep the geometry the user already set on the command
                    // line; only the shape changes here. Flags are applied in
                    // order, so `--u-wall-arm 20 --obstacles u-wall` works.
                    opts.config.obstacle_preset = match opts.config.obstacle_preset {
                        ObstaclePreset::UWall {
                            arm,
                            gap,
                            thickness,
                        } => ObstaclePreset::UWall {
                            arm,
                            gap,
                            thickness,
                        },
                        ObstaclePreset::RandomBlocks { density } => {
                            ObstaclePreset::RandomBlocks { density }
                        }
                        ObstaclePreset::None => parse_obstacle_kind(raw, s)?,
                    };
                }
                "--u-wall-arm" => {
                    let arm: f32 = number(args, &mut i, flag, s)?;
                    opts.config.obstacle_preset =
                        with_u_wall(opts.config.obstacle_preset, Some(arm), None, None);
                }
                "--u-wall-gap" => {
                    let gap: f32 = number(args, &mut i, flag, s)?;
                    opts.config.obstacle_preset =
                        with_u_wall(opts.config.obstacle_preset, None, Some(gap), None);
                }
                "--u-wall-thickness" => {
                    let thickness: f32 = number(args, &mut i, flag, s)?;
                    opts.config.obstacle_preset =
                        with_u_wall(opts.config.obstacle_preset, None, None, Some(thickness));
                }
                "--block-density" => {
                    let density = number(args, &mut i, flag, s)?;
                    opts.config.obstacle_preset = ObstaclePreset::RandomBlocks { density };
                }

                "--predators" => opts.config.predator_count = number(args, &mut i, flag, s)?,
                "--predator-speed" => opts.config.predator_speed = number(args, &mut i, flag, s)?,
                "--predator-sense" => opts.config.predator_sense = number(args, &mut i, flag, s)?,
                "--predator-catch" => {
                    opts.config.predator_catch_radius = number(args, &mut i, flag, s)?
                }

                "--evaporation" => opts.config.evaporation_rate = number(args, &mut i, flag, s)?,
                "--deposit" => opts.config.deposit_amount = number(args, &mut i, flag, s)?,
                "--deposit-radius" => opts.config.deposit_radius = number(args, &mut i, flag, s)?,
                "--sensor-distance" => opts.config.sensor_distance = number(args, &mut i, flag, s)?,
                "--sensor-angle" => opts.config.sensor_angle = number(args, &mut i, flag, s)?,
                "--sensor-box" => opts.config.sensor_box_radius = number(args, &mut i, flag, s)?,
                "--random-walk" => {
                    opts.config.random_walk_strength = number(args, &mut i, flag, s)?
                }
                "--turn-gain" => opts.config.sensor_turn_gain = number(args, &mut i, flag, s)?,
                "--speed" => opts.config.speed = number(args, &mut i, flag, s)?,
                "--individuality" => opts.config.individuality = number(args, &mut i, flag, s)?,
                "--crowd-threshold" => opts.config.crowd_threshold = number(args, &mut i, flag, s)?,
                "--crowd-relief" => opts.config.crowd_relief = number(args, &mut i, flag, s)?,

                "--sample-interval" => opts.config.sample_interval = number(args, &mut i, flag, s)?,
                "--report-every" => opts.report_every = number(args, &mut i, flag, s)?,
                "--csv" => opts.csv = Some(value(args, &mut i, flag, s)?.to_string()),
                "--quiet" => opts.quiet = true,
                "--lang" => {
                    let raw = value(args, &mut i, flag, s)?;
                    opts.lang =
                        Lang::parse(raw).ok_or_else(|| s.err_not_a_number.replace("{raw}", raw))?;
                }
                "--scenario" => {
                    // Flags apply in order, so later flags can still override
                    // a single value on top of the scenario. The scenario is
                    // applied eagerly here, exactly like the other flags.
                    let raw = value(args, &mut i, flag, s)?;
                    let scenario = load_scenario(raw).ok_or_else(|| {
                        s.err_unknown_scenario
                            .replace("{name}", raw)
                            .replace("{list}", &Scenario::builtin_names().join(", "))
                    })?;
                    scenario.apply(&mut opts.config);
                    opts.scenario = Some(scenario);
                }
                "--chart-width" => opts.chart_width = number(args, &mut i, flag, s)?,
                "--chart-height" => opts.chart_height = number(args, &mut i, flag, s)?,

                "--sweep-evaporation" => {
                    opts.evaporation_sweep = parse_list(value(args, &mut i, flag, s)?, flag, s)?
                }
                "--sweep-random-walk" => {
                    opts.random_walk_sweep = parse_list(value(args, &mut i, flag, s)?, flag, s)?
                }
                "--sweep-predators" => {
                    opts.predator_sweep = parse_list(value(args, &mut i, flag, s)?, flag, s)?
                }
                "--sweep-individuality" => {
                    opts.individuality_sweep = parse_list(value(args, &mut i, flag, s)?, flag, s)?
                }

                other => {
                    return Err(s.err_unknown_option.replace("{flag}", other));
                }
            }
            i += 1;
        }

        opts.config.sanitize();
        Ok(Some(opts))
    }
}

/// Load a scenario by built-in name or by TOML file path.
///
/// Files win over built-ins when both exist, but built-ins take their names
/// first - a file called `labyrinth.toml` is reachable only by its path,
/// which keeps the plain-name spelling unambiguous.
fn load_scenario(name_or_path: &str) -> Option<Scenario> {
    let path = std::path::Path::new(name_or_path);
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
    {
        let text = std::fs::read_to_string(path).ok()?;
        return Scenario::parse_toml(&text).ok();
    }
    Scenario::builtin(name_or_path)
}

/// Fill in the geometry of a U-wall preset, keeping whatever was already set.
fn with_u_wall(
    current: ObstaclePreset,
    arm: Option<f32>,
    gap: Option<f32>,
    thickness: Option<f32>,
) -> ObstaclePreset {
    let defaults = ObstaclePreset::UWall {
        arm: 14.0,
        gap: 14.0,
        thickness: 3.0,
    };
    let (a, g, t) = match current {
        ObstaclePreset::UWall {
            arm,
            gap,
            thickness,
        } => (arm, gap, thickness),
        _ => match defaults {
            ObstaclePreset::UWall {
                arm,
                gap,
                thickness,
            } => (arm, gap, thickness),
            _ => unreachable!(),
        },
    };
    ObstaclePreset::UWall {
        arm: arm.unwrap_or(a),
        gap: gap.unwrap_or(g),
        thickness: thickness.unwrap_or(t),
    }
}

fn parse_obstacle_kind(raw: &str, s: &'static Strings) -> Result<ObstaclePreset, String> {
    match raw.to_ascii_lowercase().replace('_', "-").as_str() {
        "none" | "off" | "open" => Ok(ObstaclePreset::None),
        "u-wall" | "u" | "wall" => Ok(ObstaclePreset::UWall {
            arm: 14.0,
            gap: 14.0,
            thickness: 3.0,
        }),
        "blocks" | "random" | "rubble" => Ok(ObstaclePreset::RandomBlocks { density: 0.15 }),
        other => Err(s.err_unknown_obstacle.replace("{kind}", other)),
    }
}

fn parse_list(raw: &str, flag: &str, s: &'static Strings) -> Result<Vec<f32>, String> {
    raw.split(',')
        .map(|part| {
            let part = part.trim();
            part.parse::<f32>()
                .map_err(|_| format!("{flag}: {}", s.err_not_a_number.replace("{raw}", part)))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// running
// ---------------------------------------------------------------------------

struct Outcome {
    stats: SimStats,
    walls: usize,
}

fn run_once(opts: &Options) -> Outcome {
    let s = opts.lang.strings();
    let mut app = headless_app(opts.config.clone());
    let interval = opts.report_every;

    if !opts.quiet && interval > 0 {
        println!(
            "{}",
            s.running_fmt
                .replace("{ticks}", &opts.ticks.to_string())
                .replace("{ants}", &opts.config.ant_count.to_string())
                .replace("{colonies}", &opts.config.colonies().to_string())
                .replace("{w}", &opts.config.width.to_string())
                .replace("{h}", &opts.config.height.to_string())
                .replace("{evap:.3}", &format!("{:.3}", opts.config.evaporation_rate))
        );
    }

    let mut done = 0u64;
    while done < opts.ticks {
        let chunk = if interval == 0 {
            opts.ticks - done
        } else {
            interval.min(opts.ticks - done)
        };
        run_ticks(&mut app, chunk);
        done += chunk;

        if !opts.quiet && interval > 0 {
            let stats = app.world().resource::<SimStats>();
            println!(
                "{}",
                s.progress_line(
                    stats.tick,
                    stats.delivered_total,
                    stats.recovery_fraction() * 100.0,
                    stats.searching,
                    stats.returning,
                    stats.food_remaining,
                    stats.ants_eaten,
                    stats.crowd_max_bin,
                )
            );
        }
    }

    let walls = app.world().resource::<Obstacles>().count();
    let stats = app.world().resource::<SimStats>().clone();
    Outcome { stats, walls }
}

fn print_report(opts: &Options, outcome: &Outcome) {
    let s = opts.lang.strings();
    let config = &opts.config;
    let stats = &outcome.stats;
    let report = analyze(&stats.samples);
    let colonies = config.colonies();

    println!("\n{}", s.report_header);
    if let Some(scenario) = &opts.scenario {
        println!(
            "{}     {} - {}",
            s.report_scenario, scenario.name, scenario.description
        );
    }
    println!(
        "{}         {}x{} cells, {} ants x {} colony = {} ants, {} food piles x {:.0} = {:.0} units",
        s.report_world,
        config.width,
        config.height,
        config.ant_count,
        colonies,
        config.total_ants(),
        config.food_pile_count,
        config.food_per_pile,
        stats.food_initial
    );
    println!(
        "{}  evaporation {:.3}/tick, deposit {:.2} (kernel r={}), sensor r={:.1} @ {:.2} rad (box {}), walk {:.2}, turn-gain {:.2}, speed {:.1}/tick, individuality {:.2}",
        s.report_behaviour,
        config.evaporation_rate,
        config.deposit_amount,
        config.deposit_radius,
        config.sensor_distance,
        config.sensor_angle,
        config.sensor_box_radius,
        config.random_walk_strength,
        config.sensor_turn_gain,
        config.speed,
        config.individuality
    );
    if !matches!(config.obstacle_preset, ObstaclePreset::None) {
        println!(
            "{}  {} - {} {} ({:.1}% of the map)",
            s.report_obstacles,
            config.obstacle_preset.label_for(s),
            outcome.walls,
            s.wall_cells,
            100.0 * outcome.walls as f32 / config.cell_count() as f32
        );
    }
    if config.predator_count > 0 {
        println!(
            "{}  {} at {:.1} cells/tick, sense {:.0}, catch radius {:.0} - {} ants eaten",
            s.report_predators,
            config.predator_count,
            config.predator_speed,
            config.predator_sense,
            config.predator_catch_radius,
            stats.ants_eaten
        );
    }
    println!("{}       {}", s.report_seed, config.seed);

    println!("\n{}", s.chart_title);
    print!(
        "{}",
        ascii_curve(&stats.samples, opts.chart_width, opts.chart_height)
    );

    if colonies > 1 {
        println!("{}", s.per_colony_title);
        for index in 0..colonies {
            let colony = analyze_series(&stats.samples, move |sample| sample.colony(index));
            println!(
                "{}",
                s.per_colony_fmt
                    .replace("{index}", &index.to_string())
                    .replace("{:>8.1}", &format!("{:>8.1}", stats.colony_total(index)))
                    .replace(
                        "{:>5.1}",
                        &format!(
                            "{:>5.1}",
                            if stats.delivered_total > 0.0 {
                                100.0 * stats.colony_total(index) / stats.delivered_total
                            } else {
                                0.0
                            }
                        )
                    )
                    .replace("{:>5}", &format!("{:>5}", stats.colony_trips(index)))
                    .replace(
                        "{:>6}",
                        &format!(
                            "{:>6}",
                            colony
                                .t50
                                .map(|t| t.to_string())
                                .unwrap_or_else(|| "-".into())
                        )
                    )
            );
            print!(
                "{}",
                ascii_series(&stats.samples, opts.chart_width, 8, move |s| s
                    .colony(index))
            );
        }
    }

    println!("\n{}", s.milestones_title);
    let milestone = |label: &str, tick: Option<u64>| match tick {
        Some(t) => println!("  {label:<22} {t:>7}"),
        None => println!("  {label:<22}       -"),
    };
    milestone(s.m_first_delivery, report.t_first);
    milestone(s.m_10, report.t10);
    milestone(s.m_50, report.t50);
    milestone(s.m_90, report.t90);
    milestone(s.m_peak_rate, report.t_peak_rate);

    println!("\n{}", s.result_title);
    println!(
        "  {:<22} {:>10.1} {} / {} {}",
        s.r_delivered, stats.delivered_total, s.units_suffix, stats.deliveries, s.trips_suffix
    );
    println!(
        "  {:<22} {:>9.1}%  ({} {:.0})",
        s.r_recovery,
        report.recovery_fraction * 100.0,
        s.map_remaining,
        stats.food_remaining
    );
    if stats.food_removed > 0.0 {
        println!(
            "  {:<22} {:>10.1} {}",
            s.r_removed, stats.food_removed, s.units_suffix
        );
    }
    println!(
        "  {:<22} {:>10.4} {}",
        s.r_mean_rate, report.mean_rate, s.rate_unit
    );
    println!(
        "  {:<22} {:>10.4} {}  ({} {} ticks)",
        s.r_peak_rate,
        report.peak_rate / config.sample_interval.max(1) as f64,
        s.rate_unit,
        s.window_of,
        config.sample_interval
    );
    println!("  {:<22} {:>10.2}", s.r_sigmoid, report.sigmoid_score);
    println!(
        "  {:<22} {:>10}",
        s.r_still_rising,
        if report.still_rising { s.r_yes } else { s.r_no }
    );
    println!(
        "  {:<22} {:>10} {} / {} {}",
        s.r_ants_at_end, stats.searching, s.searching_label, stats.returning, s.returning_label
    );
    if stats.ants_eaten > 0 {
        println!("  {:<22} {:>10}", s.r_ants_eaten, stats.ants_eaten);
    }
    println!(
        "  {:<22} {:>10}  ({}; {} {}, {} {})",
        s.r_concentration,
        stats.crowd_max_bin,
        s.busiest_bin,
        s.peak_label,
        stats.crowd_peak_bin,
        s.occupied_label,
        stats.crowd_occupied
    );
    println!("\n  {}: {}", s.verdict_label, report.verdict_for(s));
}

// ---------------------------------------------------------------------------
// sweeps
// ---------------------------------------------------------------------------

fn run_sweeps(opts: &Options) -> ExitCode {
    let s = opts.lang.strings();
    let sweeps: Vec<(&str, Vec<f32>)> = [
        ("evaporation", opts.evaporation_sweep.clone()),
        ("random-walk", opts.random_walk_sweep.clone()),
        ("predators", opts.predator_sweep.clone()),
        ("individuality", opts.individuality_sweep.clone()),
    ]
    .into_iter()
    .filter(|(_, values)| !values.is_empty())
    .collect();

    let mut any_food = false;

    for (name, values) in sweeps {
        println!(
            "\n{}",
            s.sweep_header_fmt
                .replace("{name}", name)
                .replace("{ticks}", &opts.ticks.to_string())
                .replace("{ants}", &opts.config.ant_count.to_string())
                .replace("{colonies}", &opts.config.colonies().to_string())
                .replace("{w}", &opts.config.width.to_string())
                .replace("{h}", &opts.config.height.to_string())
        );
        let headers = s.sweep_table();
        println!(
            "{:>14}  {:>10}  {:>8}  {:>8}  {:>8}  {:>7}  curve",
            name, headers[1], headers[2], headers[3], headers[4], headers[5]
        );

        for value in &values {
            let mut config = opts.config.clone();
            match name {
                "evaporation" => config.evaporation_rate = *value,
                "random-walk" => config.random_walk_strength = *value,
                "predators" => config.predator_count = *value as usize,
                "individuality" => config.individuality = *value,
                _ => unreachable!(),
            }
            config.sanitize();

            let run = Options {
                config,
                quiet: true,
                ..opts.clone()
            };
            let outcome = run_once(&run);
            any_food |= outcome.stats.delivered_total > 0.0;

            let spark = sparkline(
                &outcome
                    .stats
                    .samples
                    .iter()
                    .map(|s| s.delivered)
                    .collect::<Vec<_>>(),
                34,
            );
            println!(
                "{:>14.3}  {:>10.1}  {:>7.1}%  {:>8.2}  {:>8}  {:>8}  {}",
                value,
                outcome.stats.delivered_total,
                outcome.stats.recovery_fraction() * 100.0,
                analyze(&outcome.stats.samples).sigmoid_score,
                outcome.stats.ants_eaten,
                outcome
                    .stats
                    .first_delivery_tick
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "-".into()),
                spark
            );
        }
    }

    if any_food {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// ---------------------------------------------------------------------------
// csv
// ---------------------------------------------------------------------------

fn write_csv(path: &str, stats: &SimStats) -> std::io::Result<()> {
    let mut out = String::with_capacity(stats.samples.len() * 64);
    out.push_str(
        "tick,delivered,colony0,colony1,deliveries,delta,searching,returning,food_remaining,food_pheromone,home_pheromone,crowd_max_bin\n",
    );
    for s in &stats.samples {
        let _ = writeln!(
            out,
            "{},{:.4},{:.4},{:.4},{},{:.4},{},{},{:.4},{:.3},{:.3},{}",
            s.tick,
            s.delivered,
            s.colony(0),
            s.colony(1),
            s.deliveries,
            s.delta,
            s.searching,
            s.returning,
            s.food_remaining,
            s.food_pheromone,
            s.home_pheromone,
            s.crowd_max_bin
        );
    }
    std::fs::write(path, out)
}
