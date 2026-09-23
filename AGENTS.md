# AGENTS.md — Formica

> Instructions for AI coding agents (Copilot, Claude, Cursor, ...). Human
> readers: see [README.md](README.md) / [README.zh-CN.md](README.zh-CN.md).

## Overview

- **Goal**: Formica — a pheromone-based ant colony foraging simulator written
  from scratch in Rust. Each ant runs three rules (follow food scent, lay home
  scent, turn back when food is found); trails emerge from positive feedback.
- **Non-negotiable property**: the project is *measurable*. The same systems in
  the same schedule drive both the renderer-free CLI and the Bevy window. Any
  change must keep the two paths identical.

## Project structure

```
Cargo.toml                     # workspace (resolver = "2")
crates/
  formica-core/                # pure logic; depends ONLY on bevy_ecs + bevy_app
    src/
      config.rs                # SimConfig (every tunable) + ObstaclePreset
      scenario.rs              # Scenario: TOML shareable runs (overrides + custom map)
      grid.rs                  # PheromoneGrid: two fields per colony, wall-aware
      world.rs                 # Nests, FoodField, FoodPile
      obstacles.rs             # Obstacles + U-wall/blocks presets + scenario walls
      crowd.rs                 # CrowdGrid: 4x4 coarse density grid
      motion.rs                # bounds reflection + wall sliding (ants & predators)
      ant.rs / predator.rs     # components
      rng.rs                   # SplitMix64 + Box-Muller, deterministic, zero deps
      metrics.rs               # SimStats, curve analysis, ASCII chart, verdicts
      i18n.rs                  # EN/ZH string tables + Lang (all UI/CLI strings)
      plugin.rs                # SimPlugin + SimulationTick schedule
      systems/                 # one file per tick stage
    tests/                     # phase1_acceptance, phase4_features, emergence_shape
  formica-headless/            # CLI: args, scenarios, ASCII curves, CSV, report, sweeps
  formica-app/                 # Bevy window: render, interact, ui, lang, paint
scripts/
  capture-figures.ps1          # regenerates every docs/figures/*.png via env-var scripting
  measure-readme.ps1           # re-measures numbers quoted in the READMEs
docs/                          # figures, CSV, and long-form notes (en-*/zh-*)
```

## Key commands

```bash
cargo build --release                     # full build
cargo run -p formica-headless --release -- --ticks 5000   # CLI acceptance run
cargo run -p formica-app --release                        # interactive window
cargo test --release                      # all unit + integration tests
cargo test -p formica-core --release -- --ignored --nocapture  # 5000-tick acceptance
```

## Conventions

- **Comments and doc comments in English.** Code, configs, scripts — all new
  prose in source files is English. User-facing *display* strings are the one
  exception and must be bilingual (see i18n below).
- **All user-facing strings go through `formica_core::i18n`.** Add the field to
  `Strings`, fill both `EN` and `ZH` tables, and route widgets/prints through
  it. The i18n tests enforce non-empty values in both tables; a missing ZH
  translation fails `cargo test`.
- **Determinism matters.** Same seed + same config = bit-identical run. Never
  introduce `rand`, system time, or iteration over `HashMap`/`HashSet` into
  simulation code; use `SimRng`. Count-based ordering (components, entities)
  must not leak into behaviour.
- **No new dependencies in `formica-core`** without discussion — it must stay
  render-free and light. The one sanctioned exception is `serde` + `toml`, in
  service of scenario files only: a shareable scenario needs a real
  serializer, and hand-rolling a TOML parser is worse than a small data-only
  dependency. Nothing GUI-related may enter the crate.
- **Scenarios are deterministic.** A scenario is a pure diff onto `SimConfig`
  (overrides + wall rectangles + food spots); it must never consult the RNG or
  wall-clock time, so scenario + seed reproduces bit for bit. Its food spots
  replace the seeded ring at init/reset (`spawn_food`), and its walls stamp
  through `apply_preset` (via `custom_walls`), so no scenario-specific world
  building code path exists.
- **3D is a planned future phase**, not a licence to put rendering into the
  core. When it comes, it is a second viewer crate over the same resources;
  the renderer stays read-only with respect to the simulation.
- **Comments explain *why***, especially when a number was measured (tolerances,
  thresholds, tuning defaults). If you change a measured constant, re-measure
  and update the comment.
- Windows-first environment (PowerShell); commands above work in any shell.

## Architecture invariants (do not break)

1. **Schedule purity**: `SimulationTick` is never run by the engine. The CLI
   calls `run_ticks` directly; the app drives it from `Update`. Never add
   simulation logic to `Update`/`FixedUpdate`.
2. **Allocation contract**: `PheromoneGrid` is always sized for
   `MAX_COLONIES`, never the current colony count (otherwise enabling colony 2
   aliases colony 1's fields — a silent shared-scent bug). Test
   `the_field_does_not_shrink_when_a_colony_leaves` guards this.
3. **Numerics**: per-cell concentrations and per-ant loads are `f32`; all
   cumulative bookkeeping (`SimStats` totals) is `f64`. The conservation test
   tolerance is `initial × 1e-5`; do not loosen it.
4. **Component-only state**: ants are eight components (`AntMarker`,
   `AntColony`, `AntPos`, `AntHeading`, `AntState`, `Carrying`,
   `PheromoneSensors`, `AntVigor`), not a `Vec<Ant>`. `AntVigor` is written
   once at spawn and never mutated.
5. **Presets use real data**: `Obstacles::apply_preset` receives the *actual*
   food piles; never regenerate food from a derived seed inside a helper (this
   caused the "phantom pile" bug — see docs/en-experiments.md #9).
6. **Predation order**: `predators_catch_ants` runs before `sync_ant_count`,
   so eaten ants are replaced within the tick and totals stay comparable.
7. **Per-colony isolation**: each colony reads/writes only its own pheromone
   fields and books; colonies never smell each other. The two-colony tests
   assert trails stay separate.

## Testing expectations

- Every behavioural change ships with a test that could fail. Control
  experiments assert **direction and magnitude** and must include an
  **unsaturated assertion** on both arms (`delivered < food_initial * 0.95`) —
  a saturated comparison is zero evidence.
- Tests measuring transients (e.g. the startup ring) must measure at a tick
  where the phenomenon exists (tick 30, not 600) and must carry a **control
  group self-check** asserting the control still shows the phenomenon.
- Prefer invariant tests (food conservation at every checkpoint) over
  end-to-end yield numbers, which are chaotic.
- Renderer tests assert on pixel values, not on "looks right"; remember the
  row flip (grid row 0 = image's last row).

## i18n quick reference

- Window app: `UiLang` resource, seeded from `FORMICA_LANG`, switchable live
  from the panel. CJK fallback font registered in `crates/formica-app/src/lang.rs`
  (`FORMICA_CJK_FONT` overrides the search path).
- Headless: `--lang en|zh` or `FORMICA_LANG`; defaults to English because the
  Windows console code page cannot render CJK.
- When adding a config field with a UI slider: add the slider label + hover
  hint to both `EN` and `ZH` in `i18n.rs`, and add the CLI flag description to
  **both** help texts (the flags-parity test will catch a missing one).

## Documentation layout

- `README.md` (English, default) and `README.zh-CN.md` are mirrors; keep the
  acceptance numbers, parameter tables and quick-start commands in sync.
- Long-form content (mechanics, architecture, Phase 4 details, experiments and
  pitfalls) lives in `docs/en-*.md` / `docs/zh-*.md`; the READMEs link to them.
  When you change simulation behaviour, update the corresponding docs *and*
  re-run `scripts/measure-readme.ps1` so quoted numbers stay honest.
- Figures in `docs/figures/*.png` are regenerated by
  `scripts/capture-figures.ps1`; do not hand-edit them.

## Commit messages

Format: `type: short description` — `type` is one of `feat`, `fix`, `refactor`,
`docs`, `test`, `chore`. Examples:

```
feat: add crowding relief with a soft jam threshold
fix: home-bound ants with a partial load now return to the nest
docs: split experiments and pitfalls into docs/en-experiments.md
```

## Pre-commit checklist

1. `cargo fmt` (no rustfmt.toml overrides; default style).
2. `cargo build --release` — clean, no new warnings.
3. `cargo test --release` — all green, including i18n parity and conservation.
4. If UI-visible behaviour changed: re-capture affected `docs/figures/*.png` via
   `scripts/capture-figures.ps1`.
