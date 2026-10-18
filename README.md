# Formica — Ant Colony Foraging Simulator

> English | [中文](README.zh-CN.md)

![Formica's badge: an amber ant heading toward a food pile along its pheromone trail](docs/figures/logo.svg)

An ant colony foraging simulator built from scratch in Rust + Bevy. Each ant follows only three rules: **follow the food scent, drop home scent as you go, and turn around when you find food**.
No single ant knows where the food is, and there is no global plan — yet within a few thousand ticks the colony spontaneously forms a stable trail from nest to food.

The point of this repo is not "drawing ants" but **measurability**: the same systems, the same schedule, drive both a renderer-free CLI acceptance run and an interactive Bevy window.

![Formica running, tick 1000](docs/figures/phase2-converged.png)

```
cargo run -p formica-headless --release -- --ticks 5000   # CLI + ASCII curves + parameter sweeps
cargo run -p formica-app     --release                    # window + live tuning panel + map editing
```

In the window you can drag sliders to retune parameters live, resize the map, **drop food with a click**, drag to draw walls, toggle predators, and start a second competing colony. The CLI runs the same configurations and prints numbers. Both the UI and the CLI output support **English and Chinese** (`FORMICA_LANG=zh` or `--lang zh`; the panel also has a live switch).

---

## Contents

- [Acceptance results](#acceptance-results)
- [Quick start](#quick-start)
- [i18n (English / Chinese)](#i18n-english--chinese)
- [Mechanics](#mechanics)
- [Architecture](#architecture)
- [Parameter reference](#parameter-reference)
- [Phase 4 features](#phase-4-features)
- [Implementation notes & lessons learned](#implementation-notes--lessons-learned)
- [Project layout](#project-layout)
- [Testing](#testing)
- [Known limitations / future work](#known-limitations--future-work)

---

## Acceptance results

### Phase 1

> See the delivered-food curve grow as an S-shape within 5000 ticks

**Pass.** Default parameters (500 ants, 256×256, evaporation 0.1, sensor radius 3, random walk 0.3, speed 2 cells/tick), full 5000-tick run:

```
9000 |                                        ######################################
     |                                   ###########################################
     |                               ###############################################
     |                            ##################################################
     |                         #####################################################
     |                       #######################################################
     |                     #########################################################
     |                   ###########################################################
     |                ##############################################################
     |              ################################################################
     |            ##################################################################
     |          ####################################################################
     |        ######################################################################
     |      ########################################################################
     |    ##########################################################################
   0 |##############################################################################
     +------------------------------------------------------------------------------
      tick 0                                                            tick 5000
```

| Metric | Value |
|---|---|
| Food delivered | 9000.0 / 9000.0 units (**100%** recovered) |
| Round trips | 9207 |
| First delivery | tick 60 |
| 10% / 50% / 90% | tick 400 / 1360 / 2630 |
| Peak delivery rate | tick 400 |
| **S-score** | **3.50** |

**Definition of the S-score** (`formica_core::metrics::analyze`):

```
S = peak-interval delivery / mean-interval delivery
```

A straight-line accumulation has peak = mean, score 1.0; an S-curve has a lag phase, a steep phase and a plateau, so the peak far exceeds the mean. Thresholds used here: `>= 2.0` strong S, `>= 1.5` clear S, `>= 1.15` weak S, otherwise linear.

The curve matches the milestones exactly: the first 400 ticks deliver 10% (lag), ticks 400→2630 deliver 80% (steep), after tick 2630 it is nearly flat (plateau).

### Phase 2

> Watch the trails go from chaos to convergence

The pheromone field is a single 256×256 `Image` resource: the CPU rewrites the byte buffer every frame, Bevy re-uploads it, and one sprite covers the whole map with nearest-neighbour sampling. Ants, nests and food piles are plain coloured sprites.

**tick 1000** (`docs/figures/phase2-converged.png`): three trails have formed, from the nest (amber square) to the left-middle, top-right and bottom-right food piles (green squares). White dots are searching ants, orange dots are returning ants carrying food. The violet haze is home pheromone; the teal on the trails is food pheromone.

![Converging colony](docs/figures/phase2-converged.png)

**tick 60** (`docs/figures/phase2-early.png`): the same run earlier. The trails are just thin lines and most ants still wander — the starting point of the "chaos".

![Early colony](docs/figures/phase2-early.png)

Reading the panel's `searching / returning` counters across frames makes the positive feedback visible:

| tick | searching / returning | returning share | delivered |
|---|---|---|---|
| 500 | 331 / 169 | 34% | 15.4% |
| 1500 | 362 / 138 | 28% | 55.9% |
| 3000 | 432 / 68 | 14% | 97.0% |

The returning share peaks at 34% around tick 500: once a trail forms, a larger fraction of ants finds food and hauls it home. It then falls monotonically because the food is nearly gone.

### Scripted screenshots (for unattended verification)

Whether the picture is right cannot be judged by eye. The window app supports environment-driven scripted runs; `scripts/capture-figures.ps1` regenerates every figure in these docs with them:

```bash
FORMICA_SCREENSHOT=docs/figures/shot.png \
FORMICA_SCREENSHOT_FRAME=90 \
FORMICA_TICKS_PER_FRAME=4 \
FORMICA_EXIT_AFTER=115 \
cargo run -p formica-app --release
```

| Variable | Effect |
|---|---|
| `FORMICA_SCREENSHOT` | write one frame to this path |
| `FORMICA_SCREENSHOT_FRAME` | which frame to capture (default 600) |
| `FORMICA_TICKS_PER_FRAME` | fast-forward multiplier at startup |
| `FORMICA_EXIT_AFTER` | frame to quit on |
| `FORMICA_ANTS` / `FORMICA_COLONIES` / `FORMICA_MAP_SIZE` | override population and map |
| `FORMICA_INDIVIDUALITY` | override trait spread (`0` = identical ants) |
| `FORMICA_OBSTACLES` / `FORMICA_BLOCK_DENSITY` / `FORMICA_U_WALL_GAP` | override obstacles |
| `FORMICA_CROWD_THRESHOLD` / `FORMICA_CROWD_RELIEF` | override crowding relief |
| `FORMICA_SHOW_FOOD` / `FORMICA_SHOW_HOME` | `0` hides that field |
| `FORMICA_FOOD_GAIN` / `FORMICA_HOME_GAIN` | override display gains |

These switches exist to **verify** colour claims, not as decoration. A per-pixel count of the rendered PNG (580×580 map area) with `FORMICA_SHOW_HOME=0` shows **2255 green-dominant pixels and 0 violet-dominant pixels** — confirming that "the food trail is the green one" holds on screen, not just in the mapping formula.

### Phase 3

> Dragging a slider changes the trail shape in real time

Every slider writes straight into the `SimConfig` resource, and **every simulation system re-reads it on every tick**. "Hot reload" is not a feature that had to be built; it is a consequence of the data-oriented design — there is no Apply button anywhere in the panel.

Colony count, nests, walls and predators all take effect **at runtime**: `sync_ant_count` / `sync_nests` / `sync_obstacles` / `sync_predator_count` each pull the world back to what the config describes. The one thing that needs a button is map size.

### Phase 4 (optional features)

> Obstacles (U-shaped wall), predators, two competing colonies

All three are implemented, each with a controlled experiment and quantified results — see [Phase 4 features](docs/en-phase4-features.md):

| Feature | Result |
|---|---|
| U-shaped wall | 6761 → **5638** (−16.6%), and the colony still finds a way around |
| Predators (8) | 7887 → **2550** (−67.7%); the cost is time and cargo, not lives |
| Two colonies | one colony of 150: 6161 → two of 75: 6104 (same nest spacing, average cost over four seeds ≈ **3%**) |

---

## Quick start

Requires Rust 1.95+ (verified on 1.97.1). The first Bevy build takes a few minutes.

### Phase 1: renderer-free CLI

```bash
# Default parameters, 5000 ticks, ASCII curve and full report
cargo run -p formica-headless --release -- --ticks 5000

# Export the curve to CSV (tick, delivered, colony0, colony1, searching, returning, ...)
cargo run -p formica-headless --release -- --ticks 5000 --csv docs/curve.csv

# Parameter sweeps: run several settings and compare
cargo run -p formica-headless --release -- --sweep-evaporation 0.005,0.01,0.05,0.1,0.2,0.3
cargo run -p formica-headless --release -- --sweep-random-walk 0.0,0.1,0.3,0.6,1.0
cargo run -p formica-headless --release -- --sweep-predators 0,1,2,4,8
cargo run -p formica-headless --release -- --sweep-individuality 0.0,0.2,0.4,0.6,1.0,1.5

# Individual features
cargo run -p formica-headless --release -- --obstacles u-wall --ticks 2500
cargo run -p formica-headless --release -- --predators 6 --food-per-pile 4000
cargo run -p formica-headless --release -- --colonies 2 --ants 250

# Exit distribution: spray the colony out toward one bearing, in batches
cargo run -p formica-headless --release -- --spawn-mode jet --spawn-direction 90 --spawn-spread 45
cargo run -p formica-headless --release -- --spawn-rate 10 --spawn-initial 20

# A scenario: built-in name or a TOML file; later flags still override it
cargo run -p formica-headless --release -- --scenario labyrinth
cargo run -p formica-headless --release -- --scenario my-run.toml

# Share a set-up: write any built-in (or your flag-overridden variant of it)
# to a complete TOML — walls, food, geometry, seed all included
cargo run -p formica-headless --release -- --scenario labyrinth --export-scenario share-labyrinth.toml

# All options
cargo run -p formica-headless --release -- --help
```

Exit code: `0` if any colony delivered food, `1` if none did — usable directly as a CI check.

### Scenarios

A scenario is a shareable TOML file: parameter overrides **plus an explicit map** (wall rectangles, food spots). It is what makes a custom map reproducible — scenario + seed runs bit for bit.

| Name | What it is |
|---|---|
| `classic` | the brief's defaults: 3 seeded piles, open field |
| `duel` | two colonies, private trails, one shared food ring |
| `labyrinth` | 45% walls — the maze the crowding relief exists for |
| `fortress` | food walled into a U, three predators hunting |
| `arid` | fast evaporation, sparse colony: hard mode |
| `long-haul` | 512×512 map, eight piles, one long caravan |
| `maze-run` | hand-built corridors and a fixed feast at the far end |

`--scenario <name-or-file>` (CLI), the **Scenario** section of the panel (dropdown to load, export/import buttons with a file path box), or `FORMICA_SCENARIO=fortress` for scripted runs. The window also gained a refined look: ants are elongated bodies that rotate with their heading, nests are mounds, food piles are soft discs, walls catch a low sun, and the ground reads as soil instead of a black void.

Exports are complete and feed straight back into imports: **export preset** serialises the built-in shown in the dropdown (walls, food spots, U-wall geometry, seed included), **export current set-up** captures what is on screen plus every pile you dropped **and walls you drew with the brush — erasures included**: a hand-edited map is decomposed into explicit wall rectangles in the file, while a map you never touched keeps the compact preset keyword. **import from file** reads any of those TOMLs back through the same control box. The CLI equivalent is `--export-scenario <path>`; re-exporting an imported file is byte-identical, which is the reproducibility contract.

### Phase 2 + 3 + 4: interactive window

```bash
cargo run -p formica-app --release
```

**Mouse** (mode and master switch in the `Map edit` section):

| Action | Effect |
|---|---|
| Left click on the map | apply the current mode: drop food / draw wall / erase |
| Left drag | draw walls continuously (food drops only on press, otherwise food would become a function of frame rate) |
| Right click or drag | erase: remove the food pile under the cursor and clear walls in the brush |

Left panel:

| Section | Contents |
|---|---|
| Status | tick, delivered, **per-colony totals**, round trips, searching/returning, ants eaten, FPS |
| Delivery curve | live cumulative curve (**two lines with two colonies**) + S-score |
| Run | icon buttons: pause/resume (two states), reset, **clear trails**, ticks/frame fast-forward |
| Presets & scenarios | seven reproducible comparison parameter sets, then **built-in scenarios (dropdown to load)**, export current set-up / selected preset as TOML, **import from file** |
| World & food | **map size (drag, then press apply size)**, pile count / total / radius |
| Map edit | map-editing switch (mode and brush grey out while it is off), mode, brush radius, food per new pile |
| Obstacles | open field / U-shaped wall / scattered blocks; sizes and density |
| Colonies | **colony count 1–2**, nest spacing |
| Pheromones | evaporation (log slider), deposit strength, kernel radius, cell ceiling |
| Ants | ants per colony, **exit distribution** (uniform disk / directed jet: direction + spread; batched release: emerge-per-tick + initial count), speed, perception radius, antenna spread, box radius, random walk, trail gain, home-trail guidance, **individuality** |
| Crowding | **jam threshold, relief step**, plus a live "ants in the busiest 4×4 bin" readout |
| Predators | **count**, speed, sight radius, catch radius, wander |
| Display | food/home trail gains and visibility |
| Language | **English / 中文, live switch** |

Map size needs a button rather than a live slider because resizing reallocates the pheromone texture and rebuilds the scene; doing that per pixel of a slider drag would be unusable. Everything else is immediate.

### Testing

```bash
cargo test --release                                            # all unit + integration tests
cargo test -p formica-core --release -- --ignored --nocapture   # full 5000-tick acceptance (prints the curve)
```

---

## i18n (English / Chinese)

The UI and CLI output both have English and Chinese copies of every user-facing string, centralised in the two tables (`EN` / `ZH`) of `formica_core::i18n` — no macros, no external dependencies, and a missing translation fails the build's tests.

| Surface | Default | How to switch |
|---|---|---|
| Window panel (egui) | English | the Language combo at the top of the panel, **live**; or set `FORMICA_LANG=zh` before launching |
| CLI (help / progress / report / sweeps) | English (the Windows console's default code page cannot render CJK) | `--lang zh` or `FORMICA_LANG=zh` |

Details:

- egui's default fonts contain no CJK glyphs. At startup the window app scans common system CJK fonts (Microsoft YaHei / SimHei / Noto Sans CJK / PingFang) and registers one as an egui **fallback font** — the English UI looks unchanged and Chinese renders instead of tofu. Set `FORMICA_CJK_FONT` if your font lives somewhere unusual.
- The Chinese report's column alignment is designed for display width and reads best in a UTF-8 terminal (Windows Terminal, `chcp 65001`).
- The i18n module ships 3 tests: both tables are field-by-field non-empty, the two help texts list identical flags, and the Chinese table is not a copy of the English one. A missing translation breaks the tests immediately.

---

## Mechanics

The four "whys" — two pheromones, why it converges, why ants are not identical, why three antennae instead of a gradient — live in **[Mechanics](docs/en-mechanics.md)**.

One-paragraph version: two separate pheromone fields turn "scent" into a directional signal (no ant ever writes the field it follows); convergence comes from the deposit/evaporation positive-feedback balance; individual differences (`AntVigor`) break the symmetry so the colony does not expand as a hollow ring; three-point antenna sampling performs **trail alignment**, not gradient ascent.

---

## Architecture

Data-oriented design, the eight ant components, the resource list, the 16-system tick schedule order, and why an independent schedule — see **[Architecture](docs/en-architecture.md)**.

The core promise: `formica-core` depends only on `bevy_ecs` + `bevy_app`, never on rendering; the CLI and the window run **the same systems in the same order** — the numbers measured in Phase 1 are predictions for the Phase 2 picture.

Versions: **Bevy 0.19.1** + **bevy_egui 0.42.0** (egui 0.36).

---

## Parameter reference

| Parameter | Default | CLI | Description |
|---|---|---|---|
| `ant_count` | 500 | `--ants` | ants **per colony** (doubled with two colonies) |
| `colony_count` | 1 | `--colonies` | competing colonies, 1–2 |
| `nest_orbit` | 55 | `--nest-orbit` | nest distance from map centre with two colonies |
| `spawn_mode` | disk | `--spawn-mode` | how ants leave the nest: `disk` (uniform disc, radial headings — brief behaviour) / `jet` (cone around one bearing) |
| `spawn_direction` | 0 | `--spawn-direction` | jet bearing in degrees, 0 = east (+x), counter-clockwise |
| `spawn_spread` | 60 | `--spawn-spread` | jet cone width in degrees, 0–360; at 360 the distribution is the uniform disk again |
| `spawn_rate` | 0 | `--spawn-rate` | ants emerging from the nest per tick (0 = all at once) |
| `spawn_initial` | 0 | `--spawn-initial` | population at tick 0 while `spawn_rate` > 0; the rest emerges at the rate |
| `width` × `height` | 256×256 | `--width/--height` | map size |
| `evaporation_rate` | 0.1 | `--evaporation` | per tick, `c *= (1 - r)` |
| `sensor_distance` | 3.0 | `--sensor-distance` | perception radius (cells) |
| `sensor_angle` | 0.6 | `--sensor-angle` | antenna half-angle (radians) |
| `sensor_box_radius` | 1 | `--sensor-box` | box-average radius per antenna |
| `random_walk_strength` | 0.3 | `--random-walk` | heading jitter per tick |
| `speed` | 2.0 | `--speed` | movement per tick (cells) |
| `deposit_amount` | 1.0 | `--deposit` | pheromone laid per ant per tick (total) |
| `deposit_radius` | 1 | `--deposit-radius` | deposit kernel radius (mass conserving) |
| `sensor_turn_gain` | 0.6 | `--turn-gain` | turn toward the best antenna |
| `max_turn_rate` | 0.8 | — | max heading change while homing |
| `home_pheromone_weight` | 0.0 | — | returning ants following home scent (0 = brief behaviour) |
| `homing_mode` | path | `--homing` | returning-ant navigation: `path` (direct, brief) / `wall` (whiskers sense walls ahead and escape jams) / `flow` (BFS flow field to the nest) |
| `individuality` | 0.6 | `--individuality` | **per-ant trait spread** (log-normal sigma; 0 = clones) |
| `crowd_threshold` | 24 | `--crowd-threshold` | ants per 4×4 bin counted as a jam |
| `crowd_relief` | 0.6 | `--crowd-relief` | cells stepped out of a jam per tick (0 = off) |
| `food_pile_count` | 3 | `--piles` | food piles (1–8) |
| `food_per_pile` | 3000 | `--food-per-pile` | food units per pile |
| `food_pile_radius` | 6.0 | `--pile-radius` | pile radius (cells) |
| `ant_capacity` | 1.0 | — | load one ant can carry |
| `obstacle_preset` | None | `--obstacles` | `none` / `u-wall` / `blocks` |
| `predator_count` | 0 | `--predators` | predators, 0 = off |
| `predator_speed` | 1.6 | `--predator-speed` | cells per tick |
| `predator_sense` | 26 | `--predator-sense` | sight radius (cells) |
| `predator_catch_radius` | 3 | `--predator-catch` | catch radius (cells) |
| `max_pheromone` | 50.0 | — | per-cell concentration ceiling |
| `seed` | 20240517 | `--seed` | RNG seed (same seed = bit-identical run) |

---

## Phase 4 features

Obstacles (U-shaped wall), predators and two competing colonies — each with a controlled experiment and quantified results. Full data and screenshots: **[Phase 4 features](docs/en-phase4-features.md)**.

| Feature | Finding |
|---|---|
| U-shaped wall | −16.6% yield, but the colony still routes around — a direct demonstration of wrong paths being evaporated away |
| Predators | 8 predators push yield to 28.3%; the cost is time and cargo, not lives (nests are a refuge) |
| Two colonies | ≈ **3%** average loss over four seeds at the same nest spacing — splitting up halves the scent budget |
| All three on | no conflicts, runs on one screen (`docs/figures/p4-everything.png`) |

---

## Implementation notes & lessons learned

The three visual-quality fixes that came after the project worked (the ring, leftover food piles, maze crowding), the assumptions overturned by data, two tests that "passed but proved nothing", and the f32 bookkeeping precision issue — these are split into two documents:

- **[Three visual-quality fixes](docs/en-fixes-notes.md)**: each one reproduced first, quantified second, fixed last, with before/after numbers and screenshots.
- **[Experiments & findings](docs/en-experiments.md)**: the pheromone control experiment, evaporation / random-walk / individuality sweeps, two real bugs caught by the conservation invariant, the display-gain "optimisation" refuted by pixel counts, and "a test that measured at the wrong moment".

---

## Project layout

```
Formica/
├── Cargo.toml                      # workspace
├── crates/
│   ├── formica-core/               # pure logic, only bevy_ecs + bevy_app
│   │   ├── src/
│   │   │   ├── lib.rs              # module map + prelude + doc example
│   │   │   ├── config.rs           # SimConfig + ObstaclePreset
│   │   │   ├── scenario.rs         # Scenario: TOML shareable runs (overrides + custom map)
│   │   │   ├── grid.rs             # PheromoneGrid: two fields per colony + evaporate/sample/deposit (wall-aware)
│   │   │   ├── world.rs            # Nests, Nest, FoodField (add/remove piles), FoodPile
│   │   │   ├── obstacles.rs        # Obstacles: walls + U-wall/blocks presets + sync system
│   │   │   ├── crowd.rs            # CrowdGrid: 4×4 coarse density grid + emptiest-neighbour direction
│   │   │   ├── motion.rs           # boundary reflection + wall sliding + escape (ants and predators)
│   │   │   ├── predator.rs         # predator components + spawning
│   │   │   ├── ant.rs              # eight ant components + AntVigor individuality + state machine
│   │   │   ├── rng.rs              # SplitMix64 + Box-Muller normal (deterministic, zero deps)
│   │   │   ├── metrics.rs          # SimStats (per-colony books + density) + curve analysis + ASCII chart
│   │   │   ├── i18n.rs             # EN/ZH string tables + Lang
│   │   │   ├── plugin.rs           # SimPlugin, SimulationTick schedule, init/reset
│   │   │   └── systems/            # one file per stage
│   │   │       ├── sense.rs  decide.rs  movement.rs  deposit.rs  evaporate.rs
│   │   │       ├── crowd.rs  interact.rs  spawn.rs  stats.rs  predator.rs  obstacles.rs  flow.rs
│   │   ├── tests/
│   │   │   ├── phase1_acceptance.rs  # acceptance + conservation invariants + pheromone control
│   │   │   ├── phase4_features.rs    # obstacles/predators/two colonies + map resize + player food
│   │   │   └── emergence_shape.rs    # regression tests for the three visual fixes
│   ├── formica-headless/           # CLI, zero rendering
│   │   └── src/main.rs             # arg parsing, ASCII curves, CSV, per-colony report, sweeps
│   └── formica-app/                # Bevy window
│       └── src/
│           ├── main.rs             # plugin assembly + SimulationTick driver + scripted screenshots
│           ├── lang.rs             # panel language resource + CJK fallback font registration
│           ├── paint.rs            # concentration/walls → pixels (pure function, unit tested)
│           ├── render.rs           # camera, world texture, sprite sync, pile scaling, resize rebuild
│           ├── interact.rs         # mouse → drop food / draw walls / erase
│           └── ui.rs               # egui panel + hand-drawn curve + presets
├── scripts/
│   ├── capture-figures.ps1         # regenerate every figure in docs/
│   └── measure-readme.ps1          # re-measure every number quoted in the READMEs
└── docs/
    ├── phase1-default.csv          # full curve of the default 5000-tick run
    ├── en-*.md / zh-*.md           # notes split out of the READMEs (mechanics, architecture, ...)
    └── figures/*.png               # run screenshots
```

---

## Testing

```
cargo test --release
```

All unit + integration tests (plus one `#[ignore]` full 5000-tick acceptance, run with `-- --ignored`), covering grid semantics, ant behaviour, individuality, crowding, movement and walls, obstacles, predators, two colonies, the food system, food rendering, homing navigation modes, map resize, statistics, scheduling, integration invariants, shape regressions, rendering, interaction and i18n.

The full write-up of the test design (the conservation law, control experiments as tests, the row flip, the phantom-pile regression, "measure displacement, not yield", "the control group self-check") lives in **[Experiments & findings](docs/en-experiments.md)**.

---

## Known limitations / future work

- **At most two colonies**: `MAX_COLONIES = 2`. The data structures (flat `Vec` chunks + `AntColony` indices + per-colony books) have no such ceiling; raising it to 3 or 4 just adds 512 KiB of fields and a colour per colony.
- **Two-colony fairness depends on the seed**: nests are symmetric about the map centre, but food is scattered on a ring by seed, so a single seed is never perfectly fair.
- **Individuality is single-axis**: one "boldness" value drives both wander and speed.
- **Crowding relief treats crowding, not blockage**: at 45% wall density the terrain itself does not percolate and ants cannot get out — a map problem, not a rules problem.
- **Ants have no individual memory**: `AntVigor` is a fixed temperament drawn at spawn, not "I have been here" memory, and it is not learning.
- **Homing is a straight heading**: per the brief; `home_pheromone_weight` already exposes the follow-the-scent switch. For walls between an ant and its nest, `--homing wall` (whiskers + jam escape) and `--homing flow` (BFS nest flow field) are the optional answers; the default stays brief-exact and bit-identical (measured at 1500 ticks over a one-gap wall map: direct 344 units delivered, wall-sense 452, flow-field 2020).
- **Predators neither tire nor satiate**: predation pressure is constant.
- **The U-shaped wall wraps only the first food pile**: to keep experiments reproducible.
- **Map size cannot be dragged continuously**: drag, then press apply size, because resizing reallocates the texture and rebuilds the scene.
