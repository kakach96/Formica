//! Simulation systems, split one-per-stage of the tick:
//!
//! ```text
//!   sync_obstacles            stamp the configured wall shape
//!        |
//!   scrub_obstacle_pheromones erase any trail left inside a wall
//!        |
//!   sync_ant_count            per-colony population -> config
//!   sync_predator_count       per-predator population -> config
//!        |
//!   evaporate_pheromones      c *= (1 - rate), every colony
//!        |
//!   sense_pheromones          3 antennae -> PheromoneSensors (own colony only)
//!        |
//!   decide_heading            sensors + noise + homing -> AntHeading
//!        |
//!   move_ants                 AntHeading + speed -> AntPos (walls, obstacles)
//!   move_predators            hunt the nearest ant, or wander
//!        |
//!   tally_crowding            ants -> coarse density bins
//!   relieve_crowding          step out of a jam, toward the emptiest neighbour
//!        |
//!   deposit_pheromones        AntPos + state -> own colony's grid
//!        |
//!   pickup_food               cell food -> Carrying, state := Returning
//!   deliver_food              Carrying -> own colony's stats, state := Searching
//!        |
//!   predators_catch_ants      despawn ants within reach (nests are a refuge)
//!        |
//!   update_stats              counters + curve sample
//! ```
//!
//! The two crowding systems sit between movement and deposit on purpose: the
//! tally must see final positions, and the relief must land before the trail is
//! laid, so an ant that has been shoved out of a jam writes its pheromone where
//! it actually ended up.

pub mod crowd;
pub mod decide;
pub mod deposit;
pub mod evaporate;
pub mod flow;
pub mod interact;
pub mod movement;
pub mod obstacles;
pub mod predator;
pub mod sense;
pub mod spawn;
pub mod stats;

pub use crowd::{relieve_crowding, tally_crowding};
pub use decide::decide_heading;
pub use deposit::deposit_pheromones;
pub use evaporate::evaporate_pheromones;
pub use flow::sync_nest_flow;
pub use interact::{deliver_food, pickup_food};
pub use movement::move_ants;
pub use obstacles::scrub_obstacle_pheromones;
pub use predator::{move_predators, predators_catch_ants, sync_predator_count};
pub use sense::sense_pheromones;
pub use spawn::{sync_ant_count, sync_nests};
pub use stats::update_stats;
