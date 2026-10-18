//! Homing navigation vs walls: does the optional navigation machinery get
//! food-bearing ants across an intervening wall?
//!
//! Measured baseline (this test's exact world, `--homing`, 1500 ticks):
//!
//! | mode          | delivered | recovery |
//! |---------------|-----------|----------|
//! | path          | 312       | 10.4%    |
//! | wall          | 436       | 14.5%    |
//! | flow          | 2206      | 73.5%    |
//!
//! The map puts a two-cell wall with an eight-cell gap *off* the nest-food
//! line, so a directly-homing ant piles against the wall instead of flowing
//! to the gap. The numbers above are what determine the margins below; if a
//! change moves any number materially, re-measure and update this table -
//! the assertions only pin direction and rough magnitude, but the comment is
//! the honest record.

use bevy_ecs::prelude::*;
use formica_core::config::{FoodSpot, HomingMode, SimConfig, WallRect};
use formica_core::metrics::SimStats;
use formica_core::plugin::{headless_app, run_ticks};

fn walled_config(mode: HomingMode) -> SimConfig {
    SimConfig {
        width: 96,
        height: 96,
        ant_count: 300,
        colony_count: 1,
        food_pile_count: 1,
        food_per_pile: 3000.0,
        food_pile_radius: 6.0,
        homing_mode: mode,
        custom_food: vec![FoodSpot {
            x: 48.0,
            y: 16.0,
            radius: 6.0,
            amount: 3000.0,
        }],
        // The wall sits between the nest (map centre 48,48) and the food
        // (48,16), with one gap on the west side, x in [20, 28].
        custom_walls: vec![
            WallRect {
                x: 4.0,
                y: 30.0,
                w: 16.0,
                h: 2.0,
            },
            WallRect {
                x: 28.0,
                y: 30.0,
                w: 64.0,
                h: 2.0,
            },
        ],
        ..Default::default()
    }
}

fn run(config: SimConfig, ticks: u64) -> SimStats {
    let mut app = headless_app(config);
    run_ticks(&mut app, ticks);
    app.world().resource::<SimStats>().clone()
}

/// Direction and magnitude: with a wall between food and home, richer
/// navigation must deliver more. The margins come from the measured table at
/// the top of the file; every arm stays under 95% of the initial food, so the
/// comparisons are evidence-producing rather than saturated.
#[test]
fn richer_homing_rescues_the_walled_return() {
    let run = |mode| run(walled_config(mode), 1500);
    let path = run(HomingMode::PathIntegration);
    let wall = run(HomingMode::WallSense);
    let flow = run(HomingMode::FlowField);

    // The printed record of the table at the top of this file; re-run with
    // --nocapture after any behaviour change and update the table.
    println!(
        "delivered @1500 ticks: path {:.0}, wall {:.0}, flow {:.0}",
        path.delivered_total, wall.delivered_total, flow.delivered_total
    );

    for (name, stats) in [
        ("path integration", &path),
        ("wall whiskers", &wall),
        ("flow field", &flow),
    ] {
        // Control-group self-check: none of the arms may be saturated, or the
        // comparison between the arms would prove nothing.
        assert!(
            stats.delivered_total < stats.food_initial * 0.95,
            "{name} saturated the comparison: {}",
            stats.delivered_total
        );
    }
    assert!(
        wall.delivered_total > path.delivered_total * 1.15,
        "wall-whisker homing should beat direct homing past a wall: {} vs {}",
        wall.delivered_total,
        path.delivered_total
    );
    assert!(
        flow.delivered_total > wall.delivered_total * 1.2,
        "flow-field homing should beat whisker homing: {} vs {}",
        flow.delivered_total,
        wall.delivered_total
    );
}
