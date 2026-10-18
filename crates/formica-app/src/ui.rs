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

/// Which scenario action a pending native file dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    /// Pick a TOML file and load it.
    Import,
    /// Pick where to write the live set-up.
    ExportCurrent,
    /// Pick where to write the built-in shown in the dropdown.
    ExportPreset,
}

/// Reply channel of a native file dialog opened on a worker thread.
type DialogReply = std::sync::mpsc::Receiver<Option<std::path::PathBuf>>;

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
    // The preset applied last, shown as the pressed button and an "applied:"
    // line - the only feedback a parameter diff can give.
    mut active_preset: Local<Option<Preset>>,
    // A native file dialog opened from the scenario section, answered on a
    // worker thread and polled every frame until it settles.
    mut file_dialog: Local<Option<(DialogKind, DialogReply)>>,
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

                // Presets and scenarios live in one section on purpose: they
                // are two levels of the same idea (a named starting point),
                // and splitting them made them read as competing features.
                // The hint line says what each level covers.
                egui::CollapsingHeader::new(egui::RichText::new(s.presets_section).strong())
                    .default_open(true)
                    .show(ui, |ui| {
                        ui.small(s.presets_hint);
                        ui.add_space(2.0);
                        egui::Grid::new("presets").num_columns(1).show(ui, |ui| {
                            for preset in Preset::ALL {
                                // Pressed state marks the preset applied last,
                                // so a click has visible feedback even when the
                                // parameter diff is too subtle to see (two
                                // evaporation rates differ little on screen).
                                let applied = *active_preset == Some(preset);
                                if ui
                                    .add(
                                        egui::Button::new(preset.label(s))
                                            .small()
                                            .selected(applied),
                                    )
                                    .on_hover_text(preset.hint(s))
                                    .clicked()
                                {
                                    preset.apply(&mut config);
                                    // A preset that changes the colonies or the
                                    // walls changes the world's shape, so give
                                    // the renderer a clean slate rather than
                                    // leaving half of it stale.
                                    *draft_size = Some((config.width, config.height));
                                    *active_preset = Some(preset);
                                }
                                ui.end_row();
                            }
                        });
                        if let Some(preset) = *active_preset {
                            ui.small(s.preset_active_fmt.replace("{name}", preset.label(s)));
                        }
                        ui.separator();
                        scenario_controls(
                            ui,
                            &config,
                            &mut commands,
                            &food,
                            &obstacles,
                            s,
                            &mut scenario_selection,
                            &mut export_status,
                            &mut file_dialog,
                        );
                    });
                ui.separator();
                world_controls(ui, &mut config, &mut commands, &stats, &mut draft_size, s);
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
                ant_controls(ui, &mut config, &mut commands, s);
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

/// A small square button showing a font glyph with a hover tooltip.
///
/// The glyphs come from egui's built-in icon fallback fonts (NotoEmoji and
/// emoji-icon-font); every codepoint used here was verified against their
/// cmap tables, so no button can ever render as a tofu box. `enabled =
/// false` greys the button out, the same look the rest of the panel uses.
fn icon_button(ui: &mut egui::Ui, glyph: &str, tip: String, enabled: bool) -> egui::Response {
    ui.scope(|ui| {
        if !enabled {
            ui.disable();
        }
        ui.add_sized(
            [30.0, 22.0],
            egui::Button::new(egui::RichText::new(glyph).size(14.0)),
        )
    })
    .inner
    .on_hover_text(tip)
}

/// The panel's slider control, in the house layout: the **name on its own
/// line**, the slider (with its live value drawn in the rail) beneath it.
///
/// egui's default puts the name to the right of the rail, which reads
/// "rail - value - name" and leaves every rail a different width; a label
/// above keeps the names scannable in one column and the rails full-width.
/// `hint` may be empty, in which case no tooltip is attached.
fn labelled_slider<'a>(
    ui: &mut egui::Ui,
    name: &str,
    hint: &str,
    slider: egui::Slider<'a>,
) -> egui::Response {
    ui.label(name);
    let response = ui.add(slider);
    if hint.is_empty() {
        response
    } else {
        response.on_hover_text(hint)
    }
}

fn run_controls(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    commands: &mut SimCommands,
    s: &Strings,
) {
    ui.label(egui::RichText::new(s.run_section).strong());
    ui.horizontal(|ui| {
        // Icon buttons, media-player style: the glyph shows the action the
        // click will perform, the tooltip spells it out.
        let (pause_glyph, pause_tip, pause_hint) = if config.paused {
            ("\u{25B6}", s.resume, s.resume_hint)
        } else {
            ("\u{23F8}", s.pause, s.pause_hint)
        };
        if icon_button(
            ui,
            pause_glyph,
            format!("{} - {}", pause_tip, pause_hint),
            true,
        )
        .clicked()
        {
            config.paused = !config.paused;
        }
        if icon_button(
            ui,
            "\u{21BA}",
            format!("{} - {}", s.restart, s.restart_hint),
            true,
        )
        .clicked()
        {
            // "Restart" is really a reset: the same world is rebuilt from the
            // current config, nothing is remembered.
            commands.restart = true;
        }
        if icon_button(
            ui,
            "\u{1F5D1}",
            format!("{} - {}", s.clear_trails, s.clear_trails_hint),
            true,
        )
        .clicked()
        {
            commands.clear_pheromones = true;
        }
    });
    labelled_slider(
        ui,
        s.ticks_per_frame,
        s.ticks_per_frame_hint,
        egui::Slider::new(&mut config.ticks_per_frame, 0..=32),
    );
}

fn world_controls(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    commands: &mut SimCommands,
    stats: &SimStats,
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
            if labelled_slider(
                ui,
                s.starting_piles,
                s.starting_piles_hint,
                egui::Slider::new(&mut piles, 1..=8),
            )
            .changed()
            {
                config.food_pile_count = piles as usize;
            }
            labelled_slider(
                ui,
                s.food_per_pile,
                "",
                egui::Slider::new(&mut config.food_per_pile, 0.0..=20_000.0),
            );
            labelled_slider(
                ui,
                s.pile_radius,
                "",
                egui::Slider::new(&mut config.food_pile_radius, 2.0..=24.0),
            );
            // Live recovery fraction, not a constant: the same line that says
            // how much food the map starts with says how much has come home.
            ui.label(
                egui::RichText::new(format!(
                    "{:.0} {} ({:.1}% {})",
                    config.food_per_pile * config.food_pile_count as f32,
                    s.units_on_map,
                    stats.recovery_fraction() * 100.0,
                    s.delivered
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
///
/// File picking uses the OS dialog (rfd) instead of a typed path box: the
/// dialog opens on a worker thread and replies over a channel that this
/// function polls each frame, so the window keeps rendering while the dialog
/// is up and the panel never blocks on COM's modal message pump.
fn scenario_controls(
    ui: &mut egui::Ui,
    config: &SimConfig,
    commands: &mut SimCommands,
    food: &FoodField,
    obstacles: &Obstacles,
    s: &Strings,
    selected: &mut Option<String>,
    export_status: &mut Option<String>,
    dialog: &mut Option<(DialogKind, DialogReply)>,
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

    // Settle a pending dialog: take the picked path (or a cancellation), run
    // the action it was opened for, and free the slot so the buttons re-arm.
    let mut settled = None;
    if let Some((kind, rx)) = dialog.as_ref() {
        if let Ok(result) = rx.try_recv() {
            settled = Some((*kind, result));
        }
    }
    if let Some((kind, result)) = settled {
        *dialog = None;
        if let Some(path) = result {
            let path = ensure_toml_extension(path);
            match kind {
                DialogKind::Import => import_scenario(commands, path, s, export_status),
                DialogKind::ExportCurrent => {
                    export_current_setup(config, food, obstacles, path, s, export_status);
                }
                DialogKind::ExportPreset => {
                    export_selected_preset(selected, path, s, export_status);
                }
            }
        }
    }

    // One dialog at a time: the buttons grey out while a dialog is pending,
    // matching the disabled look the rest of the panel uses.
    let busy = dialog.is_some();
    // Icon buttons, one row: open (import), save (export the live set-up),
    // share (export the selected built-in). The tooltips keep the labels and
    // the "…" that marks them as opening a dialog.
    ui.horizontal(|ui| {
        if icon_button(
            ui,
            "\u{1F4C2}",
            format!("{} - {}", s.scenario_import, s.scenario_import_hint),
            !busy,
        )
        .clicked()
        {
            spawn_file_dialog(dialog, DialogKind::Import, false, String::new());
        }
        if icon_button(
            ui,
            "\u{1F4BE}",
            format!("{} - {}", s.scenario_export, s.scenario_export_hint),
            !busy,
        )
        .clicked()
        {
            spawn_file_dialog(
                dialog,
                DialogKind::ExportCurrent,
                true,
                "formica-scenario.toml".to_string(),
            );
        }
        if icon_button(
            ui,
            "\u{1F4E4}",
            format!(
                "{} - {}",
                s.scenario_export_preset, s.scenario_export_preset_hint
            ),
            !busy,
        )
        .clicked()
        {
            let name = selected.as_deref().unwrap_or("scenario").to_string();
            spawn_file_dialog(
                dialog,
                DialogKind::ExportPreset,
                true,
                format!("{name}.toml"),
            );
        }
    });
    ui.add_space(2.0);
    if let Some(status) = export_status {
        ui.small(status.clone());
    }
}

/// Open a native file dialog on a worker thread and remember how to receive
/// its answer. `save` picks the save variant (with a default file name),
/// otherwise an open dialog is shown. The thread ends when the dialog closes;
/// a cancelled dialog replies `None`.
fn spawn_file_dialog(
    slot: &mut Option<(DialogKind, DialogReply)>,
    kind: DialogKind,
    save: bool,
    default_name: String,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let dialog = rfd::FileDialog::new().add_filter("TOML", &["toml"]);
        let picked = if save {
            dialog.set_file_name(&default_name).save_file()
        } else {
            dialog.pick_file()
        };
        let _ = tx.send(picked);
    });
    *slot = Some((kind, rx));
}

/// Write the live config, pile layout and wall shape to a TOML file.
///
/// Hand-painted walls ride along: `capture_from_grid` keys on
/// `Obstacles::painted` and dumps the brush's shape as explicit rectangles
/// when the map has been hand-edited, so a saved classic map contains
/// exactly what was on screen.
fn export_current_setup(
    config: &SimConfig,
    food: &FoodField,
    obstacles: &Obstacles,
    picked: std::path::PathBuf,
    s: &Strings,
    status: &mut Option<String>,
) {
    let path = ensure_toml_extension(picked);
    // Exports land in their own folder rather than the repo root, where the
    // binary happens to run from.
    if let Some(directory) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(directory);
    }
    let scenario = Scenario::capture_from_grid("exported", "", config, &food.piles, obstacles);
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

/// Write the built-in currently shown in the dropdown to the picked file.
///
/// This serialises the preset itself, so its hand-written walls and food
/// spots survive the round trip byte for byte - that is the "share this
/// exact map" path.
fn export_selected_preset(
    selected: &Option<String>,
    picked: std::path::PathBuf,
    s: &Strings,
    status: &mut Option<String>,
) {
    let name = selected.as_deref().unwrap_or("");
    match Scenario::builtin(name) {
        Some(preset) => {
            let path = ensure_toml_extension(picked);
            if let Some(directory) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                let _ = std::fs::create_dir_all(directory);
            }
            *status = Some(match std::fs::write(&path, preset.to_toml_string()) {
                Ok(()) => s
                    .scenario_preset_exported_fmt
                    .replace("{name}", preset.name.as_str())
                    .replace("{path}", &path.display().to_string()),
                Err(err) => s
                    .scenario_failed_fmt
                    .replace("{path}", &path.display().to_string())
                    .replace("{err}", &err.to_string()),
            });
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

/// Read a scenario TOML from the picked path and queue a world rebuild.
fn import_scenario(
    commands: &mut SimCommands,
    path: std::path::PathBuf,
    s: &Strings,
    status: &mut Option<String>,
) {
    let attempt = std::fs::read_to_string(&path)
        .map_err(|err| err.to_string())
        .and_then(|text| Scenario::parse_toml(&text).map(|scenario| (path.clone(), scenario)));
    match attempt {
        Ok((path, scenario)) => {
            *status = Some(
                s.scenario_imported_fmt
                    .replace("{path}", &path.display().to_string())
                    .replace("{name}", &scenario.name),
            );
            commands.load_scenario = Some(scenario);
        }
        Err(err) => {
            *status = Some(
                s.scenario_import_failed_fmt
                    .replace("{path}", &path.display().to_string())
                    .replace("{err}", &err),
            );
        }
    }
}

/// Force the picked path to end in `.toml`, so an export can be read back by
/// the import dialog and by the CLI's `--scenario` (both resolve files by
/// that extension before falling back to built-in names). The native save
/// dialog suggests the name but lets the user type anything.
fn ensure_toml_extension(mut path: std::path::PathBuf) -> std::path::PathBuf {
    let has_toml = path
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case("toml"))
        .unwrap_or(false);
    if !has_toml {
        path.set_extension("toml");
    }
    path
}

fn interact_controls(ui: &mut egui::Ui, interaction: &mut InteractionState, s: &Strings) {
    egui::CollapsingHeader::new(s.mouse_section)
        .default_open(true)
        .show(ui, |ui| {
            // The master switch gates everything below: while map editing is
            // off, the mode buttons and the brush sliders grey out, and the
            // pointer handler in `interact.rs` ignores the map entirely.
            ui.checkbox(&mut interaction.enabled, s.enable_map_editing);
            ui.add_enabled_ui(interaction.enabled, |ui| {
                // Icon mode buttons, pressed = active mode; the tooltip names
                // the mode in the panel language.
                ui.horizontal(|ui| {
                    for mode in PaintMode::ALL {
                        let active = interaction.mode == mode;
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(mode.glyph()).size(14.0))
                                    .small()
                                    .selected(active),
                            )
                            .on_hover_text(mode.label_for(s))
                            .clicked()
                        {
                            interaction.mode = mode;
                        }
                    }
                });
                labelled_slider(
                    ui,
                    s.brush_radius,
                    "",
                    egui::Slider::new(&mut interaction.brush_radius, 2.0..=24.0),
                );
                if interaction.mode == PaintMode::Food {
                    labelled_slider(
                        ui,
                        s.food_per_pile,
                        "",
                        egui::Slider::new(&mut interaction.pile_amount, 100.0..=10_000.0),
                    );
                }
                ui.small(s.mouse_help);
            });
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
                ui.label(s.obstacle_kind);
                let is_none = matches!(config.obstacle_preset, ObstaclePreset::None);
                let is_wall = matches!(config.obstacle_preset, ObstaclePreset::UWall { .. });
                let is_blocks =
                    matches!(config.obstacle_preset, ObstaclePreset::RandomBlocks { .. });
                let selected_text = if is_wall {
                    s.obstacle_u_wall
                } else if is_blocks {
                    s.obstacle_blocks
                } else {
                    s.obstacle_none
                };
                egui::ComboBox::from_id_salt("formica_obstacle_preset")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        // None / u-wall carry hints; the blocks entry has none.
                        if ui
                            .selectable_label(is_none, s.obstacle_none)
                            .on_hover_text(s.obstacle_none_hint)
                            .clicked()
                            && !is_none
                        {
                            config.obstacle_preset = ObstaclePreset::None;
                        }
                        // Size the U to the pile it wraps, so the preset works
                        // on any map.
                        if ui
                            .selectable_label(is_wall, s.obstacle_u_wall)
                            .on_hover_text(s.obstacle_u_wall_hint)
                            .clicked()
                            && !is_wall
                        {
                            config.obstacle_preset = ObstaclePreset::UWall {
                                arm: config.food_pile_radius + 8.0,
                                gap: config.food_pile_radius * 2.0 + 4.0,
                                thickness: 3.0,
                            };
                        }
                        if ui.selectable_label(is_blocks, s.obstacle_blocks).clicked() && !is_blocks
                        {
                            config.obstacle_preset = ObstaclePreset::RandomBlocks { density: 0.15 };
                        }
                    });
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
                    labelled_slider(ui, s.arm_length, "", egui::Slider::new(arm, 4.0..=40.0));
                    labelled_slider(ui, s.clear_width, "", egui::Slider::new(gap, 4.0..=40.0));
                    labelled_slider(ui, s.thickness, "", egui::Slider::new(thickness, 1.0..=8.0));
                    ui.small(s.obstacle_u_wall_hint);
                }
                ObstaclePreset::RandomBlocks { density } => {
                    labelled_slider(
                        ui,
                        s.wall_density,
                        "",
                        egui::Slider::new(density, 0.01..=0.4),
                    );
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
                let selected_text = modes
                    .iter()
                    .find(|(mode, ..)| *mode == config.homing_mode)
                    .map(|(_, option, _)| *option)
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("formica_homing_mode")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        for (mode, option, hint) in modes {
                            let is_selected = config.homing_mode == mode;
                            if ui
                                .selectable_label(is_selected, option)
                                .on_hover_text(hint)
                                .clicked()
                            {
                                config.homing_mode = mode;
                            }
                        }
                    });
            });
        });
}

fn colony_controls(ui: &mut egui::Ui, config: &mut SimConfig, s: &Strings) {
    egui::CollapsingHeader::new(s.colonies_section)
        .default_open(false)
        .show(ui, |ui| {
            let mut colonies = config.colony_count as u32;
            if labelled_slider(
                ui,
                s.competing_colonies,
                s.competing_colonies_hint,
                egui::Slider::new(&mut colonies, 1..=2),
            )
            .changed()
            {
                config.colony_count = colonies as usize;
            }
            if config.colonies() > 1 {
                labelled_slider(
                    ui,
                    s.nest_spacing,
                    "",
                    egui::Slider::new(&mut config.nest_orbit, 20.0..=140.0),
                );
                ui.small(s.colony_colors_note);
            }
        });
}

fn pheromone_controls(ui: &mut egui::Ui, config: &mut SimConfig, s: &Strings) {
    egui::CollapsingHeader::new(egui::RichText::new(s.pheromones_section).strong())
        .default_open(false)
        .show(ui, |ui| {
            labelled_slider(
                ui,
                s.evaporation,
                s.evaporation_hint,
                egui::Slider::new(&mut config.evaporation_rate, 0.001..=0.5).logarithmic(true),
            );
            labelled_slider(
                ui,
                s.deposit_strength,
                "",
                egui::Slider::new(&mut config.deposit_amount, 0.0..=5.0),
            );
            labelled_slider(
                ui,
                s.deposit_kernel_radius,
                "",
                egui::Slider::new(&mut config.deposit_radius, 0..=3),
            );
            labelled_slider(
                ui,
                s.cell_ceiling,
                "",
                egui::Slider::new(&mut config.max_pheromone, 1.0..=100.0),
            );
        });
}

/// Ant behaviour, plus the "exit distribution" sub-section: how ants leave
/// the nest when they spawn. Exit changes ride `SimCommands::restart` (a
/// slider drag fires it once, on release) so the whole colony re-emerges with
/// the new settings - the point of the section is the *initial* state, and
/// letting future spawns silently differ would just confuse.
fn ant_controls(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    commands: &mut SimCommands,
    s: &Strings,
) {
    egui::CollapsingHeader::new(egui::RichText::new(s.ants_section).strong())
        .default_open(false)
        .show(ui, |ui| {
            let mut ant_count = config.ant_count as u32;
            if labelled_slider(
                ui,
                s.ants_per_colony_slider,
                s.ants_per_colony_hint,
                egui::Slider::new(&mut ant_count, 1..=2_000),
            )
            .changed()
            {
                config.ant_count = ant_count as usize;
            }

            labelled_slider(
                ui,
                s.speed,
                "",
                egui::Slider::new(&mut config.speed, 0.1..=6.0),
            );
            labelled_slider(
                ui,
                s.perception_radius,
                s.perception_radius_hint,
                egui::Slider::new(&mut config.sensor_distance, 0.0..=12.0),
            );
            labelled_slider(
                ui,
                s.antenna_spread,
                "",
                egui::Slider::new(&mut config.sensor_angle, 0.0..=1.5),
            );
            labelled_slider(
                ui,
                s.antenna_box_radius,
                "",
                egui::Slider::new(&mut config.sensor_box_radius, 0..=3),
            );
            labelled_slider(
                ui,
                s.random_walk,
                "",
                egui::Slider::new(&mut config.random_walk_strength, 0.0..=1.5),
            );
            labelled_slider(
                ui,
                s.trail_turn_gain,
                s.trail_turn_gain_hint,
                egui::Slider::new(&mut config.sensor_turn_gain, 0.0..=1.5),
            );
            labelled_slider(
                ui,
                s.home_trail_guidance,
                s.home_trail_guidance_hint,
                egui::Slider::new(&mut config.home_pheromone_weight, 0.0..=1.0),
            );
            labelled_slider(
                ui,
                s.individuality,
                s.individuality_hint,
                egui::Slider::new(&mut config.individuality, 0.0..=1.5),
            );

            // The exit distribution trails the section behind a separator,
            // presets-and-scenarios style: everything above tunes the
            // ongoing behaviour, the block below is the initial state.
            ui.separator();
            exit_controls(ui, config, commands, s);
        });
}

/// The exit-distribution controls: mode picker, jet geometry, and the
/// batched-release rate. Every edit rebuilds the world on the spot, so what
/// the user sees is always the configured initial state - see the doc comment
/// on [`ant_controls`].
fn exit_controls(
    ui: &mut egui::Ui,
    config: &mut SimConfig,
    commands: &mut SimCommands,
    s: &Strings,
) {
    egui::CollapsingHeader::new(egui::RichText::new(s.exit_section).strong())
        .default_open(false)
        .show(ui, |ui| {
            ui.small(s.exit_hint);

            ui.horizontal_wrapped(|ui| {
                ui.label(s.exit_mode);
                let modes = [
                    (SpawnMode::Disk, s.exit_disk, s.exit_disk_hint),
                    (SpawnMode::Jet, s.exit_jet, s.exit_jet_hint),
                ];
                let selected = config.spawn_mode;
                let selected_text = modes
                    .iter()
                    .find(|(mode, ..)| *mode == selected)
                    .map(|(_, option, _)| *option)
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("formica_exit_mode")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        for (mode, option, hint) in modes {
                            let is_selected = config.spawn_mode == mode;
                            if ui
                                .selectable_label(is_selected, option)
                                .on_hover_text(hint)
                                .clicked()
                            {
                                if !is_selected {
                                    config.spawn_mode = mode;
                                    commands.restart = true;
                                }
                            }
                        }
                    });
            });

            if config.spawn_mode == SpawnMode::Jet {
                let direction = labelled_slider(
                    ui,
                    s.exit_direction,
                    s.exit_direction_hint,
                    egui::Slider::new(&mut config.spawn_direction, 0.0..=360.0),
                );
                let spread = labelled_slider(
                    ui,
                    s.exit_spread,
                    s.exit_spread_hint,
                    egui::Slider::new(&mut config.spawn_spread, 0.0..=360.0),
                );
                // Fires once per drag, on release - not on every pixel.
                if direction.drag_stopped() || spread.drag_stopped() {
                    commands.restart = true;
                }
            }

            let mut rate = config.spawn_rate as u32;
            let rate_slider = labelled_slider(
                ui,
                s.exit_rate,
                s.exit_rate_hint,
                egui::Slider::new(&mut rate, 0..=50),
            );
            if rate_slider.changed() {
                config.spawn_rate = rate as usize;
            }
            if rate_slider.drag_stopped() {
                commands.restart = true;
            }
            // Only meaningful while trickling; hidden otherwise so the 0
            // state stays unambiguous ("everything leaves at once").
            if config.spawn_rate > 0 {
                let max = config.ant_count.max(1) as u32;
                let mut initial = config.spawn_initial.min(config.ant_count) as u32;
                let initial_slider = labelled_slider(
                    ui,
                    s.exit_initial,
                    s.exit_initial_hint,
                    egui::Slider::new(&mut initial, 0..=max),
                );
                if initial_slider.changed() {
                    config.spawn_initial = initial as usize;
                }
                if initial_slider.drag_stopped() {
                    commands.restart = true;
                }
            }
        });
}

/// Crowding: the knob that stops a bottleneck collapsing the colony onto a
/// point, plus the live readout that says whether it is happening.
fn crowd_controls(ui: &mut egui::Ui, config: &mut SimConfig, stats: &SimStats, s: &Strings) {
    egui::CollapsingHeader::new(s.crowding_section)
        .default_open(false)
        .show(ui, |ui| {
            labelled_slider(
                ui,
                s.jam_threshold,
                s.jam_threshold_hint,
                egui::Slider::new(&mut config.crowd_threshold, 4.0..=120.0),
            );
            labelled_slider(
                ui,
                s.relief_step,
                s.relief_step_hint,
                egui::Slider::new(&mut config.crowd_relief, 0.0..=2.0),
            );

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
            if labelled_slider(
                ui,
                s.predators,
                s.predators_hint,
                egui::Slider::new(&mut count, 0..=40),
            )
            .changed()
            {
                config.predator_count = count as usize;
            }
            ui.add_enabled_ui(config.predator_count > 0, |ui| {
                labelled_slider(
                    ui,
                    s.speed,
                    "",
                    egui::Slider::new(&mut config.predator_speed, 0.0..=4.0),
                );
                labelled_slider(
                    ui,
                    s.sight_radius,
                    "",
                    egui::Slider::new(&mut config.predator_sense, 0.0..=120.0),
                );
                labelled_slider(
                    ui,
                    s.catch_radius,
                    "",
                    egui::Slider::new(&mut config.predator_catch_radius, 0.5..=12.0),
                );
                labelled_slider(
                    ui,
                    s.wander,
                    "",
                    egui::Slider::new(&mut config.predator_wander, 0.0..=1.5),
                );
                ui.small(s.predator_note);
            });
        });
}

fn display_controls(ui: &mut egui::Ui, view: &mut PheromoneView, s: &Strings) {
    egui::CollapsingHeader::new(egui::RichText::new(s.display_section).strong())
        .default_open(false)
        .show(ui, |ui| {
            labelled_slider(
                ui,
                s.food_trail_gain,
                "",
                egui::Slider::new(&mut view.food_gain, 0.02..=2.0).logarithmic(true),
            );
            labelled_slider(
                ui,
                s.home_trail_gain,
                s.gain_hint,
                egui::Slider::new(&mut view.home_gain, 0.0..=1.0).logarithmic(true),
            );
            ui.horizontal(|ui| {
                // Icon toggle buttons, pressed = shown. 🍎 is the food trail,
                // 🏠 the trail home (both glyphs verified in the icon fonts).
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("\u{1F34E}").size(14.0))
                            .small()
                            .selected(view.show_food),
                    )
                    .on_hover_text(s.food_trail)
                    .clicked()
                {
                    view.show_food = !view.show_food;
                }
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("\u{1F3E0}").size(14.0))
                            .small()
                            .selected(view.show_home),
                    )
                    .on_hover_text(s.home_trail)
                    .clicked()
                {
                    view.show_home = !view.show_home;
                }
            });
            ui.small(s.color_legend);
            ui.small(s.legend_actors);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The save dialog suggests a name but lets the user type anything, so
    /// both the extensionless and the wrong-extension case must land on a
    /// path the CLI's `--scenario` resolves.
    #[test]
    fn picked_paths_always_end_in_toml() {
        assert_eq!(
            ensure_toml_extension(std::path::PathBuf::from("maps/fortress")),
            std::path::PathBuf::from("maps/fortress.toml")
        );
        assert_eq!(
            ensure_toml_extension(std::path::PathBuf::from("maps/fortress.toml")),
            std::path::PathBuf::from("maps/fortress.toml")
        );
        // Case-insensitive: an upper-case extension is still a .toml.
        assert_eq!(
            ensure_toml_extension(std::path::PathBuf::from("maps/fortress.TOML")),
            std::path::PathBuf::from("maps/fortress.TOML")
        );
    }
}
