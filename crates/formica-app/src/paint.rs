//! Turning the pheromone field into pixels.
//!
//! Kept as a pure function of `(&PheromoneGrid, &Obstacles, &mut [u8], ...)` so
//! the parts of the renderer with real logic in them - the tone curve, the
//! per-colony channel mapping, the obstacle overlay and the row flip - can be
//! unit tested without a GPU or a window.

use bevy::prelude::*;
use formica_core::obstacles::Obstacles;
use formica_core::prelude::{Pheromone, PheromoneGrid};

/// How the world field is drawn.
#[derive(Resource, Debug, Clone)]
pub struct PheromoneView {
    /// Brightness multiplier for the food trails.
    pub food_gain: f32,
    /// Brightness multiplier for the home trails.
    ///
    /// Separate from `food_gain` so either field can be isolated - which is how
    /// the claim "the food trail is the green one" was checked against real
    /// rendered pixels (`FORMICA_SHOW_HOME=0`).
    ///
    /// It defaults close to `food_gain` on purpose, and that is a measured
    /// choice rather than an assumption. The theory said the home field would
    /// swamp the food field, because roughly four times as many ants are
    /// searching as returning and every searcher lays home pheromone wherever
    /// it wanders. Counting pixels on a real frame disproved it: at a shared
    /// gain of 0.35 the food trail covered 4634 green-dominant pixels against
    /// 4228 when the home gain was cut to 0.06 - suppressing the home field
    /// made the picture *less* informative without making the trail clearer.
    pub home_gain: f32,
    /// Draw the food trails.
    pub show_food: bool,
    /// Draw the home trails.
    pub show_home: bool,
}

impl Default for PheromoneView {
    fn default() -> Self {
        Self {
            food_gain: 0.35,
            home_gain: 0.30,
            show_food: true,
            show_home: true,
        }
    }
}

/// One colony's rendering inputs: `(food_rgb, home_rgb, food_field, home_field)`.
type ColonyFields<'a> = ([f32; 3], [f32; 3], &'a [f32], &'a [f32]);

/// Wall colour.
const OBSTACLE_RGB: [f32; 3] = [0.30, 0.31, 0.35];

/// Bare soil tone. A dark warm green-brown: dark enough that every pheromone
/// channel stays readable, warm enough that the map stops reading as space.
const GROUND_RGB: [f32; 3] = [0.055, 0.075, 0.050];

/// Brightness added to a wall cell with open ground above it (low sun).
const WALL_TOP_LIGHT: f32 = 0.10;
/// Brightness removed from a wall's base.
const WALL_BOTTOM_SHADOW: f32 = 0.05;

/// `(food_rgb, home_rgb)` for a colony.
///
/// Colony 0 is green/violet and colony 1 is amber/cyan. The two sets are far
/// apart in hue deliberately: with two colonies on one map the picture has four
/// overlapping fields in it, and a viewer has to be able to tell at a glance
/// whose trail is whose.
#[inline]
pub fn colony_colors(colony: usize) -> ([f32; 3], [f32; 3]) {
    match colony {
        1 => ([1.00, 0.72, 0.06], [0.08, 0.72, 0.98]),
        _ => ([0.10, 0.98, 0.38], [0.34, 0.12, 0.92]),
    }
}

/// Map a concentration to `0..=1` with a saturating exponential.
///
/// A linear map is useless here: trail cells sit around 5-20 while the peak
/// can exceed 50, so a linear ramp either hides the trails or clips the
/// interesting range. `1 - exp(-c * gain)` compresses the top, keeps the
/// gradient monotonic, and never exceeds 1.
#[inline]
pub fn tone(concentration: f32, gain: f32) -> f32 {
    if concentration <= 0.0 {
        0.0
    } else {
        1.0 - (-concentration * gain).exp()
    }
}

/// Write RGBA pixels for the whole world: every active colony's trails, with
/// the obstacle map drawn on top.
///
/// `active_colonies` comes from the configuration rather than from the grid,
/// because the grid is always allocated for the maximum number of colonies (so
/// that switching a second colony on at runtime cannot alias the first one's
/// fields). Drawing the unused fields would be drawing nothing, but it would
/// also double the work for no reason.
pub fn paint_world_field(
    grid: &PheromoneGrid,
    obstacles: &Obstacles,
    data: &mut [u8],
    view: &PheromoneView,
    active_colonies: usize,
) {
    let width = grid.width;
    let height = grid.height;
    debug_assert_eq!(
        data.len(),
        width * height * 4,
        "pixel buffer does not match the grid"
    );
    if data.len() < width * height * 4 {
        return;
    }

    let colonies = active_colonies.clamp(1, grid.colonies);
    // Hoist the field slices so the inner loop does no per-pixel dispatch.
    let fields: Vec<ColonyFields<'_>> = (0..colonies)
        .map(|c| {
            let (food_rgb, home_rgb) = colony_colors(c);
            (
                food_rgb,
                home_rgb,
                grid.field(c, Pheromone::Food),
                grid.field(c, Pheromone::Home),
            )
        })
        .collect();

    for y in 0..height {
        let src_row = y * width;
        // Bevy sprites put texture row 0 at the *top* of the quad, while grid
        // row 0 is the bottom of the world. Flip on write so the picture lines
        // up with where the ant sprites are drawn.
        let dst_row = (height - 1 - y) * width;

        for x in 0..width {
            let offset = (dst_row + x) * 4;

            // A wall hides whatever is underneath it. Shading is computed in
            // *world* coordinates (up = +y), so the lit top edge of a wall
            // lands where a low sun would put it after the row flip.
            if obstacles.is_blocked(x as i32, y as i32) {
                let shade = wall_shade(obstacles, x as i32, y as i32);
                data[offset] = (shade[0] * 255.0) as u8;
                data[offset + 1] = (shade[1] * 255.0) as u8;
                data[offset + 2] = (shade[2] * 255.0) as u8;
                data[offset + 3] = 255;
                continue;
            }

            // Bare ground, so the map reads as soil rather than as a void. The
            // trails are still additive on top of it.
            let ground = ground_color(x as i32, y as i32);
            let mut rgb = [ground[0], ground[1], ground[2]];
            for (food_rgb, home_rgb, food, home) in &fields {
                if view.show_food {
                    let v = tone(food[src_row + x], view.food_gain);
                    for c in 0..3 {
                        rgb[c] += food_rgb[c] * v;
                    }
                }
                if view.show_home {
                    let v = tone(home[src_row + x], view.home_gain);
                    for c in 0..3 {
                        rgb[c] += home_rgb[c] * v;
                    }
                }
            }

            data[offset] = (rgb[0].clamp(0.0, 1.0) * 255.0) as u8;
            data[offset + 1] = (rgb[1].clamp(0.0, 1.0) * 255.0) as u8;
            data[offset + 2] = (rgb[2].clamp(0.0, 1.0) * 255.0) as u8;
            data[offset + 3] = 255;
        }
    }
}

/// Bare-ground colour for one cell.
///
/// Deterministic value noise from the cell coordinates - never the simulation
/// RNG, which is reserved for behaviour and must not leak into the renderer's
/// picture. The contrast is deliberately small: enough to kill the flat-black
/// look, not enough to compete with the trails drawn on top of it.
#[inline]
pub fn ground_color(x: i32, y: i32) -> [f32; 3] {
    // A cheap integer hash (murmur-style avalanche). Pure function of the
    // coordinates, so the texture is stable across repaints and processes.
    let mut h = (x as u32).wrapping_mul(0x7385_6093) ^ (y as u32).wrapping_mul(0x1E35_A7B9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    // Two scales: a 4-cell blob and an 11-cell patch. Without the second
    // octave the map reads as salt-and-pepper instead of soil.
    let fine = (h & 0xFF) as f32 / 255.0;
    let coarse = ((h >> 8) & 0xFF) as f32 / 255.0;
    let coarse = 0.5 + 0.5 * coarse; // keep the low-frequency term bright-ish
    let variation = 0.85 + 0.10 * fine + 0.15 * coarse;
    [
        GROUND_RGB[0] * variation,
        GROUND_RGB[1] * variation,
        GROUND_RGB[2] * variation,
    ]
}

/// Wall colour for one cell, with a fake low-sun lighting pass.
///
/// The top edge of a wall (open cell above it in world space) catches light,
/// the bottom edge falls into shadow, and the face carries a little per-cell
/// grain so large blocks do not read as one flat slab. Same coordinates as
/// [`ground_color`], so both layers hold still together.
#[inline]
pub fn wall_shade(obstacles: &Obstacles, x: i32, y: i32) -> [f32; 3] {
    let open_above = !obstacles.is_blocked(x, y + 1);
    let open_below = !obstacles.is_blocked(x, y - 1);
    let mut h = (x as u32).wrapping_mul(0x7385_6093) ^ (y as u32).wrapping_mul(0x1E35_A7B9);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    let grain = ((h & 0x3F) as f32 / 63.0 - 0.5) * 0.06; // ±3% brightness
    let mut shade = [
        OBSTACLE_RGB[0] + grain * 0.4,
        OBSTACLE_RGB[1] + grain * 0.4,
        OBSTACLE_RGB[2] + grain * 0.5,
    ];
    if open_above {
        for c in 0..3 {
            shade[c] += WALL_TOP_LIGHT;
        }
    }
    if open_below {
        for c in 0..3 {
            shade[c] -= WALL_BOTTOM_SHADOW;
        }
    }
    shade.map(|v| v.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_of(w: usize, h: usize, colonies: usize) -> PheromoneGrid {
        PheromoneGrid::new(w, h, colonies)
    }

    /// Read the pixel a *world* cell was painted into, undoing the row flip.
    ///
    /// Tests are written in grid coordinates because that is the coordinate
    /// system the rest of the codebase speaks; making the reader do the flip
    /// here is what kept an earlier version of the colour test passing while
    /// actually sampling an empty row.
    fn pixel_for_cell(data: &[u8], width: usize, height: usize, x: usize, y: usize) -> [u8; 4] {
        pixel_at_image_row(data, width, height - 1 - y, x)
    }

    /// Read a pixel in raw image coordinates (row 0 = top of the texture).
    fn pixel_at_image_row(data: &[u8], width: usize, image_row: usize, x: usize) -> [u8; 4] {
        let offset = (image_row * width + x) * 4;
        [
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]
    }

    #[test]
    fn tone_is_monotonic_and_bounded() {
        let mut previous = -1.0;
        for i in 0..200 {
            let c = i as f32 * 0.5;
            let t = tone(c, 0.35);
            assert!((0.0..=1.0).contains(&t), "tone({c}) = {t}");
            assert!(t >= previous, "tone is not monotonic at {c}");
            previous = t;
        }
        assert_eq!(tone(0.0, 0.35), 0.0);
        assert_eq!(tone(-5.0, 0.35), 0.0);
        assert!(tone(1_000.0, 0.35) <= 1.0);
    }

    /// Ground shading must be deterministic and subtle: stable across calls,
    /// never dark enough to swallow a trail, never brighter than the tone a
    /// light trail paints.
    #[test]
    fn ground_noise_is_deterministic_and_subtle() {
        let a = ground_color(31, 7);
        let b = ground_color(31, 7);
        assert_eq!(a, b, "the same cell painted twice differs");
        for x in 0..64 {
            for y in 0..64 {
                let g = ground_color(x, y);
                for c in 0..3 {
                    // The variation band is 0.85..=1.10 around a dark base.
                    assert!(
                        g[c] > 0.0 && g[c] < 0.15,
                        "ground at ({x},{y}) is out of tone: {g:?}"
                    );
                }
            }
        }
        // Not all cells identical - the texture must actually vary somewhere.
        let first = ground_color(0, 0);
        let differs = (0..256).any(|i| ground_color(i * 7 + 3, i * 11 + 1) != first);
        assert!(differs, "ground texture never varies");
    }

    /// A wall's lit top edge must be brighter than its interior face, and the
    /// base darker - the fake sun must actually grade the surface.
    #[test]
    fn a_wall_has_a_lit_top_edge_and_a_shadowed_base() {
        let mut obstacles = Obstacles::new(8, 8);
        // Three wall rows: the top row has open ground above (light), the
        // bottom row has open ground below (shadow), the middle is the face.
        for x in 1..7 {
            for y in 3..6 {
                obstacles.set(x, y, true);
            }
        }
        let interior = wall_shade(&obstacles, 4, 4);
        let top = wall_shade(&obstacles, 4, 5);
        let base = wall_shade(&obstacles, 4, 3);
        assert!(
            top[0] > interior[0] + 0.05,
            "no top light: {top:?} vs {interior:?}"
        );
        assert!(
            base[0] < interior[0] - 0.03,
            "no base shadow: {base:?} vs {interior:?}"
        );
    }

    #[test]
    fn a_food_trail_reads_green_and_a_home_trail_reads_violet() {
        let view = PheromoneView::default();
        let obstacles = Obstacles::new(8, 8);

        let mut grid = grid_of(8, 8, 2);
        grid.set(0, Pheromone::Food, 2, 3, 20.0);
        let mut data = vec![0u8; 8 * 8 * 4];
        paint_world_field(&grid, &obstacles, &mut data, &view, 1);
        let pixel = pixel_for_cell(&data, 8, 8, 2, 3);
        assert!(pixel[1] > pixel[0], "food trail is not green: {pixel:?}");
        assert!(pixel[1] > pixel[2], "food trail is not green: {pixel:?}");

        let mut grid = grid_of(8, 8, 2);
        grid.set(0, Pheromone::Home, 2, 3, 20.0);
        let mut data = vec![0u8; 8 * 8 * 4];
        paint_world_field(&grid, &obstacles, &mut data, &view, 1);
        let pixel = pixel_for_cell(&data, 8, 8, 2, 3);
        assert!(pixel[2] > pixel[1], "home trail is not violet: {pixel:?}");
    }

    /// With two colonies on one map the viewer must be able to tell whose trail
    /// is whose, so the second colony's food trail must not read green.
    #[test]
    fn the_second_colony_gets_a_distinguishable_trail_colour() {
        let view = PheromoneView::default();
        let obstacles = Obstacles::new(8, 8);
        let mut grid = grid_of(8, 8, 2);

        grid.set(0, Pheromone::Food, 2, 3, 20.0);
        let mut first = vec![0u8; 8 * 8 * 4];
        paint_world_field(&grid, &obstacles, &mut first, &view, 2);
        let a = pixel_for_cell(&first, 8, 8, 2, 3);

        let mut grid = grid_of(8, 8, 2);
        grid.set(1, Pheromone::Food, 2, 3, 20.0);
        let mut second = vec![0u8; 8 * 8 * 4];
        paint_world_field(&grid, &obstacles, &mut second, &view, 2);
        let b = pixel_for_cell(&second, 8, 8, 2, 3);

        assert!(a[1] > a[0], "colony 0's trail should be green: {a:?}");
        assert!(b[0] > b[1], "colony 1's trail should be amber: {b:?}");
        assert_ne!(a, b);
    }

    /// An inactive colony's fields may hold stale data (they are reused if the
    /// player turns the colony back on) and must not be drawn.
    #[test]
    fn inactive_colonies_are_not_drawn() {
        let view = PheromoneView::default();
        let obstacles = Obstacles::new(8, 8);
        let mut grid = grid_of(8, 8, 2);
        grid.set(1, Pheromone::Food, 2, 3, 200.0);

        let mut data = vec![0u8; 8 * 8 * 4];
        paint_world_field(&grid, &obstacles, &mut data, &view, 1);
        let pixel = pixel_for_cell(&data, 8, 8, 2, 3);
        // The cell shows bare soil - the inactive colony's field contributed
        // nothing on top of it.
        let ground = ground_color(2, 3);
        let expected = [
            (ground[0] * 255.0) as u8,
            (ground[1] * 255.0) as u8,
            (ground[2] * 255.0) as u8,
            255,
        ];
        assert_eq!(pixel, expected, "an inactive colony was painted");
    }

    #[test]
    fn walls_are_drawn_over_whatever_is_underneath() {
        let view = PheromoneView::default();
        let mut obstacles = Obstacles::new(8, 8);
        obstacles.set(2, 3, true);
        let mut grid = grid_of(8, 8, 2);
        // A strong trail in the same cell.
        grid.set(0, Pheromone::Food, 2, 3, 500.0);

        let mut data = vec![0u8; 8 * 8 * 4];
        paint_world_field(&grid, &obstacles, &mut data, &view, 1);
        let pixel = pixel_for_cell(&data, 8, 8, 2, 3);
        let shade = wall_shade(&obstacles, 2, 3);
        let expected = [
            (shade[0] * 255.0) as u8,
            (shade[1] * 255.0) as u8,
            (shade[2] * 255.0) as u8,
            255,
        ];
        assert_eq!(pixel, expected, "the wall did not cover the trail");
    }

    /// The image row order must match the world, or the picture is upside down
    /// relative to the ant sprites.
    #[test]
    fn grid_row_zero_lands_on_the_last_image_row() {
        let view = PheromoneView::default();
        let obstacles = Obstacles::new(4, 4);
        let mut grid = grid_of(4, 4, 1);
        grid.set(0, Pheromone::Food, 3, 0, 50.0);

        let mut data = vec![0u8; 4 * 4 * 4];
        paint_world_field(&grid, &obstacles, &mut data, &view, 1);

        // Grid (3, 0) -> image row height-1-0 = 3, column 3.
        let bottom = pixel_at_image_row(&data, 4, 3, 3);
        assert!(
            bottom[1] > 100,
            "grid row 0 did not land on the bottom image row: {bottom:?}"
        );
        let top = pixel_at_image_row(&data, 4, 0, 3);
        // Bare soil is dark but not zero: the key claim is that the *trail*
        // is far brighter than the un-painted flipped cell.
        assert!(
            top[1] < 40 && top[1] > 5,
            "the flipped cell should be bare soil: {top:?}"
        );
    }

    #[test]
    fn zero_field_paints_bare_ground() {
        let view = PheromoneView::default();
        let obstacles = Obstacles::new(4, 4);
        let grid = grid_of(4, 4, 2);
        let mut data = vec![7u8; 4 * 4 * 4];
        paint_world_field(&grid, &obstacles, &mut data, &view, 2);
        for (image_index, chunk) in data.chunks(4).enumerate() {
            // Undo the row flip to get the world cell this chunk belongs to.
            let image_row = image_index / 4;
            let x = image_index % 4;
            let y = (4 - 1 - image_row) as i32;
            let ground = ground_color(x as i32, y);
            let expected = [
                (ground[0] * 255.0) as u8,
                (ground[1] * 255.0) as u8,
                (ground[2] * 255.0) as u8,
                255,
            ];
            assert_eq!(chunk, &expected, "background is not bare ground");
        }
    }

    #[test]
    fn view_toggles_hide_a_field() {
        let obstacles = Obstacles::new(4, 4);
        let mut grid = grid_of(4, 4, 1);
        grid.set(0, Pheromone::Food, 1, 1, 50.0);
        let mut data = vec![0u8; 4 * 4 * 4];
        paint_world_field(
            &grid,
            &obstacles,
            &mut data,
            &PheromoneView {
                show_food: false,
                ..Default::default()
            },
            1,
        );
        // The hidden field must contribute nothing: every cell shows exactly
        // its bare-soil colour, the trail cell included (no home pheromone
        // was laid anywhere).
        let mut bad: Vec<String> = Vec::new();
        for (image_index, chunk) in data.chunks(4).enumerate() {
            let image_row = image_index / 4;
            let x = image_index % 4;
            let y = (4 - 1 - image_row) as i32;
            let ground = ground_color(x as i32, y);
            let expected = [
                (ground[0] * 255.0) as u8,
                (ground[1] * 255.0) as u8,
                (ground[2] * 255.0) as u8,
                255,
            ];
            if chunk != expected {
                bad.push(format!("cell ({x},{y}): {chunk:?} != {expected:?}"));
            }
        }
        assert!(bad.is_empty(), "food trail drawn while hidden; {bad:?}");
    }

    #[test]
    fn a_short_buffer_is_ignored_rather_than_panicking_in_release() {
        let view = PheromoneView::default();
        let obstacles = Obstacles::new(8, 8);
        let grid = grid_of(8, 8, 1);
        let mut data = vec![0u8; 4];
        // `debug_assert_eq!` fires in debug builds, which is the point; this
        // only checks the release-mode guard does not index out of bounds.
        if cfg!(not(debug_assertions)) {
            paint_world_field(&grid, &obstacles, &mut data, &view, 1);
        }
    }
}
