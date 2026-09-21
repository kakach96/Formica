//! Static obstacles.
//!
//! An obstacle is a per-cell flag. Ants cannot enter a blocked cell, cannot lay
//! pheromone in one, and any pheromone that was there before a wall appeared is
//! erased (see [`Obstacles::revision`]).
//!
//! This is the piece that makes the colony's behaviour *legible* rather than
//! merely impressive. On an empty map, "the ants found the food" can always be
//! explained by luck or by the food simply being close. Put the food behind a
//! U-shaped wall and the interesting claims become testable: the first ants to
//! arrive are the ones that happened to go round, the trail that survives is
//! the one that goes round, and the straight-line trail that dead-ends at the
//! wall evaporates away.

use bevy_ecs::prelude::*;

use crate::config::{ObstaclePreset, SimConfig};
use crate::rng::SimRng;

/// Per-cell blocked flags, plus a revision counter for cheap change detection.
#[derive(Resource, Debug, Clone)]
pub struct Obstacles {
    /// Grid width in cells.
    pub width: usize,
    /// Grid height in cells.
    pub height: usize,
    blocked: Vec<bool>,
    /// The preset currently rasterised into `blocked`.
    pub built: ObstaclePreset,
    /// Bumped on every mutation, so the renderer and the pheromone scrubber can
    /// skip work when nothing changed.
    pub revision: u64,
}

impl Obstacles {
    /// An empty map.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            blocked: vec![false; width * height],
            built: ObstaclePreset::None,
            revision: 0,
        }
    }

    /// True when a cell is a wall **or** outside the map.
    ///
    /// Treating out-of-bounds as blocked makes this usable as a single
    /// "can an ant stand here?" test at every call site.
    #[inline]
    pub fn is_blocked(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return true;
        }
        self.blocked[y as usize * self.width + x as usize]
    }

    /// [`Self::is_blocked`] from continuous world coordinates.
    #[inline]
    pub fn is_blocked_at(&self, x: f32, y: f32) -> bool {
        self.is_blocked(x as i32, y as i32)
    }

    /// Set or clear one cell.
    pub fn set(&mut self, x: i32, y: i32, blocked: bool) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return;
        }
        let idx = y as usize * self.width + x as usize;
        if self.blocked[idx] != blocked {
            self.blocked[idx] = blocked;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Number of blocked cells.
    pub fn count(&self) -> usize {
        self.blocked.iter().filter(|b| **b).count()
    }

    /// Fraction of the map that is wall.
    pub fn coverage(&self) -> f32 {
        let cells = self.width * self.height;
        if cells == 0 {
            0.0
        } else {
            self.count() as f32 / cells as f32
        }
    }

    /// Clear every cell.
    pub fn clear(&mut self) {
        if self.blocked.iter().any(|b| *b) {
            self.blocked.fill(false);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Reallocate for a new map size, discarding the shape.
    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.blocked = vec![false; width * height];
        self.built = ObstaclePreset::None;
        self.revision = self.revision.wrapping_add(1);
    }

    /// Rebuild the shape from a preset.
    ///
    /// `nests` and `piles` are the **actual** world contents, passed in rather
    /// than regenerated. An earlier version re-derived the pile positions from
    /// the seed inside this function, with a differently-mixed RNG stream - so
    /// the U-wall was stamped around where a pile *would* have been under a
    /// different seed. Nothing errored; the wall simply appeared in an empty
    /// part of the map, wrapping nothing.
    pub fn apply_preset(
        &mut self,
        preset: ObstaclePreset,
        config: &SimConfig,
        nests: &[(f32, f32)],
        piles: &[crate::world::FoodPile],
    ) {
        self.clear();
        match preset {
            ObstaclePreset::None => {}
            ObstaclePreset::UWall {
                arm,
                gap,
                thickness,
            } => {
                // Wrap the first food pile, with the opening pointing *away*
                // from the colony and the solid back facing it. Ants leaving
                // the nest therefore walk into a wall and have to go round.
                if let (Some(pile), Some(&(nx, ny))) = (piles.first(), nests.first()) {
                    let opening = (pile.y - ny).atan2(pile.x - nx);
                    self.add_u_wall(
                        (pile.x, pile.y),
                        opening,
                        arm.max(pile.radius + 2.0),
                        gap.max(pile.radius * 2.0 + 2.0),
                        thickness.max(1.0),
                    );
                }
            }
            ObstaclePreset::RandomBlocks { density } => {
                let mut rng = SimRng::new(config.seed ^ 0x5EED_0B57);
                // Keep the nests and their immediate surroundings clear, or the
                // colony starts entombed.
                for y in 0..self.height {
                    for x in 0..self.width {
                        if rng.unit() >= density {
                            continue;
                        }
                        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                        let near_nest = nests.iter().any(|(nx, ny)| {
                            (px - nx).powi(2) + (py - ny).powi(2)
                                <= (config.nest_radius + 6.0).powi(2)
                        });
                        if !near_nest {
                            self.set(x as i32, y as i32, true);
                        }
                    }
                }
            }
        }
        self.built = preset;
    }

    /// Stamp a U-shaped wall whose **interior contains `interior_center`**.
    ///
    /// The caller passes the point it wants *inside* the U - normally a food
    /// pile - and the opening direction. The geometry is worked out in the
    /// wall's own frame with the opening along local `+X`:
    ///
    /// ```text
    ///         back                mouth
    ///   lx = -arm ................ lx = 0 ....... > +lx (opening)
    ///            +---------------+          ^
    ///            |               |          | ly = +gap/2   (arm)
    ///            |   interior    |
    ///            |  (food here)  |          | ly = -gap/2   (arm)
    ///            +---------------+          v
    /// ```
    ///
    /// An earlier version placed both arms at `ly = 0`, which produced a single
    /// thick bar down the middle rather than a U - the "interior" was open to
    /// the sides and the shape did not enclose anything. The arms belong at
    /// `ly = +/- gap/2`.
    pub fn add_u_wall(
        &mut self,
        interior_center: (f32, f32),
        opening_angle: f32,
        arm: f32,
        gap: f32,
        thickness: f32,
    ) {
        let (sin, cos) = opening_angle.sin_cos();
        // The local origin sits half an arm further along the opening than the
        // interior centre, so that lx = -arm/2 lands on the interior centre and
        // the caller's point really is inside the U.
        let origin = (
            interior_center.0 + cos * arm * 0.5,
            interior_center.1 + sin * arm * 0.5,
        );
        let half_thickness = thickness * 0.5;
        let half_gap = gap * 0.5;

        for y in 0..self.height {
            for x in 0..self.width {
                let px = x as f32 + 0.5 - origin.0;
                let py = y as f32 + 0.5 - origin.1;
                // Rotate the world offset into the wall's local frame.
                let lx = px * cos + py * sin;
                let ly = -px * sin + py * cos;

                // The two arms run from the closed back (lx = -arm) up to the
                // mouth (lx = 0), at ly = +/- gap/2.
                let on_arm = (ly.abs() - half_gap).abs() <= half_thickness
                    && (-arm..=0.0).contains(&lx);
                // The back caps the far end.
                let on_back =
                    (lx + arm).abs() <= half_thickness && ly.abs() <= half_gap + half_thickness;

                if on_arm || on_back {
                    self.set(x as i32, y as i32, true);
                }
            }
        }
    }
}

/// Rebuild the obstacle map whenever the configured preset changes.
///
/// Comparing the preset value (which *carries* its geometry parameters) means a
/// UI slider for arm length or density causes a rebuild for free, with no
/// explicit "apply" plumbing.
pub fn sync_obstacles(
    config: Res<SimConfig>,
    nests: Res<crate::world::Nests>,
    food: Res<crate::world::FoodField>,
    mut obstacles: ResMut<Obstacles>,
) {
    if obstacles.built == config.obstacle_preset
        && obstacles.width == config.width
        && obstacles.height == config.height
    {
        return;
    }
    if obstacles.width != config.width || obstacles.height != config.height {
        obstacles.resize(config.width, config.height);
    }
    obstacles.apply_preset(config.obstacle_preset, &config, &nests.centers(), &food.piles);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_map_is_empty_and_bounds_are_blocked() {
        let obstacles = Obstacles::new(16, 16);
        assert_eq!(obstacles.count(), 0);
        assert!(!obstacles.is_blocked(5, 5));
        // Outside the map counts as blocked so callers get one predicate.
        assert!(obstacles.is_blocked(-1, 5));
        assert!(obstacles.is_blocked(16, 5));
        assert!(obstacles.is_blocked(5, -1));
        assert!(obstacles.is_blocked(5, 16));
    }

    #[test]
    fn revision_only_moves_on_a_real_change() {
        let mut obstacles = Obstacles::new(8, 8);
        let start = obstacles.revision;
        obstacles.set(2, 2, true);
        assert_eq!(obstacles.revision, start + 1);
        // Setting the same value again is not a change.
        obstacles.set(2, 2, true);
        assert_eq!(obstacles.revision, start + 1);
        obstacles.set(2, 2, false);
        assert_eq!(obstacles.revision, start + 2);
        // Out-of-bounds writes are dropped.
        obstacles.set(-1, 0, true);
        assert_eq!(obstacles.revision, start + 2);
    }

    #[test]
    fn out_of_bounds_writes_are_dropped() {
        let mut obstacles = Obstacles::new(8, 8);
        obstacles.set(100, 100, true);
        obstacles.set(-3, 2, true);
        assert_eq!(obstacles.count(), 0);
    }

    #[test]
    fn resize_reallocates_and_clears() {
        let mut obstacles = Obstacles::new(8, 8);
        obstacles.set(1, 1, true);
        obstacles.resize(16, 12);
        assert_eq!(obstacles.width, 16);
        assert_eq!(obstacles.height, 12);
        assert_eq!(obstacles.count(), 0);
        assert!(!obstacles.is_blocked(1, 1));
    }

    /// Map a point in the wall's local frame to the world cell it lands in.
    ///
    /// Tests reason in local coordinates (`lx` along the opening, `ly` across
    /// it) because that is how the shape is defined; guessing world cells is
    /// what let the arm-placement bug slip through the first time.
    fn wall_cell(
        interior: (f32, f32),
        opening: f32,
        arm: f32,
        lx: f32,
        ly: f32,
    ) -> (i32, i32) {
        let (sin, cos) = opening.sin_cos();
        let origin = (interior.0 + cos * arm * 0.5, interior.1 + sin * arm * 0.5);
        let wx = origin.0 + lx * cos - ly * sin;
        let wy = origin.1 + lx * sin + ly * cos;
        (wx as i32, wy as i32)
    }

    /// Is any cell within `radius` of this cell a wall?
    ///
    /// A radius search rather than an exact lookup, because a rotated wall's
    /// arm does not land on a predictable cell - what matters is that wall
    /// exists *there*, not that it occupies one particular index.
    fn any_blocked_near(obstacles: &Obstacles, cell: (i32, i32), radius: i32) -> bool {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if obstacles.is_blocked(cell.0 + dx, cell.1 + dy) {
                    return true;
                }
            }
        }
        false
    }

    fn assert_u_wall_is_well_formed(opening: f32) {
        let interior = (32.0, 32.0);
        let (arm, gap, thickness) = (14.0, 14.0, 3.0);
        let mut obstacles = Obstacles::new(64, 64);
        obstacles.add_u_wall(interior, opening, arm, gap, thickness);
        assert!(obstacles.count() > 0, "no wall was built");

        let at = |lx: f32, ly: f32| wall_cell(interior, opening, arm, lx, ly);

        // 1. The caller's point is genuinely inside the U.
        assert!(
            !obstacles.is_blocked(interior.0 as i32, interior.1 as i32),
            "the interior centre was walled in"
        );
        // 2. The back, at the far end of the interior, is solid.
        assert!(
            any_blocked_near(&obstacles, at(-arm, 0.0), 2),
            "the back wall is missing"
        );
        // 3. The mouth is open: nothing blocks the path out of the U.
        for lx in [-2.0f32, 0.0, 2.0, 5.0] {
            assert!(
                !any_blocked_near(&obstacles, at(lx, 0.0), 1),
                "the mouth is blocked at lx = {lx}"
            );
        }
        // 4. Both arms are solid, midway along their length.
        for side in [1.0f32, -1.0] {
            assert!(
                any_blocked_near(&obstacles, at(-arm * 0.5, side * gap * 0.5), 2),
                "the arm at ly = {} is missing",
                side * gap * 0.5
            );
        }
        // 5. Between the arms is a corridor, not wall - the bug being fixed
        //    here put a bar straight down the middle.
        for lx in [-arm * 0.3f32, -arm * 0.7] {
            assert!(
                !any_blocked_near(&obstacles, at(lx, 0.0), 1),
                "the interior is blocked at lx = {lx}: this is a bar, not a U"
            );
        }
    }

    #[test]
    fn a_u_wall_opens_toward_positive_x() {
        assert_u_wall_is_well_formed(0.0);
    }

    #[test]
    fn a_rotated_u_wall_keeps_its_shape() {
        use std::f32::consts::{FRAC_PI_2, PI};
        assert_u_wall_is_well_formed(FRAC_PI_2);
        assert_u_wall_is_well_formed(PI);
        assert_u_wall_is_well_formed(-FRAC_PI_2);
        assert_u_wall_is_well_formed(0.9);
    }

    #[test]
    fn a_larger_gap_makes_a_roomier_u() {
        let mut narrow = Obstacles::new(64, 64);
        narrow.add_u_wall((32.0, 32.0), 0.0, 12.0, 6.0, 2.0);
        let mut wide = Obstacles::new(64, 64);
        wide.add_u_wall((32.0, 32.0), 0.0, 12.0, 24.0, 2.0);
        assert!(
            wide.count() > narrow.count(),
            "a wider gap should need more wall"
        );
    }

    /// The real food layout, generated exactly the way `init_simulation` does.
    ///
    /// The wall test must use this and not its own generator: an earlier
    /// version re-derived the piles with a differently-mixed RNG stream, so the
    /// wall was stamped around a pile position that did not exist and the test
    /// still passed - it was comparing the wall against the same phantom pile
    /// the wall was built from.
    fn real_food(config: &SimConfig) -> crate::world::FoodField {
        let mut rng = SimRng::new(config.seed);
        crate::world::FoodField::generate(config, &mut rng)
    }

    #[test]
    fn the_u_wall_preset_forces_a_detour_to_the_first_pile() {
        let config = SimConfig::default();
        let nests = crate::world::Nests::from_config(&config);
        let food = real_food(&config);
        let pile = food.piles[0];
        let nest = nests.get(0);

        let mut obstacles = Obstacles::new(config.width, config.height);
        obstacles.apply_preset(
            ObstaclePreset::UWall {
                arm: 14.0,
                gap: 14.0,
                thickness: 3.0,
            },
            &config,
            &nests.centers(),
            &food.piles,
        );
        assert!(obstacles.count() > 0, "no wall was built");

        // The food stays reachable, and the nest is not entombed.
        assert!(
            !obstacles.is_blocked(pile.x as i32, pile.y as i32),
            "the food was walled in completely"
        );
        assert!(
            !obstacles.is_blocked(nest.x as i32, nest.y as i32),
            "the nest was entombed"
        );

        // The wall must actually surround the pile it was asked to wrap: some
        // wall must exist within a short distance of it. This is the assertion
        // that the phantom-pile bug would have failed.
        let mut near_the_pile = 0;
        let reach = 24i32;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                if obstacles.is_blocked(pile.x as i32 + dx, pile.y as i32 + dy) {
                    near_the_pile += 1;
                }
            }
        }
        assert!(
            near_the_pile > 0,
            "the U wall was not built around the first pile"
        );

        // And the straight nest -> pile path is interrupted.
        let mut blocked_on_the_line = 0;
        for i in 1..40 {
            let t = i as f32 / 40.0;
            let px = nest.x + (pile.x - nest.x) * t;
            let py = nest.y + (pile.y - nest.y) * t;
            // Cells inside the pile itself don't count - we are looking for
            // wall *between* the nest and the food.
            if (px - pile.x).hypot(py - pile.y) <= pile.radius {
                continue;
            }
            if obstacles.is_blocked(px as i32, py as i32) {
                blocked_on_the_line += 1;
            }
        }
        assert!(
            blocked_on_the_line > 0,
            "the U wall does not block the straight path from the nest to the food"
        );
    }

    #[test]
    fn random_blocks_never_entomb_a_nest() {
        let config = SimConfig::default();
        let nests = crate::world::Nests::from_config(&config);
        let food = real_food(&config);
        let mut obstacles = Obstacles::new(config.width, config.height);
        obstacles.apply_preset(
            ObstaclePreset::RandomBlocks { density: 0.5 },
            &config,
            &nests.centers(),
            &food.piles,
        );

        for nest in &nests.0 {
            assert!(
                !obstacles.is_blocked(nest.x as i32, nest.y as i32),
                "a nest was entombed"
            );
        }
        assert!(obstacles.coverage() > 0.1, "density 0.5 produced almost nothing");
        assert!(obstacles.coverage() < 0.9);
    }

    #[test]
    fn rebuilding_the_same_preset_is_deterministic() {
        let config = SimConfig::default();
        let nests = crate::world::Nests::from_config(&config);
        let food = real_food(&config);
        let centers = nests.centers();

        let mut a = Obstacles::new(config.width, config.height);
        let mut b = Obstacles::new(config.width, config.height);
        let preset = ObstaclePreset::RandomBlocks { density: 0.2 };
        a.apply_preset(preset, &config, &centers, &food.piles);
        b.apply_preset(preset, &config, &centers, &food.piles);
        assert_eq!(a.count(), b.count());
        for i in 0..a.blocked.len() {
            assert_eq!(a.blocked[i], b.blocked[i], "cell {i} differs");
        }
    }

    /// The U-wall must land on the pile even when the food layout moves, which
    /// is what a different seed produces.
    #[test]
    fn the_u_wall_follows_the_piles_when_the_seed_changes() {
        for seed in [1u64, 7, 99, 20_240_517] {
            let config = SimConfig {
                seed,
                ..SimConfig::default()
            };
            let nests = crate::world::Nests::from_config(&config);
            let food = real_food(&config);
            let pile = food.piles[0];

            let mut obstacles = Obstacles::new(config.width, config.height);
            obstacles.apply_preset(
                ObstaclePreset::UWall {
                    arm: 14.0,
                    gap: 16.0,
                    thickness: 3.0,
                },
                &config,
                &nests.centers(),
                &food.piles,
            );

            let mut near_the_pile = 0;
            for dy in -24i32..=24 {
                for dx in -24i32..=24 {
                    if obstacles.is_blocked(pile.x as i32 + dx, pile.y as i32 + dy) {
                        near_the_pile += 1;
                    }
                }
            }
            assert!(
                near_the_pile > 0,
                "seed {seed}: the wall missed the pile at ({}, {})",
                pile.x,
                pile.y
            );
        }
    }
}
