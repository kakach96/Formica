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
use formica_core::i18n::{lang_choices, Lang, Strings};
use formica_core::obstacles::Obstacles;
use formica_core::prelude::*;
use formica_core::scenario::Scenario;

use crate::interact::{InteractionState, PaintMode, UiFocus};
use crate::lang::UiLang;
use crate::paint::{colony_colors, PheromoneView};
use crate::render::{PanelMetrics, SimCommands};

/// Install the CJK fallback font and the panel look the first time the egui
/// context exists.
///
/// Runs every pass until it succeeds: on the very first frames the primary
/// egui context has not been created yet (a `Startup`-time install panics on
/// exactly that - "no entities fit the query"), so this retries silently and
/// becomes a no-op afterwards.
pub fn install_panel_font(mut contexts: EguiContexts, mut done: Local<bool>) -> Result {
    if *done {
        return Ok(());
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return Ok(());
    };
    crate::lang::install_cjk_fallback(ctx);
    apply_panel_style(ctx);
    *done = true;
    Ok(())
}

/// One-set styling for the panel: a moss-dark board instead of egui's
/// neutral grey, rounded buttons and a green selection accent, so the control
/// panel reads as part of the soil-coloured scene rather than a floating
/// spreadsheet.
fn apply_panel_style(ctx: &egui::Context) {
    // Both themes, so a user-forced light theme keeps the same accent.
    ctx.all_styles_mut(|style| {
        let visuals = &mut style.visuals;
        visuals.panel_fill = egui::Color32::from_rgb(22, 26, 22);
        visuals.selection.bg_fill = egui::Color32::from_rgb(72, 150, 88);
        visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(190, 240, 195));
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.weak_bg_fill = egui::Color32::from_rgb(34, 40, 34);
            widget.bg_fill = egui::Color32::from_rgb(30, 36, 30);
            widget.corner_radius = (4.0).into();
        }
        visuals.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(44, 53, 44);
        visuals.widgets.active.weak_bg_fill = egui::Color32::from_rgb(56, 68, 56);
        style.spacing.item_spacing = egui::vec2(10.0, 7.0);
    });
}

/// Installs the egui control panel.
pub struct FormicaUiPlugin;

impl Plugin for FormicaUiPlugin {
    fn build(&self, app: &mut App) {
        // The font install must land before the first panel frame reads the
        // string tables; the pass schedule guarantees the ordering.
        app.add_systems(
            EguiPrimaryContextPass,
            (install_panel_font, control_panel).chain(),
        );
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

    fn label(self, s: &Strings) -> &'static str {
        match self {
            Preset::Brief => s.preset_brief,
            Preset::NoPheromone => s.preset_no_pheromone,
            Preset::Volatile => s.preset_volatile,
            Preset::Persistent => s.preset_persistent,
            Preset::Wall => s.preset_wall,
            Preset::Hunted => s.preset_hunted,
            Preset::Contest => s.preset_contest,
        }
    }

    fn hint(self, s: &Strings) -> &'static str {
        match self {
            Preset::Brief => s.preset_brief_hint,
            Preset::NoPheromone => s.preset_no_pheromone_hint,
            Preset::Volatile => s.preset_volatile_hint,
            Preset::Persistent => s.preset_persistent_hint,
            Preset::Wall => s.preset_wall_hint,
            Preset::Hunted => s.preset_hunted_hint,
            Preset::Contest => s.preset_contest_hint,
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
    food: Res<'w, FoodField>,
    time: Res<'w, Time>,
    ui_lang: ResMut<'w, UiLang>,
    panel: ResMut<'w, PanelMetrics>,
}

fn control_panel(
    mut contexts: EguiContexts,
    state: PanelState,
    mut smoothed_fps: Local<f32>,
    mut draft_size: Local<Option<(usize, usize)>>,
    // Result of the last scenario export/import press, cleared on load.
    mut export_status: Local<Option<String>>,
    // The currently selected built-in scenario name, shown in the dropdown.
    mut scenario_selection: Local<Option<String>>,
    // The export/import file path; defaults to the repo-root scenarios folder.
    mut scenario_path: Local<String>,
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
        food,
        time,
        mut ui_lang,
        mut panel,
    } = state;
    // One table per frame; every widget below reads fields from it.
    let s = ui_lang.strings();

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

    let panel_response = egui::Panel::left("formica_params")
        .resizable(true)
        .default_size(330.0)
        .max_size(420.0)
        .show(&mut root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Formica");
                ui.small(s.app_subtitle);

                // Live language switch: the table is re-read every frame, so
                // this takes effect on the next repaint with no restart.
                egui::ComboBox::from_id_salt("formica_lang")
                    .selected_text(ui_lang.lang.code())
                    .show_ui(ui, |ui| {
                        for (code, name) in lang_choices(ui_lang.lang) {
                            let value = Lang::parse(code).unwrap_or_default();
                            ui.selectable_value(
                                &mut ui_lang.lang,
                                value,
                                format!("{name} ({code})"),
                            );
                        }
                    });
                ui.separator();

                run_stats(ui, &config, &stats, *smoothed_fps, s);
                ui.separator();

                ui.label(egui::RichText::new(s.curve_title).strong());
                delivery_curve(ui, &stats.samples, 90.0, config.colonies(), s);
                ui.separator();

                run_controls(ui, &mut config, &mut commands, s);
                ui.separator();

                egui::CollapsingHeader::new(egui::RichText::new(s.presets_section).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::Grid::new("presets").num_columns(1).show(ui, |ui| {
                            for preset in Preset::ALL {
                                if ui
                                    .button(preset.label(s))
                                    .on_hover_text(preset.hint(s))
                                    .clicked()
                                {
                                    preset.apply(&mut config);
                                    // A preset that changes the colonies or the
                                    // walls changes the world's shape, so give
                                    // the renderer a clean slate rather than
                                    // leaving half of it stale.
                                    *draft_size = Some((config.width, config.height));
                                }
                                ui.end_row();
                            }
                        });
                    });
                ui.separator();

                world_controls(ui, &mut config, &mut commands, &mut draft_size, s);
                ui.separator();
                egui::CollapsingHeader::new(egui::RichText::new(s.scenario_section).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        scenario_controls(
                            ui,
                            &config,
                            &mut commands,
                            &food,
                            s,
                            &mut scenario_selection,
                            &mut export_status,
                            &mut scenario_path,
                        );
                    });
                ui.separator();
                interact_controls(ui, &mut interaction, s);
                ui.separator();
                obstacle_controls(ui, &mut config, &obstacles, s);
                ui.separator();
                homing_controls(ui, &mut config, s);
                ui.separator();
                colony_controls(ui, &mut config, s);
                ui.separator();
                pheromone_controls(ui, &mut config, s);
                ui.separator();
                ant_controls(ui, &mut config, s);
                ui.separator();
                crowd_controls(ui, &mut config, &stats, s);
                ui.separator();
                predator_controls(ui, &mut config, s);
                ui.separator();
                display_controls(ui, &mut view, s);
            });
        });
    // The renderer centers the map in the area free of this panel, so it needs
    // to know how wide the panel currently is.
    panel.width = panel_response.response.rect.width();

    Ok(())
}

fn run_stats(ui: &mut egui::Ui, config: &SimConfig, stats: &SimStats, fps: f32, s: &Strings) {
    let colonies = config.colonies();
    egui::Grid::new("formica_stats")
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            ui.label(s.tick);
            ui.label(egui::RichText::new(stats.tick.to_string()).monospace());
            ui.end_row();

            ui.label(s.delivered);
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
                    ui.label(format!("  {} {index}", s.colony_row));
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

            ui.label(s.round_trips);
            ui.label(egui::RichText::new(stats.deliveries.to_string()).monospace());
            ui.end_row();

            ui.label(s.ants_per_colony);
            ui.label(
                egui::RichText::new(format!("{} x {}", config.ant_count, config.colonies()))
                    .monospace(),
            );
            ui.end_row();

            ui.label(s.searching_returning);
            ui.label(
                egui::RichText::new(format!("{} / {}", stats.searching, stats.returning))
                    .monospace(),
            );
            ui.end_row();

            ui.label(s.food_left);
            ui.label(egui::RichText::new(format!("{:.0}", stats.food_remaining)).monospace());
            ui.end_row();

            if stats.ants_eaten > 0 {
                ui.label(s.ants_eaten);
                ui.label(egui::RichText::new(stats.ants_eaten.to_string()).monospace());
                ui.end_row();
            }

            ui.label(s.first_delivery);
            ui.label(
                egui::RichText::new(match stats.first_delivery_tick {
                    Some(t) => format!("{} {t}", s.tick),
                    None => s.not_yet.to_string(),
                })
                .monospace(),
            );
            ui.end_row();

            ui.label(s.fps);
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
fn delivery_curve(
    ui: &mut egui::Ui,
    samples: &[Sample],
    height: f32,
    colonies: usize,
    s: &Strings,
) {
    let (rect, _response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, egui::Color32::from_rgb(16, 18, 24));

    if samples.len() < 2 {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            s.curve_collecting,
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
        let y = inset.bottom() - (samples[idx].delivered / max_delivered) as f32 * inset.height();
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
        format!("{} {:.2}", s.curve_s_score, report.sigmoid_score),
        egui::FontId::monospace(10.0),
        egui::Color32::from_gray(150),
    );
}

fn run_controls(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    commands: &mut SimCommands,
    s: &Strings,
) {
    ui.label(egui::RichText::new(s.run_section).strong());
    ui.horizontal(|ui| {
        // One toggle instead of a checkbox plus label: pressed = paused.
        if ui
            .add(egui::Button::new(s.pause).small().selected(config.paused))
            .on_hover_text(s.pause_hint)
            .clicked()
        {
            config.paused = !config.paused;
        }
        if ui.button(s.restart).on_hover_text(s.restart_hint).clicked() {
            commands.restart = true;
        }
        if ui
            .button(s.clear_trails)
            .on_hover_text(s.clear_trails_hint)
            .clicked()
        {
            commands.clear_pheromones = true;
        }
    });
    ui.add(egui::Slider::new(&mut config.ticks_per_frame, 0..=32).text(s.ticks_per_frame))
        .on_hover_text(s.ticks_per_frame_hint);
}

fn world_controls(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    commands: &mut SimCommands,
    draft: &mut Option<(usize, usize)>,
    s: &Strings,
) {
    let current = draft.unwrap_or((config.width, config.height));
    let (mut width, mut height) = current;
    let mut dirty = false;

    egui::CollapsingHeader::new(s.world_section)
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(s.size);
                dirty |= ui
                    .add(egui::DragValue::new(&mut width).range(32..=1024).speed(4.0))
                    .changed();
                ui.label("x");
                dirty |= ui
                    .add(
                        egui::DragValue::new(&mut height)
                            .range(32..=1024)
                            .speed(4.0),
                    )
                    .changed();
            });
            // Keep it square unless the user really wants otherwise: the camera
            // fits the map to the shorter axis, so a very long map wastes space.
            if ui
                .button(s.make_square)
                .on_hover_text(s.make_square_hint)
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
                    .add_enabled(pending, egui::Button::new(s.apply_size))
                    .on_hover_text(s.apply_size_hint)
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
                .add(egui::Slider::new(&mut piles, 1..=8).text(s.starting_piles))
                .on_hover_text(s.starting_piles_hint)
                .changed()
            {
                config.food_pile_count = piles as usize;
            }
            ui.add(
                egui::Slider::new(&mut config.food_per_pile, 0.0..=20_000.0).text(s.food_per_pile),
            );
            ui.add(egui::Slider::new(&mut config.food_pile_radius, 2.0..=24.0).text(s.pile_radius));
            ui.label(
                egui::RichText::new(format!(
                    "{:.0} {} ({:.1}% recovered)",
                    config.food_per_pile * config.food_pile_count as f32,
                    s.units_on_map,
                    0.0
                ))
                .small()
                .color(egui::Color32::from_gray(140)),
            );
        });
}

/// Scenario loading, export and import.
///
/// Loading goes through `SimCommands` rather than mutating the config here:
/// the apply needs `&mut World` (the reset respawns ants), so the request
/// rides the same command path as the restart button. Import is just a load
/// from a file - it goes through the identical command path, which is why an
/// imported scenario and a picked built-in end up in exactly the same world.
/// Export needs only readable data - the config plus the live pile list -
/// so it happens here.
fn scenario_controls(
    ui: &mut egui::Ui,
    config: &SimConfig,
    commands: &mut SimCommands,
    food: &FoodField,
    s: &Strings,
    selected: &mut Option<String>,
    export_status: &mut Option<String>,
    import_path: &mut String,
) {
    ui.label(egui::RichText::new(s.scenario_section).strong());

    let builtins = Scenario::builtins();
    ui.horizontal(|ui| {
        let label = selected
            .as_deref()
            .unwrap_or_else(|| match builtins.first() {
                Some(first) => first.name.as_str(),
                None => "",
            })
            .to_string();
        egui::ComboBox::from_id_salt("formica_scenario")
            .selected_text(label)
            .show_ui(ui, |ui| {
                for scenario in &builtins {
                    let is_selected = selected.as_deref() == Some(scenario.name.as_str());
                    if ui
                        .selectable_label(is_selected, &scenario.name)
                        .on_hover_text(&scenario.description)
                        .clicked()
                    {
                        *selected = Some(scenario.name.clone());
                        commands.load_scenario = Some(scenario.clone());
                        // Loading changes the map shape, so the draft size is
                        // no longer what the renderer was built for.
                        *export_status = None;
                    }
                }
            });
    });
    ui.small(s.scenario_builtins_hint);

    // The export/import path box doubles for both directions: name a file,
    // export writes it, import reads it. That round trip is the whole point -
    // an exported set-up is reproducible by typing the same path.
    if import_path.is_empty() {
        *import_path = "scenarios/formica-scenario.toml".to_string();
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(import_path)
                .desired_width(200.0)
                .hint_text("scenarios/….toml"),
        );
        if ui
            .button(s.scenario_import)
            .on_hover_text(s.scenario_import_hint)
            .clicked()
        {
            import_scenario(commands, import_path.trim(), s, export_status);
        }
    });

    ui.horizontal(|ui| {
        if ui
            .button(s.scenario_export)
            .on_hover_text(s.scenario_export_hint)
            .clicked()
        {
            export_current_setup(config, food, import_path.trim(), s, export_status);
        }
        if ui
            .button(s.scenario_export_preset)
            .on_hover_text(s.scenario_export_preset_hint)
            .clicked()
        {
            export_selected_preset(selected, s, export_status);
        }
    });
    if let Some(status) = export_status {
        ui.small(status.clone());
    }
}

/// Write the live config and pile layout to the named TOML file.
fn export_current_setup(
    config: &SimConfig,
    food: &FoodField,
    raw_path: &str,
    s: &Strings,
    status: &mut Option<String>,
) {
    let Some(path) = with_toml_extension(raw_path) else {
        *status = Some(s.scenario_no_path.to_string());
        return;
    };
    let path = std::path::PathBuf::from(path);
    // Exports land in their own folder rather than the repo root, where the
    // binary happens to run from.
    if let Some(directory) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(directory);
    }
    let scenario = Scenario::capture("exported", "", config, &food.piles);
    *status = Some(match std::fs::write(&path, scenario.to_toml_string()) {
        Ok(()) => s
            .scenario_exported_fmt
            .replace("{path}", &path.display().to_string()),
        Err(err) => s
            .scenario_failed_fmt
            .replace("{path}", &path.display().to_string())
            .replace("{err}", &err.to_string()),
    });
}

/// Write the built-in currently shown in the dropdown as a complete TOML.
///
/// Unlike the live capture above this serialises the preset itself, so its
/// hand-written walls and food spots survive the round trip byte for byte -
/// that is the "share this exact map" path.
fn export_selected_preset(selected: &Option<String>, s: &Strings, status: &mut Option<String>) {
    let name = selected.as_deref().unwrap_or("");
    match Scenario::builtin(name) {
        Some(preset) => {
            let directory = std::path::Path::new("scenarios");
            let path = directory.join(format!("{}.toml", preset.name));
            *status = Some(
                match std::fs::create_dir_all(directory)
                    .and_then(|()| std::fs::write(&path, preset.to_toml_string()))
                {
                    Ok(()) => s
                        .scenario_preset_exported_fmt
                        .replace("{name}", &preset.name)
                        .replace("{path}", &path.display().to_string()),
                    Err(err) => s
                        .scenario_failed_fmt
                        .replace("{path}", &path.display().to_string())
                        .replace("{err}", &err.to_string()),
                },
            );
        }
        None => {
            let list = Scenario::builtin_names().join(", ");
            *status = Some(
                s.err_unknown_scenario
                    .replace("{name}", name)
                    .replace("{list}", &list),
            );
        }
    }
}

/// Read a scenario TOML from disk and queue a world rebuild.
fn import_scenario(
    commands: &mut SimCommands,
    raw_path: &str,
    s: &Strings,
    status: &mut Option<String>,
) {
    let attempt = with_toml_extension(raw_path)
        .ok_or_else(|| s.scenario_no_path.to_string())
        .and_then(|path| {
            std::fs::read_to_string(&path)
                .map_err(|err| err.to_string())
                .and_then(|text| Scenario::parse_toml(&text).map(|scenario| (path, scenario)))
        });
    match attempt {
        Ok((path, scenario)) => {
            *status = Some(
                s.scenario_imported_fmt
                    .replace("{path}", &path)
                    .replace("{name}", &scenario.name),
            );
            commands.load_scenario = Some(scenario);
        }
        Err(err) => {
            *status = Some(
                s.scenario_import_failed_fmt
                    .replace("{path}", raw_path)
                    .replace("{err}", &err),
            );
        }
    }
}

/// Force the typed path to end in `.toml`, so the export can be read back by
/// the import box and by the CLI's `--scenario` (both resolve files by that
/// extension before falling back to built-in names).
fn with_toml_extension(path: &str) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.to_ascii_lowercase().ends_with(".toml") {
        Some(trimmed.to_string())
    } else {
        Some(format!("{trimmed}.toml"))
    }
}

fn interact_controls(ui: &mut egui::Ui, interaction: &mut InteractionState, s: &Strings) {
    egui::CollapsingHeader::new(s.mouse_section)
        .default_open(true)
        .show(ui, |ui| {
            ui.checkbox(&mut interaction.enabled, s.enable_map_editing);
            ui.horizontal_wrapped(|ui| {
                for mode in PaintMode::ALL {
                    ui.selectable_value(&mut interaction.mode, mode, mode.label_for(s));
                }
            });
            ui.add(
                egui::Slider::new(&mut interaction.brush_radius, 2.0..=24.0).text(s.brush_radius),
            );
            if interaction.mode == PaintMode::Food {
                ui.add(
                    egui::Slider::new(&mut interaction.pile_amount, 100.0..=10_000.0)
                        .text(s.food_per_pile),
                );
            }
            ui.small(s.mouse_help);
        });
}

fn obstacle_controls(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    obstacles: &Obstacles,
    s: &Strings,
) {
    egui::CollapsingHeader::new(s.obstacles_section)
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let is_none = matches!(config.obstacle_preset, ObstaclePreset::None);
                if ui.selectable_label(is_none, s.obstacle_none).clicked() {
                    config.obstacle_preset = ObstaclePreset::None;
                }
                let is_wall = matches!(config.obstacle_preset, ObstaclePreset::UWall { .. });
                if ui.selectable_label(is_wall, s.obstacle_u_wall).clicked() && !is_wall {
                    config.obstacle_preset = ObstaclePreset::UWall {
                        arm: config.food_pile_radius + 8.0,
                        gap: config.food_pile_radius * 2.0 + 4.0,
                        thickness: 3.0,
                    };
                }
                let is_blocks =
                    matches!(config.obstacle_preset, ObstaclePreset::RandomBlocks { .. });
                if ui.selectable_label(is_blocks, s.obstacle_blocks).clicked() && !is_blocks {
                    config.obstacle_preset = ObstaclePreset::RandomBlocks { density: 0.15 };
                }
            });

            match &mut config.obstacle_preset {
                ObstaclePreset::None => {
                    ui.small(s.obstacle_none_hint);
                }
                ObstaclePreset::UWall {
                    arm,
                    gap,
                    thickness,
                } => {
                    ui.add(egui::Slider::new(arm, 4.0..=40.0).text(s.arm_length));
                    ui.add(egui::Slider::new(gap, 4.0..=40.0).text(s.clear_width));
                    ui.add(egui::Slider::new(thickness, 1.0..=8.0).text(s.thickness));
                    ui.small(s.obstacle_u_wall_hint);
                }
                ObstaclePreset::RandomBlocks { density } => {
                    ui.add(egui::Slider::new(density, 0.01..=0.4).text(s.wall_density));
                }
            }

            ui.label(
                egui::RichText::new(format!(
                    "{} {} ({:.1}% of the map)",
                    obstacles.count(),
                    s.wall_cells,
                    obstacles.coverage() * 100.0
                ))
                .small()
                .color(egui::Color32::from_gray(140)),
            );
            ui.small(s.draw_your_own);
        });
}

/// How returning ants get home, live-selectable: the anti-jam knob for maps
/// where a drawn wall sits between an ant and its nest.
fn homing_controls(ui: &mut egui::Ui, config: &mut SimConfig, s: &Strings) {
    egui::CollapsingHeader::new(egui::RichText::new(s.homing_section).strong())
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(s.homing_mode);
                let modes = [
                    (
                        HomingMode::PathIntegration,
                        s.homing_path,
                        s.homing_path_hint,
                    ),
                    (HomingMode::WallSense, s.homing_wall, s.homing_wall_hint),
                    (HomingMode::FlowField, s.homing_flow, s.homing_flow_hint),
                ];
                for (mode, label, hint) in modes {
                    let selected = config.homing_mode == mode;
                    if ui
                        .selectable_label(selected, label)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        config.homing_mode = mode;
                    }
                }
            });
        });
}

fn colony_controls(ui: &mut egui::Ui, config: &mut SimConfig, s: &Strings) {
    egui::CollapsingHeader::new(s.colonies_section)
        .default_open(false)
        .show(ui, |ui| {
            let mut colonies = config.colony_count as u32;
            if ui
                .add(egui::Slider::new(&mut colonies, 1..=2).text(s.competing_colonies))
                .on_hover_text(s.competing_colonies_hint)
                .changed()
            {
                config.colony_count = colonies as usize;
            }
            if config.colonies() > 1 {
                ui.add(
                    egui::Slider::new(&mut config.nest_orbit, 20.0..=140.0).text(s.nest_spacing),
                );
                ui.small(s.colony_colors_note);
            }
        });
}

fn pheromone_controls(ui: &mut egui::Ui, config: &mut SimConfig, s: &Strings) {
    egui::CollapsingHeader::new(egui::RichText::new(s.pheromones_section).strong())
        .default_open(false)
        .show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut config.evaporation_rate, 0.001..=0.5)
                    .logarithmic(true)
                    .text(s.evaporation),
            )
            .on_hover_text(s.evaporation_hint);
            ui.add(
                egui::Slider::new(&mut config.deposit_amount, 0.0..=5.0).text(s.deposit_strength),
            );
            ui.add(
                egui::Slider::new(&mut config.deposit_radius, 0..=3).text(s.deposit_kernel_radius),
            );
            ui.add(egui::Slider::new(&mut config.max_pheromone, 1.0..=100.0).text(s.cell_ceiling));
        });
}

fn ant_controls(ui: &mut egui::Ui, config: &mut SimConfig, s: &Strings) {
    egui::CollapsingHeader::new(egui::RichText::new(s.ants_section).strong())
        .default_open(false)
        .show(ui, |ui| {
            let mut ant_count = config.ant_count as u32;
            if ui
                .add(egui::Slider::new(&mut ant_count, 1..=2_000).text(s.ants_per_colony_slider))
                .on_hover_text(s.ants_per_colony_hint)
                .changed()
            {
                config.ant_count = ant_count as usize;
            }

            ui.add(egui::Slider::new(&mut config.speed, 0.5..=6.0).text(s.speed));
            ui.add(
                egui::Slider::new(&mut config.sensor_distance, 0.0..=12.0)
                    .text(s.perception_radius),
            )
            .on_hover_text(s.perception_radius_hint);
            ui.add(egui::Slider::new(&mut config.sensor_angle, 0.0..=1.5).text(s.antenna_spread));
            ui.add(
                egui::Slider::new(&mut config.sensor_box_radius, 0..=3).text(s.antenna_box_radius),
            );
            ui.add(
                egui::Slider::new(&mut config.random_walk_strength, 0.0..=1.5).text(s.random_walk),
            );
            ui.add(
                egui::Slider::new(&mut config.sensor_turn_gain, 0.0..=1.5).text(s.trail_turn_gain),
            )
            .on_hover_text(s.trail_turn_gain_hint);
            ui.add(
                egui::Slider::new(&mut config.home_pheromone_weight, 0.0..=1.0)
                    .text(s.home_trail_guidance),
            )
            .on_hover_text(s.home_trail_guidance_hint);
            ui.add(egui::Slider::new(&mut config.individuality, 0.0..=1.5).text(s.individuality))
                .on_hover_text(s.individuality_hint);
        });
}

/// Crowding: the knob that stops a bottleneck collapsing the colony onto a
/// point, plus the live readout that says whether it is happening.
fn crowd_controls(ui: &mut egui::Ui, config: &mut SimConfig, stats: &SimStats, s: &Strings) {
    egui::CollapsingHeader::new(s.crowding_section)
        .default_open(false)
        .show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut config.crowd_threshold, 4.0..=120.0).text(s.jam_threshold),
            )
            .on_hover_text(s.jam_threshold_hint);
            ui.add(egui::Slider::new(&mut config.crowd_relief, 0.0..=2.0).text(s.relief_step))
                .on_hover_text(s.relief_step_hint);

            // Live concentration: if this climbs into the hundreds while food
            // is walled off, the colony is piling onto one spot.
            let uniform = if config.cell_count() > 0 {
                config.total_ants() as f32 * 16.0 / config.cell_count() as f32
            } else {
                0.0
            };
            ui.small(
                s.busiest_bin_fmt
                    .replace("{max}", &stats.crowd_max_bin.to_string())
                    .replace("{peak}", &stats.crowd_peak_bin.to_string())
                    .replace("{occupied}", &stats.crowd_occupied.to_string())
                    .replace("{uniform:.1}", &format!("{uniform:.1}")),
            );
        });
}

fn predator_controls(ui: &mut egui::Ui, config: &mut SimConfig, s: &Strings) {
    egui::CollapsingHeader::new(s.predators_section)
        .default_open(false)
        .show(ui, |ui| {
            let mut count = config.predator_count as u32;
            if ui
                .add(egui::Slider::new(&mut count, 0..=40).text(s.predators))
                .on_hover_text(s.predators_hint)
                .changed()
            {
                config.predator_count = count as usize;
            }
            ui.add_enabled_ui(config.predator_count > 0, |ui| {
                ui.add(egui::Slider::new(&mut config.predator_speed, 0.0..=4.0).text(s.speed));
                ui.add(
                    egui::Slider::new(&mut config.predator_sense, 0.0..=120.0).text(s.sight_radius),
                );
                ui.add(
                    egui::Slider::new(&mut config.predator_catch_radius, 0.5..=12.0)
                        .text(s.catch_radius),
                );
                ui.add(egui::Slider::new(&mut config.predator_wander, 0.0..=1.5).text(s.wander));
                ui.small(s.predator_note);
            });
        });
}

fn display_controls(ui: &mut egui::Ui, view: &mut PheromoneView, s: &Strings) {
    egui::CollapsingHeader::new(egui::RichText::new(s.display_section).strong())
        .default_open(false)
        .show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut view.food_gain, 0.02..=2.0)
                    .logarithmic(true)
                    .text(s.food_trail_gain),
            );
            ui.add(
                egui::Slider::new(&mut view.home_gain, 0.0..=1.0)
                    .logarithmic(true)
                    .text(s.home_trail_gain),
            )
            .on_hover_text(s.gain_hint);
            ui.horizontal(|ui| {
                // Toggle buttons, not checkboxes: pressed = shown.
                if ui
                    .add(
                        egui::Button::new(s.food_trail)
                            .small()
                            .selected(view.show_food),
                    )
                    .clicked()
                {
                    view.show_food = !view.show_food;
                }
                if ui
                    .add(
                        egui::Button::new(s.home_trail)
                            .small()
                            .selected(view.show_home),
                    )
                    .clicked()
                {
                    view.show_home = !view.show_home;
                }
            });
            ui.small(s.color_legend);
            ui.small("ants: white / orange    predators: red    walls: grey");
        });
}
