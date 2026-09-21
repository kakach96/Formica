//! Local ant density, measured on a coarse grid.
//!
//! Two things need this, and they need exactly the same numbers:
//!
//! * **Diagnosis.** "The whole colony is standing on one cell" is a claim about
//!   a spatial distribution, so it has to be measured rather than eyeballed. A
//!   coarse bin grid makes "how concentrated is the population?" a single cheap
//!   integer per tick, comparable across runs and assertable in a test.
//! * **Crowding relief.** When a bottleneck admits fewer ants than want through
//!   it, the queue has to go *somewhere*. A bottleneck means a gap in a wall or
//!   a busy food pile, and without a local density field an ant has no way to
//!   know it is standing on twenty of its sisters, so the queue degenerates
//!   into a point mass.
//!
//! # Why a coarse bin and not a per-cell count
//!
//! A 256x256 map has 65 536 cells; clearing that every tick costs more than the
//! rest of the statistics system put together, and per-cell resolution is the
//! wrong scale anyway - an ant sprite is ~2 cells across, so "two ants in one
//! cell" is not crowding, it is rendering. A 4x4-cell bin holds 16 cells, which
//! is roughly the area a dozen ants can occupy without touching, so the bin
//! count is a usable proxy for "am I in a jam?".

use bevy_ecs::prelude::*;

/// Side of one bin, in cells.
///
/// Four is a compromise: 8 would smear the metric over 64 cells (a jam and a
/// normal trail read the same), 2 would make the bin grid as expensive to clear
/// as a per-cell count and would flag ordinary two-abreast trail traffic.
pub const CROWD_BIN: usize = 4;

/// Ant counts on a coarse grid, rebuilt from scratch every tick.
#[derive(Resource, Debug, Clone)]
pub struct CrowdGrid {
    /// Side of one bin, in cells.
    pub bin: usize,
    /// Map width in cells.
    width: usize,
    /// Map height in cells.
    height: usize,
    /// Bins across.
    pub bins_x: usize,
    /// Bins down.
    pub bins_y: usize,
    counts: Vec<u32>,
    /// Ants in the busiest bin right now. The headline number.
    pub max_bin: u32,
    /// Ants in the busiest bin seen since the last [`CrowdGrid::reset_peak`].
    pub peak_bin: u32,
    /// Bins holding at least one ant. A spread indicator: it falls when the
    /// population collapses onto a few spots even if the head count is fine.
    pub occupied: u32,
    /// Ants counted during the last rebuild.
    pub total: u32,
}

impl CrowdGrid {
    /// An empty grid for a `width` x `height` map.
    pub fn new(width: usize, height: usize, bin: usize) -> Self {
        let bin = bin.max(1);
        let bins_x = width.div_ceil(bin);
        let bins_y = height.div_ceil(bin);
        Self {
            bin,
            width,
            height,
            bins_x,
            bins_y,
            counts: vec![0; bins_x * bins_y],
            max_bin: 0,
            peak_bin: 0,
            occupied: 0,
            total: 0,
        }
    }

    /// Reallocate for a new map size.
    pub fn resize(&mut self, width: usize, height: usize) {
        let bins_x = width.div_ceil(self.bin);
        let bins_y = height.div_ceil(self.bin);
        if bins_x == self.bins_x && bins_y == self.bins_y && width == self.width {
            return;
        }
        self.width = width;
        self.height = height;
        self.bins_x = bins_x;
        self.bins_y = bins_y;
        self.counts = vec![0; bins_x * bins_y];
        self.max_bin = 0;
        self.occupied = 0;
        self.total = 0;
        self.reset_peak();
    }

    /// Forget the observed peak (but not the current counts).
    pub fn reset_peak(&mut self) {
        self.peak_bin = 0;
    }

    /// True when this grid was binned for a different map than `width` x
    /// `height`.
    ///
    /// Checked rather than assumed because the map size is a live setting: a
    /// resize that the crowd grid slept through would leave every ant binned by
    /// the old geometry, and the relief system would then push ants toward
    /// neighbourhoods that do not exist.
    #[inline]
    pub fn width_mismatch(&self, width: usize, height: usize) -> bool {
        self.width != width || self.height != height
    }

    /// Bin containing a world position, or `None` when it is off the map.
    ///
    /// Bounds are checked against the *map*, not against the bin grid. A
    /// 10-cell map binned by 4 has three bins, and the third one nominally
    /// covers cells 8..12 - so a bin-index check alone would happily count an
    /// ant standing at x = 10 on a map only ten cells wide.
    #[inline]
    pub fn bin_of(&self, x: f32, y: f32) -> Option<usize> {
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let (cx, cy) = (x as usize, y as usize);
        if cx >= self.width || cy >= self.height {
            return None;
        }
        Some(cy / self.bin * self.bins_x + cx / self.bin)
    }

    /// Rebuild the counts from every ant position.
    ///
    /// One pass to clear, one pass to count: `O(bins + ants)`, with no
    /// allocation, which is why this can run every tick.
    pub fn rebuild(&mut self, positions: impl Iterator<Item = (f32, f32)>) {
        self.counts.fill(0);
        self.max_bin = 0;
        self.occupied = 0;
        self.total = 0;
        for (x, y) in positions {
            let Some(index) = self.bin_of(x, y) else {
                continue;
            };
            let count = &mut self.counts[index];
            if *count == 0 {
                self.occupied += 1;
            }
            *count += 1;
            self.total += 1;
            if *count > self.max_bin {
                self.max_bin = *count;
            }
        }
        self.peak_bin = self.peak_bin.max(self.max_bin);
    }

    /// Ants in the bin containing `(x, y)`; 0 off the map.
    #[inline]
    pub fn at(&self, x: f32, y: f32) -> u32 {
        self.bin_of(x, y).map_or(0, |i| self.counts[i])
    }

    /// The neighbouring bin with the fewest ants, as a direction to step in.
    ///
    /// Used by the crowding-relief system: an ant in an over-full bin walks
    /// toward whichever adjacent bin is emptiest, which turns a point mass into
    /// a spreading queue. Returns `None` when the ant's own bin is not the
    /// busiest of its neighbourhood - i.e. there is somewhere better to be, but
    /// not from here.
    pub fn escape_direction(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        let index = self.bin_of(x, y)?;
        let (bx, by) = (index % self.bins_x, index / self.bins_x);
        let own = self.counts[index];
        let mut best: Option<(u32, f32, f32)> = None;
        for (dx, dy) in [
            (-1i32, 0i32),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            let (nx, ny) = (bx as i32 + dx, by as i32 + dy);
            if nx < 0 || ny < 0 || nx as usize >= self.bins_x || ny as usize >= self.bins_y {
                continue;
            }
            let count = self.counts[ny as usize * self.bins_x + nx as usize];
            if count >= own {
                continue;
            }
            if best.is_none_or(|(c, _, _)| count < c) {
                best = Some((count, dx as f32, dy as f32));
            }
        }
        best.map(|(_, dx, dy)| {
            let len = (dx * dx + dy * dy).sqrt();
            (dx / len, dy / len)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bins_cover_the_map_including_a_ragged_edge() {
        // 10 cells with a bin of 4 needs 3 bins across.
        let grid = CrowdGrid::new(10, 10, 4);
        assert_eq!((grid.bins_x, grid.bins_y), (3, 3));
        assert!(grid.bin_of(9.5, 9.5).is_some(), "the last cell fell off");
        assert!(grid.bin_of(10.0, 5.0).is_none(), "an off-map cell was counted");
        assert!(grid.bin_of(-1.0, 5.0).is_none());
    }

    #[test]
    fn a_zero_bin_is_promoted_to_one() {
        let grid = CrowdGrid::new(8, 8, 0);
        assert_eq!(grid.bin, 1);
        assert_eq!((grid.bins_x, grid.bins_y), (8, 8));
    }

    #[test]
    fn rebuild_counts_each_bin_and_tracks_the_busiest() {
        let mut grid = CrowdGrid::new(16, 16, 4);
        let positions = [
            (0.5, 0.5),
            (1.5, 0.5),
            (2.5, 0.5),
            (8.5, 8.5),
            (9.5, 8.5),
        ];
        grid.rebuild(positions.into_iter());

        assert_eq!(grid.total, 5);
        assert_eq!(grid.max_bin, 3);
        assert_eq!(grid.peak_bin, 3);
        assert_eq!(grid.occupied, 2, "two bins were touched");
        assert_eq!(grid.at(0.5, 0.5), 3);
        assert_eq!(grid.at(8.5, 8.5), 2);
        assert_eq!(grid.at(20.0, 20.0), 0);
    }

    #[test]
    fn a_rebuild_forgets_the_previous_tick() {
        let mut grid = CrowdGrid::new(16, 16, 4);
        grid.rebuild(vec![(0.5, 0.5); 40].into_iter());
        assert_eq!(grid.max_bin, 40);
        grid.rebuild(vec![(0.5, 0.5)].into_iter());
        assert_eq!(grid.max_bin, 1, "counts leaked across ticks");
        assert_eq!(grid.total, 1);
        // But the peak survives until it is explicitly cleared.
        assert_eq!(grid.peak_bin, 40);
        grid.reset_peak();
        assert_eq!(grid.peak_bin, 0);
    }

    #[test]
    fn off_map_positions_are_ignored_rather_than_misplaced() {
        let mut grid = CrowdGrid::new(8, 8, 4);
        grid.rebuild(vec![(100.0, 100.0), (-3.0, 2.0), (1.0, 1.0)].into_iter());
        assert_eq!(grid.total, 1);
        assert_eq!(grid.max_bin, 1);
        assert_eq!(grid.occupied, 1);
    }

    #[test]
    fn resize_reallocates_and_clears() {
        let mut grid = CrowdGrid::new(8, 8, 4);
        grid.rebuild(vec![(1.0, 1.0)].into_iter());
        grid.resize(32, 16);
        assert_eq!((grid.bins_x, grid.bins_y), (8, 4));
        assert_eq!(grid.total, 0);
        assert_eq!(grid.max_bin, 0);
    }

    /// `n` positions inside bin `(bx, by)`, all well away from its edges.
    ///
    /// Tests build occupancy patterns through `rebuild` rather than reaching
    /// into the counts, so they exercise the same path the simulation uses.
    fn fill(bx: usize, by: usize, n: usize, bin: usize) -> Vec<(f32, f32)> {
        let (x0, y0) = ((bx * bin) as f32 + 0.5, (by * bin) as f32 + 0.5);
        (0..n)
            .map(|i| {
                (
                    x0 + (i % 3) as f32 * 0.1,
                    y0 + (i.div_euclid(3) % 3) as f32 * 0.1,
                )
            })
            .collect()
    }

    /// A 4x4-bin grid with `counts(bx, by)` ants in every bin.
    fn grid_where(bin: usize, counts: impl Fn(usize, usize) -> usize) -> CrowdGrid {
        let mut grid = CrowdGrid::new(bin * 4, bin * 4, bin);
        let mut positions = Vec::new();
        for by in 0..4 {
            for bx in 0..4 {
                positions.extend(fill(bx, by, counts(bx, by), bin));
            }
        }
        grid.rebuild(positions.into_iter());
        grid
    }

    #[test]
    fn escape_direction_points_at_the_emptiest_neighbour() {
        // Bin (1,1) is packed; every neighbour has five ants except the one to
        // the right, which is empty. There is exactly one right answer.
        let grid = grid_where(4, |bx, by| match (bx, by) {
            (1, 1) => 9,
            (2, 1) => 0,
            _ => 5,
        });

        let (dx, dy) = grid.escape_direction(5.0, 5.0).expect("nowhere to go");
        assert!(dx > 0.0, "should step right (dx = {dx})");
        assert!(dy.abs() < 1e-6, "should not drift vertically (dy = {dy})");
    }

    #[test]
    fn escape_direction_is_none_when_nowhere_is_emptier() {
        // Uniform occupancy: no ant has any reason to move, so no ant should be
        // told to. Getting this wrong would stir the whole colony forever.
        let grid = grid_where(4, |_, _| 5);
        assert!(
            grid.escape_direction(5.0, 5.0).is_none(),
            "reported an escape from an evenly crowded map"
        );
    }

    #[test]
    fn escape_direction_handles_map_edges() {
        // The corner bin has only three in-map neighbours; it must still answer
        // rather than falling off the end of the grid.
        let grid = grid_where(4, |bx, by| match (bx, by) {
            (0, 0) => 9,
            (1, 0) => 0,
            _ => 5,
        });
        let (dx, dy) = grid.escape_direction(0.5, 0.5).expect("no escape from a corner");
        assert!(dx > 0.0 && dy.abs() < 1e-6, "got ({dx}, {dy})");
    }
}
