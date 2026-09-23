//! The pheromone grid.
//!
//! Inside one `Resource` live `colonies * 2` independent scalar fields. For each
//! colony there are two trails:
//!
//! * [`Pheromone::Food`] - laid by ants that are carrying food home. It marks
//!   "there is food this way", so **searching** ants follow it.
//! * [`Pheromone::Home`] - laid by ants that are still searching. It marks
//!   "the nest is this way", so **returning** ants can follow it.
//!
//! # Why per-colony fields
//!
//! With two colonies sharing one pair of fields the ants would be *cooperating*:
//! a trail laid by colony A would be followed by colony B, and the two nests
//! would effectively run one shared foraging economy. Competition only becomes
//! real once each colony can smell only its own pheromone while still competing
//! for the same finite food piles - which is exactly the arrangement here.
//!
//! The fields are kept in one flat `Vec<f32>` (rather than a `Vec<Cell>` or a
//! `Vec<Vec<f32>>`) so the evaporation sweep stays a single linear pass over
//! contiguous memory across every colony at once.

use bevy_ecs::prelude::*;

/// Everything needed to lay one deposit.
///
/// Bundled rather than passed as seven positional arguments: five of those are
/// bare numbers, and `deposit_kernel(c, kind, x, y, amount, radius, cap)` is one
/// transposition away from silently swapping the position for the strength.
#[derive(Debug, Clone, Copy)]
pub struct Deposit {
    /// Which colony's field to write.
    pub colony: usize,
    /// Which of that colony's trails.
    pub kind: Pheromone,
    /// Centre x, in cells.
    pub x: f32,
    /// Centre y, in cells.
    pub y: f32,
    /// Total mass to deposit, spread over the kernel.
    pub amount: f32,
    /// Kernel radius in cells; `0` means a single cell.
    pub radius: i32,
    /// Per-cell ceiling.
    pub cap: f32,
}

impl Deposit {
    /// Shorthand constructor.
    pub fn new(
        colony: usize,
        kind: Pheromone,
        x: f32,
        y: f32,
        amount: f32,
        radius: i32,
        cap: f32,
    ) -> Self {
        Self {
            colony,
            kind,
            x,
            y,
            amount,
            radius,
            cap,
        }
    }
}

/// Values below this are snapped to zero.
///
/// Keeps the field from accumulating denormals forever and lets the renderer
/// and the sensors treat "essentially nothing" as exactly nothing.
pub const PHEROMONE_EPSILON: f32 = 1.0e-4;

/// Largest supported number of competing colonies.
pub const MAX_COLONIES: usize = 2;

/// Number of distinct trails a colony lays.
pub const TRAILS_PER_COLONY: usize = 2;

/// Which of a colony's two pheromone trails is meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pheromone {
    /// Food trail: laid by returning ants, followed by searching ants.
    Food,
    /// Home trail: laid by searching ants, followed by returning ants.
    Home,
}

impl Pheromone {
    /// Dense slot index within a colony's block of fields.
    #[inline]
    pub const fn slot(self) -> usize {
        match self {
            Pheromone::Food => 0,
            Pheromone::Home => 1,
        }
    }
}

/// Global 2D field of pheromone concentrations: one `f32` per cell, per trail,
/// per colony.
///
/// `colonies` here is **storage capacity**, not the number of active colonies:
/// [`crate::plugin::init_simulation`] always allocates for
/// [`MAX_COLONIES`] so that turning a second colony on at runtime cannot make
/// it alias the first colony's fields.
#[derive(Resource, Debug, Clone)]
pub struct PheromoneGrid {
    /// Grid width in cells.
    pub width: usize,
    /// Grid height in cells.
    pub height: usize,
    /// Number of colony field-sets allocated.
    pub colonies: usize,
    /// `colonies * TRAILS_PER_COLONY` blocks of `width * height`, row-major.
    fields: Vec<f32>,
}

impl PheromoneGrid {
    /// Allocate a zeroed grid for `colonies` colonies.
    pub fn new(width: usize, height: usize, colonies: usize) -> Self {
        let colonies = colonies.clamp(1, MAX_COLONIES);
        Self {
            width,
            height,
            colonies,
            fields: vec![0.0; width * height * colonies * TRAILS_PER_COLONY],
        }
    }

    /// Number of cells in one field.
    #[inline]
    pub fn cells(&self) -> usize {
        self.width * self.height
    }

    /// Offset of a colony's trail block within `fields`.
    #[inline]
    fn block_start(&self, colony: usize, kind: Pheromone) -> usize {
        debug_assert!(colony < self.colonies, "colony {colony} out of range");
        let colony = colony.min(self.colonies.saturating_sub(1));
        (colony * TRAILS_PER_COLONY + kind.slot()) * self.cells()
    }

    /// Row-major index of a cell. Coordinates must already be in bounds.
    #[inline]
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    /// Borrow one colony's trail as a flat slice.
    #[inline]
    pub fn field(&self, colony: usize, kind: Pheromone) -> &[f32] {
        let start = self.block_start(colony, kind);
        &self.fields[start..start + self.cells()]
    }

    /// Mutably borrow one colony's trail as a flat slice.
    #[inline]
    pub fn field_mut(&mut self, colony: usize, kind: Pheromone) -> &mut [f32] {
        let cells = self.cells();
        let start = self.block_start(colony, kind);
        &mut self.fields[start..start + cells]
    }

    /// True when the cell coordinate is inside the grid.
    #[inline]
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    /// Read a cell; out-of-bounds reads return `0.0` (the world is walled).
    #[inline]
    pub fn get(&self, colony: usize, kind: Pheromone, x: i32, y: i32) -> f32 {
        if !self.in_bounds(x, y) {
            return 0.0;
        }
        self.field(colony, kind)[self.index(x as usize, y as usize)]
    }

    /// Read a cell from continuous world coordinates (truncating toward zero).
    #[inline]
    pub fn get_at(&self, colony: usize, kind: Pheromone, x: f32, y: f32) -> f32 {
        self.get(colony, kind, x as i32, y as i32)
    }

    /// Set a cell, ignoring out-of-bounds writes.
    #[inline]
    pub fn set(&mut self, colony: usize, kind: Pheromone, x: i32, y: i32, value: f32) {
        if !self.in_bounds(x, y) {
            return;
        }
        let idx = self.index(x as usize, y as usize);
        self.field_mut(colony, kind)[idx] = value;
    }

    /// Add `amount` to a cell, saturating at `cap`.
    #[inline]
    pub fn add_clamped(
        &mut self,
        colony: usize,
        kind: Pheromone,
        x: i32,
        y: i32,
        amount: f32,
        cap: f32,
    ) {
        if !self.in_bounds(x, y) {
            return;
        }
        let idx = self.index(x as usize, y as usize);
        let cell = &mut self.field_mut(colony, kind)[idx];
        *cell = (*cell + amount).min(cap);
    }

    /// Maximum over every colony's field for one trail kind. Diagnostics only.
    pub fn global_peak(&self, kind: Pheromone) -> f32 {
        (0..self.colonies)
            .map(|c| self.peak(c, kind))
            .fold(0.0f32, f32::max)
    }

    /// Average the field over a `(2r+1)^2` box centred on a continuous point.
    ///
    /// Averaging (rather than point-sampling) is what makes a discrete grid
    /// legible to an ant: a single ant's deposit is a single cell, but the
    /// antenna sees a smoothed neighbourhood, so the comparison between the
    /// three antennae is stable instead of flickering.
    pub fn sample_box(&self, colony: usize, kind: Pheromone, cx: f32, cy: f32, radius: i32) -> f32 {
        let field = self.field(colony, kind);
        if radius <= 0 {
            return self.get(colony, kind, cx as i32, cy as i32);
        }
        let (x0, y0) = (cx as i32, cy as i32);
        let mut sum = 0.0f32;
        let mut count = 0u32;
        for dy in -radius..=radius {
            let y = y0 + dy;
            if y < 0 || y as usize >= self.height {
                continue;
            }
            let row = y as usize * self.width;
            for dx in -radius..=radius {
                let x = x0 + dx;
                if x < 0 || x as usize >= self.width {
                    continue;
                }
                sum += field[row + x as usize];
                count += 1;
            }
        }
        if count == 0 {
            0.0
        } else {
            sum / count as f32
        }
    }

    /// Lay a radial kernel of pheromone centred on a continuous position.
    ///
    /// The total mass deposited is exactly `deposit.amount` (weights are
    /// normalised), so changing `radius` changes how *spread out* a trail is,
    /// never how strong it is - which keeps the brief's "deposit strength 1.0"
    /// honest.
    ///
    /// `blocked` lets the caller veto individual cells. It exists because a
    /// kernel is wider than an ant: an ant standing one cell away from a wall
    /// would otherwise write its 3x3 deposit *into* the wall. Passing
    /// `|_, _| false` disables the check. Mass that would have gone into a
    /// vetoed cell is simply not deposited, which is the physically sensible
    /// answer - the wall absorbs it.
    pub fn deposit_kernel(&mut self, deposit: Deposit, blocked: impl Fn(i32, i32) -> bool) {
        if deposit.amount <= 0.0 {
            return;
        }
        if deposit.radius <= 0 {
            if !blocked(deposit.x as i32, deposit.y as i32) {
                self.add_clamped(
                    deposit.colony,
                    deposit.kind,
                    deposit.x as i32,
                    deposit.y as i32,
                    deposit.amount,
                    deposit.cap,
                );
            }
            return;
        }
        // Pre-computed weights for r == 1, the common case.
        const KERNEL_1: [f32; 9] = [
            0.25, 0.5, 0.25, //
            0.5, 1.0, 0.5, //
            0.25, 0.5, 0.25,
        ];
        let weights: &[f32] = match deposit.radius {
            1 => &KERNEL_1,
            other => {
                // Generic Gaussian falloff for larger radii.
                let mut sum = 0.0f32;
                let mut w = Vec::with_capacity(((2 * other + 1) * (2 * other + 1)) as usize);
                let sigma = other as f32;
                for dy in -other..=other {
                    for dx in -other..=other {
                        let d2 = (dx * dx + dy * dy) as f32;
                        let v = (-d2 / (2.0 * sigma * sigma)).exp();
                        w.push(v);
                        sum += v;
                    }
                }
                for v in w.iter_mut() {
                    *v /= sum;
                }
                self.deposit_weighted(deposit, deposit.amount, &w, &blocked);
                return;
            }
        };
        let total: f32 = weights.iter().sum();
        self.deposit_weighted(deposit, deposit.amount / total, weights, &blocked);
    }

    /// Apply a square, row-major weight kernel centred on a continuous point.
    ///
    /// The kernel's side length is derived from `weights.len()` rather than
    /// passed alongside it - one less argument, and one less way for the two to
    /// disagree.
    fn deposit_weighted(
        &mut self,
        deposit: Deposit,
        unit_mass: f32,
        weights: &[f32],
        blocked: &impl Fn(i32, i32) -> bool,
    ) {
        let side = (weights.len() as f32).sqrt().round() as i32;
        if side <= 0 || (side * side) as usize != weights.len() {
            // Not a square kernel; fall back to a single-cell deposit rather
            // than indexing nonsense.
            if !blocked(deposit.x as i32, deposit.y as i32) {
                self.add_clamped(
                    deposit.colony,
                    deposit.kind,
                    deposit.x as i32,
                    deposit.y as i32,
                    unit_mass,
                    deposit.cap,
                );
            }
            return;
        }
        let radius = side / 2;
        let (x0, y0) = (deposit.x as i32, deposit.y as i32);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let w = weights[((dy + radius) * side + (dx + radius)) as usize];
                if w > 0.0 && !blocked(x0 + dx, y0 + dy) {
                    self.add_clamped(
                        deposit.colony,
                        deposit.kind,
                        x0 + dx,
                        y0 + dy,
                        unit_mass * w,
                        deposit.cap,
                    );
                }
            }
        }
    }

    /// One evaporation step over every colony and trail: `c *= (1 - rate)`.
    ///
    /// This is the hot loop of the whole simulation; it is a single linear
    /// sweep over the entire allocation with no branching except the epsilon
    /// snap, and it covers all colonies in the same pass.
    pub fn evaporate(&mut self, rate: f32) {
        let keep = (1.0 - rate).clamp(0.0, 1.0);
        if keep >= 1.0 {
            return;
        }
        if keep <= 0.0 {
            self.fields.fill(0.0);
            return;
        }
        for v in self.fields.iter_mut() {
            let next = *v * keep;
            *v = if next < PHEROMONE_EPSILON { 0.0 } else { next };
        }
    }

    /// Zero every field.
    pub fn clear(&mut self) {
        self.fields.fill(0.0);
    }

    /// Total mass in one colony's trail. Diagnostics only.
    pub fn total(&self, colony: usize, kind: Pheromone) -> f64 {
        self.field(colony, kind).iter().map(|v| *v as f64).sum()
    }

    /// Total mass across every colony, for one trail kind.
    pub fn total_all(&self, kind: Pheromone) -> f64 {
        (0..self.colonies).map(|c| self.total(c, kind)).sum()
    }

    /// Largest value in one colony's trail. Diagnostics only.
    pub fn peak(&self, colony: usize, kind: Pheromone) -> f32 {
        self.field(colony, kind)
            .iter()
            .copied()
            .fold(0.0f32, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaporation_is_geometric() {
        let mut grid = PheromoneGrid::new(4, 4, 1);
        grid.set(0, Pheromone::Food, 1, 1, 1.0);
        grid.evaporate(0.1);
        assert!((grid.get(0, Pheromone::Food, 1, 1) - 0.9).abs() < 1e-6);
        grid.evaporate(0.1);
        assert!((grid.get(0, Pheromone::Food, 1, 1) - 0.81).abs() < 1e-6);
    }

    #[test]
    fn evaporation_snaps_tiny_values_to_zero() {
        let mut grid = PheromoneGrid::new(4, 4, 1);
        grid.set(0, Pheromone::Home, 0, 0, PHEROMONE_EPSILON * 0.5);
        grid.evaporate(0.1);
        assert_eq!(grid.get(0, Pheromone::Home, 0, 0), 0.0);
    }

    #[test]
    fn trails_are_independent() {
        let mut grid = PheromoneGrid::new(8, 8, 1);
        grid.set(0, Pheromone::Food, 2, 2, 5.0);
        assert_eq!(grid.get(0, Pheromone::Home, 2, 2), 0.0);
        grid.evaporate(1.0);
        assert_eq!(grid.get(0, Pheromone::Food, 2, 2), 0.0);
        assert_eq!(grid.get(0, Pheromone::Home, 2, 2), 0.0);
    }

    /// The whole point of per-colony fields: one colony cannot smell another's
    /// trail, even though both share the same grid.
    #[test]
    fn colonies_cannot_smell_each_others_trails() {
        let mut grid = PheromoneGrid::new(8, 8, 2);
        grid.set(0, Pheromone::Food, 3, 3, 9.0);

        assert_eq!(grid.get(0, Pheromone::Food, 3, 3), 9.0);
        assert_eq!(grid.get(1, Pheromone::Food, 3, 3), 0.0);
        assert_eq!(grid.get(1, Pheromone::Home, 3, 3), 0.0);

        // ...and a deposit by colony 1 does not disturb colony 0.
        grid.deposit_kernel(
            Deposit::new(1, Pheromone::Home, 3.0, 3.0, 4.0, 1, 1_000.0),
            |_, _| false,
        );
        assert_eq!(grid.get(0, Pheromone::Food, 3, 3), 9.0);
        assert!(grid.get(1, Pheromone::Home, 3, 3) > 0.0);
    }

    #[test]
    fn evaporation_reaches_every_colony() {
        let mut grid = PheromoneGrid::new(4, 4, 2);
        grid.set(0, Pheromone::Food, 1, 1, 1.0);
        grid.set(1, Pheromone::Home, 2, 2, 1.0);
        grid.evaporate(1.0);
        assert_eq!(grid.total_all(Pheromone::Food), 0.0);
        assert_eq!(grid.total_all(Pheromone::Home), 0.0);
    }

    #[test]
    fn out_of_bounds_reads_are_zero_and_writes_are_dropped() {
        let mut grid = PheromoneGrid::new(4, 4, 1);
        assert_eq!(grid.get(0, Pheromone::Food, -1, 0), 0.0);
        assert_eq!(grid.get(0, Pheromone::Food, 4, 0), 0.0);
        grid.set(0, Pheromone::Food, -1, -1, 9.0);
        grid.add_clamped(0, Pheromone::Food, 99, 99, 9.0, 50.0);
        assert_eq!(grid.total(0, Pheromone::Food), 0.0);
    }

    #[test]
    fn deposit_kernel_conserves_mass_and_respects_cap() {
        let mut grid = PheromoneGrid::new(32, 32, 1);
        grid.deposit_kernel(
            Deposit::new(0, Pheromone::Food, 16.0, 16.0, 1.0, 1, 1_000.0),
            |_, _| false,
        );
        assert!(
            (grid.total(0, Pheromone::Food) - 1.0).abs() < 1e-4,
            "mass was {}",
            grid.total(0, Pheromone::Food)
        );
        // Centre of a 3x3 weight kernel gets the largest share.
        assert!(grid.get(0, Pheromone::Food, 16, 16) > grid.get(0, Pheromone::Food, 15, 16));

        grid.deposit_kernel(
            Deposit::new(0, Pheromone::Food, 16.0, 16.0, 10.0, 1, 1.05),
            |_, _| false,
        );
        assert!(grid.peak(0, Pheromone::Food) <= 1.05 + 1e-6);
    }

    #[test]
    fn larger_deposit_kernels_also_conserve_mass() {
        let mut grid = PheromoneGrid::new(32, 32, 1);
        for radius in 0..=3 {
            grid.clear();
            grid.deposit_kernel(
                Deposit::new(0, Pheromone::Food, 16.0, 16.0, 2.0, radius, 1_000.0),
                |_, _| false,
            );
            assert!(
                (grid.total(0, Pheromone::Food) - 2.0).abs() < 1e-3,
                "radius {radius} deposited {}",
                grid.total(0, Pheromone::Food)
            );
        }
    }

    #[test]
    fn sample_box_averages_available_cells() {
        let mut grid = PheromoneGrid::new(8, 8, 1);
        for y in 0..3 {
            for x in 0..3 {
                grid.set(0, Pheromone::Food, x, y, 1.0);
            }
        }
        // Corner sample only sees the in-bounds part of the box.
        let corner = grid.sample_box(0, Pheromone::Food, 0.0, 0.0, 1);
        assert!(corner > 0.0 && corner <= 1.0);
        // Fully interior sample of a uniformly filled area is exactly 1.0.
        assert!((grid.sample_box(0, Pheromone::Food, 1.0, 1.0, 1) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn colony_count_is_clamped_to_the_supported_maximum() {
        let grid = PheromoneGrid::new(4, 4, 99);
        assert_eq!(grid.colonies, MAX_COLONIES);
        let grid = PheromoneGrid::new(4, 4, 0);
        assert_eq!(grid.colonies, 1);
    }

    /// An out-of-range colony index must not panic or corrupt a neighbour:
    /// it clamps into the last real colony.
    #[test]
    fn out_of_range_colony_index_is_clamped() {
        let mut grid = PheromoneGrid::new(4, 4, 1);
        grid.set(7, Pheromone::Food, 1, 1, 3.0);
        assert_eq!(grid.get(0, Pheromone::Food, 1, 1), 3.0);
        assert_eq!(grid.get(9, Pheromone::Food, 1, 1), 3.0);
    }
}
