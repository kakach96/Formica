<!-- English counterpart of docs/zh-experiments.md; see README.md for the overview. -->

# Experiments and findings

### 1. Control experiment: do pheromones actually help?

Running the defaults once and seeing an S-curve **does not prove the S-curve is caused by pheromones**. So I ran a controlled experiment: identical parameters, identical seed, with the second group setting `deposit_amount = 0` and `sensor_turn_gain = 0` (pure random walk, no pheromones at all).

Scale: 128×128, 150 ants, 3 piles × 2000 units, 4000 ticks.

| | Total delivered | 50% milestone | S-score |
|---|---|---|---|
| **With pheromones** | **6000** (all) | **tick 1510** | 2.37 |
| Without | 2688 | tick 2050 | 2.68 |

The pheromone group delivered **2.23×** more food and reached the halfway point **540 ticks** earlier.

**But there is an honest surprise here**: the no-pheromone group scored **2.68** — higher than the pheromone group, and far above the 1.5 "S-curve" threshold. I had assumed "no pheromones should look linear"; the code proved me wrong.

The reason is not hard to see: the ants all start at the nest, bunched up, and spread outward over time — **the new area covered per tick grows**, so even pure random walking finds food at an accelerating rate.

Conclusion: `sigmoid_score` measures "is it accelerating", **not** "is it self-organising". To show pheromones work you must look at the **control** (total delivered, t50), not at one curve's S-score. This is written into the doc comments of `metrics.rs` and into the integration tests so it cannot be misread later.

### 2. Evaporation sweep: a counter-intuitive result

```bash
cargo run -p formica-headless --release -- --sweep-evaporation 0.005,0.01,0.03,0.05,0.1,0.2,0.3
```

**Default scale (500 ants, 3 piles × 3000, 5000 ticks) — evaporation barely matters:**

| Evaporation | Delivered | Recovery | S-score | First tick | Curve |
|---|---|---|---|---|---|
| 0.005 | 9000.0 | 100.0% | 3.90 | 53 | `___.--==+***###@@@@@@@@@@@@@@@@@@@` |
| 0.010 | 9000.0 | 100.0% | 3.88 | 53 | `___.--==++*###@@@@@@@@@@@@@@@@@@@@` |
| 0.030 | 9000.0 | 100.0% | 3.64 | 53 | `___.--==++***###@@@@@@@@@@@@@@@@@@` |
| 0.050 | 9000.0 | 100.0% | 3.67 | 53 | `___.--==++**####@@@@@@@@@@@@@@@@@@` |
| **0.100 (default)** | 9000.0 | 100.0% | 3.50 | 53 | `___..--==+++***####@@@@@@@@@@@@@@@` |
| 0.200 | 9000.0 | 100.0% | 3.17 | 53 | `___..---===++***####@@@@@@@@@@@@@@` |
| 0.300 | 8848.3 | **98.3%** | 2.55 | 53 | `___...--===+++******########@@@@@@` |

From 0.005 to 0.3 is a **60×** range and recovery barely moves (only 0.3 loses 1.7%). Why: when 500 ants walk the same path, the trail is replenished far faster than it decays — even `0.3/tick` (half-life ≈ 2 ticks) cannot suppress it.

**But shrink the colony to 60 ants and evaporation becomes the decisive parameter:**

```bash
cargo run -p formica-headless --release -- --sweep-evaporation 0.005,0.01,0.02,0.05,0.1,0.2,0.3 \
    --ants 60 --ticks 8000 --food-per-pile 2500
```

| Evaporation | Delivered | Recovery | S-score | Curve |
|---|---|---|---|---|
| 0.005 | 4430.7 | **59.1%** | 2.17 | `____....----====+++++*****#####@@@` |
| 0.010 | 4285.2 | 57.1% | 2.80 | `____.....----====+++++****#####@@@` |
| 0.020 | 3780.3 | 50.4% | 2.33 | `___....-----====+++++*****#####@@@` |
| 0.050 | 3112.1 | 41.5% | 3.08 | `___.....-----====++++******####@@@` |
| **0.100 (default)** | 2842.8 | 37.9% | 2.81 | `____....-----=====++++****#####@@@` |
| 0.200 | 1967.2 | 26.2% | 3.25 | `____.....---====+++******######@@@` |
| 0.300 | 659.0 | **8.8%** | 4.86 | `___....-----=====+++++*****#####@@` |

A clean monotone trade-off appears:

- **Low evaporation → high yield**: trails persist; the colony reuses known paths and empties the piles. The cost is **poor exploration** — once a path hardens, the colony ossifies and misses food right next to it.
- **High evaporation → low yield, high S-score**: the lag phase stretches (almost no trail found early), then a steep phase. At 0.3 the colony essentially fails, recovering only 8.8%.
- The sweet spot is **0.005–0.02**, a half-life of 35–140 ticks — slightly longer than one round trip.

That is the practical feel of the "deposit vs evaporate" balance: **a trail should live a bit longer than one round trip, but not forever**.

### 3. Random-walk sweep: there is an optimum

```bash
cargo run -p formica-headless --release -- --sweep-random-walk 0.0,0.1,0.3,0.6,1.0,1.5 \
    --ants 60 --ticks 8000 --food-per-pile 2500
```

| Random walk | Delivered | Recovery | S-score | First tick |
|---|---|---|---|---|
| 0.0 | 3203.6 | 42.7% | 3.00 | 51 |
| **0.1** | **3525.7** | **47.0%** | 2.95 | 58 |
| 0.3 (default) | 2842.8 | 37.9% | 2.81 | 66 |
| 0.6 | 1522.5 | 20.3% | 3.32 | 63 |
| 1.0 | 708.5 | 9.4% | 5.65 | 83 |
| 1.5 | 245.0 | 3.3% | 9.80 | 105 |

Both ends are bad; there is a peak near 0.1:

- **Zero jitter**: ants walk straight and only turn when they hit a wall. Paths repeat so heavily that the covered area is *smaller* than with a little randomness.
- **Too much jitter (≥ 1.0/tick)**: heading is drowned in noise. Turning ±1 radian per tick means an ant **cannot remember which way it was going**, trail-following is buried — recovery drops to 9.4%.
- So `random_walk_strength` is the **exploration-vs-exploitation knob**, not "smaller is better".

### 4. Individuality sweep: arrives faster, hauls less when sparse

```bash
cargo run -p formica-headless --release -- --sweep-individuality 0.0,0.2,0.4,0.6,1.0,1.5 --ticks 5000
```

**Default scale (500 ants) — only "first delivery" carries information, because every group empties the map:**

| Individuality | Delivered | First tick | S-score |
|---|---|---|---|
| 0.0 | 9000.0 | 72 | 3.74 |
| 0.2 | 9000.0 | 63 | 3.39 |
| 0.4 | 9000.0 | 57 | 3.32 |
| **0.6 (default)** | 9000.0 | **53** | 3.50 |
| 1.0 | 9000.0 | 52 | 2.78 |
| 1.5 | 9000.0 | 52 | 3.06 |

The colony finds its first food **28% sooner** (72 → 52 ticks): some ants run far and straight, effectively extra scouts. The S-score falls at the same time because the lag phase compresses.

**Shrink the colony to 60 ants and the cost appears:**

| Individuality | Delivered | Recovery | First tick |
|---|---|---|---|
| 0.0 | **3537.8** | **47.2%** | 68 |
| 0.2 | 3350.7 | 44.7% | 66 |
| 0.4 | 3118.9 | 41.6% | 61 |
| **0.6 (default)** | 2842.8 | 37.9% | 66 |
| 1.0 | 2382.3 | 31.8% | 61 |
| 1.5 | 2096.1 | 27.9% | 57 |

**Monotonically decreasing**: the fewer ants, the more individuality hurts. The mechanism is blunt — "bold" ants run far and do not come back; with only 60 ants, losing one individual is losing 1.7% of the trail-maintenance workforce, while at 500 the loss is diluted and only the "faster scouting" benefit remains.

This is a **real trade-off**, not bad tuning: the default 0.6 exists to fix [the ring problem](en-fixes-notes.md), and at the default scale (the brief's configuration) it costs nothing. For much smaller colonies, pulling the slider back to 0.2–0.4 pays better — which is precisely why the slider exists.

### 5. Watching re-convergence with "clear trails"

The window app has a **clear trails** button: both pheromone fields go to zero, the ants are untouched. A well-converged colony re-converges after a spell of chaos. It is the fastest way to watch the positive feedback.

### 6. A real bug: crumbs that never made it home

The first version's 5000-tick report said `food left 0.0` but only **8987 / 9000** units delivered — **13 units of food had vanished into thin air**.

Root cause: the last cells of an exhausted pile hold less than one ant's full load. An ant picked up `0.3` units (not full), and `pickup_food` only switched to Returning **when full** — so that ant wandered in Searching state forever with 0.3 units in its jaws. When a pile ran out, a dozen ants each stood there holding a bite they would never carry home.

Fix: an ant goes home when **full**, or **when there is no more food at its current position to fill up on**. Recovery went from 99.86% to **100.00%**.

The bug is now guarded by `food_is_conserved_at_every_checkpoint`, which asserts at 8 checkpoints that `initial == delivered + remaining on map + in ants' jaws`.

### 7. A second precision problem: f32 bookkeeping

After fixing that bug the conservation assertion was still off by `1.3e-2`. Too small for a logic error; it was `f32` accumulation error — repeatedly adding 1.0 into a value near 9000 rounds on a ~0.001 spacing every time.

Handling: **per-cell concentrations and per-ant loads stay `f32`** (read once and discarded, precision irrelevant), but **the cumulative books moved to `f64`** (accumulators spanning tens of thousands of ticks). Error dropped to `1.6e-5` — another 1000× down.

The test tolerance of `initial × 1e-5` was measured, not guessed: the noise floor is `1.3e-8` relative error and the real bug above was `1.4e-3` — the tolerance sits between them with 100× headroom on both sides.

### 8. An assumption overturned by data: display gains

Once the colours were set, I looked at the render and felt "the food trail is not prominent enough" — home pheromone is laid by 4× as many searching ants, spread across the whole map; intuitively it must be washing out the food trail.

So I added **per-field gains**, cut the home gain from 0.35 to 0.06, and wrote a very plausible rationale into the code comments. Then I counted pixels (the scripted screenshots + per-pixel statistics described above):

| Food gain / home gain | green-dominant pixels | violet-dominant pixels |
|---|---|---|
| 0.35 / **0.35** (original) | **4634** | **227** |
| 0.35 / 0.06 (my "fix") | 4228 | 30 |

Cutting the home gain did **not** increase the food trail's green pixels (they fell by 406), and the picture lost nearly all of its home pheromone (227 → 30). That "optimisation" was a **net loss**: it made nothing clearer, only less visible.

The defaults went back to `0.35 / 0.30`. The per-field sliders stay, because they are genuinely useful — for **isolating one field to verify things** (that is exactly how I verified the colours), not for "correcting an imbalance that never existed".

The comment about "4× the ants swamping the background" was deleted. It read convincingly. It was wrong.

### 9. Two tests that "passed but proved nothing"

While building Phase 4, two tests were green from the start and also completely empty. Both are worth recording, because they are two faces of the same mistake.

**(a) The wall enclosed a food pile that did not exist.** The U-wall wraps the first food pile, so `apply_preset` needs to know where the piles are. My first version **regenerated the food inside the function from the seed**:

```rust
let mut rng = SimRng::new(config.seed ^ 0x0B57_AC1E);   // ← a different seed from the real one!
let food = FoodField::generate(config, &mut rng);
```

The real food is generated with `SimRng::new(config.seed)`. Two completely different random sequences, so the wall was stamped where food *would have been under some other seed*.

The test **still passed**, because it generated food from the **same wrong seed** and compared the wall against that phantom pile — self versus self, always consistent.

What caught it was a screenshot: the wall standing alone in open ground, enclosing nothing.

The fix was to pass the real piles in (`apply_preset(..., &food.piles)`) instead of re-deriving them. There is now a `the_u_wall_follows_the_piles_when_the_seed_changes` test asserting for four different seeds that "the wall must appear near the real food pile" — an assertion the phantom-pile version fails immediately.

**(b) Two curves both pinned at the ceiling cannot be compared.** The two-colony competition test originally compared a single colony of 300 against two colonies of 150 by total delivered. Both came out **1500/1500** — the food was emptied, both arms hit the ceiling, a 0% gap.

Sneakier: it **did not fail**, because the assertion was `competing < cooperating` and the saturated values differed by a hair of floating point, letting a 0.5% gap through. A test like this is worse than no test: it shows a green light.

The fix had two steps. Add an **unsaturated assertion** to both arms (`delivered < food_initial * 0.95`) so hitting the ceiling errors out with "this comparison is saturated, change the config". Then move to a configuration with headroom so the real gap becomes visible.

**Lesson**: in a controlled comparison, more important than "is the conclusion right" is confirming first that **both arms ran in the same unsaturated regime**. A saturated comparison is not weak evidence — it is zero evidence.

**(Postscript: the test broke again, a different way.)** After individuality landed, `two_colonies_do_not_share_trails` still passed — but by luck: the gap on the default seed had fallen from 11.8% to 1.4%, and of five other seeds I tried, two had the **opposite sign**. An assertion that holds on one seed is a coin flip. The current form is a **four-seed ensemble**: every seed must lose, the mean loss must exceed 1.5%, and the "second nest is a second search origin" confounder is removed with `nest_orbit = 12` (see [Phase 4 · two colonies](en-phase4-features.md)).

### 10. A test that measured at the wrong moment

While fixing the ring problem, I wrote a test to quantify "are the ants still bunched in a ring": compute the radial distribution, assert ants in the inner ring and a modest outer share. It ran at tick 600 and **failed** — I nearly went parameter-hunting.

The problem: tick 600 is too late on a 200-cell map. The ants had long since bounced off the boundary walls and mixed; at that moment even the "all ants identical" control group passed. The test was measuring **a phenomenon that had already disappeared**.

Moved to tick 30, the control group's signal became glaring: **66% of ants in the outer 1/5 radius, 0.2% in the inner ring**. The fixed group: 14% / 5.0%.

Two lessons, both cheap and both easy to forget:

- **The phenomenon you are testing may be a transient.** Confirm it actually exists at the moment you measure, or your test is just asserting "the world eventually becomes uniform" — that is not a test, that is the second law of thermodynamics.
- **The control group belongs in the same test.** This test now runs `individuality = 0` and the default side by side and **first asserts that the control group really is a ring**. If the model ever changes and the control stops being a ring, the test says explicitly "this test can no longer distinguish the fix from the bug" instead of staying green.
