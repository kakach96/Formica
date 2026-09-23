//! Integration tests for the optional Phase 4 features: obstacles, predators
//! and two competing colonies - plus the interactive world editing (runtime map
//! resize and player-placed food).
//!
//! Each test is written as a *comparison* against a baseline wherever it can
//! be. "The ants still deliver food" is weak evidence about a wall; "the ants
//! deliver less food with the wall, but still find a way round" is evidence.

use formica_core::obstacles::Obstacles as ObstacleGrid;
use formica_core::prelude::*;

fn base_config() -> SimConfig {
    SimConfig {
        width: 96,
        height: 96,
        ant_count: 140,
        food_per_pile: 500.0,
        ..Default::default()
    }
}

/// A configuration with far more food than the colony can clear in the window
/// being measured.
///
/// This matters more than it looks. An earlier version of the obstacle and
/// colony comparisons used a small, easy map and a long run, so *both* arms
/// harvested every unit and the "with feature" and "without feature" numbers
/// came out identical - the tests passed the starvation check and proved
/// nothing. Comparing two runs that have both hit the ceiling is not a
/// comparison.
fn headroom_config() -> SimConfig {
    SimConfig {
        ant_count: 140,
        food_per_pile: 4_000.0,
        ..base_config()
    }
}

fn run(config: SimConfig, ticks: u64) -> SimStats {
    let mut app = headless_app(config);
    run_ticks(&mut app, ticks);
    app.world().resource::<SimStats>().clone()
}

// ---------------------------------------------------------------------------
// obstacles
// ---------------------------------------------------------------------------

#[test]
fn a_u_wall_slows_the_colony_down_but_does_not_stop_it() {
    let ticks = 2_500;

    // Same map, same seed, same food: only the wall differs.
    let mut open_app = headless_app(SimConfig {
        obstacle_preset: ObstaclePreset::None,
        ..headroom_config()
    });
    let mut wall_app = headless_app(SimConfig {
        obstacle_preset: ObstaclePreset::UWall {
            arm: 14.0,
            gap: 14.0,
            thickness: 3.0,
        },
        ..headroom_config()
    });

    // The preset is stamped by `sync_obstacles` on the first tick, so the
    // obstacle map is still empty until the world has been stepped once.
    run_ticks(&mut wall_app, 10);
    let wall_cells = wall_app.world().resource::<ObstacleGrid>().count();
    assert!(wall_cells > 0, "the U-wall preset was never applied");

    run_ticks(&mut open_app, ticks - 10);
    run_ticks(&mut wall_app, ticks - 10);

    let wall_cells_later = wall_app.world().resource::<ObstacleGrid>().count();
    assert_eq!(wall_cells, wall_cells_later, "the wall changed mid-run");

    let open = open_app.world().resource::<SimStats>().clone();
    let walled = wall_app.world().resource::<SimStats>().clone();

    println!(
        "open field:    delivered {:.0} in {} trips (first at {:?})",
        open.delivered_total, open.deliveries, open.first_delivery_tick
    );
    println!(
        "U-shaped wall: delivered {:.0} in {} trips (first at {:?}), {} wall cells",
        walled.delivered_total, walled.deliveries, walled.first_delivery_tick, wall_cells_later
    );

    assert!(
        walled.delivered_total > 0.0,
        "the colony gave up entirely when faced with a wall"
    );
    assert!(
        open.delivered_total < open.food_initial,
        "the baseline cleared the whole map, so this comparison is saturated - \
         raise food_per_pile or shorten the run"
    );
    // Wrapping one of three piles makes that pile much more expensive to reach,
    // so the yield must drop. If it does not, the wall does nothing.
    assert!(
        walled.delivered_total < open.delivered_total,
        "the wall made no difference: {:.0} vs {:.0}",
        walled.delivered_total,
        open.delivered_total
    );
}

#[test]
fn no_ant_ever_ends_up_inside_a_wall() {
    let config = SimConfig {
        width: 96,
        height: 96,
        ant_count: 200,
        obstacle_preset: ObstaclePreset::RandomBlocks { density: 0.12 },
        ..Default::default()
    };
    let mut app = headless_app(config);
    run_ticks(&mut app, 1_500);

    let obstacles = app.world().resource::<ObstacleGrid>().clone();
    assert!(obstacles.count() > 0, "no obstacles were generated");

    let mut query = app.world_mut().query::<&AntPos>();
    let mut inside = 0;
    let mut total = 0;
    for pos in query.iter(app.world()) {
        total += 1;
        if obstacles.is_blocked_at(pos.x, pos.y) {
            inside += 1;
        }
    }
    assert!(total > 0);
    assert_eq!(inside, 0, "{inside} of {total} ants were inside a wall");
}

#[test]
fn no_pheromone_survives_inside_a_wall() {
    let config = SimConfig {
        width: 96,
        height: 96,
        ant_count: 80,
        obstacle_preset: ObstaclePreset::RandomBlocks { density: 0.1 },
        ..Default::default()
    };
    let mut app = headless_app(config);
    run_ticks(&mut app, 600);

    let obstacles = app.world().resource::<ObstacleGrid>().clone();
    let grid = app.world().resource::<PheromoneGrid>();
    let mut polluted = 0;
    for y in 0..obstacles.height as i32 {
        for x in 0..obstacles.width as i32 {
            if !obstacles.is_blocked(x, y) {
                continue;
            }
            for colony in 0..grid.colonies {
                if grid.get(colony, Pheromone::Food, x, y) > 0.0
                    || grid.get(colony, Pheromone::Home, x, y) > 0.0
                {
                    polluted += 1;
                }
            }
        }
    }
    assert_eq!(polluted, 0, "pheromone was found inside walls");
}

// ---------------------------------------------------------------------------
// predators
// ---------------------------------------------------------------------------

#[test]
fn predators_eat_ants_without_shrinking_the_colony() {
    let config = SimConfig {
        width: 96,
        height: 96,
        ant_count: 120,
        predator_count: 4,
        predator_sense: 30.0,
        ..Default::default()
    };
    let mut app = headless_app(config.clone());
    run_ticks(&mut app, 900);

    let stats = app.world().resource::<SimStats>().clone();
    assert!(stats.ants_eaten > 0, "predators never caught anything");

    // Predation costs time and cargo, not lives: `sync_ant_count` runs after
    // `predators_catch_ants`, so every victim is replaced in the same tick and
    // the population at a tick boundary is exactly the configured size.
    let mut query = app.world_mut().query::<&AntState>();
    let alive = query.iter(app.world()).count();
    assert_eq!(
        alive, config.ant_count,
        "predation shrank the colony permanently"
    );
    println!(
        "predators ate {} ants while the population stayed at {}",
        stats.ants_eaten, alive
    );
}

#[test]
fn predators_reduce_the_food_hauled_home() {
    let ticks = 2_500;
    let quiet = run(
        SimConfig {
            predator_count: 0,
            ..headroom_config()
        },
        ticks,
    );
    let hunted = run(
        SimConfig {
            predator_count: 5,
            predator_sense: 30.0,
            ..headroom_config()
        },
        ticks,
    );

    println!(
        "no predators: {:.0} units, {} trips, {} eaten",
        quiet.delivered_total, quiet.deliveries, quiet.ants_eaten
    );
    println!(
        "5 predators:  {:.0} units, {} trips, {} eaten",
        hunted.delivered_total, hunted.deliveries, hunted.ants_eaten
    );

    assert!(hunted.ants_eaten > 0, "the predators did nothing");
    assert!(
        hunted.delivered_total < quiet.delivered_total,
        "predation had no cost: {:.0} vs {:.0}",
        hunted.delivered_total,
        quiet.delivered_total
    );
}

/// Predators that follow trails home would, without a refuge rule, park on the
/// nest and eat every replacement on the tick it appeared - the colony could
/// never recover and would go permanently extinct in practice.
#[test]
fn a_colony_under_heavy_predation_still_survives_and_forages() {
    let mut app = headless_app(SimConfig {
        width: 64,
        height: 64,
        ant_count: 60,
        predator_count: 8,
        predator_sense: 140.0,
        ..Default::default()
    });
    run_ticks(&mut app, 2_000);

    let world = app.world();
    let stats = world.resource::<SimStats>().clone();
    let mut query = app.world_mut().query::<&AntState>();
    let alive = query.iter(app.world()).count();

    println!(
        "under 8 predators: {} ants alive, {} eaten, {:.0} units delivered",
        alive, stats.ants_eaten, stats.delivered_total
    );

    assert_eq!(alive, 60, "the colony lost ants permanently");
    assert!(
        stats.delivered_total > 0.0,
        "the colony stopped foraging entirely under predation"
    );
}

// ---------------------------------------------------------------------------
// two colonies
// ---------------------------------------------------------------------------

/// A configuration where neither arm of a comparison can clear the map, and
/// where the two arms start from the same place.
///
/// Two things are load-bearing here, and both were found by measuring rather
/// than by reasoning.
///
/// **Food.** The two-colony comparison needs more food than either arm can
/// harvest in the window. An earlier version used a small, easy map and a long
/// run, so both arms cleared it and the difference vanished - the test "passed"
/// with a 0.5% gap, which is evidence of nothing.
///
/// **Nest geometry.** With the default `nest_orbit` of 55 the two colonies nest
/// 110 cells apart, which makes the competing arm a *better* configuration in
/// one respect: two nests are two independent search origins. On a 128-cell map
/// that coverage advantage is worth more than the trail it costs, and the
/// measured gap comes out negative on most seeds. Putting both nests near the
/// middle (24 cells apart) removes the confound, and the sign becomes
/// consistent.
fn contest_config() -> SimConfig {
    SimConfig {
        width: 128,
        height: 128,
        food_per_pile: 4_000.0,
        nest_orbit: 12.0,
        ..SimConfig::default()
    }
}

#[test]
fn two_colonies_are_credited_separately_and_sum_to_the_total() {
    let config = SimConfig {
        ant_count: 150,
        colony_count: 2,
        ..headroom_config()
    };
    let stats = run(config, 2_500);

    assert!(stats.colony_total(0) > 0.0, "colony 0 delivered nothing");
    assert!(stats.colony_total(1) > 0.0, "colony 1 delivered nothing");
    assert!(
        (stats.colony_total(0) + stats.colony_total(1) - stats.delivered_total).abs() < 1e-6,
        "the per-colony split does not add up to the total"
    );
    assert_eq!(
        stats.colony_trips(0) + stats.colony_trips(1),
        stats.deliveries
    );
    assert!(stats.colony_first_delivery[0].is_some());
    assert!(stats.colony_first_delivery[1].is_some());

    println!(
        "colony 0: {:.0} units in {} trips (first {:?})",
        stats.colony_total(0),
        stats.colony_trips(0),
        stats.colony_first_delivery[0]
    );
    println!(
        "colony 1: {:.0} units in {} trips (first {:?})",
        stats.colony_total(1),
        stats.colony_trips(1),
        stats.colony_first_delivery[1]
    );
}

/// The defining property of competition: neither colony can use the other's
/// trails, so the two together do worse than the same ants working as one.
///
/// # Why this is an ensemble and not one run
///
/// On any single seed the effect is real but small - measured over four seeds,
/// the gap runs 1.2% to 5.0% while the seed-to-seed scatter of the *totals* is
/// several times that. A single-seed assertion would therefore be a coin flip
/// dressed up as a test, which is what this one had quietly become: after
/// per-ant individuality was added it still passed on the default seed while
/// coming out *negative* on two of the five other seeds tried.
///
/// What is robust is the sign. Averaged over the seeds below, splitting the
/// pheromone budget always costs yield, and the mean cost is about 3%. That is
/// the claim asserted: every seed loses something, and the average loss is
/// worth at least 1.5%.
///
/// Note also what changed and why. Before individuality, this comparison
/// measured 11.8% on the default seed alone; the penalty shrank because a
/// colony of individuals explores better on its own, which substitutes for the
/// shared trail it is missing. That is a genuine finding about the model, not a
/// weakened assertion.
#[test]
fn two_colonies_do_not_share_trails() {
    let ticks = 4_000;
    // Two colonies of 75 = 150 ants, against one colony of 150 ants. Same total
    // population, same map, same nests (24 cells apart), same food - the only
    // difference is whether the pheromone budget is shared.
    const SEEDS: [u64; 4] = [20_240_517, 1, 7, 99];

    let mut gaps = Vec::new();
    for seed in SEEDS {
        let cooperating = run(
            SimConfig {
                seed,
                ant_count: 150,
                colony_count: 1,
                ..contest_config()
            },
            ticks,
        );
        let competing = run(
            SimConfig {
                seed,
                ant_count: 75,
                colony_count: 2,
                ..contest_config()
            },
            ticks,
        );

        // Neither arm may hit the ceiling, or the comparison is meaningless.
        for (label, stats) in [("one colony", &cooperating), ("two colonies", &competing)] {
            assert!(
                stats.delivered_total < stats.food_initial * 0.95,
                "seed {seed}: the {label} arm cleared the map, so this comparison is \
                 saturated ({:.0} of {:.0})",
                stats.delivered_total,
                stats.food_initial
            );
        }
        assert!(
            competing.delivered_total > 0.0,
            "seed {seed}: nobody foraged"
        );

        let gap = 1.0 - competing.delivered_total / cooperating.delivered_total;
        println!(
            "seed {seed:>9}: one colony {:.0}, two colonies {:.0} ({} + {}) -> {:.1}%",
            cooperating.delivered_total,
            competing.delivered_total,
            competing.colony_total(0) as u64,
            competing.colony_total(1) as u64,
            100.0 * gap
        );
        gaps.push(gap);
    }

    let losing = gaps.iter().filter(|g| **g > 0.0).count();
    let mean = gaps.iter().sum::<f64>() / gaps.len() as f64;
    println!(
        "mean cost of splitting the pheromone budget: {:.1}% over {} seeds ({losing} of {} \
         lost yield)",
        100.0 * mean,
        gaps.len(),
        gaps.len()
    );

    assert_eq!(
        losing,
        gaps.len(),
        "splitting the pheromone budget did not cost yield on every seed: {gaps:?}"
    );
    assert!(
        mean > 0.015,
        "the average cost of not sharing trails is only {:.1}%, too small to be the effect \
         this test claims to measure",
        100.0 * mean
    );
}

#[test]
fn dropping_to_one_colony_disbands_the_other() {
    let mut app = headless_app(SimConfig {
        ant_count: 40,
        colony_count: 2,
        ..base_config()
    });
    run_ticks(&mut app, 50);

    {
        let mut query = app.world_mut().query::<&AntColony>();
        assert_eq!(query.iter(app.world()).count(), 80);
    }

    app.world_mut().resource_mut::<SimConfig>().colony_count = 1;
    run_ticks(&mut app, 5);

    let mut query = app.world_mut().query::<&AntColony>();
    let survivors: Vec<usize> = query.iter(app.world()).map(|c| c.index()).collect();
    assert_eq!(survivors.len(), 40, "the surplus colony was not disbanded");
    assert!(
        survivors.iter().all(|c| *c == 0),
        "colony 1 ants survived the disbanding"
    );

    // The pheromone storage is not shrunk - that would throw away the surviving
    // colony's trails - so it stays at capacity by design.
    assert_eq!(
        app.world().resource::<PheromoneGrid>().colonies,
        formica_core::grid::MAX_COLONIES,
        "the grid was resized, which would have wiped colony 0's trails"
    );
}

// ---------------------------------------------------------------------------
// world editing
// ---------------------------------------------------------------------------

#[test]
fn resizing_the_map_at_runtime_reallocates_everything() {
    let mut app = headless_app(base_config());
    run_ticks(&mut app, 100);

    app.world_mut().resource_mut::<SimConfig>().width = 48;
    app.world_mut().resource_mut::<SimConfig>().height = 128;
    reset_simulation(app.world_mut());
    run_ticks(&mut app, 200);

    let world = app.world();
    let grid = world.resource::<PheromoneGrid>();
    assert_eq!((grid.width, grid.height), (48, 128));
    let obstacles = world.resource::<ObstacleGrid>();
    assert_eq!((obstacles.width, obstacles.height), (48, 128));
    let food = world.resource::<FoodField>();
    assert_eq!((food.width, food.height), (48, 128));

    // And the ants are actually inside the new bounds.
    let mut query = app.world_mut().query::<&AntPos>();
    for pos in query.iter(app.world()) {
        assert!(
            (0.0..48.0).contains(&pos.x) && (0.0..128.0).contains(&pos.y),
            "an ant was left outside the resized map at ({}, {})",
            pos.x,
            pos.y
        );
    }
}

#[test]
fn player_placed_food_is_found_and_harvested() {
    // A bare map with no piles at all: the colony cannot deliver anything.
    let mut app = headless_app(SimConfig {
        width: 64,
        height: 64,
        ant_count: 120,
        food_pile_count: 1,
        food_per_pile: 0.0,
        ..Default::default()
    });
    run_ticks(&mut app, 400);
    assert_eq!(
        app.world().resource::<SimStats>().delivered_total,
        0.0,
        "an empty map should yield nothing"
    );

    // The player drops a pile right next to the nest.
    let nest = *app.world().resource::<Nests>().get(0);
    app.world_mut()
        .resource_mut::<FoodField>()
        .add_pile(FoodPile {
            x: nest.x + 12.0,
            y: nest.y,
            radius: 5.0,
            initial: 400.0,
        });

    run_ticks(&mut app, 1_500);

    let stats = app.world().resource::<SimStats>().clone();
    assert!(
        stats.delivered_total > 0.0,
        "the colony ignored food the player placed"
    );
    assert!(
        stats.food_initial >= 400.0,
        "the placed pile was not added to the books"
    );
    println!(
        "player-placed pile: {:.0} units delivered in {} trips",
        stats.delivered_total, stats.deliveries
    );
}

#[test]
fn removing_a_pile_leaves_the_books_balanced() {
    let mut app = headless_app(base_config());
    run_ticks(&mut app, 500);

    // Remove the first pile, whatever it still holds.
    app.world_mut()
        .resource_mut::<FoodField>()
        .remove_pile(0)
        .expect("pile 0 should exist");
    run_ticks(&mut app, 300);

    let (initial, delivered, remaining, removed, in_transit) = {
        let stats = app.world().resource::<SimStats>();
        (
            stats.food_initial,
            stats.delivered_total,
            stats.food_remaining,
            stats.food_removed,
            food_in_transit(app.world_mut()),
        )
    };
    assert!(removed > 0.0, "the removal destroyed nothing");
    let accounted = delivered + remaining + removed + in_transit;
    assert!(
        (accounted - initial).abs() < initial * 1e-5,
        "after removal: initial {initial}, accounted {accounted} \
         (delivered {delivered}, remaining {remaining}, removed {removed}, \
         in transit {in_transit})"
    );
}
