//! Regression tests for the three behaviours the viewer's owner asked to fix.
//!
//! Each of these is a claim about a *shape* or a *distribution*, so each one is
//! measured rather than eyeballed. The property in every case was confirmed
//! broken first, against the code as it stood, and the measured numbers are
//! quoted in the assertions so a future change that quietly undoes the fix
//! fails with a number rather than a shrug.

use formica_core::crowd::{CrowdGrid, CROWD_BIN};
use formica_core::obstacles::Obstacles;
use formica_core::prelude::*;
use formica_core::systems::{relieve_crowding, tally_crowding};

/// Distance of every ant from the nest, sorted ascending.
fn radial_positions(app: &mut bevy_ecs::world::World, nest: (f32, f32)) -> Vec<f32> {
    let mut query = app.query::<&AntPos>();
    let mut distances: Vec<f32> = query
        .iter(app)
        .map(|p| (p.x - nest.0).hypot(p.y - nest.1))
        .collect();
    distances.sort_by(f32::total_cmp);
    distances
}

/// Fraction of the colony in the outermost fifth of its own radius.
///
/// This is the annulus statistic. In a hollow ring every ant is at nearly the
/// same distance from the nest, so the outermost fifth is over-represented;
/// after a real dispersal the ants cover all distances and the outer fifth
/// holds about a fifth of them.
fn outer_fifth(distances: &[f32]) -> f64 {
    let r_max = distances.last().copied().unwrap_or(1.0).max(1.0) as f64;
    let outer = distances.iter().filter(|d| **d as f64 >= r_max * 0.8).count();
    outer as f64 / distances.len().max(1) as f64
}

/// Fraction of the colony inside 40% of its own radius.
fn inner_two_fifths(distances: &[f32]) -> f64 {
    let r_max = distances.last().copied().unwrap_or(1.0).max(1.0) as f64;
    let inner = distances.iter().filter(|d| **d as f64 <= r_max * 0.4).count();
    inner as f64 / distances.len().max(1) as f64
}

/// The controlled configuration for the dispersal tests.
///
/// Food is present but trivially small and equally far away in every direction,
/// so nothing about it can shape the population's spread: these runs measure
/// only how a colony disperses from a point on an otherwise empty map.
fn dispersal_config(individuality: f32) -> SimConfig {
    SimConfig {
        width: 200,
        height: 200,
        ant_count: 400,
        food_per_pile: 1.0,
        food_distance_min: 90.0,
        food_distance_max: 90.0,
        individuality,
        ..Default::default()
    }
}

/// Fix 1: the colony must not disperse as a hollow ring.
///
/// With every ant identical they all leave the nest at the same speed on the
/// same heading, so the spawn disc becomes an annulus. Measured at tick 30 on a
/// 200x200 map with 400 ants, comparing the only thing that differs -
/// `individuality`:
///
/// ```text
/// individuality 0.0 : 65% of the colony in the outermost fifth of its radius,
///                     0.0% inside the inner 40%
/// individuality 0.6 : 14% outermost, 2.2% inner
/// ```
///
/// The comparison holds at ticks 20 and 45 too (65%/17% and 63%/10%), which is
/// why the assertion margins are wide rather than tuned to one tick.
///
/// # Why tick 30 and not tick 600
///
/// The ring is a *transient*, and an earlier version of this test missed that
/// entirely: by tick 600 on a 200-cell map the ants have reached the walls,
/// bounced and re-mixed, so even identical ants look uniform and the comparison
/// measures nothing. The phenomenon the viewer reported - "at the initial state
/// they spread as a circle" - is the first second or two.
#[test]
fn the_colony_does_not_disperse_as_a_hollow_ring() {
    let sample = |individuality: f32| -> (f64, f64, f32) {
        let mut app = headless_app(dispersal_config(individuality));
        let nest = *app.world().resource::<Nests>().get(0);
        run_ticks(&mut app, 30);
        let distances = radial_positions(app.world_mut(), (nest.x, nest.y));
        let last = *distances.last().unwrap();
        (outer_fifth(&distances), inner_two_fifths(&distances), last)
    };

    let (clones_outer, clones_inner, clones_max) = sample(0.0);
    let (individual_outer, individual_inner, individual_max) = sample(0.6);

    println!(
        "identical ants: {:.0}% outermost fifth, {:.1}% inner 40% (r_max {clones_max:.0})",
        100.0 * clones_outer,
        100.0 * clones_inner
    );
    println!(
        "individuals:    {:.0}% outermost fifth, {:.1}% inner 40% (r_max {individual_max:.0})",
        100.0 * individual_outer,
        100.0 * individual_inner
    );

    // The control must really be a ring, or this test stops testing anything.
    assert!(
        clones_outer > 0.55,
        "the identical-ant control is no longer an annulus ({:.0}% outermost), so this \
         test can no longer tell the fix from the bug",
        100.0 * clones_outer
    );
    assert!(
        clones_inner < 0.01,
        "the identical-ant control has a populated middle ({:.1}%), so it is not an \
         annulus at all",
        100.0 * clones_inner
    );

    assert!(
        individual_outer < 0.30,
        "{:.0}% of the colony is still crowding the outer rim - the dispersal is still \
         a ring",
        100.0 * individual_outer
    );
    assert!(
        individual_inner > 0.02,
        "the middle of the colony's radius is still empty ({:.1}%)",
        100.0 * individual_inner
    );
    assert!(
        individual_outer < clones_outer / 2.0,
        "individuality barely changed the shape: {:.0}% vs {:.0}%",
        100.0 * clones_outer,
        100.0 * individual_outer
    );
}

/// Fix 3: a dense maze must not collapse the colony onto a point.
///
/// With 45% of the cells walled, the open space is a labyrinth of one-cell
/// passages. Measured on a 256x256 map with 500 ants, over 2800 ticks after a
/// 200-tick warm-up:
///
/// ```text
/// relief off: peak bin 121, 310 bins occupied, delivered 599
/// relief on:  peak bin  27, 348 bins occupied, delivered 683
/// ```
///
/// 121 ants is a quarter of the colony standing on sixteen cells.
///
/// The measurement starts after 200 ticks because the first tick genuinely does
/// put the entire colony into one bin - every ant starts at its nest - and that
/// is not the phenomenon being fixed.
#[test]
fn a_dense_maze_does_not_collapse_the_colony_onto_a_point() {
    let base = SimConfig {
        width: 256,
        height: 256,
        ant_count: 500,
        obstacle_preset: ObstaclePreset::RandomBlocks { density: 0.45 },
        ..Default::default()
    };

    let run = |relief: f32| -> (u32, u32, f64) {
        let mut app = headless_app(SimConfig {
            crowd_relief: relief,
            ..base.clone()
        });
        run_ticks(&mut app, 200);
        let mut grid = CrowdGrid::new(base.width, base.height, CROWD_BIN);
        let mut peak = 0u32;
        for _ in 0..2_800 {
            run_ticks(&mut app, 1);
            let mut query = app.world_mut().query::<&AntPos>();
            grid.rebuild(query.iter(app.world()).map(|p| (p.x, p.y)));
            peak = peak.max(grid.max_bin);
        }
        let delivered = app.world().resource::<SimStats>().delivered_total;
        (peak, grid.occupied, delivered)
    };

    let (peak_without, occupied_without, delivered_without) = run(0.0);
    let (peak_with, occupied_with, delivered_with) = run(0.6);

    println!(
        "relief off: peak bin {peak_without:>4}, {occupied_without:>4} bins occupied, \
         delivered {delivered_without:.0}"
    );
    println!(
        "relief on:  peak bin {peak_with:>4}, {occupied_with:>4} bins occupied, \
         delivered {delivered_with:.0}"
    );

    assert!(
        peak_without > 60,
        "the control no longer collapses ({peak_without} ants in one bin), so this test \
         can no longer tell the fix from the bug"
    );
    assert!(
        peak_with < peak_without / 2,
        "relief did not relieve: peak bin {peak_without} -> {peak_with}"
    );
    assert!(
        peak_with < 50,
        "the colony still piles up: {peak_with} ants on a single 4x4 block"
    );
    assert!(
        occupied_with >= occupied_without,
        "the colony covers *less* ground with relief on: {occupied_without} -> \
         {occupied_with} bins"
    );
    // Foraging is the point, so the fix has to help or at least not hurt here.
    assert!(
        delivered_with >= delivered_without,
        "spreading the colony out cost food: {delivered_without:.0} -> {delivered_with:.0}"
    );
}

/// Fix 3 must be negligible where there is no jam.
///
/// The relief fires when a 4x4 bin holds more ants than `crowd_threshold`, and
/// a busy trail on an open map is the densest thing that happens there: over
/// 1600 ticks it peaks at 28 ants in a bin against a default threshold of 24,
/// exceeding it on 7 ticks out of 1600. The push is scaled by how far over the
/// threshold the bin is, so those marginal ticks get a few hundredths of the
/// step. This test measures that directly, by running the relief system once
/// against a real colony state and adding up every displacement it applies.
///
/// Measuring displacement rather than yield is deliberate: yield is chaotic -
/// two runs differing by a rounding error diverge in their delivery curve - so
/// "the totals match" would be a much weaker claim than "the system barely
/// moved anybody".
#[test]
fn relief_barely_moves_an_open_map_colony() {
    use bevy_ecs::system::RunSystemOnce;

    let config = SimConfig {
        width: 256,
        height: 256,
        ant_count: 500,
        ..Default::default()
    };
    let mut app = headless_app(config.clone());
    // No relief during the warm-up: this test is about what relief would do to
    // a colony that relief has not already shaped.
    app.world_mut().resource_mut::<SimConfig>().crowd_relief = 0.0;
    run_ticks(&mut app, 200);

    // Track the busiest bin over the run, which is the number the threshold is
    // calibrated against.
    let mut grid = CrowdGrid::new(config.width, config.height, CROWD_BIN);
    let mut peak = 0u32;
    let mut ticks_over = 0u32;
    const MEASURED_TICKS: u32 = 1_600;
    for _ in 0..MEASURED_TICKS {
        run_ticks(&mut app, 1);
        let mut query = app.world_mut().query::<&AntPos>();
        grid.rebuild(query.iter(app.world()).map(|p| (p.x, p.y)));
        peak = peak.max(grid.max_bin);
        if grid.max_bin as f32 > config.crowd_threshold {
            ticks_over += 1;
        }
    }

    let before: Vec<(f32, f32)> = {
        let mut query = app.world_mut().query::<&AntPos>();
        query.iter(app.world()).map(|p| (p.x, p.y)).collect()
    };

    // Now run exactly one relief pass over the current state, at the real
    // default setting.
    app.world_mut().resource_mut::<SimConfig>().crowd_relief = config.crowd_relief;
    app.world_mut().run_system_once(tally_crowding).unwrap();
    app.world_mut().run_system_once(relieve_crowding).unwrap();

    let displaced: f64 = {
        let mut query = app.world_mut().query::<&AntPos>();
        query
            .iter(app.world())
            .zip(before.iter())
            .map(|(after, (bx, by))| ((after.x - bx).hypot(after.y - by)) as f64)
            .sum()
    };
    let one_step = config.total_ants() as f64 * config.speed as f64;
    let over_fraction = ticks_over as f64 / MEASURED_TICKS as f64;
    println!(
        "open map: busiest bin {peak} (threshold {}), over it on {ticks_over}/{MEASURED_TICKS} \
         ticks ({:.2}%); final relief displacement {displaced:.2} cells vs {one_step:.0} for one \
         movement step ({:.3}%)",
        config.crowd_threshold,
        100.0 * over_fraction,
        100.0 * displaced / one_step
    );

    // Well clear of the collapse this system exists to fix, which measures 121
    // ants in a bin on the same map with 45% of the cells walled.
    assert!(
        peak < 60,
        "a normal trail on an open map now runs at {peak} ants in one bin, close enough to \
         the jam threshold of {} that the calibration no longer holds",
        config.crowd_threshold
    );
    assert!(
        over_fraction < 0.02,
        "relief fires on {:.1}% of open-map ticks - it is not a rare event there",
        100.0 * over_fraction
    );
    assert!(
        displaced < one_step * 0.05,
        "relief moved the colony {displaced:.1} cells, {:.1}% of one movement step - it is \
         meddling where there is no jam",
        100.0 * displaced / one_step
    );
}

/// Fix 3's relief must never park an ant inside a wall.
///
/// A bottleneck is *made* of wall, so this is the scenario where the push is
/// most likely to be illegal, and getting it wrong is silent: `motion::step`
/// would move the ant back out on the next tick, so nothing crashes and the
/// only symptom would be a colony mysteriously unable to make progress.
#[test]
fn relief_never_puts_an_ant_in_a_wall() {
    let config = SimConfig {
        width: 128,
        height: 128,
        ant_count: 300,
        obstacle_preset: ObstaclePreset::RandomBlocks { density: 0.45 },
        ..Default::default()
    };
    let mut app = headless_app(config);
    run_ticks(&mut app, 1_500);

    let obstacles = app.world().resource::<Obstacles>().clone();
    assert!(obstacles.count() > 0, "the fixture built no walls");

    let mut query = app.world_mut().query::<&AntPos>();
    let mut inside = 0;
    for pos in query.iter(app.world()) {
        if obstacles.is_blocked_at(pos.x, pos.y) {
            inside += 1;
        }
    }
    assert_eq!(inside, 0, "{inside} ants ended up inside a wall");
}
