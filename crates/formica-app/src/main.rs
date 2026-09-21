//! Formica Phase 2 + 3 + 4 - interactive viewer.
//!
//! ```text
//! cargo run -p formica-app --release
//! ```
//!
//! # How the phases fit together
//!
//! The simulation lives in its own [`SimulationTick`] schedule (see
//! `formica-core`), which the engine never runs on its own. `Update` runs it a
//! configurable number of times per frame, `PostUpdate` mirrors the result
//! onto sprites, and mouse input writes straight into the same `FoodField` and
//! `Obstacles` resources the simulation reads. So:
//!
//! * the **same** systems, in the **same** order, produce the headless numbers
//!   and this picture - the viewer cannot drift from the model,
//! * "fast forward", "pause" and "reset" are properties of the driver, not of
//!   the physics, and
//! * food the player drops is indistinguishable, to the simulation, from food
//!   the generator scattered.
//!
//! # Controls
//!
//! | input | effect |
//! |---|---|
//! | left panel | live sliders, presets, restart, map size, colonies |
//! | left click on map | apply the current Mouse mode (food / wall / erase) |
//! | right click on map | erase food and wall under the cursor |
//! | `Esc` | quit |
//!
//! Every slider writes straight into the `SimConfig` resource that all systems
//! read each tick, so there is no apply step and no restart - except for the
//! map size, which has to reallocate the pheromone texture.

mod interact;
mod paint;
mod render;
mod ui;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy_egui::EguiPlugin;
use formica_core::obstacles::Obstacles;
use formica_core::prelude::*;

use interact::FormicaInteractPlugin;
use render::{FormicaRenderPlugin, SimCommands};

fn main() {
    let mut config = SimConfig::default();
    apply_env_overrides(&mut config);

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Formica - ant colony foraging simulator".to_string(),
                        // Wider than tall on purpose: the map is square and the
                        // camera fits it to the *shorter* axis, so the extra
                        // horizontal room is exactly where the control panel
                        // goes without covering any of the colony.
                        resolution: (1600u32, 900u32).into(),
                        ..default()
                    }),
                    ..default()
                })
                // Crisp cells: the world texture is blown up to ~900 px, and
                // bilinear filtering would turn trails to mush.
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugins(EguiPlugin::default())
        .add_plugins(SimPlugin::new(config))
        .add_plugins(FormicaRenderPlugin)
        .add_plugins(FormicaInteractPlugin)
        .add_plugins(ui::FormicaUiPlugin)
        .add_systems(Startup, (arm_script, configure_view_from_env, seed_obstacles))
        .add_systems(
            Update,
            (
                apply_sim_commands,
                drive_simulation,
                run_script.after(drive_simulation),
            )
                .chain(),
        )
        .run();
}

fn env_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok()?.parse().ok()
}

fn env_f32(name: &str) -> Option<f32> {
    std::env::var(name).ok()?.parse().ok()
}

/// Start-up overrides, so a demo or an automated capture can configure the app
/// without anyone touching the panel.
fn apply_env_overrides(config: &mut SimConfig) {
    if let Some(ticks) = env_u32("FORMICA_TICKS_PER_FRAME") {
        config.ticks_per_frame = ticks;
    }
    if let Some(colonies) = env_u32("FORMICA_COLONIES") {
        config.colony_count = colonies as usize;
    }
    if let Some(predators) = env_u32("FORMICA_PREDATORS") {
        config.predator_count = predators as usize;
    }
    if let Some(ants) = env_u32("FORMICA_ANTS") {
        config.ant_count = ants as usize;
    }
    if let Some(size) = env_u32("FORMICA_MAP_SIZE") {
        config.width = size as usize;
        config.height = size as usize;
    }
    if let Some(evaporation) = env_f32("FORMICA_EVAPORATION") {
        config.evaporation_rate = evaporation;
    }
    if let Some(individuality) = env_f32("FORMICA_INDIVIDUALITY") {
        config.individuality = individuality;
    }
    if let Some(threshold) = env_f32("FORMICA_CROWD_THRESHOLD") {
        config.crowd_threshold = threshold;
    }
    if let Some(relief) = env_f32("FORMICA_CROWD_RELIEF") {
        config.crowd_relief = relief;
    }
    if let Ok(kind) = std::env::var("FORMICA_OBSTACLES") {
        config.obstacle_preset = match kind.as_str() {
            "u-wall" | "u_wall" | "wall" => ObstaclePreset::UWall {
                arm: config.food_pile_radius + 8.0,
                gap: config.food_pile_radius * 2.0 + 4.0,
                thickness: 3.0,
            },
            "blocks" | "random" => ObstaclePreset::RandomBlocks { density: 0.15 },
            _ => ObstaclePreset::None,
        };
    }
    // Density is what turns "scattered blocks" into a labyrinth of one-cell
    // passages, which is the scenario the crowding relief exists for, so it
    // needs to be reachable from a scripted capture.
    if let (Some(density), true) = (
        env_f32("FORMICA_BLOCK_DENSITY"),
        matches!(config.obstacle_preset, ObstaclePreset::RandomBlocks { .. }),
    ) {
        config.obstacle_preset = ObstaclePreset::RandomBlocks { density };
    }
    // Narrowing the U's mouth is the cheapest way to script a real bottleneck:
    // the food stays reachable, but only a couple of ants fit through at once.
    if let (Some(gap), ObstaclePreset::UWall { arm, thickness, .. }) =
        (env_f32("FORMICA_U_WALL_GAP"), config.obstacle_preset)
    {
        config.obstacle_preset = ObstaclePreset::UWall {
            arm,
            gap,
            thickness,
        };
    } else if let Some(arm) = env_f32("FORMICA_U_WALL_ARM") {
        if let ObstaclePreset::UWall { gap, thickness, .. } = config.obstacle_preset {
            config.obstacle_preset = ObstaclePreset::UWall {
                arm,
                gap,
                thickness,
            };
        }
    }
    config.sanitize();
}

/// Scripted-run controls, all driven by environment variables so that the
/// viewer's output can be produced and checked without a human at the keyboard:
///
/// | variable | effect |
/// |---|---|
/// | `FORMICA_SCREENSHOT` | path to write one frame to |
/// | `FORMICA_SCREENSHOT_FRAME` | which frame to capture (default 600) |
/// | `FORMICA_EXIT_AFTER` | frame to quit on |
/// | `FORMICA_SHOW_FOOD` / `FORMICA_SHOW_HOME` | `0` hides that field |
/// | `FORMICA_FOOD_GAIN` / `FORMICA_HOME_GAIN` | display gain overrides |
#[derive(Resource, Debug)]
struct ScriptPlan {
    screenshot_path: Option<String>,
    screenshot_frame: u32,
    exit_after: Option<u32>,
    captured: bool,
}

fn arm_script(mut commands: Commands) {
    let screenshot_path = std::env::var("FORMICA_SCREENSHOT").ok();
    let exit_after = env_u32("FORMICA_EXIT_AFTER");
    if screenshot_path.is_none() && exit_after.is_none() {
        return;
    }
    let screenshot_frame = env_u32("FORMICA_SCREENSHOT_FRAME").unwrap_or(600);
    if let Some(path) = &screenshot_path {
        info!("will write a screenshot to {path} at frame {screenshot_frame}");
    }
    commands.insert_resource(ScriptPlan {
        screenshot_path,
        screenshot_frame,
        exit_after,
        captured: false,
    });
}

fn configure_view_from_env(mut view: ResMut<paint::PheromoneView>) {
    if let Ok(v) = std::env::var("FORMICA_SHOW_FOOD") {
        view.show_food = v != "0";
    }
    if let Ok(v) = std::env::var("FORMICA_SHOW_HOME") {
        view.show_home = v != "0";
    }
    if let Some(g) = env_f32("FORMICA_FOOD_GAIN") {
        view.food_gain = g;
    }
    if let Some(g) = env_f32("FORMICA_HOME_GAIN") {
        view.home_gain = g;
    }
}

/// Stamp the configured obstacle preset before the first tick.
///
/// Not strictly necessary - `sync_obstacles` would do it on tick one - but it
/// means the very first painted frame already shows the walls, and it lets a
/// screenshot taken at frame 1 be meaningful.
fn seed_obstacles(
    config: Res<SimConfig>,
    nests: Res<Nests>,
    food: Res<FoodField>,
    mut obstacles: ResMut<Obstacles>,
) {
    obstacles.apply_preset(config.obstacle_preset, &config, &nests.centers(), &food.piles);
}

fn run_script(
    mut commands: Commands,
    plan: Option<ResMut<ScriptPlan>>,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut plan) = plan else {
        return;
    };
    *frame += 1;

    if !plan.captured && *frame >= plan.screenshot_frame {
        plan.captured = true;
        if let Some(path) = plan.screenshot_path.clone() {
            info!("captured frame {} -> {path}", *frame);
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
        }
    }

    if let Some(limit) = plan.exit_after {
        if *frame >= limit {
            exit.write(AppExit::Success);
        }
    }
}

/// Service UI requests that need `&mut World`.
fn apply_sim_commands(world: &mut World) {
    let (restart, clear_trails) = {
        let mut requests = world.resource_mut::<SimCommands>();
        (
            core::mem::take(&mut requests.restart),
            core::mem::take(&mut requests.clear_pheromones),
        )
    };

    if clear_trails {
        world.resource_mut::<PheromoneGrid>().clear();
    }
    if restart {
        reset_simulation(world);
        // `reset_simulation` has already respawned the ants and rebuilt the
        // food and obstacle data; the food and nest *sprites* are separate
        // entities that know only about the old layout, so drop them and let
        // the sync systems recreate them this frame.
        despawn_marked::<render::FoodVisual>(world);
        despawn_marked::<render::NestVisual>(world);
        // Re-stamp the obstacle preset for the rebuilt world.
        let config = world.resource::<SimConfig>().clone();
        let centers = world.resource::<Nests>().centers();
        let piles = world.resource::<FoodField>().piles.clone();
        world
            .resource_mut::<Obstacles>()
            .apply_preset(config.obstacle_preset, &config, &centers, &piles);
        info!("world restarted");
    }
}

fn despawn_marked<T: Component>(world: &mut World) {
    let entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<T>>();
        query.iter(world).collect()
    };
    for entity in entities {
        let _ = world.despawn(entity);
    }
}

/// Run the simulation schedule for this frame.
///
/// An exclusive system because running a schedule needs `&mut World`. It is the
/// only place the tick schedule is driven while the window is open.
fn drive_simulation(world: &mut World) {
    let (paused, ticks_per_frame) = {
        let config = world.resource::<SimConfig>();
        (config.paused, config.ticks_per_frame)
    };
    if paused {
        return;
    }
    for _ in 0..ticks_per_frame {
        world.run_schedule(SimulationTick);
    }
}
