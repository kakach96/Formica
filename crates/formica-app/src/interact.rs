//! Mouse interaction: drop food where you click, draw walls where you drag.
//!
//! This is the only place the renderer *writes* to the simulation, and it does
//! so through the ordinary data structures ([`FoodField::add_pile`],
//! [`Obstacles::set`]) rather than through any special case. The simulation
//! cannot tell the difference between a pile the player dropped and one the
//! generator scattered, which is exactly the point: no separate "scripted food"
//! path exists to drift out of sync with the real one.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use formica_core::obstacles::Obstacles;
use formica_core::prelude::*;

use crate::render::{clamp_pan, clamp_zoom, zoom_anchor_pan, SceneGeometry};

/// What a left-click does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaintMode {
    /// Drop a food pile.
    #[default]
    Food,
    /// Draw wall.
    Wall,
    /// Remove food and wall under the cursor.
    Erase,
}

impl PaintMode {
    /// Every mode, in UI order.
    pub const ALL: [PaintMode; 3] = [PaintMode::Food, PaintMode::Wall, PaintMode::Erase];

    /// Short label for the UI (English).
    ///
    /// Kept alongside [`PaintMode::label_for`] because the metrics-style
    /// English-only call sites (tests, logs) still use it.
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            PaintMode::Food => "place food",
            PaintMode::Wall => "draw wall",
            PaintMode::Erase => "erase",
        }
    }

    /// [`PaintMode::label`] in the language of the given string table.
    pub fn label_for(self, s: &formica_core::i18n::Strings) -> &'static str {
        match self {
            PaintMode::Food => s.mode_place_food,
            PaintMode::Wall => s.mode_draw_wall,
            PaintMode::Erase => s.mode_erase,
        }
    }

    /// Panel button glyph, from egui's built-in icon fallback fonts (both
    /// codepoints verified in their cmap tables). The brush's mode buttons
    /// are icons with this glyph; the tooltip carries the [`Self::label_for`]
    /// name.
    pub fn glyph(self) -> &'static str {
        match self {
            // An apple for "drop food".
            PaintMode::Food => "\u{1F34E}",
            // A filled square for "wall block".
            PaintMode::Wall => "\u{25A0}",
            // Trash for "take it back".
            PaintMode::Erase => "\u{1F5D1}",
        }
    }
}

/// What the mouse is currently allowed to do.
#[derive(Resource, Debug, Clone)]
pub struct InteractionState {
    /// Master switch, so a stray click cannot rewrite the map mid-demo.
    pub enabled: bool,
    /// What a left-click does.
    pub mode: PaintMode,
    /// Brush radius in cells, for both food piles and walls.
    pub brush_radius: f32,
    /// Food units a newly dropped pile contains.
    pub pile_amount: f32,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: PaintMode::Food,
            brush_radius: 6.0,
            pile_amount: 2_000.0,
        }
    }
}

/// Set by the panel each frame: is the pointer over an egui widget?
///
/// Without this, clicking a slider on the far side of the window would also
/// drop a food pile on the map underneath it.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct UiFocus {
    /// True when egui wants the pointer.
    pub pointer_over_ui: bool,
}

/// Installs pointer handling.
pub struct FormicaInteractPlugin;

impl Plugin for FormicaInteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InteractionState>()
            .init_resource::<UiFocus>()
            .add_systems(Update, (handle_view_input, handle_pointer));
    }
}

/// Is the cursor over the egui panel's strip?
///
/// `egui_wants_pointer_input` only fires over *interactive* widgets - over
/// plain panel text or empty panel space it is false, so it cannot be the
/// only authority for "this click/wheel belongs to the panel". While the
/// pointer is inside the panel's strip the map must not react: the wheel
/// scrolls the panel (and zooming there yanks the map out from under a
/// widget the user is not even hovering), and a click would paint a pile
/// hidden underneath the panel. Zero width (first frame) owns nothing.
fn over_panel(cursor_x: f32, panel_width: f32) -> bool {
    panel_width > 0.0 && cursor_x < panel_width
}

/// Wheel zoom and drag pan for the map view.
///
/// The camera view (`CameraView`) is presentation state, so the simulation is
/// untouched; zoom rides `OrthographicProjection::scale` and pan rides the
/// camera transform, both applied by `center_camera`. Painting keys off
/// `viewport_to_world_2d`, which honours both automatically - a zoomed map is
/// still painted cell-accurate with no extra work.
///
/// Buttons: wheel zooms anchored at the cursor; middle-drag pans; while map
/// editing is off, left-drag pans too (left is otherwise the paint button).
/// Anything over an egui widget is ignored, and a drag that crosses onto the
/// panel is dropped rather than stealing the map around behind it.
fn handle_view_input(
    mut scroll: MessageReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    geometry: Res<SceneGeometry>,
    panel: Res<crate::render::PanelMetrics>,
    focus: Res<UiFocus>,
    state: Res<InteractionState>,
    mut view: ResMut<crate::render::CameraView>,
    mut drag_anchor: Local<Option<Vec2>>,
) {
    if focus.pointer_over_ui {
        *drag_anchor = None;
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        *drag_anchor = None;
        return;
    };
    // The panel owns its strip outright (see `over_panel`).
    if over_panel(cursor.x, panel.width) {
        *drag_anchor = None;
        return;
    }
    let (fw, fh) = (geometry.width as f32, geometry.height as f32);
    if fw <= 0.0 || fh <= 0.0 {
        return;
    }
    // World units per screen pixel at the current zoom: the same AutoMin
    // scale `center_camera` computes, divided by the zoom factor.
    let base = (window.width() / (fw + 10.0))
        .min(window.height() / (fh + 10.0))
        .max(f32::EPSILON);
    let world_per_px = 1.0 / (base * view.zoom);

    // --- zoom: wheel, anchored at the cursor ---------------------------------
    let wheel: f32 = scroll.read().map(|event| event.y).sum();
    if wheel != 0.0 {
        let old = clamp_zoom(view.zoom);
        // A notch is ±1..3 lines on most wheels; 0.15 per line ≈ two notches
        // from fit to 1.5x - deliberate, gentle steps.
        let new = clamp_zoom(old * (1.0 + 0.15 * wheel.clamp(-3.0, 3.0)));
        if new != old {
            // The cursor's world offset from the camera centre, measured
            // *before* the zoom (screen y points down, world y up).
            let center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
            let cursor_offset = Vec2::new(cursor.x - center.x, center.y - cursor.y) * world_per_px;
            view.pan = clamp_pan(
                zoom_anchor_pan(view.pan, cursor_offset, old, new),
                fw,
                fh,
                new,
            );
            view.zoom = new;
        }
    }

    // --- pan: middle-drag, or left-drag while editing is off -----------------
    let panning = buttons.pressed(MouseButton::Middle)
        || (!state.enabled && buttons.pressed(MouseButton::Left));
    if panning {
        // First frame of the drag: remember where it started, move nothing.
        let anchor = drag_anchor.get_or_insert(cursor);
        // Screen delta (x right, y down) → world delta (y up): dragging the
        // map with the cursor means the camera moves against the cursor.
        let delta = cursor - *anchor;
        view.pan = clamp_pan(
            view.pan - Vec2::new(delta.x, -delta.y) * world_per_px,
            fw,
            fh,
            view.zoom,
        );
        *anchor = cursor;
    } else {
        *drag_anchor = None;
    }
}

fn handle_pointer(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
    panel: Res<crate::render::PanelMetrics>,
    focus: Res<UiFocus>,
    state: Res<InteractionState>,
    mut obstacles: ResMut<Obstacles>,
    mut food: ResMut<FoodField>,
) {
    if !state.enabled || focus.pointer_over_ui {
        return;
    }
    // Nothing to do on a frame with no button down.
    let left = buttons.pressed(MouseButton::Left);
    let right = buttons.pressed(MouseButton::Right);
    if !left && !right {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    // The panel owns its strip outright (see `over_panel`).
    if over_panel(cursor.x, panel.width) {
        return;
    }
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    // Screen -> world. Fails only if the camera is not a 2D orthographic one.
    let Ok(point) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };

    // Right-drag is always "take it back", whatever the mode.
    let erasing = right || (left && state.mode == PaintMode::Erase);
    if erasing {
        stamp(&mut obstacles, point, state.brush_radius, false);
        // Piles are removed on the click, not continuously: holding the button
        // down over a pile should not repeatedly re-remove it.
        if buttons.just_pressed(MouseButton::Right) || buttons.just_pressed(MouseButton::Left) {
            food.remove_pile_near(point.x, point.y, state.brush_radius + 6.0);
        }
        return;
    }

    match state.mode {
        PaintMode::Food => {
            // One pile per click. Piling up dozens while the button is held
            // would make the food supply an accident of frame rate.
            if buttons.just_pressed(MouseButton::Left) && !obstacles.is_blocked_at(point.x, point.y)
            {
                food.add_pile(FoodPile {
                    x: point.x,
                    y: point.y,
                    radius: state.brush_radius,
                    initial: state.pile_amount,
                });
            }
        }
        PaintMode::Wall => {
            // Walls are a brush: dragging paints a stroke.
            stamp(&mut obstacles, point, state.brush_radius, true);
        }
        PaintMode::Erase => unreachable!("erase is handled above"),
    }
}

/// Set or clear every cell whose centre lies inside the brush disc.
fn stamp(obstacles: &mut Obstacles, center: Vec2, radius: f32, blocked: bool) {
    let r = radius.max(1.0);
    let (x0, y0) = ((center.x - r).floor() as i32, (center.y - r).floor() as i32);
    let (x1, y1) = ((center.x + r).ceil() as i32, (center.y + r).ceil() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f32 + 0.5 - center.x;
            let dy = y as f32 + 0.5 - center.y;
            if dx * dx + dy * dy <= r * r {
                // The brush path, not the rebuild path: painting must mark
                // the map hand-edited so a later export dumps the drawn
                // shape instead of trusting the preset keyword.
                obstacles.paint(x, y, blocked);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Clicks and wheel over the panel strip belong to the panel: egui's own
    /// "wants pointer" test does not fire over plain panel text.
    #[test]
    fn the_panel_strip_claims_the_pointer() {
        assert!(over_panel(0.0, 330.0), "the panel's left edge");
        assert!(over_panel(329.9, 330.0), "just inside the panel");
        assert!(!over_panel(330.0, 330.0), "the boundary is the map's");
        assert!(!over_panel(800.0, 330.0));
        // Before the first frame the panel width is unknown: own nothing,
        // or the whole window would be inert.
        assert!(!over_panel(0.0, 0.0));
        assert!(!over_panel(10.0, 0.0));
    }

    #[test]
    fn a_brush_stamps_a_disc_of_the_requested_radius() {
        let mut obstacles = Obstacles::new(32, 32);
        stamp(&mut obstacles, Vec2::new(16.0, 16.0), 4.0, true);

        // The centre is set...
        assert!(obstacles.is_blocked(16, 16));
        // ...cells just inside the radius are set...
        assert!(obstacles.is_blocked(16, 19));
        // ...and cells outside it are not.
        assert!(!obstacles.is_blocked(16, 22));
        assert!(!obstacles.is_blocked(16, 10));
        // The disc is roughly circular, not square.
        assert!(!obstacles.is_blocked(19, 19));
        assert!(obstacles.is_blocked(18, 18) || obstacles.is_blocked(17, 17));
    }

    #[test]
    fn a_zero_radius_brush_still_stamps_one_cell() {
        let mut obstacles = Obstacles::new(32, 32);
        stamp(&mut obstacles, Vec2::new(5.5, 5.5), 0.0, true);
        assert!(obstacles.count() >= 1);
        assert!(obstacles.is_blocked(5, 5));
    }

    #[test]
    fn erasing_clears_what_drawing_set() {
        let mut obstacles = Obstacles::new(32, 32);
        stamp(&mut obstacles, Vec2::new(16.0, 16.0), 3.0, true);
        assert!(obstacles.count() > 0);
        stamp(&mut obstacles, Vec2::new(16.0, 16.0), 3.0, false);
        assert_eq!(obstacles.count(), 0, "the erase brush left wall behind");
    }

    #[test]
    fn stamping_outside_the_map_is_safe() {
        let mut obstacles = Obstacles::new(16, 16);
        stamp(&mut obstacles, Vec2::new(-50.0, 200.0), 6.0, true);
        // Nothing in bounds was touched, and nothing panicked.
        assert_eq!(obstacles.count(), 0);
    }

    #[test]
    fn a_drawn_wall_advances_the_revision() {
        // The renderer and the pheromone scrubber both key off this, so a
        // player-drawn wall that did not bump it would be invisible.
        let mut obstacles = Obstacles::new(16, 16);
        let before = obstacles.revision;
        stamp(&mut obstacles, Vec2::new(8.0, 8.0), 2.0, true);
        assert!(obstacles.revision > before);
    }
}
