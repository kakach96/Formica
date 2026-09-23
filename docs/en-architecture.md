<!-- English counterpart of docs/zh-architecture.md; see README.md for the overview. -->

# Architecture

### Data-oriented design

The brief requires "ant data stored as Components, not a standalone `struct Vec`". So ants are not a `Vec<Ant>` but **eight separate components**:

| Component | Meaning | Written by |
|---|---|---|
| `AntMarker` | marker | — |
| `AntColony(usize)` | which colony: decides which pheromone fields to read/write and which nest to return to | fixed at spawn |
| `AntPos { x, y }` | continuous coordinates | `move_ants`, `relieve_crowding` |
| `AntHeading { angle }` | heading (radians) | `decide_heading`, `move_ants`, `relieve_crowding` |
| `AntState` | state machine: `Searching` / `Returning` | `pickup_food`, `deliver_food` |
| `Carrying { amount }` | carried load | `pickup_food`, `deliver_food` |
| `PheromoneSensors { left, center, right }` | three antenna readings | `sense_pheromones` |
| `AntVigor { wander, speed }` | **individuality**: this ant's own wander and speed multipliers | fixed at spawn, never changed |

`AntVigor` is the only "individuality" in the codebase, and the only component **written exactly once at spawn and never touched again**. Why it exists: [visual fix #1](en-fixes-notes.md) — without it, a population of identically-ruled ants is radially symmetric by mathematical necessity.

Predators are likewise split into four components (`PredatorMarker` / `PredatorPos` / `PredatorHeading` / `PredatorAlert`).

Resources:

| Resource | Contents |
|---|---|
| `SimConfig` | every tunable parameter |
| `PheromoneGrid` | **two `f32` fields per colony**, packed in one flat `Vec` (256×256×2 colonies×2 channels) |
| `Obstacles` | per-cell wall flag + shape version counter |
| `FoodField` | food per cell + pile info + total bookkeeping (including player deletions) |
| `Nests` | each colony's nest position and radius |
| `CrowdGrid` | 4×4 coarse ant counts + peak + occupied bin count (shared by diagnostics and crowding relief) |
| `SimRng` | SplitMix64 deterministic RNG |
| `SimStats` | statistics, **per-colony books**, delivery curve, density readings |

Two design details worth calling out:

- **`PheromoneGrid` is always allocated for the maximum colony count.** Colony count is runtime-changeable; if the fields were sized to the current count, enabling a second colony would clamp colony 1's index onto colony 0's field — the two colonies would silently **share one set of scents**, with no error anywhere. The spare fields cost 256 KiB each and one slightly longer evaporation sweep.
- **`Carrying.amount` is `f32`; the cumulative books are `f64`.** The former is read once and discarded; the latter accumulates across tens of thousands of ticks.

### System breakdown

The brief requires sense / decide / move / deposit / evaporate as separate stages. In practice there are **16 systems**, chained in a **standalone `SimulationTick` schedule**:

```
sync_nests              rebuild nests when the colony count changes (else both colonies share one nest)
      ↓
sync_obstacles          re-stamp walls from the preset (geometry lives in the preset, so slider drags rebuild automatically)
      ↓
scrub_obstacle_pheromones  zero pheromones under walls
      ↓
sync_predator_count     predator count follows the config
      ↓
evaporate_pheromones    c *= (1 - rate), all colonies in one sweep
      ↓
sense_pheromones        three antennae → PheromoneSensors (reads only its own colony)
      ↓
decide_heading          sensors + individual noise + homing → AntHeading
      ↓
move_ants               AntHeading × individual speed → AntPos (boundary reflection + wall sliding)
      ↓
move_predators          chase the nearest ant, else wander
      ↓
tally_crowding          ant positions → 4×4 coarse density grid
      ↓
relieve_crowding        ants in over-dense bins step toward the emptiest neighbour
      ↓
deposit_pheromones      AntPos + state → its own colony's fields (the deposit kernel also avoids walls)
      ↓
pickup_food             step on food → Carrying, state := Returning
      ↓
deliver_food            reach **your own** nest → credit your colony's books, state := Searching
      ↓
predators_catch_ants    eat ants close enough (nest is a refuge)
      ↓
sync_ant_count          replace the eaten — after predation, so counts are exact at the tick boundary
      ↓
update_stats            counters + curve samples
```

The two crowding systems sit between movement and deposition deliberately: `tally_crowding` must see this tick's final positions, and `relieve_crowding` must land before deposition so that a displaced ant leaves scent where **it actually stands**.

**Why a standalone schedule rather than `Update` / `FixedUpdate`**: the engine **never runs this schedule by itself**. The CLI runs `world.run_schedule(SimulationTick)` × N directly — zero engine overhead, completely decoupled from wall-clock time; the window runs it N times inside `Update`, so "fast forward" is just a counter, not a different physics. Both paths execute **the same systems in the same order** — the numbers measured in Phase 1 are predictions for the Phase 2 picture.

### Dependencies

`formica-core` depends only on `bevy_ecs` + `bevy_app`, **never on rendering**. So the Phase 1 acceptance run needs no GPU, no window, and runs in CI. Only `formica-app` pulls in full Bevy (only the `2d` + `ui` features; no 3D, no audio).

Versions: **Bevy 0.19.1** + **bevy_egui 0.42.0** (egui 0.36).
