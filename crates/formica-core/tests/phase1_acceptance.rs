//! Phase 1 acceptance tests: does the colony actually forage, and does the
//! food go where the books say it goes?
//!
//! The full-scale acceptance run is `#[ignore]`d because it takes a few seconds
//! in release and a good deal longer in debug. Run it with:
//!
//! ```text
//! cargo test -p formica-core --release -- --ignored --nocapture
//! ```

use formica_core::prelude::*;

/// Everything that was on the map must still be accounted for: on the map, in
/// an ant's jaws, banked at the nest, or explicitly destroyed by the player
/// deleting a pile. No exceptions, at any tick.
fn assert_food_is_conserved(app: &mut bevy_ecs::world::World, context: &str) {
    let (initial, delivered, remaining, removed, in_transit) = {
        let stats = app.resource::<SimStats>();
        (
            stats.food_initial,
            stats.delivered_total,
            stats.food_remaining,
            stats.food_removed,
            food_in_transit(app),
        )
    };
    let accounted = delivered + remaining + in_transit + removed;
    // Tolerance is 1 part in 100,000, chosen from measurement rather than
    // taste. The residual comes from `Carrying.amount` being `f32` (per-ant
    // cargo, deliberately not promoted to f64): on a 1200-unit run it measures
    // 1.6e-5 absolute, i.e. 1.3e-8 relative - below even `f32` epsilon because
    // the rounding errors partly cancel. The partial-load leak this test exists
    // to catch measured 1.4e-3 relative, so this threshold sits ~100x above the
    // noise and ~100x below a real defect.
    let tolerance = initial * 1e-5;
    assert!(
        (accounted - initial).abs() < tolerance,
        "{context}: initial {initial}, delivered {delivered}, remaining {remaining}, \
         removed {removed}, in transit {in_transit} -> accounted {accounted} \
         (tolerance {tolerance})"
    );
}

#[test]
fn food_is_conserved_at_every_checkpoint() {
    let mut app = headless_app(SimConfig {
        width: 80,
        height: 80,
        ant_count: 120,
        food_per_pile: 400.0,
        ..Default::default()
    });

    for checkpoint in 1..=8 {
        run_ticks(&mut app, 250);
        assert_food_is_conserved(app.world_mut(), &format!("checkpoint {checkpoint}"));
    }
}

#[test]
fn a_colony_finds_food_and_delivers_it() {
    let mut app = headless_app(SimConfig {
        width: 80,
        height: 80,
        ant_count: 150,
        food_per_pile: 500.0,
        ..Default::default()
    });

    run_ticks(&mut app, 3000);

    let world = app.world();
    let stats = world.resource::<SimStats>();
    assert!(
        stats.delivered_total > 0.0,
        "the colony never delivered anything in 3000 ticks"
    );
    assert!(
        stats.deliveries > 10,
        "only {} round trips - that is a fluke, not foraging",
        stats.deliveries
    );
    assert_eq!(stats.tick, 3000);
    assert!(
        stats.first_delivery_tick.is_some_and(|t| t < 3000),
        "first delivery recorded at {:?}",
        stats.first_delivery_tick
    );

    // The delivery curve must accelerate: this is the positive feedback that
    // the whole simulation exists to demonstrate.
    let report = analyze(&stats.samples);
    assert!(
        report.sigmoid_score > 1.15,
        "curve is linear (score {:.2}), trails are not reinforcing",
        report.sigmoid_score
    );
}

/// A controlled experiment: the only difference between the two runs is the
/// pheromone system, and the same seed drives both.
///
/// Note what this test does *not* claim. An earlier version asserted that a
/// colony with no pheromone at all would accumulate food linearly; that is
/// false, and the run proved it. Ants spreading outward from a single nest
/// cover more ground every tick, so even a pure random walk discovers food at
/// an accelerating rate and scores above 1.6 on the sigmoid metric. The
/// measurable effect of pheromones is therefore not "an S-curve appears" but
/// "the same S-curve arrives much sooner", which is what is asserted here.
#[test]
fn pheromone_trails_measurably_speed_up_foraging() {
    let base = SimConfig {
        width: 128,
        height: 128,
        ant_count: 150,
        food_per_pile: 2_000.0,
        ..Default::default()
    };

    let with_trails = {
        let mut app = headless_app(base.clone());
        run_ticks(&mut app, 4_000);
        let stats = app.world().resource::<SimStats>();
        analyze(&stats.samples)
    };
    let without_trails = {
        let mut app = headless_app(SimConfig {
            deposit_amount: 0.0,
            sensor_turn_gain: 0.0,
            ..base
        });
        run_ticks(&mut app, 4_000);
        let stats = app.world().resource::<SimStats>();
        analyze(&stats.samples)
    };

    println!(
        "with trails:    delivered {:.0}, t50 {:?}, score {:.2}",
        with_trails.total, with_trails.t50, with_trails.sigmoid_score
    );
    println!(
        "without trails: delivered {:.0}, t50 {:?}, score {:.2}",
        without_trails.total, without_trails.t50, without_trails.sigmoid_score
    );

    assert!(
        with_trails.total > 0.0 && without_trails.total > 0.0,
        "one of the colonies never found the food"
    );
    assert!(
        with_trails.t50 < Some(without_trails.t50.unwrap_or(u64::MAX)),
        "trails did not help: t50 {:?} vs {:?}",
        with_trails.t50,
        without_trails.t50
    );
    assert!(
        with_trails.total > without_trails.total,
        "trails did not increase total yield: {:.0} vs {:.0}",
        with_trails.total,
        without_trails.total
    );
}

#[test]
fn food_is_exhausted_and_the_colony_goes_idle() {
    let mut app = headless_app(SimConfig {
        width: 64,
        height: 64,
        ant_count: 80,
        food_per_pile: 200.0,
        ..Default::default()
    });

    run_ticks(&mut app, 8_000);

    let world = app.world();
    let stats = world.resource::<SimStats>();
    assert!(
        stats.food_remaining < stats.food_initial * 0.01,
        "{:.1} of {:.1} units were never harvested",
        stats.food_remaining,
        stats.food_initial
    );
    assert_eq!(stats.returning, 0, "ants still claim to be returning");
    // Once the map is bare and everyone has handed over, nothing is in transit.
    let transit = food_in_transit(app.world_mut());
    assert!(transit < 1e-3, "{transit} units stranded in ant jaws");
    assert_food_is_conserved(app.world_mut(), "after exhaustion");
}

/// The literal Phase 1 acceptance criterion, on the brief's default parameters.
#[test]
#[ignore = "full-scale 5000-tick run; use --release -- --ignored"]
fn phase1_acceptance_default_config() {
    let config = SimConfig::default();
    let mut app = headless_app(config.clone());
    run_ticks(&mut app, 5000);

    let world = app.world();
    let stats = world.resource::<SimStats>().clone();
    let report = analyze(&stats.samples);

    println!(
        "\n=== Phase 1 acceptance: {}x{}, {} ants, evaporation {:.3} ===",
        config.width, config.height, config.ant_count, config.evaporation_rate
    );
    print!("{}", ascii_curve(&stats.samples, 78, 16));
    println!(
        "\ndelivered {:.1}/{:.1} units in {} round trips",
        stats.delivered_total, stats.food_initial, stats.deliveries
    );
    println!(
        "milestones: first {:?}  10% {:?}  50% {:?}  90% {:?}  peak-rate {:?}",
        report.t_first, report.t10, report.t50, report.t90, report.t_peak_rate
    );
    println!(
        "sigmoid score {:.2}  recovery {:.1}%  still rising: {}",
        report.sigmoid_score,
        report.recovery_fraction * 100.0,
        report.still_rising
    );
    println!("verdict: {}\n", report.verdict());

    assert!(
        stats.delivered_total > 0.0,
        "no food was delivered in 5000 ticks"
    );
    assert!(
        report.sigmoid_score >= 1.5,
        "delivery curve is not S shaped: score {:.2} ({})",
        report.sigmoid_score,
        report.verdict()
    );
    // The lag phase must be real - an S-curve has a slow start.
    assert!(
        report.t10.is_some_and(|t| t > 0),
        "the curve had no lag phase at all"
    );
    assert_food_is_conserved(app.world_mut(), "end of acceptance run");
}
