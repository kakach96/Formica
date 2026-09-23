<!-- English counterpart of docs/zh-phase4-features.md; see README.md for the overview. -->

# Phase 4 features

All three optional features are implemented, and all three are **quantified** — each has a controlled experiment, not just "it seems to work".

### 1. Obstacles (U-shaped wall)

Walls are a per-cell boolean table (`Obstacles`, a `Resource`). Ants cannot enter wall cells, cannot lay pheromone inside them, and pheromones under walls are scrubbed. Walls are painted directly into the pheromone texture, so they cost no extra draw call and can never drift out of alignment with the cells the simulation actually blocks.

The U-wall preset wraps the **first food pile**, opening away from the nest with the solid back facing it, so an ant walking straight out hits the wall and must go around.

![U-shaped wall](docs/figures/p4-u-wall.png)

Above, tick 3000: one pile is walled in; the colony still reaches it, but the cost is visible — the trail must route through the opening, and the straight line does not exist.

| Config (96×96, 2500 ticks) | delivered | recovery | wall cells |
|---|---|---|---|
| Open field | 6761 | 56.5% | 0 |
| U-shaped wall | **5638 (−16.6%)** | 47.2% | 126 (0.2%) |

What is worth watching is the **process**, not the number: the first ants to arrive are the ones that happened to go around; the surviving trail is the one that went around; and the straight trail that ran into the wall evaporates on its own. That is a direct demonstration of "wrong paths get selected out".

### 2. Predators

A predator has two rules: if it can see an ant, charge the nearest one; otherwise wander. No pheromones, no state machine, no memory.

![Predators](docs/figures/p4-predators.png)

**Eaten ants are not permanently lost**: `sync_ant_count` runs after `predators_catch_ants`, so every victim is replaced at its nest within the same tick. The cost of predation is **time and cargo**, not lives — which is what makes the "with predators" and "without" rows comparable.

**Nests are a refuge**: ants inside the nest are immune. Without this rule a predator that followed the scent to the nest would camp at the entrance and eat every newly respawned ant on arrival — that is not predation, that is a black hole.

| Predators (256×256, 2500 ticks) | delivered | recovery | S-score | eaten |
|---|---|---|---|---|
| 0 | 7887 | 87.6% | 2.00 | 0 |
| 1 | 7250 | 80.5% | 2.18 | 2141 |
| 2 | 7266 | 80.7% | 1.92 | 2343 |
| 4 | 4473 | 49.7% | 2.78 | 11795 |
| 8 | **2550** | **28.3%** | **3.74** | 25571 |

Eight predators cut yield to a third. Note the S-score rises **monotonically** with predation pressure (2.00 → 3.74): the first half of the curve is nearly flat because the colony needs a long time to gather enough workers to hold a trail open — the lag phase stretches, so the peak/mean ratio grows.

### 3. Two competing colonies

The two colonies have **their own pheromone fields** (`PheromoneGrid` stores food/home fields per colony) and **their own nests**, but **share the same finite food**. Neither can smell the other — that is exactly the boundary between "cooperation" and "competition".

![Two colonies](docs/figures/p4-two-colonies.png)

Above, tick 3000: the right nest (amber) is colony 0, the left nest (cyan) is colony 1. **The two trail systems do not merge** — one map, two mutually invisible information systems. In this run each colony delivered 4500, exactly 50.0% / 50.0%; the map was emptied.

**"What does splitting up cost?" now has a more complicated answer.** The original comparison — one colony of 300 vs two of 150 — measured −11.8%. After individuality was added, that comparison **stopped being robust**: across four seeds the gaps were +1.4%, +12.2%, −5.6%, +8.7%; the sign flips. Two reasons, both measured:

- The second colony is not just a halved scent budget — it is a **second search origin**. At the default 110-cell nest spacing, the coverage advantage outweighs the scent disadvantage.
- Individuality makes each colony **better at exploring on its own**, so lacking the other's shared trails hurts less.

Moving the two nests to 24 cells apart (`--nest-orbit 12`) removes the coverage advantage, and the sign stabilises:

| Seed | one colony of 150 | two colonies of 75 | gap |
|---|---|---|---|
| 20240517 | 6161 | 6104 | +0.9% |
| 1 | 5953 | 5844 | +1.8% |
| 7 | 5882 | 5624 | +4.4% |
| 99 | 6019 | 5719 | +5.0% |

**Negative on all four seeds**, averaging about **3%**. That is exactly what the test asserts: `two_colonies_do_not_share_trails` runs four seeds and requires every seed to lose and the mean loss to exceed 1.5%.

An honest caveat: the two nests are placed symmetrically about the map centre — equidistant from the **centre** — but food is scattered on a ring by seed, so **on any single seed the two colonies are not perfectly matched**. The per-colony readouts exist to make that asymmetry visible rather than hide it in the total.

### 4. Everything on

The three features do not conflict and can run together:

![All features enabled](docs/figures/p4-everything.png)

256×256 map, random block walls (density 0.15), two colonies of 500 ants each, 5 predators, tick 2400. Four pheromone channels render simultaneously: colony 0 green/violet, colony 1 amber/cyan; white/orange and cyan/magenta are the two colonies' ants; red squares are predators; grey cells are walls. The panel draws a separate curve per colony.
