//! Static world features: the nests and the (finite, editable) food piles.

use bevy_ecs::prelude::*;

use crate::config::SimConfig;
use crate::rng::SimRng;

/// One colony nest.
#[derive(Debug, Clone, Copy)]
pub struct Nest {
    /// Centre x in cells.
    pub x: f32,
    /// Centre y in cells.
    pub y: f32,
    /// An ant within this radius of the centre is "home".
    pub radius: f32,
}

impl Nest {
    /// Squared distance from a point to the nest centre.
    #[inline]
    pub fn distance_sq(&self, x: f32, y: f32) -> f32 {
        let dx = x - self.x;
        let dy = y - self.y;
        dx * dx + dy * dy
    }

    /// True when a point is inside the nest.
    #[inline]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.distance_sq(x, y) <= self.radius * self.radius
    }
}

/// Every colony's nest.
///
/// With one colony this is the map centre (or the configured `nest_pos`). With
/// two, the nests sit opposite each other on a ring around the map centre so
/// neither colony starts closer to the food ring.
#[derive(Resource, Debug, Clone)]
pub struct Nests(pub Vec<Nest>);

impl Nests {
    /// Lay out the nests described by `config`.
    pub fn from_config(config: &SimConfig) -> Self {
        let colonies = config.colonies();
        if colonies <= 1 {
            let (x, y) = config.nest_center();
            return Self(vec![Nest {
                x,
                y,
                radius: config.nest_radius,
            }]);
        }

        let (cx, cy) = config.map_center();
        let nests = (0..colonies)
            .map(|i| {
                let angle = std::f32::consts::TAU * i as f32 / colonies as f32;
                Nest {
                    x: cx + angle.cos() * config.nest_orbit,
                    y: cy + angle.sin() * config.nest_orbit,
                    radius: config.nest_radius,
                }
            })
            .collect();
        Self(nests)
    }

    /// The nest for a colony, clamping the index rather than panicking.
    #[inline]
    pub fn get(&self, colony: usize) -> &Nest {
        &self.0[colony.min(self.0.len().saturating_sub(1))]
    }

    /// Nest centres, in colony order.
    pub fn centers(&self) -> Vec<(f32, f32)> {
        self.0.iter().map(|n| (n.x, n.y)).collect()
    }

    /// Number of nests.
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when there are no nests at all (never true in practice).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One finite food pile.
#[derive(Debug, Clone, Copy)]
pub struct FoodPile {
    /// Centre x in cells.
    pub x: f32,
    /// Centre y in cells.
    pub y: f32,
    /// Radius in cells.
    pub radius: f32,
    /// Units present when the pile was created.
    pub initial: f32,
}

/// Per-cell food amounts plus the aggregate bookkeeping the stats need.
///
/// # The conservation law
///
/// ```text
/// initial_total == delivered + remaining_total + in_transit + removed_total
/// ```
///
/// `removed_total` exists because the player can delete a pile that still holds
/// food. That food was neither delivered nor left on the map, so without a
/// bucket for it the invariant would silently break and the regression test
/// that guards it would start lying.
#[derive(Resource, Debug, Clone)]
pub struct FoodField {
    /// Grid width in cells.
    pub width: usize,
    /// Grid height in cells.
    pub height: usize,
    /// Remaining food per cell, row-major.
    pub amount: Vec<f32>,
    /// The piles this field was built from (used by the renderer).
    pub piles: Vec<FoodPile>,
    /// Total food ever placed on this map.
    pub initial_total: f64,
    /// Total food still on the map (maintained incrementally, never summed).
    pub remaining_total: f64,
    /// Food destroyed by the player removing a pile.
    pub removed_total: f64,
    /// Bumped whenever the *set* of piles changes, so the renderer knows to
    /// rebuild its pile sprites rather than reusing stale indices.
    pub revision: u64,
}

impl FoodField {
    /// Scatter `config.food_pile_count` discs in a ring around the map centre.
    ///
    /// The layout is a deterministic function of `config.seed`, so a seed
    /// reproduces an experiment exactly - piles are spread over a ring with a
    /// little jitter rather than dropped fully at random, which keeps every
    /// run a fair comparison (no pile spawned on top of a nest, and with two
    /// colonies neither starts nearer the food).
    ///
    /// The ring radius is additionally clamped to what the map can hold. The
    /// defaults (70..115 cells) assume the 256x256 map from the brief; on a
    /// 64x64 map they would push every pile into a corner, which is a
    /// degenerate layout rather than a hard scenario.
    pub fn generate(config: &SimConfig, rng: &mut SimRng) -> Self {
        let (cx, cy) = config.map_center();
        let count = config.food_pile_count.clamp(1, 8);
        let mut piles = Vec::with_capacity(count);
        let base_angle = rng.range(0.0, std::f32::consts::TAU);

        // Furthest a pile centre can sit from the map centre and still have its
        // whole disc on the map.
        let reach = config.width.min(config.height) as f32 * 0.5;
        let max_dist = (reach - config.food_pile_radius - 2.0).max(1.0);
        let min_dist = config.food_distance_min.min(max_dist);

        for i in 0..count {
            let spread = std::f32::consts::TAU / count as f32;
            let angle = base_angle + spread * i as f32 + rng.range(-spread * 0.3, spread * 0.3);
            let dist = rng
                .range(min_dist, config.food_distance_max.max(min_dist))
                .min(max_dist);
            let margin = config.food_pile_radius + 2.0;
            let x = (cx + angle.cos() * dist).clamp(margin, config.width as f32 - margin);
            let y = (cy + angle.sin() * dist).clamp(margin, config.height as f32 - margin);
            piles.push(FoodPile {
                x,
                y,
                radius: config.food_pile_radius,
                initial: config.food_per_pile,
            });
        }
        Self::from_piles(config.width, config.height, piles)
    }

    /// Build the field from explicitly placed spots instead of the seeded ring.
    ///
    /// A scenario carries its own map, so its food is *the map*, not a draw
    /// from the seed. Spots are clamped into the map the same way the config's
    /// own values are, and no RNG is consulted - a scenario file plus any seed
    /// reproduces the same pile layout bit for bit.
    pub fn from_spots(config: &SimConfig, spots: &[crate::config::FoodSpot]) -> Self {
        let piles = spots
            .iter()
            .map(|spot| FoodPile {
                x: spot.x,
                y: spot.y,
                radius: spot.radius,
                initial: spot.amount,
            })
            .collect();
        Self::from_piles(config.width, config.height, piles)
    }

    /// Rasterise piles into a per-cell amount grid.
    pub fn from_piles(width: usize, height: usize, piles: Vec<FoodPile>) -> Self {
        let mut field = Self {
            width,
            height,
            amount: vec![0.0; width * height],
            piles: Vec::new(),
            initial_total: 0.0,
            remaining_total: 0.0,
            removed_total: 0.0,
            revision: 1,
        };
        for pile in piles {
            field.add_pile(pile);
        }
        field
    }

    /// Cell indices covered by a pile's disc.
    fn disc_cells(pile: &FoodPile, width: usize, height: usize) -> Vec<usize> {
        let r = pile.radius;
        let (x0, y0) = ((pile.x - r).floor() as i32, (pile.y - r).floor() as i32);
        let (x1, y1) = ((pile.x + r).ceil() as i32, (pile.y + r).ceil() as i32);
        let mut cells = Vec::new();
        for y in y0..=y1 {
            if y < 0 || y as usize >= height {
                continue;
            }
            for x in x0..=x1 {
                if x < 0 || x as usize >= width {
                    continue;
                }
                let dx = x as f32 + 0.5 - pile.x;
                let dy = y as f32 + 0.5 - pile.y;
                if dx * dx + dy * dy <= r * r {
                    cells.push(y as usize * width + x as usize);
                }
            }
        }
        if cells.is_empty() {
            // Degenerate pile (radius smaller than a cell); put it all in the
            // single cell under the centre.
            let cx = (pile.x as i32).clamp(0, width as i32 - 1) as usize;
            let cy = (pile.y as i32).clamp(0, height as i32 - 1) as usize;
            cells.push(cy * width + cx);
        }
        cells
    }

    /// Add a pile, spreading its mass evenly over the cells it covers.
    ///
    /// This is what the "click the map to drop food" feature calls.
    pub fn add_pile(&mut self, pile: FoodPile) {
        let cells = Self::disc_cells(&pile, self.width, self.height);
        let share = pile.initial / cells.len() as f32;
        for idx in cells {
            self.amount[idx] += share;
        }
        self.initial_total += pile.initial as f64;
        self.remaining_total += pile.initial as f64;
        self.piles.push(pile);
        self.revision = self.revision.wrapping_add(1);
    }

    /// Remove a pile by index, destroying whatever food it still held.
    ///
    /// Returns the removed pile, or `None` if the index is out of range.
    pub fn remove_pile(&mut self, index: usize) -> Option<FoodPile> {
        if index >= self.piles.len() {
            return None;
        }
        let pile = self.piles.remove(index);
        let mut destroyed = 0.0f64;
        for idx in Self::disc_cells(&pile, self.width, self.height) {
            destroyed += self.amount[idx] as f64;
            // Cancel the rounding residue rather than leaving crumbs behind.
            self.amount[idx] = 0.0;
        }
        self.remaining_total = (self.remaining_total - destroyed).max(0.0);
        self.removed_total += destroyed;
        self.revision = self.revision.wrapping_add(1);
        Some(pile)
    }

    /// Remove the pile whose centre is nearest to `(x, y)`, if any is within
    /// `max_distance`. Returns its index.
    pub fn remove_pile_near(&mut self, x: f32, y: f32, max_distance: f32) -> Option<usize> {
        let limit = max_distance * max_distance;
        let index = self
            .piles
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (p.x - x).powi(2) + (p.y - y).powi(2)))
            .filter(|(_, d2)| *d2 <= limit)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)?;
        self.remove_pile(index)?;
        Some(index)
    }

    /// Food remaining in a cell (0.0 outside the map).
    #[inline]
    pub fn get(&self, x: f32, y: f32) -> f32 {
        let (xi, yi) = (x as i32, y as i32);
        if xi < 0 || yi < 0 || xi as usize >= self.width || yi as usize >= self.height {
            return 0.0;
        }
        self.amount[yi as usize * self.width + xi as usize]
    }

    /// Remove up to `wanted` units from a cell, returning how much was taken.
    #[inline]
    pub fn take(&mut self, x: f32, y: f32, wanted: f32) -> f32 {
        if wanted <= 0.0 {
            return 0.0;
        }
        let (xi, yi) = (x as i32, y as i32);
        if xi < 0 || yi < 0 || xi as usize >= self.width || yi as usize >= self.height {
            return 0.0;
        }
        let idx = yi as usize * self.width + xi as usize;
        let available = self.amount[idx];
        let taken = available.min(wanted);
        if taken > 0.0 {
            self.amount[idx] = available - taken;
            self.remaining_total = (self.remaining_total - taken as f64).max(0.0);
        }
        taken
    }

    /// Total food left in one pile. Used by the renderer.
    pub fn pile_remaining(&self, pile: &FoodPile) -> f32 {
        Self::disc_cells(pile, self.width, self.height)
            .into_iter()
            .map(|idx| self.amount[idx])
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SimConfig {
        SimConfig::default()
    }

    #[test]
    fn a_single_colony_nests_at_the_map_centre() {
        let config = cfg();
        let nests = Nests::from_config(&config);
        assert_eq!(nests.len(), 1);
        let (cx, cy) = config.map_center();
        assert!((nests.0[0].x - cx).abs() < 1e-6);
        assert!((nests.0[0].y - cy).abs() < 1e-6);
    }

    #[test]
    fn two_colonies_nest_on_opposite_sides_of_the_centre() {
        let config = SimConfig {
            colony_count: 2,
            ..cfg()
        };
        let nests = Nests::from_config(&config);
        assert_eq!(nests.len(), 2);
        let (cx, cy) = config.map_center();
        // Mirror images through the centre.
        assert!((nests.0[0].x + nests.0[1].x - 2.0 * cx).abs() < 1e-3);
        assert!((nests.0[0].y + nests.0[1].y - 2.0 * cy).abs() < 1e-3);
        // Equidistant from the centre, so the contest is fair.
        let d0 = (nests.0[0].x - cx).hypot(nests.0[0].y - cy);
        let d1 = (nests.0[1].x - cx).hypot(nests.0[1].y - cy);
        assert!((d0 - d1).abs() < 1e-3);
        assert!(d0 > config.nest_radius * 2.0);
    }

    #[test]
    fn generated_piles_are_inside_the_map_and_off_every_nest() {
        let config = cfg();
        let nests = Nests::from_config(&config);
        for seed in 0..64u64 {
            let mut rng = SimRng::new(seed);
            let field = FoodField::generate(&config, &mut rng);
            assert_eq!(field.piles.len(), 3);
            for pile in &field.piles {
                assert!(pile.x > 0.0 && pile.x < config.width as f32);
                assert!(pile.y > 0.0 && pile.y < config.height as f32);
                for nest in &nests.0 {
                    let d = (pile.x - nest.x).hypot(pile.y - nest.y);
                    assert!(d > nest.radius + pile.radius, "pile on a nest");
                }
            }
        }
    }

    #[test]
    fn pile_mass_is_conserved_at_generation() {
        let config = cfg();
        let mut rng = SimRng::new(3);
        let field = FoodField::generate(&config, &mut rng);
        let expected = (config.food_per_pile * 3.0) as f64;
        assert!(
            (field.initial_total - expected).abs() < 1e-3,
            "initial_total = {}",
            field.initial_total
        );
        assert!((field.remaining_total - expected).abs() < 1e-3);
        assert_eq!(field.removed_total, 0.0);
    }

    #[test]
    fn take_is_bounded_by_supply_and_by_request() {
        let mut field = FoodField::from_piles(
            16,
            16,
            vec![FoodPile {
                x: 8.0,
                y: 8.0,
                radius: 2.0,
                initial: 10.0,
            }],
        );
        let before = field.remaining_total;
        let taken = field.take(8.0, 8.0, 1.0);
        assert!(taken > 0.0 && taken <= 1.0);
        assert!((field.remaining_total - (before - taken as f64)).abs() < 1e-6);

        // Asking for more than the cell holds returns exactly what was there.
        let rest = field.get(8.0, 8.0);
        let taken = field.take(8.0, 8.0, rest + 100.0);
        assert!((taken - rest).abs() < 1e-6);
        assert_eq!(field.get(8.0, 8.0), 0.0);

        // Empty cell yields nothing.
        assert_eq!(field.take(8.0, 8.0, 1.0), 0.0);
        // Out of bounds yields nothing.
        assert_eq!(field.take(-5.0, 500.0, 1.0), 0.0);
    }

    #[test]
    fn nest_containment_uses_radius() {
        let nest = Nest {
            x: 10.0,
            y: 10.0,
            radius: 3.0,
        };
        assert!(nest.contains(12.0, 10.0));
        assert!(!nest.contains(14.0, 10.0));
    }

    // ---- player editing -------------------------------------------------

    #[test]
    fn adding_a_pile_raises_both_totals_and_the_revision() {
        let mut field = FoodField::from_piles(32, 32, Vec::new());
        assert_eq!(field.initial_total, 0.0);
        let revision = field.revision;

        field.add_pile(FoodPile {
            x: 16.0,
            y: 16.0,
            radius: 3.0,
            initial: 250.0,
        });

        assert_eq!(field.piles.len(), 1);
        assert!((field.initial_total - 250.0).abs() < 1e-3);
        assert!((field.remaining_total - 250.0).abs() < 1e-3);
        assert!(
            field.revision > revision,
            "renderer would miss the new pile"
        );
        // The mass actually landed in the grid, not just in the books.
        assert!((field.get(16.0, 16.0) - 250.0 / 29.0).abs() < 1.0);
    }

    /// Removing a pile must not silently break the conservation law.
    #[test]
    fn removing_a_pile_accounts_for_the_food_it_destroyed() {
        let mut field = FoodField::from_piles(
            32,
            32,
            vec![FoodPile {
                x: 10.0,
                y: 10.0,
                radius: 3.0,
                initial: 300.0,
            }],
        );
        let initial = field.initial_total;

        // Eat some of the pile first, so it is not pristine. The exact amount
        // is read back from the books rather than assumed: a single cell holds
        // only `initial / cells`, so 50 takes do *not* mean 50 units.
        for _ in 0..50 {
            field.take(10.0, 10.0, 1.0);
        }
        let delivered = field.initial_total - field.remaining_total;
        assert!(delivered > 0.0, "the pile was not touched");
        let revision = field.revision;

        let removed = field.remove_pile(0).expect("pile should exist");
        assert_eq!(removed.initial, 300.0);
        assert!(field.revision > revision);
        assert!(field.piles.is_empty());

        // Nothing left anywhere...
        assert_eq!(field.remaining_total, 0.0);
        assert_eq!(field.get(10.0, 10.0), 0.0);
        // ...and the books still balance.
        let accounted = delivered + field.remaining_total + field.removed_total;
        assert!(
            (accounted - initial).abs() < 1e-3,
            "initial {initial}, accounted {accounted}, destroyed {}",
            field.removed_total
        );
    }

    #[test]
    fn remove_pile_rejects_a_bad_index() {
        let mut field = FoodField::from_piles(16, 16, Vec::new());
        assert!(field.remove_pile(0).is_none());
        assert_eq!(field.removed_total, 0.0);
    }

    #[test]
    fn remove_pile_near_picks_the_closest_within_range() {
        let mut field = FoodField::from_piles(
            64,
            64,
            vec![
                FoodPile {
                    x: 10.0,
                    y: 10.0,
                    radius: 3.0,
                    initial: 100.0,
                },
                FoodPile {
                    x: 40.0,
                    y: 40.0,
                    radius: 3.0,
                    initial: 100.0,
                },
            ],
        );

        // Far from everything.
        assert!(field.remove_pile_near(60.0, 60.0, 5.0).is_none());
        assert_eq!(field.piles.len(), 2);

        // Near the second pile.
        assert_eq!(field.remove_pile_near(41.0, 40.0, 5.0), Some(1));
        assert_eq!(field.piles.len(), 1);
        assert_eq!(field.piles[0].x, 10.0);
    }

    #[test]
    fn food_can_be_dropped_anywhere_on_the_map() {
        let mut field = FoodField::from_piles(64, 64, Vec::new());
        for (x, y) in [(1.0, 1.0), (62.0, 62.0), (32.5, 10.25)] {
            field.add_pile(FoodPile {
                x,
                y,
                radius: 2.0,
                initial: 40.0,
            });
        }
        assert_eq!(field.piles.len(), 3);
        assert!((field.remaining_total - 120.0).abs() < 1e-3);
        assert!(field.get(1.0, 1.0) > 0.0, "corner pile did not rasterise");
        assert!(field.get(62.0, 62.0) > 0.0, "corner pile did not rasterise");
        assert!(
            field.get(32.5, 10.25) > 0.0,
            "fractional pile did not rasterise"
        );
    }
}
