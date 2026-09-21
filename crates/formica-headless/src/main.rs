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

use std::fmt::Write as _;
use std::process::ExitCode;

use formica_core::obstacles::Obstacles;
use formica_core::prelude::*;

const HELP: &str = "\
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

OUTPUT
    --sample-interval N   Ticks between curve samples      [default: 10]
    --report-every N      Progress line interval, 0=off    [default: 500]
    --csv PATH            Write the curve to a CSV file
    --quiet               Only print the final report
    --chart-width N       ASCII chart width                [default: 78]
    --chart-height N      ASCII chart height               [default: 16]

EXPERIMENTS
    --sweep-evaporation L  Comma-separated values; runs each and compares
    --sweep-random-walk L  Comma-separated values; runs each and compares
    --sweep-predators L    Comma-separated predator counts; runs each
    --sweep-individuality L  Comma-separated trait-spread values; runs each

    -h, --help            Show this help

EXIT CODE
    0 if any colony delivered food, 1 if none did.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let opts = match Options::parse(&args) {
        Ok(Some(opts)) => opts,
        Ok(None) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("error: {message}\n");
            eprint!("{HELP}");
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
            Ok(()) => println!("\nwrote {} samples to {path}", outcome.stats.samples.len()),
            Err(err) => {
                eprintln!("\nfailed to write {path}: {err}");
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
    ticks: u64,
    report_every: u64,
    csv: Option<String>,
    quiet: bool,
    chart_width: usize,
    chart_height: usize,
    evaporation_sweep: Vec<f32>,
    random_walk_sweep: Vec<f32>,
    predator_sweep: Vec<f32>,
    individuality_sweep: Vec<f32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            config: SimConfig::default(),
            ticks: 5000,
            report_every: 500,
            csv: None,
            quiet: false,
            chart_width: 78,
            chart_height: 16,
            evaporation_sweep: Vec::new(),
            random_walk_sweep: Vec::new(),
            predator_sweep: Vec::new(),
            individuality_sweep: Vec::new(),
        }
    }
}

impl Options {
    /// Returns `Ok(None)` when `--help` was requested.
    fn parse(args: &[String]) -> Result<Option<Self>, String> {
        let mut opts = Self::default();
        let mut i = 0;

        fn value<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str, String> {
            *i += 1;
            args.get(*i)
                .map(|s| s.as_str())
                .ok_or_else(|| format!("{flag} needs a value"))
        }
        fn number<T: std::str::FromStr>(
            args: &[String],
            i: &mut usize,
            flag: &str,
        ) -> Result<T, String> {
            let raw = value(args, i, flag)?;
            raw.parse::<T>()
                .map_err(|_| format!("{flag}: '{raw}' is not a valid number"))
        }

        while i < args.len() {
            let flag = args[i].as_str();
            match flag {
                "-h" | "--help" => return Ok(None),

                "--ants" => opts.config.ant_count = number(args, &mut i, flag)?,
                "--colonies" => opts.config.colony_count = number(args, &mut i, flag)?,
                "--ticks" => opts.ticks = number(args, &mut i, flag)?,
                "--seed" => opts.config.seed = number(args, &mut i, flag)?,

                "--width" => opts.config.width = number(args, &mut i, flag)?,
                "--height" => opts.config.height = number(args, &mut i, flag)?,
                "--piles" => opts.config.food_pile_count = number(args, &mut i, flag)?,
                "--food-per-pile" => opts.config.food_per_pile = number(args, &mut i, flag)?,
                "--pile-radius" => opts.config.food_pile_radius = number(args, &mut i, flag)?,
                "--nest-radius" => opts.config.nest_radius = number(args, &mut i, flag)?,
                "--nest-orbit" => opts.config.nest_orbit = number(args, &mut i, flag)?,

                "--obstacles" => {
                    let raw = value(args, &mut i, flag)?;
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
                        ObstaclePreset::None => parse_obstacle_kind(raw)?,
                    };
                }
                "--u-wall-arm" => {
                    let arm: f32 = number(args, &mut i, flag)?;
                    opts.config.obstacle_preset =
                        with_u_wall(opts.config.obstacle_preset, Some(arm), None, None);
                }
                "--u-wall-gap" => {
                    let gap: f32 = number(args, &mut i, flag)?;
                    opts.config.obstacle_preset =
                        with_u_wall(opts.config.obstacle_preset, None, Some(gap), None);
                }
                "--u-wall-thickness" => {
                    let thickness: f32 = number(args, &mut i, flag)?;
                    opts.config.obstacle_preset =
                        with_u_wall(opts.config.obstacle_preset, None, None, Some(thickness));
                }
                "--block-density" => {
                    let density = number(args, &mut i, flag)?;
                    opts.config.obstacle_preset = ObstaclePreset::RandomBlocks { density };
                }

                "--predators" => opts.config.predator_count = number(args, &mut i, flag)?,
                "--predator-speed" => opts.config.predator_speed = number(args, &mut i, flag)?,
                "--predator-sense" => opts.config.predator_sense = number(args, &mut i, flag)?,
                "--predator-catch" => {
                    opts.config.predator_catch_radius = number(args, &mut i, flag)?
                }

                "--evaporation" => opts.config.evaporation_rate = number(args, &mut i, flag)?,
                "--deposit" => opts.config.deposit_amount = number(args, &mut i, flag)?,
                "--deposit-radius" => opts.config.deposit_radius = number(args, &mut i, flag)?,
                "--sensor-distance" => opts.config.sensor_distance = number(args, &mut i, flag)?,
                "--sensor-angle" => opts.config.sensor_angle = number(args, &mut i, flag)?,
                "--sensor-box" => opts.config.sensor_box_radius = number(args, &mut i, flag)?,
                "--random-walk" => opts.config.random_walk_strength = number(args, &mut i, flag)?,
                "--turn-gain" => opts.config.sensor_turn_gain = number(args, &mut i, flag)?,
                "--speed" => opts.config.speed = number(args, &mut i, flag)?,
                "--individuality" => {
                    opts.config.individuality = number(args, &mut i, flag)?
                }
                "--crowd-threshold" => {
                    opts.config.crowd_threshold = number(args, &mut i, flag)?
                }
                "--crowd-relief" => opts.config.crowd_relief = number(args, &mut i, flag)?,

                "--sample-interval" => opts.config.sample_interval = number(args, &mut i, flag)?,
                "--report-every" => opts.report_every = number(args, &mut i, flag)?,
                "--csv" => opts.csv = Some(value(args, &mut i, flag)?.to_string()),
                "--quiet" => opts.quiet = true,
                "--chart-width" => opts.chart_width = number(args, &mut i, flag)?,
                "--chart-height" => opts.chart_height = number(args, &mut i, flag)?,

                "--sweep-evaporation" => {
                    opts.evaporation_sweep = parse_list(value(args, &mut i, flag)?, flag)?
                }
                "--sweep-random-walk" => {
                    opts.random_walk_sweep = parse_list(value(args, &mut i, flag)?, flag)?
                }
                "--sweep-predators" => {
                    opts.predator_sweep = parse_list(value(args, &mut i, flag)?, flag)?
                }
                "--sweep-individuality" => {
                    opts.individuality_sweep = parse_list(value(args, &mut i, flag)?, flag)?
                }

                other => return Err(format!("unknown option '{other}'")),
            }
            i += 1;
        }

        opts.config.sanitize();
        Ok(Some(opts))
    }
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

fn parse_obstacle_kind(raw: &str) -> Result<ObstaclePreset, String> {
    match raw.to_ascii_lowercase().replace('_', "-").as_str() {
        "none" | "off" | "open" => Ok(ObstaclePreset::None),
        "u-wall" | "u" | "wall" => Ok(ObstaclePreset::UWall {
            arm: 14.0,
            gap: 14.0,
            thickness: 3.0,
        }),
        "blocks" | "random" | "rubble" => Ok(ObstaclePreset::RandomBlocks { density: 0.15 }),
        other => Err(format!(
            "--obstacles: unknown kind '{other}' (expected none, u-wall or blocks)"
        )),
    }
}

fn parse_list(raw: &str, flag: &str) -> Result<Vec<f32>, String> {
    raw.split(',')
        .map(|part| {
            let part = part.trim();
            part.parse::<f32>()
                .map_err(|_| format!("{flag}: '{part}' is not a valid number"))
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
    let mut app = headless_app(opts.config.clone());
    let interval = opts.report_every;

    if !opts.quiet && interval > 0 {
        println!(
            "running {} ticks ({} ants x {} colony, {}x{}, evaporation {:.3})...",
            opts.ticks,
            opts.config.ant_count,
            opts.config.colonies(),
            opts.config.width,
            opts.config.height,
            opts.config.evaporation_rate
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
                "  tick {:>6}  delivered {:>9.1} ({:>5.1}%)  searching {:>5}  returning {:>5}  food left {:>9.1}  eaten {}  busiest bin {}",
                stats.tick,
                stats.delivered_total,
                stats.recovery_fraction() * 100.0,
                stats.searching,
                stats.returning,
                stats.food_remaining,
                stats.ants_eaten,
                stats.crowd_max_bin
            );
        }
    }

    let walls = app.world().resource::<Obstacles>().count();
    let stats = app.world().resource::<SimStats>().clone();
    Outcome { stats, walls }
}

fn print_report(opts: &Options, outcome: &Outcome) {
    let config = &opts.config;
    let stats = &outcome.stats;
    let report = analyze(&stats.samples);
    let colonies = config.colonies();

    println!("\n=== Formica colony foraging run ===");
    println!(
        "world      {}x{} cells, {} ants x {} colony = {} ants, {} food piles x {:.0} = {:.0} units",
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
        "behaviour  evaporation {:.3}/tick, deposit {:.2} (kernel r={}), sensor r={:.1} @ {:.2} rad (box {}), walk {:.2}, turn-gain {:.2}, speed {:.1}/tick, individuality {:.2}",
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
            "obstacles  {} - {} wall cells ({:.1}% of the map)",
            config.obstacle_preset.label(),
            outcome.walls,
            100.0 * outcome.walls as f32 / config.cell_count() as f32
        );
    }
    if config.predator_count > 0 {
        println!(
            "predators  {} at {:.1} cells/tick, sense {:.0}, catch radius {:.0} - {} ants eaten",
            config.predator_count,
            config.predator_speed,
            config.predator_sense,
            config.predator_catch_radius,
            stats.ants_eaten
        );
    }
    println!("seed       {}", config.seed);

    println!("\n--- cumulative food delivered vs tick (all colonies) ---");
    print!(
        "{}",
        ascii_curve(&stats.samples, opts.chart_width, opts.chart_height)
    );

    if colonies > 1 {
        println!("--- per colony ---");
        for index in 0..colonies {
            let colony = analyze_series(&stats.samples, move |s| s.colony(index));
            println!(
                "  colony {index}: delivered {:>8.1} ({:>5.1}% of the haul) in {:>5} trips, \
                 50% at {:>6}, peak rate {:>6}",
                stats.colony_total(index),
                if stats.delivered_total > 0.0 {
                    100.0 * stats.colony_total(index) / stats.delivered_total
                } else {
                    0.0
                },
                stats.colony_trips(index),
                colony
                    .t50
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "-".into()),
                colony
                    .t_peak_rate
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "-".into()),
            );
            print!(
                "{}",
                ascii_series(&stats.samples, opts.chart_width, 8, move |s| s.colony(index))
            );
        }
    }

    println!("\n--- milestones (tick at which N% of the final total was reached) ---");
    let milestone = |label: &str, tick: Option<u64>| match tick {
        Some(t) => println!("  {label:<22} {t:>7}"),
        None => println!("  {label:<22}       -"),
    };
    milestone("first delivery", report.t_first);
    milestone("10% delivered", report.t10);
    milestone("50% delivered", report.t50);
    milestone("90% delivered", report.t90);
    milestone("peak delivery rate", report.t_peak_rate);

    println!("\n--- result ---");
    println!(
        "  delivered              {:>10.1} units in {} round trips",
        stats.delivered_total, stats.deliveries
    );
    println!(
        "  recovery               {:>9.1}%  ({:.0} units left on the map)",
        report.recovery_fraction * 100.0,
        stats.food_remaining
    );
    if stats.food_removed > 0.0 {
        println!(
            "  removed by the player  {:>10.1} units",
            stats.food_removed
        );
    }
    println!(
        "  mean rate              {:>10.4} units/tick",
        report.mean_rate
    );
    println!(
        "  peak interval rate     {:>10.4} units/tick  (window of {} ticks)",
        report.peak_rate / config.sample_interval.max(1) as f64,
        config.sample_interval
    );
    println!("  sigmoid score          {:>10.2}", report.sigmoid_score);
    println!(
        "  still rising at end    {:>10}",
        if report.still_rising { "yes" } else { "no" }
    );
    println!(
        "  ants at end            {:>10} searching / {} returning",
        stats.searching, stats.returning
    );
    if stats.ants_eaten > 0 {
        println!("  ants eaten             {:>10}", stats.ants_eaten);
    }
    println!(
        "  concentration          {:>10}  (ants in the busiest 4x4 bin; peak {}, over {} occupied bins)",
        stats.crowd_max_bin, stats.crowd_peak_bin, stats.crowd_occupied
    );
    println!("\n  VERDICT: {}", report.verdict());
}

// ---------------------------------------------------------------------------
// sweeps
// ---------------------------------------------------------------------------

fn run_sweeps(opts: &Options) -> ExitCode {
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
            "\n=== sweep: {name} ({} ticks, {} ants x {} colony, {}x{}) ===",
            opts.ticks,
            opts.config.ant_count,
            opts.config.colonies(),
            opts.config.width,
            opts.config.height
        );
        println!(
            "{:>14}  {:>10}  {:>8}  {:>8}  {:>8}  {:>7}  curve",
            name, "delivered", "recovery", "sigmoid", "eaten", "1st tick"
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
