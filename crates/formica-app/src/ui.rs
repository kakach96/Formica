//! Phase 3: the live parameter panel.
//!
//! Every widget writes straight into the `SimConfig` resource, and every
//! simulation system reads that resource on every tick - so "hot reload" is
//! not a feature that had to be built, it is a consequence of the data-oriented
//! design. There is no apply button anywhere in this file.
//!
//! The one exception is the **map size**, which is held as a draft and applied
//! on a button press. That is not a limitation of the design but of the
//! hardware: resizing reallocates the pheromone texture and rebuilds the scene,
//! and doing that on every pixel of a slider drag would be unusable.
//!
//! The presets are deliberately reproducible: each one starts from the brief's
//! defaults and changes a single named thing, so the difference you see on
//! screen is attributable to the parameter you picked.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use formica_core::obstacles::Obstacles;
use formica_core::prelude::*;

use crate::interact::{InteractionState, PaintMode, UiFocus};
use crate::paint::{colony_colors, PheromoneView};
use crate::render::SimCommands;

/// Installs the egui control panel.
pub struct FormicaUiPlugin;

impl Plugin for FormicaUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, control_panel);
    }
}

/// Named parameter sets, each differing from the brief's defaults in one way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Preset {
    Brief,
    NoPheromone,
    Volatile,
    Persistent,
    Wall,
    Hunted,
    Contest,
}

impl Preset {
    const ALL: [Preset; 7] = [
        Preset::Brief,
        Preset::NoPheromone,
        Preset::Volatile,
        Preset::Persistent,
        Preset::Wall,
        Preset::Hunted,
        Preset::Contest,
    ];

    fn label(self) -> &'static str {
        match self {
            Preset::Brief => "Brief defaults",
            Preset::NoPheromone => "No pheromone",
            Preset::Volatile => "Volatile trail",
            Preset::Persistent => "Persistent trail",
            Preset::Wall => "U-shaped wall",
            Preset::Hunted => "Under predation",
            Preset::Contest => "Two colonies",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Preset::Brief => "evaporation 0.10, the parameters from the project brief",
            Preset::NoPheromone => "deposit 0, turn-gain 0 - pure random walk, for comparison",
            Preset::Volatile => "evaporation 0.30 - trails die faster than they form",
            Preset::Persistent => "evaporation 0.01 - trails barely decay, paths ossify",
            Preset::Wall => "food walled into a U - the straight path is blocked",
            Preset::Hunted => "predators on the map: time and cargo are lost, lives are not",
            Preset::Contest => "two colonies, private trails, one shared food supply",
        }
    }

    /// Reset the *behaviour* parameters to the brief's defaults, then apply the
    /// one thing this preset is about. Colony size, map size and seed are left
    /// alone so two presets are directly comparable.
    fn apply(self, config: &mut SimConfig) {
        let defaults = SimConfig::default();
        config.evaporation_rate = defaults.evaporation_rate;
        config.deposit_amount = defaults.deposit_amount;
        config.deposit_radius = defaults.deposit_radius;
        config.max_pheromone = defaults.max_pheromone;
        config.sensor_distance = defaults.sensor_distance;
        config.sensor_angle = defaults.sensor_angle;
        config.sensor_box_radius = defaults.sensor_box_radius;
        config.random_walk_strength = defaults.random_walk_strength;
        config.sensor_turn_gain = defaults.sensor_turn_gain;
        config.speed = defaults.speed;
        config.home_pheromone_weight = defaults.home_pheromone_weight;
        config.predator_count = 0;
        config.obstacle_preset = ObstaclePreset::None;
        config.colony_count = 1;

        // Size the U to the pile it wraps, so the preset works on any map.
        let arm = config.food_pile_radius + 8.0;
        let gap = config.food_pile_radius * 2.0 + 4.0;

        match self {
            Preset::Brief => {}
            Preset::NoPheromone => {
                config.deposit_amount = 0.0;
                config.sensor_turn_gain = 0.0;
            }
            Preset::Volatile => config.evaporation_rate = 0.30,
            Preset::Persistent => config.evaporation_rate = 0.01,
            Preset::Wall => {
                config.obstacle_preset = ObstaclePreset::UWall {
                    arm,
                    gap,
                    thickness: 3.0,
                }
            }
            Preset::Hunted => {
                config.predator_count = 5;
                config.predator_sense = 26.0;
            }
            Preset::Contest => config.colony_count = 2,
        }
        config.sanitize();
    }
}

/// Everything the panel reads or writes.
///
/// Bundled as a `SystemParam` rather than passed as eight separate arguments -
/// a system signature that long is unreadable at the call site and easy to
/// reorder by accident.
#[derive(bevy::ecs::system::SystemParam)]
struct PanelState<'w> {
    config: ResMut<'w, SimConfig>,
    view: ResMut<'w, PheromoneView>,
    commands: ResMut<'w, SimCommands>,
    interaction: ResMut<'w, InteractionState>,
    focus: ResMut<'w, UiFocus>,
    obstacles: Res<'w, Obstacles>,
    stats: Res<'w, SimStats>,
    time: Res<'w, Time>,
}

fn control_panel(
    mut contexts: EguiContexts,
    state: PanelState,
    mut smoothed_fps: Local<f32>,
    mut draft_size: Local<Option<(usize, usize)>>,
) -> Result {
    // Destructured by value so each binding is an owned `Res`/`ResMut`, which
    // coerces to `&mut T` at every call site below.
    let PanelState {
        mut config,
        mut view,
        mut commands,
        mut interaction,
        mut focus,
        obstacles,
        stats,
        time,
    } = state;

    let ctx = contexts.ctx_mut()?;
    // `egui_wants_pointer_input` (not the old `wants_pointer_input`): true when
    // egui wants the pointer for a widget, which is what must suppress map
    // editing underneath the panel.
    focus.pointer_over_ui = ctx.egui_wants_pointer_input();

    // egui 0.36 attaches panels to a `Ui`, not to the context, so the root Ui
    // covering the viewport has to be built explicitly. This mirrors
    // bevy_egui 0.42's own `side_panel` example.
    let viewport = ctx.viewport_rect();
    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("formica_root"),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(viewport),
    );

    let dt = time.delta_secs();
    if dt > 0.0 {
        // Exponential smoothing; a raw per-frame reciprocal is unreadable.
        *smoothed_fps = if *smoothed_fps == 0.0 {
            1.0 / dt
        } else {
            *smoothed_fps * 0.92 + (1.0 / dt) * 0.08
        };
    }

    egui::Panel::left("formica_params")
        .resizable(true)
        .default_size(330.0)
        .max_size(420.0)
        .show(&mut root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Formica");
                ui.small("ant colony foraging - simple rules, emergent trails");
                ui.separator();

                run_stats(ui, &config, &stats, *smoothed_fps);
                ui.separator();

                ui.label(egui::RichText::new("Delivery curve").strong());
                delivery_curve(ui, &stats.samples, 90.0, config.colonies());
                ui.separator();

                run_controls(ui, &mut config, &mut commands);
                ui.separator();

                ui.label(egui::RichText::new("Presets").strong());
                egui::Grid::new("presets").num_columns(1).show(ui, |ui| {
                    for preset in Preset::ALL {
                        if ui
                            .button(preset.label())
                            .on_hover_text(preset.hint())
                            .clicked()
                        {
                            preset.apply(&mut config);
                            // A preset that changes the colonies or the walls
                            // changes the world's shape, so give the renderer a
                            // clean slate rather than leaving half of it stale.
                            *draft_size = Some((config.width, config.height));
                        }
                        ui.end_row();
                    }
                });
                ui.separator();

                world_controls(ui, &mut config, &mut commands, &mut draft_size);
                ui.separator();
                interact_controls(ui, &mut interaction);
                ui.separator();
                obstacle_controls(ui, &mut config, &obstacles);
                ui.separator();
                colony_controls(ui, &mut config);
                ui.separator();
                pheromone_controls(ui, &mut config);
                ui.separator();
                ant_controls(ui, &mut config);
                ui.separator();
                crowd_controls(ui, &mut config, &stats);
                ui.separator();
                predator_controls(ui, &mut config);
                ui.separator();
                display_controls(ui, &mut view);
            });
        });

    Ok(())
}

fn run_stats(ui: &mut egui::Ui, config: &SimConfig, stats: &SimStats, fps: f32) {
    let colonies = config.colonies();
    egui::Grid::new("formica_stats")
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            ui.label("tick");
            ui.label(egui::RichText::new(stats.tick.to_string()).monospace());
            ui.end_row();

            ui.label("delivered");
            ui.label(
                egui::RichText::new(format!(
                    "{:.0} / {:.0}  ({:.1}%)",
                    stats.delivered_total,
                    stats.food_initial,
                    stats.recovery_fraction() * 100.0
                ))
                .monospace(),
            );
            ui.end_row();

            if colonies > 1 {
                for index in 0..colonies {
                    let share = if stats.delivered_total > 0.0 {
                        100.0 * stats.colony_total(index) / stats.delivered_total
                    } else {
                        0.0
                    };
                    ui.label(format!("  colony {index}"));
                    ui.label(
                        egui::RichText::new(format!(
                            "{:.0}  ({share:.1}%)",
                            stats.colony_total(index)
                        ))
                        .monospace(),
                    );
                    ui.end_row();
                }
            }

            ui.label("round trips");
            ui.label(egui::RichText::new(stats.deliveries.to_string()).monospace());
            ui.end_row();

            ui.label("ants / colony");
            ui.label(
                egui::RichText::new(format!(
                    "{} x {}",
                    config.ant_count,
                    config.colonies()
                ))
                .monospace(),
            );
            ui.end_row();

            ui.label("searching / returning");
            ui.label(
                egui::RichText::new(format!("{} / {}", stats.searching, stats.returning))
                    .monospace(),
            );
            ui.end_row();

            ui.label("food left");
            ui.label(egui::RichText::new(format!("{:.0}", stats.food_remaining)).monospace());
            ui.end_row();

            if stats.ants_eaten > 0 {
                ui.label("ants eaten");
                ui.label(egui::RichText::new(stats.ants_eaten.to_string()).monospace());
                ui.end_row();
            }

            ui.label("first delivery");
            ui.label(
                egui::RichText::new(match stats.first_delivery_tick {
                    Some(t) => format!("tick {t}"),
                    None => "not yet".to_string(),
                })
                .monospace(),
            );
            ui.end_row();

            ui.label("fps");
            ui.label(egui::RichText::new(format!("{fps:.0}")).monospace());
            ui.end_row();
        });
}

/// One plotted line: `(samples, value selector, colour)`.
type CurveSeries<'a> = (&'a [Sample], fn(&Sample) -> f64, egui::Color32);

/// Hand-drawn delivery curve.
///
/// Deliberately not `egui_plot`: bevy_egui builds egui without the `plot`
/// feature, and this is thirty lines of painter anyway. With two colonies the
/// per-colony curves are drawn on top of the total, in the same hues the
/// pheromone fields use.
fn delivery_curve(ui: &mut egui::Ui, samples: &[Sample], height: f32, colonies: usize) {
    let (rect, _response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(16, 18, 24));

    if samples.len() < 2 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "collecting samples...",
            egui::FontId::proportional(12.0),
            egui::Color32::from_gray(120),
        );
        return;
    }

    let mut series: Vec<CurveSeries<'_>> = vec![(
        samples,
        (|s: &Sample| s.delivered) as fn(&Sample) -> f64,
        egui::Color32::from_rgb(110, 235, 150),
    )];
    for index in 0..colonies.min(2) {
        let rgb = colony_colors(index).0;
        let color = egui::Color32::from_rgb(
            (rgb[0] * 255.0) as u8,
            (rgb[1] * 255.0) as u8,
            (rgb[2] * 255.0) as u8,
        );
        let pick: fn(&Sample) -> f64 = match index {
            0 => |s: &Sample| s.colony(0),
            _ => |s: &Sample| s.colony(1),
        };
        series.push((samples, pick, color));
    }

    let max_delivered = samples
        .iter()
        .map(|s| s.delivered)
        .fold(0.0f64, f64::max)
        .max(1.0);
    let last_tick = samples.last().map(|s| s.tick).unwrap_or(1).max(1) as f64;
    let inset = rect.shrink(3.0);

    // The total is filled; per-colony curves are lines on top of it.
    // Fill the area under the curve one screen column at a time. A polygon fill
    // would need convexity, which the region under a sigmoid does not have.
    let fill = egui::Color32::from_rgba_unmultiplied(70, 200, 130, 38);
    let columns = inset.width().max(1.0) as usize;
    for i in 0..columns {
        let t = i as f32 / (columns.max(2) - 1) as f32;
        let idx = ((t * (samples.len() - 1) as f32).round() as usize).min(samples.len() - 1);
        let y =
            inset.bottom() - (samples[idx].delivered / max_delivered) as f32 * inset.height();
        painter.vline(
            inset.left() + i as f32,
            y..=inset.bottom(),
            egui::Stroke::new(1.0, fill),
        );
    }

    for (data, pick, color) in &series {
        if data.is_empty() {
            continue;
        }
        let points: Vec<egui::Pos2> = data
            .iter()
            .map(|s| {
                egui::pos2(
                    inset.left() + (s.tick as f64 / last_tick) as f32 * inset.width(),
                    inset.bottom() - (pick(s) / max_delivered) as f32 * inset.height(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(points, egui::Stroke::new(1.6, *color)));
    }

    painter.text(
        egui::pos2(inset.left() + 4.0, inset.top() + 2.0),
        egui::Align2::LEFT_TOP,
        format!("{max_delivered:.0}"),
        egui::FontId::monospace(10.0),
        egui::Color32::from_gray(150),
    );

    let report = analyze(samples);
    painter.text(
        egui::pos2(inset.left() + 4.0, inset.bottom() - 2.0),
        egui::Align2::LEFT_BOTTOM,
        format!("S-score {:.2}", report.sigmoid_score),
        egui::FontId::monospace(10.0),
        egui::Color32::from_gray(150),
    );
}

fn run_controls(ui: &mut egui::Ui, config: &mut SimConfig, commands: &mut SimCommands) {
    ui.label(egui::RichText::new("Run").strong());
    ui.horizontal(|ui| {
        ui.checkbox(&mut config.paused, "pause");
        ui.separator();
        if ui
            .button("restart")
            .on_hover_text("fresh food, empty trails, zeroed statistics")
            .clicked()
        {
            commands.restart = true;
        }
        if ui
            .button("clear trails")
            .on_hover_text("zero every pheromone field without disturbing the ants")
            .clicked()
        {
            commands.clear_pheromones = true;
        }
    });
    ui.add(egui::Slider::new(&mut config.ticks_per_frame, 0..=32).text("ticks per frame"))
        .on_hover_text("fast-forward: the tick schedule is simply run more often per frame");
}

fn world_controls(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    commands: &mut SimCommands,
    draft: &mut Option<(usize, usize)>,
) {
    let current = draft.unwrap_or((config.width, config.height));
    let (mut width, mut height) = current;
    let mut dirty = false;

    egui::CollapsingHeader::new("World & food")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("size");
                dirty |= ui
                    .add(egui::DragValue::new(&mut width).range(32..=1024).speed(4.0))
                    .changed();
                ui.label("x");
                dirty |= ui
                    .add(egui::DragValue::new(&mut height).range(32..=1024).speed(4.0))
                    .changed();
            });
            // Keep it square unless the user really wants otherwise: the camera
            // fits the map to the shorter axis, so a very long map wastes space.
            if ui
                .button("make square")
                .on_hover_text("set both to the larger of the two")
                .clicked()
            {
                let side = width.max(height);
                width = side;
                height = side;
                dirty = true;
            }

            let pending = (width, height) != (config.width, config.height);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(pending, egui::Button::new("apply size"))
                    .on_hover_text(
                        "reallocates the pheromone texture and rebuilds the scene, \
                         which discards the current run",
                    )
                    .clicked()
                {
                    config.width = width;
                    config.height = height;
                    config.sanitize();
                    commands.restart = true;
                    *draft = Some((config.width, config.height));
                }
                if pending {
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 190, 90),
                        format!("{}x{} -> {}x{}", config.width, config.height, width, height),
                    );
                }
            });
            if dirty {
                *draft = Some((width, height));
            }

            ui.separator();
            let mut piles = config.food_pile_count as u32;
            if ui
                .add(egui::Slider::new(&mut piles, 1..=8).text("starting piles"))
                .on_hover_text("applied on restart; click the map to add more")
                .changed()
            {
                config.food_pile_count = piles as usize;
            }
            ui.add(egui::Slider::new(&mut config.food_per_pile, 0.0..=20_000.0).text("food per pile"));
            ui.add(egui::Slider::new(&mut config.food_pile_radius, 2.0..=24.0).text("pile radius"));
            ui.label(
                egui::RichText::new(format!(
                    "{:.0} units on the map ({:.1}% recovered)",
                    config.food_per_pile * config.food_pile_count as f32,
                    0.0
                ))
                .small()
                .color(egui::Color32::from_gray(140)),
            );
        });
}

fn interact_controls(ui: &mut egui::Ui, interaction: &mut InteractionState) {
    egui::CollapsingHeader::new("Mouse")
        .default_open(true)
        .show(ui, |ui| {
            ui.checkbox(&mut interaction.enabled, "enable map editing");
            ui.horizontal_wrapped(|ui| {
                for mode in PaintMode::ALL {
                    ui.selectable_value(&mut interaction.mode, mode, mode.label());
                }
            });
            ui.add(egui::Slider::new(&mut interaction.brush_radius, 2.0..=24.0).text("brush radius"));
            if interaction.mode == PaintMode::Food {
                ui.add(
                    egui::Slider::new(&mut interaction.pile_amount, 100.0..=10_000.0)
                        .text("food per pile"),
                );
            }
            ui.small("left click applies the mode; right click erases food and wall");
        });
}

fn obstacle_controls(ui: &mut egui::Ui, config: &mut SimConfig, obstacles: &Obstacles) {
    egui::CollapsingHeader::new("Obstacles")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let is_none = matches!(config.obstacle_preset, ObstaclePreset::None);
                if ui.selectable_label(is_none, "open field").clicked() {
                    config.obstacle_preset = ObstaclePreset::None;
                }
                let is_wall = matches!(config.obstacle_preset, ObstaclePreset::UWall { .. });
                if ui.selectable_label(is_wall, "U-shaped wall").clicked() && !is_wall {
                    config.obstacle_preset = ObstaclePreset::UWall {
                        arm: config.food_pile_radius + 8.0,
                        gap: config.food_pile_radius * 2.0 + 4.0,
                        thickness: 3.0,
                    };
                }
                let is_blocks = matches!(config.obstacle_preset, ObstaclePreset::RandomBlocks { .. });
                if ui.selectable_label(is_blocks, "scattered blocks").clicked() && !is_blocks {
                    config.obstacle_preset = ObstaclePreset::RandomBlocks { density: 0.15 };
                }
            });

            match &mut config.obstacle_preset {
                ObstaclePreset::None => {
                    ui.small("an open field: the baseline everything else is compared against");
                }
                ObstaclePreset::UWall { arm, gap, thickness } => {
                    ui.add(egui::Slider::new(arm, 4.0..=40.0).text("arm length"));
                    ui.add(egui::Slider::new(gap, 4.0..=40.0).text("clear width"));
                    ui.add(egui::Slider::new(thickness, 1.0..=8.0).text("thickness"));
                    ui.small("wraps the first pile, closed side facing the colony");
                }
                ObstaclePreset::RandomBlocks { density } => {
                    ui.add(egui::Slider::new(density, 0.01..=0.4).text("wall density"));
                }
            }

            ui.label(
                egui::RichText::new(format!(
                    "{} wall cells ({:.1}% of the map)",
                    obstacles.count(),
                    obstacles.coverage() * 100.0
                ))
                .small()
                .color(egui::Color32::from_gray(140)),
            );
            ui.small("draw your own with the Mouse section above");
        });
}

fn colony_controls(ui: &mut egui::Ui, config: &mut SimConfig) {
    egui::CollapsingHeader::new("Colonies")
        .default_open(false)
        .show(ui, |ui| {
            let mut colonies = config.colony_count as u32;
            if ui
                .add(egui::Slider::new(&mut colonies, 1..=2).text("competing colonies"))
                .on_hover_text(
                    "each colony has private pheromone fields and its own nest, but \
                     they share the same finite food",
                )
                .changed()
            {
                config.colony_count = colonies as usize;
            }
            if config.colonies() > 1 {
                ui.add(egui::Slider::new(&mut config.nest_orbit, 20.0..=140.0).text("nest spacing"));
                ui.small(
                    "colony 0 trails are green/violet, colony 1 are amber/cyan. \
                     Neither can smell the other.",
                );
            }
        });
}

fn pheromone_controls(ui: &mut egui::Ui, config: &mut SimConfig) {
    ui.label(egui::RichText::new("Pheromones").strong());
    ui.add(
        egui::Slider::new(&mut config.evaporation_rate, 0.001..=0.5)
            .logarithmic(true)
            .text("evaporation /tick"),
    )
    .on_hover_text("c *= (1 - rate) every tick");
    ui.add(egui::Slider::new(&mut config.deposit_amount, 0.0..=5.0).text("deposit strength"));
    ui.add(egui::Slider::new(&mut config.deposit_radius, 0..=3).text("deposit kernel radius"));
    ui.add(egui::Slider::new(&mut config.max_pheromone, 1.0..=100.0).text("cell ceiling"));
}

fn ant_controls(ui: &mut egui::Ui, config: &mut SimConfig) {
    ui.label(egui::RichText::new("Ants").strong());

    let mut ant_count = config.ant_count as u32;
    if ui
        .add(egui::Slider::new(&mut ant_count, 1..=2_000).text("ants per colony"))
        .on_hover_text("applied live: ants are spawned at, or removed from, their nest")
        .changed()
    {
        config.ant_count = ant_count as usize;
    }

    ui.add(egui::Slider::new(&mut config.speed, 0.5..=6.0).text("speed (cells/tick)"));
    ui.add(egui::Slider::new(&mut config.sensor_distance, 0.0..=12.0).text("perception radius"))
        .on_hover_text("how far ahead the antennae sample, in cells");
    ui.add(egui::Slider::new(&mut config.sensor_angle, 0.0..=1.5).text("antenna spread (rad)"));
    ui.add(egui::Slider::new(&mut config.sensor_box_radius, 0..=3).text("antenna box radius"));
    ui.add(egui::Slider::new(&mut config.random_walk_strength, 0.0..=1.5).text("random walk (rad)"));
    ui.add(egui::Slider::new(&mut config.sensor_turn_gain, 0.0..=1.5).text("trail turn gain"))
        .on_hover_text("0 disables trail following entirely");
    ui.add(egui::Slider::new(&mut config.home_pheromone_weight, 0.0..=1.0).text("home-trail guidance"))
        .on_hover_text("0 = returning ants head straight for the nest (brief behaviour)");
    ui.add(egui::Slider::new(&mut config.individuality, 0.0..=1.5).text("individuality"))
        .on_hover_text(
            "spread of per-ant speed and wander. 0 makes every ant identical, which is \
             what makes the colony expand as a hollow ring",
        );
}

/// Crowding: the knob that stops a bottleneck collapsing the colony onto a
/// point, plus the live readout that says whether it is happening.
fn crowd_controls(ui: &mut egui::Ui, config: &mut SimConfig, stats: &SimStats) {
    egui::CollapsingHeader::new("Crowding")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut config.crowd_threshold, 4.0..=120.0).text("jam threshold"))
                .on_hover_text("ants sharing one 4x4-cell bin before relief starts");
            ui.add(egui::Slider::new(&mut config.crowd_relief, 0.0..=2.0).text("relief step (cells)"))
                .on_hover_text("0 disables crowding relief entirely");

            // Live concentration: if this climbs into the hundreds while food
            // is walled off, the colony is piling onto one spot.
            let uniform = if config.cell_count() > 0 {
                config.total_ants() as f32 * 16.0 / config.cell_count() as f32
            } else {
                0.0
            };
            ui.small(format!(
                "busiest 4x4 bin: {} ants  (peak {}), over {} bins\n\
                 an evenly spread colony would hold about {:.1} per bin",
                stats.crowd_max_bin, stats.crowd_peak_bin, stats.crowd_occupied, uniform
            ));
        });
}

fn predator_controls(ui: &mut egui::Ui, config: &mut SimConfig) {
    egui::CollapsingHeader::new("Predators")
        .default_open(false)
        .show(ui, |ui| {
            let mut count = config.predator_count as u32;
            if ui
                .add(egui::Slider::new(&mut count, 0..=40).text("predators"))
                .on_hover_text("0 turns predation off entirely")
                .changed()
            {
                config.predator_count = count as usize;
            }
            ui.add_enabled_ui(config.predator_count > 0, |ui| {
                ui.add(egui::Slider::new(&mut config.predator_speed, 0.0..=4.0).text("speed"));
                ui.add(egui::Slider::new(&mut config.predator_sense, 0.0..=120.0).text("sight radius"));
                ui.add(egui::Slider::new(&mut config.predator_catch_radius, 0.5..=12.0).text("catch radius"));
                ui.add(egui::Slider::new(&mut config.predator_wander, 0.0..=1.5).text("wander"));
                ui.small(
                    "eaten ants are replaced at their nest, so predation costs time \
                     and cargo rather than lives. Nests are a refuge.",
                );
            });
        });
}

fn display_controls(ui: &mut egui::Ui, view: &mut PheromoneView) {
    ui.label(egui::RichText::new("Display").strong());
    ui.add(
        egui::Slider::new(&mut view.food_gain, 0.02..=2.0)
            .logarithmic(true)
            .text("food trail gain"),
    );
    ui.add(
        egui::Slider::new(&mut view.home_gain, 0.0..=1.0)
            .logarithmic(true)
            .text("home trail gain"),
    )
    .on_hover_text("drop this to 0 to isolate the food trail, and vice versa");
    ui.horizontal(|ui| {
        ui.checkbox(&mut view.show_food, "food trail");
        ui.checkbox(&mut view.show_home, "home trail");
    });
    ui.small("colony 0: green / violet    colony 1: amber / cyan");
    ui.small("ants: white / orange    predators: red    walls: grey");
}
