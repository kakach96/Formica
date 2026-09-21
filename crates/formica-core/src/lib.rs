//! Formica - a pheromone-based ant colony foraging simulation.
//!
//! The crate is deliberately renderer-free: it depends only on `bevy_ecs` and
//! `bevy_app`, so Phase 1 (the pure-logic acceptance run) and Phase 2 (the
//! Bevy-rendered viewer) execute *the same systems through the same schedule*.
//! The renderer only ever reads the ECS.
//!
//! # Layout
//!
//! | module | contents |
//! |---|---|
//! | [`config`] | [`SimConfig`] - every tunable parameter, as a `Resource` |
//! | [`grid`] | [`PheromoneGrid`] - per-colony food/home fields |
//! | [`world`] | [`Nests`], [`FoodField`], [`FoodPile`] |
//! | [`obstacles`] | [`Obstacles`] - walls, including the U-shaped preset |
//! | [`predator`] | predator components |
//! | [`motion`] | shared bounds-and-obstacle movement |
//! | [`ant`] | the ant `Component`s and the state machine |
//! | [`systems`] | one system per stage of a tick |
//! | [`metrics`] | [`SimStats`], curve analysis, ASCII chart |
//! | [`plugin`] | [`SimPlugin`], the [`SimulationTick`] schedule |
//!
//! # Example
//!
//! ```
//! use formica_core::prelude::*;
//!
//! let mut app = headless_app(SimConfig {
//!     ant_count: 50,
//!     width: 64,
//!     height: 64,
//!     ..Default::default()
//! });
//! run_ticks(&mut app, 200);
//!
//! let stats = app.world().resource::<SimStats>();
//! println!("tick {} delivered {}", stats.tick, stats.delivered_total);
//! ```

pub mod ant;
pub mod config;
pub mod grid;
pub mod metrics;
pub mod motion;
pub mod obstacles;
pub mod plugin;
pub mod predator;
pub mod rng;
pub mod systems;
pub mod world;

/// Everything a caller normally needs.
pub mod prelude {
    pub use crate::ant::{
        newborn_ant, wrap_angle, AntColony, AntHeading, AntMarker, AntPos, AntState, Carrying,
        PheromoneSensors,
    };
    pub use crate::config::{ObstaclePreset, SimConfig};
    pub use crate::grid::{
        Deposit, Pheromone, PheromoneGrid, MAX_COLONIES, PHEROMONE_EPSILON,
    };
    pub use crate::metrics::{
        analyze, analyze_series, ascii_curve, ascii_series, sparkline, CurveReport, Sample, SimStats,
    };
    pub use crate::motion::{step, MotionField, StepResult};
    pub use crate::obstacles::Obstacles;
    pub use crate::plugin::{
        food_in_transit, headless_app, init_simulation, reset_simulation, run_ticks, SimPlugin,
        SimulationTick,
    };
    pub use crate::predator::{
        newborn_predator, PredatorAlert, PredatorHeading, PredatorMarker, PredatorPos,
    };
    pub use crate::rng::SimRng;
    pub use crate::world::{FoodField, FoodPile, Nest, Nests};
}
