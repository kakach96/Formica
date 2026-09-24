//! The window and taskbar icon, generated as pixels like every other sprite.
//!
//! Bevy 0.19 has no window-icon API, but the winit backend keeps its windows
//! reachable from the main thread via `bevy_winit::WINIT_WINDOWS`, and a winit
//! window accepts an icon at any time - it shows in the title bar, the
//! taskbar and the task switcher immediately. There is no icon file to ship
//! or lose: the picture is a pure function, so the icon can never drift from
//! the app.
//!
//! The composition is `docs/figures/logo.svg` (plaque seen in both READMEs):
//! a soil-toned rounded plaque, an amber ant pointing north-east toward a
//! food pile, with a dashed green pheromone trail laid behind it. The SVG and
//! this raster share coordinates and colours; the tests pin the raster down
//! to pixel values.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;

/// Icon size in pixels. Taskbar images are square; 128 stays crisp on HiDPI
/// taskbars (the OS downscales for smaller slots).
const SIZE: usize = 128;
/// Straight-line mix of two RGB triples.
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Hermite falloff: 1 below squared distance `e0`, 0 above `e1`.
fn fade(e0: f32, e1: f32, d2: f32) -> f32 {
    let t = ((d2 - e0) / (e1 - e0)).clamp(0.0, 1.0);
    1.0 - t * t * (3.0 - 2.0 * t)
}

fn hex32(rgb: u32) -> [f32; 3] {
    [
        (rgb >> 16) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
    ]
}

// Colour samples of the composition, shared with `docs/figures/logo.svg`.
const BADGE_TOP: u32 = 0x1d25_1c;
const BADGE_BOTTOM: u32 = 0x1115_0f;
const BADGE_EDGE: u32 = 0x3c4a_35;
const TRAIL_GREEN: u32 = 0x86e5_9a;
const FOOD_OUTER: u32 = 0x8ce0_64;
const FOOD_INNER: u32 = 0xb5f5_8e;
const ANTENNA: u32 = 0xffcf_8a;
const ABDOMEN: u32 = 0xe872_2f;
const THORAX: u32 = 0xff8a_45;
const HEAD: u32 = 0xffb0_66;

/// The ant's body anchor; the body axis points north-east in pixel space.
const ANT_CENTRE: [f32; 2] = [76.0, 52.0];
/// The SVG's `rotate(-38)`, pre-baked (const floats cannot call `sin`/`cos`):
/// cosine and sine of -38 degrees. The body axis points up-right in pixels
/// because screen `y` grows downward.
const BODY_COS: f32 = 0.7880;
const BODY_SIN: f32 = -0.6157;

/// Pixel coordinates -> ant-local coordinates (pixels on a 128 canvas):
/// `[rear-to-nose, across]`, the SVG group's inner frame.
fn ant_local(x: f32, y: f32) -> [f32; 2] {
    let dx = x - ANT_CENTRE[0];
    let dy = y - ANT_CENTRE[1];
    // Transpose of the forward rotation: unbends the local frame back out.
    [
        BODY_COS * dx + BODY_SIN * dy,
        -BODY_SIN * dx + BODY_COS * dy,
    ]
}

/// Squared distance from point `p` to segment `a - b`.
fn seg_d2(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = (ap[0] * ab[0] + ap[1] * ab[1]).max(0.0).min(len2);
    let dx = ap[0] - ab[0] * (t / len2);
    let dy = ap[1] - ab[1] * (t / len2);
    dx * dx + dy * dy
}

/// Signed distance to the rounded plaque `x, y in [4, 124]`, corner r = 27.
///
/// The classic rounded-box SDF: the corner arc measures from the innerset
/// centre, and the `min(max(qx, qy), 0)` term swaps in the straight edges
/// away from the corners, so edge-adjacent interiors report their true depth.
fn badge_sdf(x: f32, y: f32) -> f32 {
    let half = SIZE as f32 * 0.5;
    let radius = 27.0;
    let inner = half - 4.0 - radius;
    let qx = (x - half).abs() - inner;
    let qy = (y - half).abs() - inner;
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let inside = qx.max(qy).min(0.0);
    outside + inside - radius
}

/// One point on the quadratic trail curve `M 52 72 Q 45 88 40 103`.
fn trail_point(t: f32) -> [f32; 2] {
    let p0 = [52.0, 72.0];
    let p1 = [45.0, 88.0];
    let p2 = [40.0, 103.0];
    let u = 1.0 - t;
    [
        u * u * p0[0] + 2.0 * u * t * p1[0] + t * t * p2[0],
        u * u * p0[1] + 2.0 * u * t * p1[1] + t * t * p2[1],
    ]
}

/// The whole badge: `[r, g, b, a]` in 0..=1, for pixel `(px, py)` centred on
/// 128-canvas coordinates (pass `px + 0.5` style values).
pub fn icon_shade(x: f32, y: f32) -> [f32; 4] {
    // ---- badge: gradient fill with a solid rim, transparent outside ----
    let d = badge_sdf(x, y);
    let gradient = ((x - 4.0) + (y - 4.0)) / (2.0 * (SIZE as f32 - 8.0));
    let mut color = mix(hex32(BADGE_TOP), hex32(BADGE_BOTTOM), gradient);
    let alpha = if d <= 0.0 {
        1.0
    } else if d < 1.0 {
        1.0 - d
    } else {
        return [0.0, 0.0, 0.0, 0.0];
    }; // The plaque's outline: a 3-px band straddling the rounded edge.
    if (-1.5..1.5).contains(&d) {
        color = hex32(BADGE_EDGE);
    }

    // ---- pheromone trail: dots from the abdomen to the pile ----
    for t in [0.18f32, 0.38, 0.58, 0.78] {
        let [tx, ty] = trail_point(t);
        let d2 = (x - tx) * (x - tx) + (y - ty) * (y - ty);
        if d2 < 2.3 * 2.3 {
            color = hex32(TRAIL_GREEN);
        }
    }

    // ---- food pile at the end of the trail (bright core, dimmer rim) ----
    let d2 = (x - 40.0) * (x - 40.0) + (y - 103.0) * (y - 103.0);
    if d2 < 4.5 * 4.5 {
        color = hex32(FOOD_INNER);
    } else if d2 < 9.0 * 9.0 {
        color = hex32(FOOD_OUTER);
    }

    // ---- body: three lobes, painted by "closest centre wins" ----
    let local = ant_local(x, y);
    let lobes = [
        (-15.0, 0.0, 16.0, 10.5, ABDOMEN),
        (4.0, 0.0, 10.0, 7.5, THORAX),
        (20.0, 0.0, 8.5, 7.0, HEAD),
    ];
    let mut hit: Option<[f32; 3]> = None;
    let mut best = f32::INFINITY;
    for (cx, cy, rx, ry, tone) in lobes {
        let dx = (local[0] - cx) / rx;
        let dy = (local[1] - cy) / ry;
        let d2 = dx * dx + dy * dy;
        if d2 < 1.0 && d2 < best {
            best = d2;
            hit = Some(hex32(tone));
        }
    }
    // Soft shoulders: fade the outer 28% of whichever lobe was hit, so the
    // body reads as rounded rather than as hard ellipses.
    if let Some(tone) = hit {
        let cover = fade(0.72, 1.0, best);
        color = mix(color, tone, cover);
    }

    // ---- antennae: two thin strokes from the head, drawn last ----
    for sign in [-1.0f32, 1.0] {
        let base = [27.0, sign * 2.0];
        let tip = [41.0, sign * 12.0];
        if seg_d2(local, base, tip) < 1.4 * 1.4 {
            color = hex32(ANTENNA);
        }
    }

    [color[0], color[1], color[2], alpha]
}

/// The icon as raw straight-alpha RGBA bytes.
pub fn icon_rgba() -> Vec<u8> {
    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let [r, g, b, a] = icon_shade(x as f32 + 0.5, y as f32 + 0.5);
            data.push((r.clamp(0.0, 1.0) * 255.0) as u8);
            data.push((g.clamp(0.0, 1.0) * 255.0) as u8);
            data.push((b.clamp(0.0, 1.0) * 255.0) as u8);
            data.push((a.clamp(0.0, 1.0) * 255.0) as u8);
        }
    }
    data
}

/// Push the icon onto the winit window once it exists.
///
/// Startup runs before the first winit window is created, so this retries in
/// the `First` schedule until it can, then becomes a no-op. `NonSendMarker`
/// pins the system to the main thread, which is where the winit backend keeps
/// its window table (a thread-local).
pub fn apply_window_icon(
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    mut done: Local<bool>,
    _main_thread: NonSendMarker,
) {
    if *done {
        return;
    }
    let Some((entity, _)) = windows.iter().next() else {
        return;
    };
    WINIT_WINDOWS.with_borrow(|winit_windows| {
        let Some(wrapper) = winit_windows.get_window(entity) else {
            return;
        };
        match winit::window::Icon::from_rgba(icon_rgba(), SIZE as u32, SIZE as u32) {
            Ok(icon) => {
                wrapper.set_window_icon(Some(icon));
                info!("window icon installed");
                *done = true;
            }
            Err(err) => {
                warn!("window icon rejected: {err}");
                *done = true; // do not retry forever on a permanent failure
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb_straight(x: f32, y: f32) -> [u8; 4] {
        let [r, g, b, a] = icon_shade(x, y);
        [
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (a * 255.0) as u8,
        ]
    }

    /// The plaque: corners transparent, interior opaque.
    #[test]
    fn the_badge_is_a_rounded_square_on_transparency() {
        assert_eq!(rgb_straight(0.5, 0.5), [0, 0, 0, 0], "corner outside drawn");
        assert_eq!(rgb_straight(127.5, 0.5)[3], 0, "corner outside drawn");
        let centre = rgb_straight(64.5, 64.5);
        assert_eq!(centre[3], 255, "plate not opaque in the middle");
        assert!(centre[0] > centre[2], "the fill is not a warm soil tone");
        // Mid-left, well inside: the gradient fill, not the rim.
        let fill = rgb_straight(14.5, 64.5);
        let top = hex32(BADGE_TOP).map(|c| c * 255.0);
        assert!((fill[0] as i32 - top[0] as i32).abs() < 12, "rim misplaced");
        // On the outline: the rim's edge colour, deeper than the fill.
        let rim = rgb_straight(64.5, 4.6);
        let edge = hex32(BADGE_EDGE).map(|c| c * 255.0);
        assert!(
            (rim[0] as i32 - edge[0] as i32).abs() < 2
                && (rim[1] as i32 - edge[1] as i32).abs() < 2
                && (rim[2] as i32 - edge[2] as i32).abs() < 2,
            "the rim lost its edge colour: {rim:?} vs {edge:?}"
        );
    }

    /// The body reads as an ant: abdomen and head are both warm, and the
    /// head highlight is brighter than the abdomen.
    #[test]
    fn the_body_is_an_amber_three_segment_ant() {
        let abdomen = rgb_straight(64.2, 61.2);
        assert_eq!(abdomen[3], 255, "abdomen not opaque");
        assert!(
            abdomen[0] > abdomen[1] && abdomen[1] > abdomen[2],
            "abdomen is not amber: {abdomen:?}"
        );
        let head = rgb_straight(92.0, 40.0);
        assert!(
            head[0] > abdomen[0] && head[1] > abdomen[1],
            "the head lost the head-lobe highlight: {head:?} vs {abdomen:?}"
        );
    }

    /// Trail and pile: dots are green along the curve, and the pile has a
    /// bright core inside a dimmer green rim.
    #[test]
    fn the_trail_leads_to_a_bright_pile() {
        let core = rgb_straight(40.5, 102.5);
        assert!(core[1] > core[0] && core[1] > core[2], "pile not green");
        let mut green_dots = 0;
        for t in [0.18, 0.38, 0.58, 0.78] {
            let [tx, ty] = trail_point(t);
            let c = rgb_straight(tx, ty);
            if c[1] > c[0] && c[1] >= c[2] {
                green_dots += 1;
            }
        }
        assert_eq!(green_dots, 4, "trail dots went missing: {green_dots}/4");
    }

    /// Full-frame sanity: exactly 128x128x4 bytes, and everything outside the
    /// plaque is fully transparent.
    #[test]
    fn the_raster_is_a_well_formed_square_image() {
        let data = icon_rgba();
        assert_eq!(data.len(), SIZE * SIZE * 4);
        for (i, px) in data.chunks_exact(4).enumerate() {
            let x = (i % SIZE) as f32 + 0.5;
            let y = (i / SIZE) as f32 + 0.5;
            let d = badge_sdf(x, y);
            let want = if d <= 0.0 {
                255u8
            } else if d >= 1.0 {
                0u8
            } else {
                ((1.0 - d) * 255.0) as u8 // the 1-px anti-alias band
            };
            assert_eq!(px[3], want, "alpha band wrong at ({x},{y})");
        }
    }

    /// Dev aid, not run on CI: dumps the icon to `target/icon_preview.rgba`
    /// so the raw pixels can be inspected in an external image tool while
    /// tuning the composition. `cargo test -p formica-app -- --ignored
    /// dump --nocapture`.
    #[test]
    #[ignore]
    fn dump_icon_rgba_for_inspection() {
        if let Err(err) = std::fs::write("../../target/icon_preview.rgba", icon_rgba()) {
            panic!("write failed (run from the formica-app crate root): {err}");
        }
    }
}
