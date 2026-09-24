//! The window and taskbar icon, generated as pixels like every other sprite.
//!
//! Bevy 0.19 has no window-icon API, but the winit backend keeps its windows
//! reachable from the main thread via `bevy_winit::WINIT_WINDOWS`, and a winit
//! window accepts an icon at any time - it shows in the title bar, the
//! taskbar and the task switcher immediately. There is no icon file to ship
//! or lose: the picture is the same pure-function rasterisation the in-game
//! sprites use, so the icon can never drift from the app.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;

/// Icon size in pixels. Taskbar images are square; 64 covers HiDPI taskbars.
const SIZE: usize = 64;

/// An ant drawn on a transparent background: three panels, amber body.
///
/// `(nx, ny)` spans `0..=1` over the canvas. The lobes reuse the silhouettes
/// in spirit only - an icon needs bolder shapes than a 24-pixel sprite.
fn icon_shade(nx: f32, ny: f32) -> [f32; 4] {
    let smooth = |d2: f32| -> f32 {
        // 1 inside, fading over the outer band: a soft-edged ellipse.
        let t = (1.0 - d2).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // Three body segments across the middle of the canvas.
    let lobes = [
        (-0.22, 0.15, 0.10, 0.60), // abdomen (cx, rx, ry, height)
        (0.06, 0.12, 0.09, 0.85),  // thorax
        (0.30, 0.11, 0.09, 1.0),   // head, the brightest segment
    ];
    let mut cover = 0.0f32;
    let mut height = 0.0f32;
    for (cx, rx, ry, level) in lobes {
        let dx = (nx - cx) / rx;
        let dy = (ny - 0.5) / ry;
        let d2 = dx * dx + dy * dy;
        if d2 < 1.0 {
            let c = smooth(d2);
            if c > cover {
                cover = c;
                height = level;
            }
        }
        // Antennae: two thin arcs from the head, purely for silhouette.
        if (ny - 0.5 - 0.55 * (nx - 0.30)).abs() < 0.008 && (nx - 0.30).abs() < 0.14 && nx > 0.30 {
            cover = cover.max(0.85);
            height = 1.0;
        }
    }

    let brightness = 0.55 + 0.45 * height;
    [
        1.00 * brightness, // warm amber-orange body, colony 0's returning hue
        0.55 * brightness,
        0.15 * brightness,
        cover,
    ]
}

/// The icon as raw RGBA bytes.
pub fn icon_rgba() -> Vec<u8> {
    let mut data = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let nx = (x as f32 + 0.5) / SIZE as f32;
            let ny = (y as f32 + 0.5) / SIZE as f32;
            let [r, g, b, a] = icon_shade(nx, ny);
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
