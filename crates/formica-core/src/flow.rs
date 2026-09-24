//! The nest flow field: a per-colony BFS distance map to the nest.
//!
//! This is the "the colony knows the building" answer to wall jams: instead of
//! fighting the obstacle, a returning ant walks down the distance gradient to
//! its nest, and the gradient cannot cross a wall because the wall's cells
//! were never given a distance.
//!
//! Data rules, not tastes:
//! * The field is pure data with no RNG input, so the determinism contract
//!   applies unchanged (same seed + same config = the same field).
//! * One field per colony, sized for [`MAX_COLONIES`] at all times, exactly
//!   like [`PheromoneGrid`]: allocating for the *current* colony count would
//!   alias colony 1's field when colony 2 arrives.
//! * The field is rebuilt only when its inputs change (obstacle revision or
//!   nest centres), never per tick: a 256x256 map costs one cheap BFS per map
//!   edit, not per tick.

use bevy_ecs::prelude::*;

use crate::grid::MAX_COLONIES;
use crate::obstacles::Obstacles;
use crate::world::Nests;

/// Distance recorded for walls and for cells with no path to the nest.
pub const UNREACHABLE: u32 = u32::MAX;

/// Distances to each colony's nest, plus the downhill direction of that
/// distance field at a point.
#[derive(Resource, Debug, Clone)]
pub struct NestFlow {
    /// Map width in cells (kept in step with the obstacle map).
    pub width: usize,
    /// Map height in cells.
    pub height: usize,
    /// Which obstacle-map revision this field was built from. A mismatch is
    /// the rebuild trigger.
    pub built_revision: u64,
    /// The nest centre the field was built around, per colony; `NaN` marks
    /// "not built" (most often: beyond the active colony count).
    built_nests: [(f32, f32); MAX_COLONIES],
    /// Per colony, one `u32` per cell: BFS distance in steps, [`UNREACHABLE`]
    /// for walls and for anything that cannot reach the nest.
    dist: Vec<u32>,
}

impl NestFlow {
    /// An empty, unbuilt field for the given map size.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            built_revision: 0,
            built_nests: [(f32::NAN, f32::NAN); MAX_COLONIES],
            dist: vec![UNREACHABLE; MAX_COLONIES * width * height],
        }
    }

    /// Per-colony slice length for the current map size.
    #[inline]
    fn cells(&self) -> usize {
        self.width * self.height
    }

    /// Distance from a cell to `colony`'s nest, or [`UNREACHABLE`].
    #[inline]
    pub fn distance(&self, colony: usize, x: i32, y: i32) -> u32 {
        if x < 0
            || y < 0
            || x as usize >= self.width
            || y as usize >= self.height
            || colony >= MAX_COLONIES
        {
            return UNREACHABLE;
        }
        self.dist[colony * self.cells() + y as usize * self.width + x as usize]
    }

    /// The downhill direction of the nest-distance field at a point.
    ///
    /// Examines the four neighbours of the cell the point stands in and aims
    /// at the lowest distance among them. Ties fall to the scan order, which
    /// is deterministic and harmless: any downhill neighbour is one step
    /// closer to the nest than the current cell. `None` means "this cell does
    /// not know a way" (a wall, an unreachable pocket, outside the map) and
    /// the caller must fall back to direct homing.
    pub fn direction(&self, colony: usize, x: f32, y: f32) -> Option<(f32, f32)> {
        let cell = (x as i32, y as i32);
        let here = self.distance(colony, cell.0, cell.1);
        if here == UNREACHABLE || here == 0 {
            return None;
        }
        let mut best = here;
        let mut delta: Option<(i32, i32)> = None;
        for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let there = self.distance(colony, cell.0 + dx, cell.1 + dy);
            if there < best {
                best = there;
                delta = Some((dx, dy));
            }
        }
        // Aim at the neighbour cell's centre from the point's own position,
        // so this is a real world-space direction, not a cell delta.
        let (dx, dy) = delta?;
        let tx = (cell.0 + dx) as f32 + 0.5 - x;
        let ty = (cell.1 + dy) as f32 + 0.5 - y;
        let norm = (tx * tx + ty * ty).sqrt();
        if norm <= 1.0e-6 {
            return None;
        }
        Some((tx / norm, ty / norm))
    }

    /// Whether the stored field is current for these `obstacles` and `nests`.
    ///
    /// `FlowField` is the only mode that maintains the field, so the mode
    /// check belongs to the caller; a stale-but-configured field is simply
    /// rebuilt by the sync system.
    pub fn is_current(&self, obstacles: &Obstacles, nests: &Nests) -> bool {
        if self.width != obstacles.width || self.height != obstacles.height {
            return false;
        }
        if self.built_revision != obstacles.revision {
            return false;
        }
        for colony in 0..MAX_COLONIES {
            if colony >= nests.len() {
                continue;
            }
            let built = self.built_nests[colony];
            let nest = *nests.get(colony);
            if built.0.is_nan()
                || (nest.x - built.0).abs() > 1.0e-4
                || (nest.y - built.1).abs() > 1.0e-4
            {
                return false;
            }
        }
        true
    }
}

/// Rebuild the flow field for every active colony.
///
/// The four-neighbourhood is deliberate: a diagonal shortcut through two
/// touching wall cells would let the BFS flow squeeze through diagonal gaps
/// an ant's body cannot physically pass, and a distance map must never
/// promise a route the mover cannot take.
pub fn rebuild_nest_flow(flow: &mut NestFlow, obstacles: &Obstacles, nests: &Nests) {
    flow.width = obstacles.width;
    flow.height = obstacles.height;
    flow.built_revision = obstacles.revision;
    let cells = obstacles.width * obstacles.height;
    flow.dist = vec![UNREACHABLE; MAX_COLONIES * cells];
    flow.built_nests = [(f32::NAN, f32::NAN); MAX_COLONIES];

    for colony in 0..MAX_COLONIES {
        if colony >= nests.len() {
            continue;
        }
        let nest = *nests.get(colony);
        flow.built_nests[colony] = (nest.x, nest.y);
        let field = &mut flow.dist[colony * cells..colony * cells + cells];
        field.fill(UNREACHABLE);

        // Seed: every passable cell inside the nest's own disk. Delivering
        // ants are "home" anywhere on the mound, so the destination is the
        // mound, not a single cell.
        let mut queue: Vec<usize> = Vec::new();
        for y in 0..obstacles.height {
            for x in 0..obstacles.width {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                if (px - nest.x).hypot(py - nest.y) <= nest.radius
                    && !obstacles.is_blocked(x as i32, y as i32)
                {
                    field[y * obstacles.width + x] = 0;
                    queue.push(y * obstacles.width + x);
                }
            }
        }
        // Plain FIFO BFS. A grow-only Vec with a read cursor acts as the
        // deque, and with all edges costing one step the visit order cannot
        // change the final distances.
        let mut head = 0;
        let width = obstacles.width;
        while head < queue.len() {
            let idx = queue[head];
            head += 1;
            let (x, y) = ((idx % width) as i32, (idx / width) as i32);
            let next = field[idx] + 1;
            for step in [-1i32, 1] {
                for (nx, ny) in [(x + step, y), (x, y + step)] {
                    if nx < 0
                        || ny < 0
                        || nx as usize >= obstacles.width
                        || ny as usize >= obstacles.height
                        || obstacles.is_blocked(nx, ny)
                    {
                        continue;
                    }
                    let n = ny as usize * obstacles.width + nx as usize;
                    if field[n] != UNREACHABLE {
                        continue;
                    }
                    field[n] = next;
                    queue.push(n);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Nest;

    fn nest() -> Nest {
        Nest {
            x: 16.0,
            y: 16.0,
            radius: 2.0,
        }
    }

    fn nest_flow(obstacles: &Obstacles, nest: Nest) -> NestFlow {
        let nests = Nests(vec![nest]);
        let mut flow = NestFlow::new(obstacles.width, obstacles.height);
        rebuild_nest_flow(&mut flow, obstacles, &nests);
        flow
    }

    /// The field rates every passable cell to the nest and marks walls.
    ///
    /// The distance is seeded over the **nest mound** (its whole radius), not
    /// a single cell, so a straight-run cell reads `manhattan − radius` - an
    /// ant inside the mound is "home" at any of those cells already.
    #[test]
    fn the_field_rates_cells_and_marks_walls() {
        let obstacles = Obstacles::new(32, 32);
        let flow = nest_flow(&obstacles, nest());
        assert_eq!(flow.distance(0, 16, 16), 0, "the nest cell is distance 0");
        // (30,16) is 14 cells east; the mound seed (radius 2) covers the
        // first hop, so the BFS reports 13.
        assert_eq!(flow.distance(0, 18, 16), 1, "just off the mound");
        assert_eq!(
            flow.distance(0, 30, 16),
            13,
            "a straight run past the mound"
        );
        assert_eq!(flow.distance(0, -1, 5), UNREACHABLE, "off-map is far");
    }

    /// A helper: a vertical, one-cell-thick wall spanning the full map
    /// height, with a gap over `[gap_range]`.
    fn wall_with_gap(
        width: usize,
        height: usize,
        column: i32,
        gap_range: std::ops::Range<i32>,
    ) -> Obstacles {
        let mut obstacles = Obstacles::new(width, height);
        for y in 0..height as i32 {
            if !gap_range.contains(&y) {
                obstacles.set(column, y, true);
            }
        }
        obstacles
    }

    /// The flow must route around a wall, not into it: a returning ant on the
    /// wrong side of a wall must be pushed towards the gap, and one step
    /// along the flow must strictly descend the distance field.
    #[test]
    fn the_flow_routes_around_a_wall_towards_the_gap() {
        let obstacles = wall_with_gap(48, 32, 20, 8..12);
        let flow = nest_flow(&obstacles, nest());

        // The ant stands east of the wall; its nest is west of the columns.
        let aim = flow
            .direction(0, 30.5, 20.5)
            .expect("a reachable cell must have an aim");
        assert!(
            aim.0 <= 0.2,
            "the flow must not walk back out eastwards: ({}, {})",
            aim.0,
            aim.1
        );
        // Descending the field is the property that matters.
        let target = ((30.5 + aim.0) as i32, (20.5 + aim.1) as i32);
        let here = flow.distance(0, 30, 20);
        assert!(
            flow.distance(0, target.0, target.1) < here,
            "the flow direction did not descend the distance field"
        );
    }

    /// With a gap in the wall the whole passable map knows a way home; a
    /// fully sealed pocket stays unreachable.
    #[test]
    fn every_reachable_cell_gets_a_distance_and_a_sealed_pocket_does_not() {
        let mut obstacles = Obstacles::new(32, 32);
        for y in 0..32 {
            obstacles.set(20, y, true);
        }
        obstacles.set(20, 16, false); // a one-cell gap in the wall
                                      // A sealed pocket: a ring of wall with no entrance.
        for y in 2..=6 {
            for x in 4..=8 {
                obstacles.set(x, y, x == 4 || x == 8 || y == 2 || y == 6);
            }
        }
        let flow = nest_flow(&obstacles, nest());
        assert_eq!(
            flow.distance(0, 6, 4),
            UNREACHABLE,
            "the sealed pocket must stay unreachable"
        );
        for y in 0..32 {
            for x in 0..32 {
                if !(4..=8).contains(&x) || !(2..=6).contains(&y) {
                    if obstacles.is_blocked(x, y) {
                        assert_eq!(flow.distance(0, x, y), UNREACHABLE);
                    } else {
                        assert_ne!(
                            flow.distance(0, x, y),
                            UNREACHABLE,
                            "cell ({x},{y}) lost the way home"
                        );
                    }
                }
            }
        }
    }

    /// Rebuilding the same map must produce the identical field, cell for
    /// cell: the flow field inherits the determinism contract.
    #[test]
    fn rebuilding_the_same_map_is_bit_identical() {
        let obstacles = wall_with_gap(32, 32, 18, 0..12);
        let nests = Nests(vec![nest()]);
        let mut a = NestFlow::new(32, 32);
        let mut b = NestFlow::new(32, 32);
        rebuild_nest_flow(&mut a, &obstacles, &nests);
        rebuild_nest_flow(&mut b, &obstacles, &nests);
        assert_eq!(a.dist, b.dist);
        // `built_nests` holds `NaN` for unbuilt colonies, which fails a plain
        // equality compare; compare with a NaN-aware rule instead.
        for (l, r) in a.built_nests.iter().zip(b.built_nests.iter()) {
            let same = (l.0 == r.0 && l.1 == r.1) || (l.0.is_nan() && r.0.is_nan());
            assert!(same, "nest stamp differs: {l:?} vs {r:?}");
        }
    }
}
