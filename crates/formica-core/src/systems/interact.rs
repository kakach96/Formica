//! Stage 5 of an ant's tick: the two events that close the loop.
//!
//! These two systems are the entire economic cycle of the colony. Everything
//! else is steering. `pickup_food` converts "found something" into
//! `Returning`; `deliver_food` converts "got home" back into `Searching` and
//! books the profit against the ant's own colony.
//!
//! Notice neither of them touches the pheromone grid - the trail is laid
//! blindly by `deposit_pheromones`, and only *becomes* a trail because
//! successful ants happen to walk it.

use bevy_ecs::prelude::*;

use crate::ant::{AntColony, AntHeading, AntPos, AntState, Carrying};
use crate::config::SimConfig;
use crate::metrics::SimStats;
use crate::world::{FoodField, Nests};

/// Load up: a searching ant standing on food takes what it can.
///
/// The ant turns for home when it is **full**, or when there is **nothing left
/// under it to top up from**. The second condition matters more than it looks:
/// the last few cells of a pile hold less than a full load, and without it an
/// ant that picked up a partial crumb would stay in `Searching` for ever,
/// wandering with cargo it never delivers. On a 256x256 default run that
/// silently stranded ~13 units of food.
pub fn pickup_food(
    config: Res<SimConfig>,
    mut food: ResMut<FoodField>,
    mut ants: Query<(&mut AntState, &AntPos, &mut Carrying, &mut AntHeading)>,
) {
    let capacity = config.ant_capacity;

    for (mut state, pos, mut cargo, mut heading) in ants.iter_mut() {
        if *state != AntState::Searching {
            continue;
        }

        let want = capacity - cargo.amount;
        if want > 0.0 {
            cargo.amount += food.take(pos.x, pos.y, want);
        }

        if cargo.amount <= 0.0 {
            continue;
        }

        let full = cargo.amount >= capacity - 1.0e-6;
        let nothing_left_here = food.get(pos.x, pos.y) <= 0.0;
        if full || nothing_left_here {
            *state = AntState::Returning;
            heading.turn(std::f32::consts::PI);
        }
    }
}

/// Unload: a returning ant inside **its own** nest hands over its cargo.
///
/// Delivery is colony-checked. An ant standing in the rival's nest is a visitor,
/// not a courier, and gets no credit - which is what makes the two colonies
/// genuinely separate economies rather than one shared one.
pub fn deliver_food(
    nests: Res<Nests>,
    mut stats: ResMut<SimStats>,
    mut ants: Query<(
        &AntColony,
        &mut AntState,
        &AntPos,
        &mut Carrying,
        &mut AntHeading,
    )>,
) {
    // `update_stats` runs after this system, so `stats.tick` is still the
    // previous tick; +1 is the tick we are inside now.
    let tick = stats.tick + 1;

    for (colony, mut state, pos, mut cargo, mut heading) in ants.iter_mut() {
        if *state != AntState::Returning {
            continue;
        }
        if !nests.get(colony.index()).contains(pos.x, pos.y) {
            continue;
        }
        if cargo.amount > 0.0 {
            stats.record_delivery(colony.index(), cargo.amount as f64, tick);
        }
        cargo.amount = 0.0;
        *state = AntState::Searching;
        // Turn around and head back out; the food trail it just left is the
        // best hint it has about where to go.
        heading.turn(std::f32::consts::PI);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::FoodPile;
    use bevy_ecs::system::RunSystemOnce;
    use bevy_ecs::world::World;

    /// One map, two colonies, each with its own nest.
    fn pile_world() -> World {
        let config = SimConfig {
            ant_capacity: 1.0,
            colony_count: 2,
            ..Default::default()
        };
        let mut world = World::new();
        world.insert_resource(Nests::from_config(&config));
        world.insert_resource(config);
        world.insert_resource(FoodField::from_piles(
            64,
            64,
            vec![FoodPile {
                x: 32.0,
                y: 32.0,
                radius: 3.0,
                initial: 100.0,
            }],
        ));
        world.insert_resource(SimStats::default());
        world
    }

    #[test]
    fn standing_on_food_fills_the_ant_and_sends_it_home() {
        let mut world = pile_world();
        let e = world
            .spawn((
                AntColony(0),
                AntState::Searching,
                AntPos::new(32.0, 32.0),
                Carrying::default(),
                AntHeading::new(0.0),
            ))
            .id();

        world.run_system_once(pickup_food).unwrap();

        let mut q = world.query::<(&AntState, &Carrying)>();
        let (state, cargo) = q.get(&world, e).unwrap();
        assert_eq!(*state, AntState::Returning);
        assert!((cargo.amount - 1.0).abs() < 1e-6);
        assert!(world.resource::<FoodField>().remaining_total < 100.0);
    }

    #[test]
    fn empty_ground_leaves_the_ant_searching() {
        let mut world = pile_world();
        let e = world
            .spawn((
                AntColony(0),
                AntState::Searching,
                AntPos::new(1.0, 1.0),
                Carrying::default(),
                AntHeading::new(0.0),
            ))
            .id();
        world.run_system_once(pickup_food).unwrap();
        let mut q = world.query::<&AntState>();
        assert_eq!(*q.get(&world, e).unwrap(), AntState::Searching);
    }

    /// Regression: an ant that can only find a partial crumb must still carry
    /// it home instead of hoarding it in `Searching` for ever.
    #[test]
    fn a_partial_load_is_carried_home_once_the_ground_is_bare() {
        let mut world = pile_world();
        world.resource_mut::<FoodField>().amount[20 * 64 + 20] = 0.05;
        let e = world
            .spawn((
                AntColony(0),
                AntState::Searching,
                AntPos::new(20.0, 20.0),
                Carrying::default(),
                AntHeading::new(0.0),
            ))
            .id();

        world.run_system_once(pickup_food).unwrap();

        let mut q = world.query::<(&AntState, &Carrying)>();
        let (state, cargo) = q.get(&world, e).unwrap();
        assert_eq!(*state, AntState::Returning, "crumb was abandoned");
        assert!((cargo.amount - 0.05).abs() < 1e-6);
        assert_eq!(world.resource::<FoodField>().get(20.0, 20.0), 0.0);
    }

    #[test]
    fn a_carrying_searcher_gives_up_on_bare_ground() {
        let mut world = pile_world();
        let e = world
            .spawn((
                AntColony(0),
                AntState::Searching,
                AntPos::new(1.0, 1.0),
                Carrying { amount: 0.4 },
                AntHeading::new(0.0),
            ))
            .id();
        world.run_system_once(pickup_food).unwrap();
        let mut q = world.query::<&AntState>();
        assert_eq!(*q.get(&world, e).unwrap(), AntState::Returning);
    }

    #[test]
    fn an_empty_searcher_keeps_searching_on_bare_ground() {
        let mut world = pile_world();
        let e = world
            .spawn((
                AntColony(0),
                AntState::Searching,
                AntPos::new(1.0, 1.0),
                Carrying::default(),
                AntHeading::new(0.0),
            ))
            .id();
        world.run_system_once(pickup_food).unwrap();
        let mut q = world.query::<&AntState>();
        assert_eq!(*q.get(&world, e).unwrap(), AntState::Searching);
    }

    #[test]
    fn delivering_inside_the_nest_banks_the_food() {
        let mut world = pile_world();
        let nest = *world.resource::<Nests>().get(0);
        let e = world
            .spawn((
                AntColony(0),
                AntState::Returning,
                AntPos::new(nest.x, nest.y),
                Carrying { amount: 1.0 },
                AntHeading::new(0.0),
            ))
            .id();

        world.run_system_once(deliver_food).unwrap();

        let stats = world.resource::<SimStats>();
        assert_eq!(stats.delivered_total, 1.0);
        assert_eq!(stats.colony_total(0), 1.0);
        assert_eq!(stats.colony_total(1), 0.0);
        assert_eq!(stats.deliveries, 1);
        assert_eq!(stats.first_delivery_tick, Some(1));

        let mut q = world.query::<(&AntState, &Carrying)>();
        let (state, cargo) = q.get(&world, e).unwrap();
        assert_eq!(*state, AntState::Searching);
        assert_eq!(cargo.amount, 0.0);
    }

    /// The rule that makes two colonies competitors rather than roommates.
    #[test]
    fn an_ant_delivering_to_the_rival_nest_gets_no_credit() {
        let mut world = pile_world();
        let rival = *world.resource::<Nests>().get(1);
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(rival.x, rival.y),
            Carrying { amount: 1.0 },
            AntHeading::new(0.0),
        ));

        world.run_system_once(deliver_food).unwrap();

        let stats = world.resource::<SimStats>();
        assert_eq!(stats.delivered_total, 0.0, "rival nest paid out");
        assert_eq!(stats.colony_total(0), 0.0);
    }

    #[test]
    fn each_colony_banks_into_its_own_nest() {
        let mut world = pile_world();
        let nests = world.resource::<Nests>().clone();
        for (index, nest) in nests.0.iter().enumerate() {
            world.spawn((
                AntColony(index),
                AntState::Returning,
                AntPos::new(nest.x, nest.y),
                Carrying { amount: 1.0 },
                AntHeading::new(0.0),
            ));
        }

        world.run_system_once(deliver_food).unwrap();

        let stats = world.resource::<SimStats>();
        assert_eq!(stats.colony_total(0), 1.0);
        assert_eq!(stats.colony_total(1), 1.0);
        assert_eq!(stats.delivered_total, 2.0);
    }

    #[test]
    fn delivering_outside_the_nest_does_nothing() {
        let mut world = pile_world();
        world.spawn((
            AntColony(0),
            AntState::Returning,
            AntPos::new(2.0, 2.0),
            Carrying { amount: 1.0 },
            AntHeading::new(0.0),
        ));
        world.run_system_once(deliver_food).unwrap();
        assert_eq!(world.resource::<SimStats>().deliveries, 0);
    }

    /// A full round trip must conserve mass: nothing appears from nowhere.
    #[test]
    fn round_trip_conserves_food() {
        let mut world = pile_world();
        let before = world.resource::<FoodField>().remaining_total;
        world.spawn((
            AntColony(0),
            AntState::Searching,
            AntPos::new(32.0, 32.0),
            Carrying::default(),
            AntHeading::new(0.0),
        ));
        world.run_system_once(pickup_food).unwrap();

        let nest = *world.resource::<Nests>().get(0);
        let mut q = world.query::<&mut AntPos>();
        for mut pos in q.iter_mut(&mut world) {
            pos.x = nest.x;
            pos.y = nest.y;
        }
        world.run_system_once(deliver_food).unwrap();

        let after = world.resource::<FoodField>().remaining_total;
        let delivered = world.resource::<SimStats>().delivered_total;
        assert!(
            ((before - after) - delivered).abs() < 1e-5,
            "before {before}, after {after}, delivered {delivered}"
        );
    }
}
