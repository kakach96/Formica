//! Keep the nest flow field in step with the map it flows through.
//!
//! The field is only maintained while `FlowField` homing is selected - the
//! other two modes never read it - but "stale" is not a mode it can leftovers
//! from: when the switch is flipped, the field is simply rebuilt against the
//! current obstacle revision on the next tick. A rebuild is one BFS per map
//! change, not per tick, and `NestFlow::is_current` is what decides.

use bevy_ecs::prelude::*;

use crate::config::{HomingMode, SimConfig};
use crate::flow::{rebuild_nest_flow, NestFlow};
use crate::obstacles::Obstacles;
use crate::world::Nests;

/// Rebuild the flow field whenever its inputs moved under it.
///
/// Runs inside `SimulationTick` right after `sync_obstacles`, so the field a
/// returning ant reads this tick can never disagree with the wall map it is
/// stepping through - the player can draw a wall mid-tick flow.
pub fn sync_nest_flow(
    config: Res<SimConfig>,
    nests: Res<Nests>,
    obstacles: Res<Obstacles>,
    mut flow: ResMut<NestFlow>,
) {
    // Not the active navigation mode: leave whatever is stored exactly as it
    // is. Switching back into `FlowField` rebuilds on the first tick via the
    // revision check, so nothing needs invalidating here.
    if config.homing_mode != HomingMode::FlowField {
        return;
    }
    if flow.is_current(&obstacles, &nests) {
        return;
    }
    rebuild_nest_flow(&mut flow, &obstacles, &nests);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Nest;
    use bevy_ecs::system::RunSystemOnce;

    /// An obstacle edit bumps the revision, and the very next sync therefore
    /// rebuilds the field against it - mapping a drawn wall cannot leave the
    /// ants walking a ghost route.
    #[test]
    fn an_obstacle_edit_rebuilds_the_field() {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            homing_mode: crate::config::HomingMode::FlowField,
            ..Default::default()
        });
        world.insert_resource(Nests::from_config(world.resource::<SimConfig>()));
        world.insert_resource(Obstacles::new(64, 64));
        world.insert_resource(NestFlow::new(64, 64));

        world.run_system_once(sync_nest_flow).unwrap();

        // The player draws a wall right through the flow.
        for y in 0..64 {
            world.resource_mut::<Obstacles>().set(40, y, true);
        }
        world.run_system_once(sync_nest_flow).unwrap();
        assert_eq!(
            world.resource::<NestFlow>().built_revision,
            world.resource::<Obstacles>().revision,
            "the field did not track the wall edit"
        );
    }

    /// In the default mode the field is never maintained.
    #[test]
    fn other_modes_leave_the_field_alone() {
        let mut world = World::new();
        world.insert_resource(SimConfig {
            homing_mode: crate::config::HomingMode::PathIntegration,
            ..Default::default()
        });
        world.insert_resource(Nests::from_config(world.resource::<SimConfig>()));
        world.insert_resource(Obstacles::new(64, 64));
        world.insert_resource(NestFlow::new(64, 64));

        world.run_system_once(sync_nest_flow).unwrap();
        let flow = world.resource::<NestFlow>();
        assert_eq!(flow.built_revision, 0, "the field was built for nothing");
    }
}
