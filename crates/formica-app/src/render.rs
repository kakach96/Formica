//! Phase 2: draw the simulation.
//!
//! The whole renderer is read-only with respect to the simulation. It never
//! writes `AntPos`, `AntState` or the grid; it only mirrors them onto
//! `Transform`/`Sprite` every frame in `PostUpdate`, after the tick schedule
//! has run.
//!
//! Layers, bottom to top:
//!
//! * **z = 0** the world field: every active colony's pheromone plus the
//!   obstacle map, baked into one `Image` asset and drawn as a single sprite
//!   stretched over the map. Obstacles live in this texture rather than being
//!   their own entities, so a wall costs nothing to draw and can never drift
//!   out of alignment with the cells the simulation actually blocks.
//! * **z = 1** food piles, **z = 2** nests, **z = 3** ants and predators.
//!
//! Ants and predators carry their own `Sprite`/`Transform` components, so there
//! is no side table mapping simulation entities to view entities. Food piles
//! and nests are plain data in a `Resource`, so those two *do* need a
//! bookkeeping step - done by comparing counts, which is immune to the stale
//! index problem a saved revision counter has across a reset.

use bevy::asset::RenderAssetUsages;
use bevy::camera::ScalingMode;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use formica_core::motion::wrap_angle;
use formica_core::obstacles::Obstacles;
use formica_core::prelude::*;

use crate::paint::{paint_world_field, PheromoneView};

/// Ants that are still looking for food, per colony.
const SEARCHING_COLORS: [Color; 2] = [
    Color::srgb(0.93, 0.96, 1.0), // colony 0: white
    Color::srgb(0.55, 0.95, 1.0), // colony 1: pale cyan
];

/// Ants carrying food home, per colony.
const RETURNING_COLORS: [Color; 2] = [
    Color::srgb(1.0, 0.42, 0.12), // colony 0: orange
    Color::srgb(1.0, 0.35, 0.85), // colony 1: magenta
];

/// Nest colours, per colony.
const NEST_COLORS: [Color; 2] = [
    Color::srgb(1.0, 0.78, 0.15), // colony 0: amber
    Color::srgb(0.25, 0.85, 1.0), // colony 1: cyan
];

/// A predator that has not seen anything.
const PREDATOR_CALM: Color = Color::srgb(0.78, 0.10, 0.16);
/// A predator with prey in sight.
const PREDATOR_HUNTING: Color = Color::srgb(1.0, 0.28, 0.45);

/// Ant body: length along the heading, width across it, in world cells.
/// An ant stops being a featureless square once it has a longer-than-wide
/// body that points where it is walking.
const ANT_LENGTH: f32 = 3.0;
const ANT_WIDTH: f32 = 1.8;
/// Predator body: a wedge twice the ant's length, nose forward.
const PREDATOR_LENGTH: f32 = 7.0;
const PREDATOR_WIDTH: f32 = 4.2;

/// Handle to the live world texture.
#[derive(Resource, Debug, Clone)]
pub struct PheromoneTexture(pub Handle<Image>);

/// The size the current scene was built for.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SceneGeometry {
    /// Map width in cells.
    pub width: usize,
    /// Map height in cells.
    pub height: usize,
}

/// How wide (in pixels) the egui control panel currently is.
///
/// The UI writes it every frame; the camera uses it to center the map inside
/// the *remaining* free area instead of the whole window, which would push the
/// map half under the panel.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct PanelMetrics {
    /// Panel width in pixels.
    pub width: f32,
}

/// Where the player has dragged and zoomed the map view.
///
/// `zoom = 1.0` is the fitted, centered framing every figure is captured at,
/// so an untouched view is bit-identical to the pre-pan behaviour; `pan` is a
/// world-unit offset applied on top of the fitted placement. Deliberately
/// *not* reset on a world restart: the player may be inspecting a trail
/// mid-experiment, and a reset should not steal their vantage point.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct CameraView {
    /// Camera offset in world units, on top of the fitted placement.
    pub pan: Vec2,
    /// Zoom factor, `1.0` = fitted view.
    pub zoom: f32,
}

/// Zoom limits. `1.0` is the fitted framing; beyond that only magnify - a
/// zoomed-out view would just show letterbox the player did not ask for.
const MIN_ZOOM: f32 = 1.0;
const MAX_ZOOM: f32 = 12.0;

/// Clamp a zoom factor into `[MIN_ZOOM, MAX_ZOOM]`.
pub fn clamp_zoom(zoom: f32) -> f32 {
    zoom.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Clamp a pan offset so the camera never leaves the map.
///
/// Panning is only meaningful while the map is bigger than the visible area,
/// so the allowed slack shrinks to zero at the fitted zoom: at `zoom = 1` the
/// pan is zero, at `zoom = 4` the camera may travel within the inner
/// `3/8` of the map (half of the `1 - 1/zoom` half-extent).
pub fn clamp_pan(pan: Vec2, fw: f32, fh: f32, zoom: f32) -> Vec2 {
    let slack = 0.5 * (1.0 - 1.0 / clamp_zoom(zoom).max(1.0));
    let limit = Vec2::new(fw * slack, fh * slack);
    pan.clamp(-limit, limit)
}

/// Pan adjustment that keeps the world point under the cursor under it while
/// the zoom goes `old -> new` (both already clamped, `new > 0`).
///
/// The cursor sits at world offset `R` from the camera centre; after zooming
/// by `k = new/old` the same cursor ray hits `R/k`, so the camera must slide
/// by `R * (1 - 1/k)`.
pub fn zoom_anchor_pan(pan: Vec2, cursor_offset_world: Vec2, old: f32, new: f32) -> Vec2 {
    let k = new / old;
    pan + cursor_offset_world * (1.0 - 1.0 / k)
}

/// The one entity drawing the world field.
#[derive(Component)]
pub struct PheromoneLayer;

/// Marks an ant entity that has been given presentation components.
#[derive(Component)]
pub struct AntVisual;

/// Marks a predator entity that has been given presentation components.
#[derive(Component)]
pub struct PredatorVisual;

/// Marks a food-pile sprite; the payload indexes `FoodField::piles`.
#[derive(Component)]
pub struct FoodVisual(pub usize);

/// Marks a nest sprite.
///
/// No payload: nest sprites are always rebuilt wholesale when the colony count
/// changes, so there is no per-sprite index to keep in step.
#[derive(Component)]
pub struct NestVisual;

/// Marks a jet-exit overlay sprite: the translucent cone showing where ants
/// leave a nest in directed-jet mode.
///
/// Also no payload - like [`NestVisual`], it is rebuilt wholesale whenever
/// the mode, the jet geometry or the nest layout changes.
#[derive(Component)]
pub struct NestExitVisual;

/// Requests from the UI that need `&mut World` to service.
#[derive(Resource, Debug, Clone, Default)]
pub struct SimCommands {
    /// Rebuild the world from the current configuration: fresh food, empty
    /// trails, zeroed statistics, new colonies on the nests.
    pub restart: bool,
    /// Zero every pheromone field but leave everything else alone. Useful for
    /// watching a converged colony re-converge from scratch.
    pub clear_pheromones: bool,
    /// Load a scenario first: apply its config and map, then restart. The
    /// normal reset path carries the scenario's walls and food because both
    /// live in `SimConfig` - there is no separate "scenario world" to build.
    pub load_scenario: Option<formica_core::scenario::Scenario>,
}

/// Installs camera, sprites and the per-frame presentation sync.
pub struct FormicaRenderPlugin;

/// Handles to the generated creature-and-object textures.
///
/// All four are white/greyscale with alpha, and take their colour from the
/// `Sprite` tint - one texture per species serves every colony and state. They
/// are built once per scene rebuild; the shapes are pure functions, so a
/// rebuilt scene is pixel-identical to the old one.
#[derive(Resource, Debug, Clone)]
pub struct VisualTextures {
    /// Elongated ant body, pointing `+X`.
    pub ant: Handle<Image>,
    /// Wedge-shaped predator, nose forward, pointing `+X`.
    pub predator: Handle<Image>,
    /// Soft mound for a nest.
    pub nest: Handle<Image>,
    /// Soft disc for a food pile.
    pub food: Handle<Image>,
}

/// Rasterise an RGBA image from a pure function of normalized coordinates.
///
/// `(nx, ny)` spans `-0.5..=0.5` in both axes, so a shape drawn at `nx = 0.4`
/// hugs its right edge regardless of the texture's pixel size. Returning the
/// row here is what keeps the flip argument out of these functions entirely -
/// sprites are centred, unlike the world field, so there is no row flip to
/// reason about.
fn raster(width: u32, height: u32, shade: impl Fn(f32, f32) -> [f32; 4]) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let data = image
        .data
        .as_mut()
        .expect("a freshly built image always has CPU data");
    for py in 0..height {
        for px in 0..width {
            let nx = (px as f32 + 0.5) / width as f32 - 0.5;
            let ny = (py as f32 + 0.5) / height as f32 - 0.5;
            let [r, g, b, a] = shade(nx, ny);
            let i = ((py * width + px) * 4) as usize;
            data[i] = (r.clamp(0.0, 1.0) * 255.0) as u8;
            data[i + 1] = (g.clamp(0.0, 1.0) * 255.0) as u8;
            data[i + 2] = (b.clamp(0.0, 1.0) * 255.0) as u8;
            data[i + 3] = (a.clamp(0.0, 1.0) * 255.0) as u8;
        }
    }
    image
}

/// Smooth Hermite interpolation between two edges; `x < e0` → 0, `x > e1` → 1.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// An ant: three body segments in a line along `+X`, soft-edged.
fn ant_shade(nx: f32, ny: f32) -> [f32; 4] {
    let part = |cx: f32, rx: f32, ry: f32, level: f32| -> (f32, f32) {
        let dx = (nx - cx) / rx;
        let dy = ny / ry;
        let d2 = dx * dx + dy * dy;
        // Cover fades over the outer 15% of each lobe; brightness dips there
        // too, which reads as a rounded body rather than a hard ellipse.
        let coverage = 1.0 - smoothstep(0.72, 1.0, d2);
        let brightness = level * coverage.sqrt();
        (coverage, brightness)
    };
    let (mut cover, mut bright) = part(-0.20, 0.30, 0.26, 0.72); // abdomen
    for (cx, rx, ry, level) in [(0.06, 0.18, 0.20, 0.85), (0.32, 0.15, 0.15, 1.0)] {
        let (c, b) = part(cx, rx, ry, level);
        cover = cover.max(c);
        bright = bright.max(b);
    }
    [bright, bright, bright, cover]
}

/// A predator: a blunt wedge with two mandible notches, nose at `+X`.
fn predator_shade(nx: f32, ny: f32) -> [f32; 4] {
    // Triangle body: wide rear, pointed nose.
    let body = |px: f32, py: f32| {
        // Half-width shrinks linearly from the rear to the nose tip.
        let t = (px + 0.38) / 0.83; // 0 at rear, 1 at nose
        let half_width = 0.30 * (1.0 - t);
        py.abs() < half_width && (0.0..=1.0).contains(&t)
    };
    let on_body = body(nx, ny);
    let coverage = if on_body {
        // Soften the outline: inside the triangle but within a few pixels of
        // an edge still draws, dimmer.
        let rear_edge = smoothstep(-0.40, -0.36, nx);
        let taper = 1.0 - smoothstep(0.80, 0.95, ((nx + 0.38) / 0.83).clamp(0.0, 1.0));
        rear_edge * taper * 1.0
    } else {
        0.0
    };
    let shade = if ny > 0.0 { 0.9 } else { 0.72 }; // lit above, shaded below
    [shade, shade, shade, coverage]
}

/// A nest: a raised mound with a bright centre and a rim shadow.
fn nest_shade(nx: f32, ny: f32) -> [f32; 4] {
    let r = (nx * nx + ny * ny).sqrt() * 2.0; // 0 at centre, 1 at the edge
    let bump = |mu: f32, sigma: f32| (-((r - mu) / sigma).powi(2)).exp();
    // A dirt mound: bright crown, a raised collar where the entrance is, a
    // dark foot where the mound meets the ground.
    let mut v = 0.95 - 0.55 * r;
    v += 0.20 * bump(0.62, 0.07);
    v -= 0.25 * bump(0.88, 0.08);
    let alpha = smoothstep(1.0, 0.90, r);
    [v, v, v, alpha]
}

/// A food pile: a soft heap with a slightly transparent rim.
fn food_shade(nx: f32, ny: f32) -> [f32; 4] {
    let r = (nx * nx + ny * ny).sqrt() * 2.0;
    let v = 0.65 + 0.35 * (1.0 - r);
    let alpha = 0.95 * smoothstep(1.0, 0.80, r);
    [v, v, v, alpha]
}

/// The jet-exit overlay: a translucent cone from the nest centre outward,
/// pointing `+X` - the sprite's rotation carries the bearing.
///
/// `spread` is the full cone width in degrees. The tip fades out before the
/// sprite edge (the texture's half-extent spans 2.5 nest radii, so the
/// visible fan starts where the mound sprite ends), and at 360° the angle
/// test degenerates to "everywhere" - drawn as a halo, not a wedge with a
/// wrap seam at the ±180° edge.
fn nest_exit_shade(nx: f32, ny: f32, spread: f32) -> [f32; 4] {
    let d = (nx * nx + ny * ny).sqrt() * 2.0; // 0 at centre, 1 at the edge
                                              // The sprite's half-extent spans 2.5 nest radii, so the fan stays at
                                              // full strength past the mound rim and fades out over its outer 40%.
    let radial = 1.0 - smoothstep(0.60, 1.0, d);
    let angular = if spread >= 359.5 {
        1.0
    } else {
        let half = spread.max(0.0).to_radians() * 0.5;
        let offset = wrap_angle(ny.atan2(nx)).abs();
        1.0 - smoothstep(half - 0.03, half + 0.03, offset)
    };
    let alpha = 0.60 * angular * radial;
    [1.0, 1.0, 1.0, alpha]
}

impl Plugin for FormicaRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PheromoneView>()
            .init_resource::<SimCommands>()
            .init_resource::<SceneGeometry>()
            .init_resource::<PanelMetrics>()
            .init_resource::<CameraView>()
            .add_systems(
                // Presentation sync runs in `PostUpdate` so it always observes a
                // fully-ticked world, whatever happens in `Update`.
                PostUpdate,
                (
                    // Creates the camera and the world texture, and rebuilds
                    // both whenever the map is resized.
                    rebuild_scene_if_needed,
                    // After either of the two above changes, keep the map
                    // centered in the area right of the control panel.
                    center_camera,
                    attach_ant_sprites,
                    sync_ant_sprites,
                    attach_predator_sprites,
                    sync_predator_sprites,
                    sync_food_sprites,
                    sync_nest_sprites,
                    sync_exit_sprites,
                    paint_world,
                )
                    .chain(),
            );
    }
}

/// Create (or recreate) the camera and the world texture for the current map
/// size.
///
/// Exclusive because it needs to despawn and respawn entities that other
/// systems also touch, and to build an `Image` asset.
fn rebuild_scene_if_needed(world: &mut World) {
    let (width, height) = {
        let config = world.resource::<SimConfig>();
        (config.width, config.height)
    };
    let current = *world.resource::<SceneGeometry>();
    if current.width == width && current.height == height {
        return;
    }

    // Tear down stale *sprites*: the camera must survive a rebuild, because
    // bevy_egui attaches its primary context (`EguiContext` +
    // `PrimaryEguiContext`) to the first camera it ever sees and never hands
    // one out again. Despawning the camera silently killed the control panel
    // from that frame on - the bug a map-size change (e.g. `long-haul`)
    // triggered on every resize.
    let stale: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, Or<(
            With<PheromoneLayer>,
            With<FoodVisual>,
            With<NestVisual>,
            With<NestExitVisual>,
            With<PredatorVisual>,
        )>>();
        query.iter(world).collect()
    };
    for entity in stale {
        let _ = world.despawn(entity);
    }

    let image = Image::new_fill(
        Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        // Both worlds: we rewrite the CPU buffer every frame, the GPU holds the
        // uploaded copy.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    let handle = world.resource_mut::<Assets<Image>>().add(image);

    let (fw, fh) = (width as f32, height as f32);
    let mut projection = OrthographicProjection::default_2d();
    projection.scaling_mode = ScalingMode::AutoMin {
        min_width: fw + 10.0,
        min_height: fh + 10.0,
    };
    // Reuse the existing camera entity (keeping its egui context) and merely
    // re-point its projection and transform at the new map; spawn a fresh one
    // only on the very first build.
    let camera_already_exists = {
        let mut query = world.query_filtered::<Entity, With<Camera2d>>();
        query.iter(world).next().is_some()
    };
    if camera_already_exists {
        let mut query = world.query_filtered::<(&mut Projection, &mut Transform), With<Camera2d>>();
        let Some((mut camera, mut transform)) = query.iter_mut(world).next() else {
            unreachable!("camera existence was checked above");
        };
        *camera = Projection::Orthographic(projection);
        *transform = Transform::from_xyz(fw * 0.5, fh * 0.5, 1_000.0);
    } else {
        world.spawn((
            Camera2d,
            Projection::Orthographic(projection),
            Transform::from_xyz(fw * 0.5, fh * 0.5, 1_000.0),
        ));
    }

    world.spawn((
        PheromoneLayer,
        Sprite {
            image: handle.clone(),
            custom_size: Some(Vec2::new(fw, fh)),
            ..default()
        },
        // A sprite is centred on its transform, so the map spans [0, w] x [0, h]
        // and texel (x, y) covers exactly the world cell (x, y).
        Transform::from_xyz(fw * 0.5, fh * 0.5, 0.0),
    ));

    world.insert_resource(PheromoneTexture(handle));
    world.insert_resource(SceneGeometry { width, height });

    // Creature and object textures are regenerated with the scene - they are
    // pure functions, so this is cheap and cannot drift from the shapes the
    // tests assert on.
    let mut images = world.resource_mut::<Assets<Image>>();
    let textures = VisualTextures {
        // 4x the historical resolutions: the shapes are pure functions of
        // normalized coordinates, so the raster is the same picture sampled
        // denser, and a zoomed-in view (up to 12x) no longer magnifies
        // blocky source textures.
        ant: images.add(raster(96, 56, ant_shade)),
        predator: images.add(raster(112, 72, predator_shade)),
        nest: images.add(raster(128, 128, nest_shade)),
        food: images.add(raster(96, 96, food_shade)),
    };
    world.insert_resource(textures);
    // The letterbox margin around the map matches the soil, not deep space.
    world.insert_resource(ClearColor(Color::srgb(0.035, 0.045, 0.035)));
    info!("scene rebuilt for a {width}x{height} map");
}

/// Center the map inside the area right of the control panel, then apply the
/// player's pan and zoom on top.
///
/// The camera fits the map to the shorter viewport axis, so with a wide window
/// the leftover width is split evenly around the map - but the panel occupies
/// the left strip, so even splitting leaves the map off-center in what the
/// player actually sees. Moving the camera so that the map's center lands on
/// the free strip's center fixes that. Runs every frame because the panel can
/// be resized and the window resizes too; it is a couple of arithmetic
/// operations, cheap enough not to bother with change detection.
fn center_camera(
    windows: Query<&Window, With<PrimaryWindow>>,
    geometry: Res<SceneGeometry>,
    panel: Res<PanelMetrics>,
    view: Res<CameraView>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<Camera2d>>,
) {
    if geometry.width == 0 || geometry.height == 0 {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((mut transform, mut projection)) = cameras.single_mut() else {
        return;
    };

    let (placement, ortho_scale) = camera_placement(
        geometry.width as f32,
        geometry.height as f32,
        Vec2::new(window.width(), window.height()),
        panel.width,
        view.zoom,
        view.pan,
    );
    if transform.translation != placement {
        transform.translation = placement;
    }
    // Zoom rides `OrthographicProjection::scale` (smaller scale = closer):
    // the projection's own docs call this the intended way to zoom, and it
    // keeps the `AutoMin` letterbox math intact for every other system.
    if let Projection::Orthographic(ortho) = projection.as_mut() {
        if ortho.scale != ortho_scale {
            ortho.scale = ortho_scale;
        }
    }
}

/// Where the camera sits, and what its orthographic scale should be, for the
/// current window, panel and view. Pure, so the framing math is testable
/// without a `Window` entity.
///
/// Returns `(camera translation, projection scale)`; `zoom = 1.0` with zero
/// pan is the fitted, centered framing everything else assumes.
fn camera_placement(
    fw: f32,
    fh: f32,
    viewport: Vec2,
    panel_width: f32,
    zoom: f32,
    pan: Vec2,
) -> (Vec3, f32) {
    let zoom = clamp_zoom(zoom);
    // `AutoMin` scale in *pixels per world unit* at zoom 1: the largest scale
    // that still shows at least `fw + 10 x fh + 10` world units (the same
    // letterbox margin the rebuild uses).
    let base = (viewport.x / (fw + 10.0))
        .min(viewport.y / (fh + 10.0))
        .max(f32::EPSILON);
    let px_per_world = base * zoom;
    // The panel occupies the strip `[0, panel.width]` on screen; in world
    // units that is `panel.width / px_per_world` wide, and the camera must
    // sit half that far to the *left* of the map's center for the map's
    // center to land on the free strip's center.
    let panel_world = panel_width / px_per_world;
    let pan = clamp_pan(pan, fw, fh, zoom);
    (
        Vec3::new(
            fw * 0.5 - panel_world * 0.5 + pan.x,
            fh * 0.5 + pan.y,
            1_000.0,
        ),
        1.0 / zoom,
    )
}

/// Give every ant entity a `Sprite` and a `Transform`.
///
/// Ants are spawned by the simulation, which knows nothing about rendering, so
/// the view components are attached here on first sight. A freshly reset or
/// resized colony is picked up automatically on the next frame.
fn attach_ant_sprites(
    mut commands: Commands,
    ants: Query<Entity, (With<AntMarker>, Without<AntVisual>)>,
    textures: Res<VisualTextures>,
) {
    for entity in ants.iter() {
        commands.entity(entity).insert((
            AntVisual,
            Sprite {
                image: textures.ant.clone(),
                color: SEARCHING_COLORS[0],
                // A longer-than-wide custom size makes the texture's elongated
                // body visible; the transform rotation gives it a heading.
                custom_size: Some(Vec2::new(ANT_LENGTH, ANT_WIDTH)),
                ..default()
            },
            // Above the pheromone field, food and nests.
            Transform::from_xyz(0.0, 0.0, 3.0),
        ));
    }
}

/// Mirror ant state onto the sprite components.
///
/// The body is drawn along `+X`, and heading angles measure from `+X` too, so
/// the rotation is the heading itself - no offset to keep in sync.
fn sync_ant_sprites(
    mut ants: Query<
        (
            &AntColony,
            &AntPos,
            &AntState,
            &AntHeading,
            &mut Transform,
            &mut Sprite,
        ),
        With<AntVisual>,
    >,
) {
    for (colony, pos, state, heading, mut transform, mut sprite) in ants.iter_mut() {
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        let rotation = Quat::from_rotation_z(heading.angle);
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
        let slot = colony.index().min(1);
        let wanted = match state {
            AntState::Searching => SEARCHING_COLORS[slot],
            AntState::Returning => RETURNING_COLORS[slot],
        };
        // Avoid touching the colour (and triggering change detection) unless it
        // actually differs.
        if sprite.color != wanted {
            sprite.color = wanted;
        }
    }
}

/// Give every predator entity a `Sprite` and a `Transform`.
fn attach_predator_sprites(
    mut commands: Commands,
    predators: Query<Entity, (With<PredatorMarker>, Without<PredatorVisual>)>,
    textures: Res<VisualTextures>,
) {
    for entity in predators.iter() {
        commands.entity(entity).insert((
            PredatorVisual,
            Sprite {
                image: textures.predator.clone(),
                color: PREDATOR_CALM,
                custom_size: Some(Vec2::new(PREDATOR_LENGTH, PREDATOR_WIDTH)),
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 4.0),
        ));
    }
}

/// Mirror predator state onto the sprite components.
fn sync_predator_sprites(
    mut predators: Query<
        (
            &PredatorPos,
            &PredatorHeading,
            &PredatorAlert,
            &mut Transform,
            &mut Sprite,
        ),
        With<PredatorVisual>,
    >,
) {
    for (pos, heading, alert, mut transform, mut sprite) in predators.iter_mut() {
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
        let rotation = Quat::from_rotation_z(heading.angle);
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
        let wanted = if alert.0 {
            PREDATOR_HUNTING
        } else {
            PREDATOR_CALM
        };
        if sprite.color != wanted {
            sprite.color = wanted;
        }
    }
}

/// Food that has never been touched.
const FOOD_FULL: [f32; 3] = [0.55, 0.95, 0.25];
/// Food that has been picked over. A *dimmed* green rather than a red one: an
/// earlier version ramped toward red, which is the predator colour, so an
/// exhausted pile read as a threat on the map.
const FOOD_EMPTY: [f32; 3] = [0.20, 0.32, 0.10];

/// Shrink and fade each food pile as it is eaten, and keep the sprite count in
/// step with the pile list.
///
/// The count comparison is what makes player-placed and player-removed food
/// work: `FoodField::piles` is plain data with no entities of its own, so the
/// sprites are rebuilt whenever the two disagree. Comparing counts rather than
/// a stored revision is deliberate - a revision counter has to survive a reset,
/// and "the world was rebuilt but the counter happened to match" is exactly the
/// bug that leaves stale sprites on the map.
fn sync_food_sprites(
    mut commands: Commands,
    food: Res<FoodField>,
    textures: Res<VisualTextures>,
    existing: Query<Entity, With<FoodVisual>>,
    mut sprites: Query<(&FoodVisual, &mut Sprite)>,
) {
    if existing.iter().count() != food.piles.len() {
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }
        for (index, pile) in food.piles.iter().enumerate() {
            let fraction = remaining_fraction(&food, pile);
            let (size, color) = food_appearance(pile.radius, fraction);
            commands.spawn((
                FoodVisual(index),
                Sprite {
                    image: textures.food.clone(),
                    color,
                    custom_size: Some(size),
                    ..default()
                },
                Transform::from_xyz(pile.x, pile.y, 1.0),
            ));
        }
        return;
    }

    if !food.is_changed() {
        return;
    }
    for (visual, mut sprite) in sprites.iter_mut() {
        let Some(pile) = food.piles.get(visual.0) else {
            continue;
        };
        let (size, color) = food_appearance(pile.radius, remaining_fraction(&food, pile));
        if sprite.custom_size != Some(size) {
            sprite.custom_size = Some(size);
        }
        if sprite.color != color {
            sprite.color = color;
        }
    }
}

/// How much of a pile is left, in `0.0..=1.0`.
fn remaining_fraction(food: &FoodField, pile: &FoodPile) -> f32 {
    if pile.initial > 0.0 {
        (food.pile_remaining(pile) / pile.initial).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Sprite size and colour for a pile with `fraction` of its food left.
///
/// The radius follows `sqrt(fraction)` because the sprite stands for a heap
/// spread over an area: halve the food, and the disc that holds it shrinks by
/// the square root, not by half. That also makes the *last* few percent
/// dramatic, which is the part worth seeing - a pile going from 4% to 0%
/// visibly collapses.
///
/// A pile that is gone is drawn at zero size rather than left as a small stub.
/// The stub was worse than it sounds: `FoodField::piles` keeps exhausted piles
/// for the player to see and erase, so without this the map slowly fills with
/// permanent coloured dots marking food that no longer exists.
fn food_appearance(radius: f32, fraction: f32) -> (Vec2, Color) {
    if fraction <= 0.005 {
        return (Vec2::ZERO, Color::NONE);
    }
    let scale = fraction.sqrt();
    let mix = |a: f32, b: f32| a + (b - a) * fraction;
    (
        Vec2::splat(radius * 2.0 * scale),
        Color::srgb(
            mix(FOOD_EMPTY[0], FOOD_FULL[0]),
            mix(FOOD_EMPTY[1], FOOD_FULL[1]),
            mix(FOOD_EMPTY[2], FOOD_FULL[2]),
        ),
    )
}

/// Keep one sprite per nest, coloured by colony.
fn sync_nest_sprites(
    mut commands: Commands,
    nests: Res<Nests>,
    textures: Res<VisualTextures>,
    existing: Query<Entity, With<NestVisual>>,
) {
    if existing.iter().count() == nests.len() {
        return;
    }
    for entity in existing.iter() {
        commands.entity(entity).despawn();
    }
    for (index, nest) in nests.0.iter().enumerate() {
        commands.spawn((
            NestVisual,
            Sprite {
                image: textures.nest.clone(),
                color: NEST_COLORS[index.min(1)],
                custom_size: Some(Vec2::splat(nest.radius * 2.0)),
                ..default()
            },
            Transform::from_xyz(nest.x, nest.y, 2.0),
        ));
    }
}

/// Keep one exit cone per nest in directed-jet mode, none in disk mode.
///
/// The cone is an overlay *under* the nest sprite (z = 1.5 vs 2.0): it slides
/// out from beneath the mound rim, which reads as an opening rather than a
/// sticker on top. Rebuilt whenever the mode, the jet geometry, the nest
/// count, or the nest layout changes - a restart always re-inserts `Nests`,
/// so the layout case is covered by change detection rather than a stored
/// revision that could go stale.
fn sync_exit_sprites(
    mut commands: Commands,
    config: Res<SimConfig>,
    nests: Res<Nests>,
    mut images: ResMut<Assets<Image>>,
    existing: Query<Entity, With<NestExitVisual>>,
    mut cache: Local<Option<(SpawnMode, u32, u32, usize)>>,
) {
    // Geometry quantised to 0.1° so a slider drag rebuilds the cone only when
    // a digit actually moves, not on every pixel.
    let wanted = (
        config.spawn_mode,
        (config.spawn_direction * 10.0).round() as u32,
        (config.spawn_spread * 10.0).round() as u32,
        nests.len(),
    );
    if !nests.is_changed() && *cache == Some(wanted) {
        return;
    }
    *cache = Some(wanted);

    for entity in existing.iter() {
        commands.entity(entity).despawn();
    }
    if config.spawn_mode != SpawnMode::Jet {
        return;
    }
    let spread = config.spawn_spread;
    // One texture serves every nest: it is drawn pointing `+X` and the
    // sprite rotation carries the bearing, so only a spread change makes a
    // new shape worth rasterising.
    let texture = images.add(raster(64, 64, |nx, ny| nest_exit_shade(nx, ny, spread)));
    for (index, nest) in nests.0.iter().enumerate() {
        commands.spawn((
            NestExitVisual,
            Sprite {
                image: texture.clone(),
                color: NEST_COLORS[index.min(1)],
                // The texture's half-extent is 2.5 nest radii, so the cone
                // reaches well past the mound rim before its tip fades out.
                custom_size: Some(Vec2::splat(nest.radius * 5.0)),
                ..default()
            },
            Transform::from_xyz(nest.x, nest.y, 1.5)
                .with_rotation(Quat::from_rotation_z(config.spawn_direction.to_radians())),
        ));
    }
}

/// Rewrite the world texture: every active colony's pheromone, plus walls.
///
/// Skipped entirely when nothing that feeds it has changed - which is what
/// makes the "pause" button stop burning CPU on repaints.
fn paint_world(
    grid: Res<PheromoneGrid>,
    obstacles: Res<Obstacles>,
    view: Res<PheromoneView>,
    config: Res<SimConfig>,
    texture: Res<PheromoneTexture>,
    mut images: ResMut<Assets<Image>>,
    mut last_colonies: Local<usize>,
) {
    // The grid always has fields for the maximum number of colonies, so the
    // *active* count has to come from the configuration - and a change to it
    // must force a repaint even while the simulation is paused.
    let colonies = config.colonies();
    let colony_count_changed = *last_colonies != colonies;
    *last_colonies = colonies;

    if !grid.is_changed() && !obstacles.is_changed() && !view.is_changed() && !colony_count_changed
    {
        return;
    }

    if let Some(mut image) = images.get_mut(&texture.0) {
        if let Some(data) = image.data.as_mut() {
            paint_world_field(&grid, &obstacles, data, &view, colonies);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ant's nose must be brighter than its abdomen (the head is the
    /// bright end), and pixels far outside the body must be transparent.
    #[test]
    fn the_ant_body_has_a_head_and_fades_out_around_it() {
        let tail = ant_shade(-0.20, 0.0);
        let head = ant_shade(0.32, 0.0);
        assert!(
            head[0] > tail[0],
            "head is not brighter: {head:?} vs {tail:?}"
        );
        assert_eq!(head[3], 1.0, "the head is not opaque: {head:?}");
        let outside = ant_shade(0.0, 0.49);
        assert_eq!(outside[3], 0.0, "transparent pixel painted: {outside:?}");
    }

    /// The predator wedge must taper: wider at the rear than at the nose, so
    /// the sprite reads as "pointing" somewhere. The upper half is also lit
    /// (y > 0) while the lower half is shaded.
    #[test]
    fn the_predator_is_a_wedge_pointing_forward() {
        let rear = predator_shade(-0.30, 0.12);
        let nose = predator_shade(0.30, 0.12);
        assert!(rear[3] > 0.0, "rear not drawn: {rear:?}");
        assert_eq!(nose[3], 0.0, "nose side did not taper: {nose:?}");
        let lit = predator_shade(0.0, 0.08);
        let shaded = predator_shade(0.0, -0.08);
        assert!(
            lit[0] > shaded[0],
            "no top/bottom shading: {lit:?} {shaded:?}"
        );
        let outside = predator_shade(0.0, 0.45);
        assert_eq!(outside[3], 0.0, "transparent pixel painted: {outside:?}");
    }

    /// A nest is a mound: bright crown, transparent past the rim.
    #[test]
    fn the_nest_mound_is_bright_in_the_middle_and_fades_at_the_rim() {
        let crown = nest_shade(0.0, 0.0);
        let rim = nest_shade(0.0, 0.499);
        assert!(
            crown[0] > rim[0] + 0.2,
            "mound not graded: {crown:?} {rim:?}"
        );
        // The rim must have all but faded; the very last pixel row blends out,
        // which is what makes the mound soft.
        assert!(rim[3] < 0.02, "rim not faded: {rim:?}");
    }

    /// A food pile fades out at its rim: opaque centre, transparent edge.
    #[test]
    fn the_food_pile_is_a_soft_disc() {
        let centre = food_shade(0.0, 0.0);
        let edge = food_shade(0.0, 0.499);
        assert!(centre[3] > 0.9, "centre not opaque: {centre:?}");
        assert!(edge[3] < 0.01, "edge not transparent: {edge:?}");
        assert!(centre[0] > edge[0], "heap not brighter at the centre");
    }

    /// A scene rebuild must keep the same camera entity: bevy_egui attaches
    /// its primary egui context (`EguiContext` + `PrimaryEguiContext`) to the
    /// first camera it ever sees and never hands out a second one. Despawning
    /// the camera during a map resize therefore removed the control panel for
    /// the rest of the session - triggered by `long-haul`, the only built-in
    /// scenario that changes the map size.
    #[test]
    fn a_scene_rebuild_keeps_the_camera_and_its_egui_context() {
        use bevy::asset::Assets;

        // Stands in for bevy_egui's `PrimaryEguiContext`: whatever the plugin
        // attached to the first camera must be found on the camera afterwards.
        #[derive(Component)]
        struct EguiContextProxy;

        let mut world = World::new();
        world.insert_resource(SimConfig::default());
        world.insert_resource(Assets::<Image>::default());
        world.insert_resource(SceneGeometry::default());

        rebuild_scene_if_needed(&mut world);
        let camera = {
            let mut query = world.query_filtered::<Entity, With<Camera2d>>();
            query
                .iter(&world)
                .next()
                .expect("first build spawns a camera")
        };
        world.entity_mut(camera).insert(EguiContextProxy);

        // Resize the map the way a scenario load does.
        world.resource_mut::<SimConfig>().width = 512;
        world.resource_mut::<SimConfig>().height = 512;
        rebuild_scene_if_needed(&mut world);

        let cameras: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<Camera2d>>();
            query.iter(&world).collect()
        };
        assert_eq!(cameras, vec![camera], "the rebuild respawned the camera");
        assert!(
            world
                .get_entity(camera)
                .expect("camera exists")
                .contains::<EguiContextProxy>(),
            "the camera's egui context was lost on resize"
        );
        assert_eq!(
            *world.resource::<SceneGeometry>(),
            SceneGeometry {
                width: 512,
                height: 512
            }
        );
    }

    /// The jet-exit overlay is a soft fan: bright on the bearing, nothing
    /// beside it, a halo at 360°, and faded out before the sprite edge.
    #[test]
    fn the_jet_cone_is_a_soft_fan_on_the_bearing() {
        let on_axis = nest_exit_shade(0.30, 0.0, 60.0);
        assert!(on_axis[3] > 0.55, "axis alpha {:?}", on_axis);
        assert_eq!(on_axis[..3], [1.0, 1.0, 1.0], "greyscale tint target");

        // Half-cone at 60° spread is 30°; a point 90° off the bearing is out.
        let off_axis = nest_exit_shade(0.0, 0.30, 60.0);
        assert_eq!(off_axis[3], 0.0, "off-axis alpha {:?}", off_axis);

        // 360° covers every bearing, seam-free.
        let full = nest_exit_shade(0.0, 0.30, 360.0);
        assert!(full[3] > 0.55, "a 360° cone must cover off-axis points");

        // The tip fades before the sprite edge, so the cone never draws a
        // hard square rim.
        let tip = nest_exit_shade(0.48, 0.0, 60.0);
        assert!(tip[3] < 0.06, "tip alpha {:?}", tip);
    }

    /// The fitted view (zoom 1) is exactly the pre-pan framing - and any pan
    /// offset left in the resource is ignored - so every captured figure is
    /// unaffected by the pan feature.
    #[test]
    fn the_fitted_view_ignores_leftover_pan() {
        let (placement, scale) = camera_placement(
            256.0,
            256.0,
            Vec2::new(1600.0, 900.0),
            330.0,
            1.0,
            Vec2::new(500.0, -80.0),
        );
        let base = 900.0 / 266.0; // px per world unit at zoom 1
        assert_eq!(scale, 1.0);
        assert_eq!(placement.x, 128.0 - (330.0 / base) * 0.5);
        assert_eq!(placement.y, 128.0);
        assert_eq!(placement.z, 1_000.0);
    }

    /// Zoom rides the orthographic scale and the camera slides to the panned
    /// spot, clamped inside the map.
    #[test]
    fn zoom_and_pan_reach_the_camera() {
        let (placement, scale) = camera_placement(
            256.0,
            256.0,
            Vec2::new(1600.0, 900.0),
            330.0,
            4.0,
            Vec2::new(500.0, 0.0),
        );
        assert_eq!(scale, 0.25);
        // Slack at zoom 4 is 0.5 * (1 - 1/4) of the map per axis = ±96.
        let base = 900.0 / 266.0;
        assert_eq!(placement.x, 128.0 - (330.0 / (base * 4.0)) * 0.5 + 96.0);
        assert_eq!(placement.y, 128.0);
    }

    /// At the fitted zoom the pan slack is zero: panning a fully visible map
    /// is a no-op, whatever the player dragged.
    #[test]
    fn the_pan_clamp_shuts_at_the_fitted_zoom() {
        assert_eq!(
            clamp_pan(Vec2::new(500.0, 500.0), 256.0, 256.0, 1.0),
            Vec2::ZERO
        );
        // ...and opens up again as the player zooms in.
        let clamped = clamp_pan(Vec2::new(500.0, 500.0), 256.0, 256.0, 4.0);
        assert_eq!(clamped, Vec2::new(96.0, 96.0));
    }

    /// Cursor-anchored zoom: the world point under the cursor stays there,
    /// which is what makes the wheel feel like grabbing the map.
    #[test]
    fn zooming_at_the_cursor_keeps_the_point_under_it() {
        // Camera centre C, cursor at world offset R: W = C + R.
        let fitted = Vec2::new(118.0, 123.0);
        let pan = Vec2::new(10.0, 5.0);
        let cursor_offset = Vec2::new(40.0, -20.0);
        let under_before = fitted + pan + cursor_offset;

        let pan = zoom_anchor_pan(pan, cursor_offset, 2.0, 4.0);
        // After zooming by k, the cursor ray hits fitted + pan + R/k.
        let under_after = fitted + pan + cursor_offset / 2.0;
        assert_eq!(under_after, under_before);
    }

    fn pile(initial: f32) -> FoodPile {
        FoodPile {
            x: 16.0,
            y: 16.0,
            radius: 6.0,
            initial,
        }
    }

    #[test]
    fn a_full_pile_is_drawn_at_its_nominal_size_and_colour() {
        let (size, color) = food_appearance(6.0, 1.0);
        assert_eq!(size, Vec2::splat(12.0));
        assert_eq!(color, Color::srgb(FOOD_FULL[0], FOOD_FULL[1], FOOD_FULL[2]));
    }

    /// The pile must actually get smaller as it is carried away - the whole
    /// point of the feature.
    #[test]
    fn a_pile_shrinks_as_it_is_eaten() {
        // Walk the pile *down* from full to nearly gone, which is the direction
        // a real run moves in, and require the drawn size to fall with it.
        let mut previous = f32::INFINITY;
        for step in (1..=200).rev() {
            let fraction = step as f32 / 200.0;
            let (size, _) = food_appearance(6.0, fraction);
            assert!(
                size.x < previous + 1e-4,
                "the pile grew as it was eaten: {previous} -> {} at {fraction}",
                size.x
            );
            previous = size.x;
        }
        let (half, _) = food_appearance(6.0, 0.5);
        assert!(
            (half.x - 12.0 * 0.5f32.sqrt()).abs() < 1e-4,
            "half a pile should be sqrt(0.5) of the radius, got {}",
            half.x
        );
        // The last few percent are the most visible part of the shrink: a 4%
        // pile is under a quarter of the original across.
        let (sliver, _) = food_appearance(6.0, 0.04);
        assert!(
            sliver.x < 12.0 * 0.25,
            "a 4% pile is still {} across",
            sliver.x
        );
    }

    /// An exhausted pile must vanish. Leaving a stub behind litters the map,
    /// because `FoodField` keeps eaten piles in its list.
    #[test]
    fn an_exhausted_pile_is_not_drawn() {
        let (size, color) = food_appearance(6.0, 0.0);
        assert_eq!(size, Vec2::ZERO, "an eaten pile still occupies space");
        assert_eq!(color.alpha(), 0.0, "an eaten pile is still opaque");
        assert_eq!(color, Color::NONE);
    }

    /// Dimming, not a hue shift: red is the predator colour, so a nearly
    /// exhausted pile must still read as food.
    #[test]
    fn a_depleted_pile_stays_green() {
        // From just above the vanish threshold: at or below it the pile is
        // deliberately not drawn at all.
        for fraction in [0.006, 0.01, 0.05, 0.2, 0.5, 0.9] {
            let (_, color) = food_appearance(6.0, fraction);
            let c = color.to_srgba();
            assert!(
                c.green > c.red && c.green > c.blue,
                "depleted food at {fraction} turned red or blue: {c:?}"
            );
            assert!(c.alpha() > 0.5, "visible pile is transparent: {c:?}");
        }
        // Darker as it empties, which is the "picked over" cue.
        let bright = food_appearance(6.0, 1.0).1.to_srgba();
        let dim = food_appearance(6.0, 0.02).1.to_srgba();
        assert!(dim.green < bright.green);
        assert!(dim.red < bright.red);
    }

    #[test]
    fn remaining_fraction_reads_the_field_not_the_book_keeping() {
        let field = FoodField::from_piles(64, 64, vec![pile(200.0)]);
        assert!((remaining_fraction(&field, &field.piles[0]) - 1.0).abs() < 1e-5);

        let mut field = field;
        // A pile whose `initial` is zero must not divide by zero.
        let empty = pile(0.0);
        assert_eq!(remaining_fraction(&field, &empty), 0.0);

        // Eat half the pile. `take` draws from one cell, so the whole disc has
        // to be visited - draining a single cell only reaches ~2% of a pile
        // this size.
        let target = field.remaining_total / 2.0;
        let mut eaten = 0.0;
        'disc: for dy in -7..=7 {
            for dx in -7..=7 {
                loop {
                    if eaten >= target {
                        break 'disc;
                    }
                    let got = field.take(16.0 + dx as f32, 16.0 + dy as f32, 0.5) as f64;
                    if got <= 0.0 {
                        break;
                    }
                    eaten += got;
                }
            }
        }
        assert!((eaten - target).abs() < 1.0, "only ate {eaten} of {target}");
        let fraction = remaining_fraction(&field, &field.piles[0]);
        assert!(
            (0.45..0.55).contains(&fraction),
            "half-eaten pile reports {fraction}"
        );
    }
}
