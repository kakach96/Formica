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

/// Side length of an ant sprite, in world cells.
const ANT_SIZE: f32 = 2.2;
/// Side length of a predator sprite, in world cells.
const PREDATOR_SIZE: f32 = 5.5;

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

/// Requests from the UI that need `&mut World` to service.
#[derive(Resource, Debug, Clone, Default)]
pub struct SimCommands {
    /// Rebuild the world from the current configuration: fresh food, empty
    /// trails, zeroed statistics, new colonies on the nests.
    pub restart: bool,
    /// Zero every pheromone field but leave everything else alone. Useful for
    /// watching a converged colony re-converge from scratch.
    pub clear_pheromones: bool,
}

/// Installs camera, sprites and the per-frame presentation sync.
pub struct FormicaRenderPlugin;

impl Plugin for FormicaRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PheromoneView>()
            .init_resource::<SimCommands>()
            .init_resource::<SceneGeometry>()
            .add_systems(
                // Presentation sync runs in `PostUpdate` so it always observes a
                // fully-ticked world, whatever happens in `Update`.
                PostUpdate,
                (
                    // Creates the camera and the world texture, and rebuilds
                    // both whenever the map is resized.
                    rebuild_scene_if_needed,
                    attach_ant_sprites,
                    sync_ant_sprites,
                    attach_predator_sprites,
                    sync_predator_sprites,
                    sync_food_sprites,
                    sync_nest_sprites,
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

    // Tear down anything positioned in the old coordinate system.
    let stale: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, Or<(
            With<PheromoneLayer>,
            With<Camera2d>,
            With<FoodVisual>,
            With<NestVisual>,
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
    world.spawn((
        Camera2d,
        Projection::Orthographic(projection),
        Transform::from_xyz(fw * 0.5, fh * 0.5, 1_000.0),
    ));

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
    info!("scene rebuilt for a {width}x{height} map");
}

/// Give every ant entity a `Sprite` and a `Transform`.
///
/// Ants are spawned by the simulation, which knows nothing about rendering, so
/// the view components are attached here on first sight. A freshly reset or
/// resized colony is picked up automatically on the next frame.
fn attach_ant_sprites(
    mut commands: Commands,
    ants: Query<Entity, (With<AntMarker>, Without<AntVisual>)>,
) {
    for entity in ants.iter() {
        commands.entity(entity).insert((
            AntVisual,
            Sprite::from_color(SEARCHING_COLORS[0], Vec2::splat(ANT_SIZE)),
            // Above the pheromone field, food and nests.
            Transform::from_xyz(0.0, 0.0, 3.0),
        ));
    }
}

/// Mirror ant state onto the sprite components.
fn sync_ant_sprites(
    mut ants: Query<(&AntColony, &AntPos, &AntState, &mut Transform, &mut Sprite), With<AntVisual>>,
) {
    for (colony, pos, state, mut transform, mut sprite) in ants.iter_mut() {
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
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
) {
    for entity in predators.iter() {
        commands.entity(entity).insert((
            PredatorVisual,
            Sprite::from_color(PREDATOR_CALM, Vec2::splat(PREDATOR_SIZE)),
            Transform::from_xyz(0.0, 0.0, 4.0),
        ));
    }
}

/// Mirror predator state onto the sprite components.
fn sync_predator_sprites(
    mut predators: Query<
        (&PredatorPos, &PredatorAlert, &mut Transform, &mut Sprite),
        With<PredatorVisual>,
    >,
) {
    for (pos, alert, mut transform, mut sprite) in predators.iter_mut() {
        transform.translation.x = pos.x;
        transform.translation.y = pos.y;
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
                Sprite::from_color(color, size),
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
            Sprite::from_color(NEST_COLORS[index.min(1)], Vec2::splat(nest.radius * 2.0)),
            Transform::from_xyz(nest.x, nest.y, 2.0),
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
