<!-- English counterpart of docs/zh-fixes-notes.md; see README.md for the overview. -->

# Three visual-quality fixes

Three improvement points were raised once the simulation worked. None of them was "adjust a parameter", because each had its own root cause — and for each one the workflow was **reproduce, quantify, then change code**. The numbers are written down so that "it's fixed" can be refuted.

### 1. "Ants expanding as a circle at startup is unrealistic"

**Reproduce**: on frame 4 with default parameters, the 500 ants form a **perfect hollow ring**: a circle of white dots around an empty nest.

**Root cause**: this is not a parameter problem, it is a **symmetry** problem, and no parameter can fix it. Every ant runs the same rules, is born facing "away from the nest", and moves at 2 cells/tick — so the whole population appears at the same radius at the same instant, and a uniform disc degenerates into a thin shell. The ensemble average of a population of **identical** random walkers is radially symmetric by construction.

**Fix**: give each ant **individuality** (`AntVigor`). A "boldness" value is drawn from a log-normal distribution and multiplies both its random-walk strength and its speed: roughly 2% are near-ballistic long-distance runners, roughly 2% are homebodies that barely leave the nest, the rest are in between. The birth heading is also no longer exactly radial — it carries a ±1 radian offset.

| tick 30, 200×200, 400 ants | share in the outer 1/5 radius | inner 40% radius |
|---|---|---|
| `individuality = 0.0` (identical) | **66%** | **0.2%** (hollow middle) |
| `individuality = 0.6` (default) | **14%** | **5.0%** |

The same comparison at tick 20 and tick 45 reads 65%/17% and 63%/10%, so the conclusion does not hinge on one frame.

![A ring of identical ants](docs/figures/fix1-ring-identical-ants.png)

*`FORMICA_INDIVIDUALITY=0`: a textbook hollow ring.*

![Diffusion with individual differences](docs/figures/fix1-ring-individuals.png)

*Default parameters, same instant: the ring has broken into irregular clumps and filamentary channels.*

**A boundary that must be stated**: the ring is a **transient**, not a steady state. On a 200-cell map, by tick 600 even identical ants have bounced off the boundary walls and mixed — the radial profiles are indistinguishable. My first version of the test ran at tick 600 and therefore measured nothing (see [Finding 10](#10-a-test-that-measured-at-the-wrong-moment)). Tick 30 is what this complaint is actually about.

### 2. "Food piles should shrink as they are carried away"

**Reproduce**: reading the code, **this feature already existed** — `sync_food_sprites` scales the sprite by the remaining amount. The real problem was in the screenshots: an emptied pile shrank to 30% and **turned red**, staying on the map.

- **Red is the predators' colour.** An empty food pile looked like a threat.
- **A 30% floor means it never disappears.** `FoodField::piles` keeps exhausted piles (the player needs them for erasing), so a finished run accumulated little coloured squares that would be there forever.

**Fix**: radius becomes `sqrt(remaining fraction)` (a pile spreads into a disc whose area is proportional to mass, so radius goes as the square root); the floor is removed; the colour ramp changes from "green→red" to **green→dark green**; below 0.5% remaining the pile is not drawn at all.

![Shrinking food piles](docs/figures/fix2-food-shrinking.png)

*tick 1800: three piles of clearly different sizes; the top-right pile is over half gone, the bottom-left barely touched.*

![Exhausted food piles](docs/figures/fix2-food-exhausted.png)

*tick 4480: 9000/9000 delivered, all three piles gone, no leftover blobs on the map.*

### 3. "With obstacles in the way, the ants all pile onto one spot"

**Reproduce**: this was the hardest of the three to reproduce. The U-shaped wall and default-density block walls show queuing, not collapse. What actually reproduced it was a **high-density labyrinth**: at 45% wall density, **121 of 500 ants** stood on the same 4×4 bin (16 cells), and the whole run delivered only 599/9000.

**Root cause**: the queue has nowhere to go. Nothing in the system knows "this cell already holds twenty nestmates". `sense → decide → move → deposit` each followed the spec exactly; what was missing was a **physical** rule.

**Fix**: two new systems. `tally_crowding` counts ants into a coarse 4×4-cell grid every tick; `relieve_crowding` moves ants standing in over-dense bins a small step toward the **emptiest neighbouring bin** (default 0.6 cells/tick), and only when the destination is not a wall and the heading can follow the move.

The threshold is **soft**: the more over the limit, the harder the push. Two regimes must coexist — a busy trail on an open map peaks at 28 ants per bin, while a labyrinth's **average** is 55. A hard threshold would either miss real jams or shove healthy trails apart.

| Wall density (256×256, 500 ants, 3000 ticks) | peak bin (off) | peak bin (on) | mean (off) | mean (on) | delivered (off) | delivered (on) |
|---|---|---|---|---|---|---|
| 0.15 | 46 | **31** | 17.5 | 14.1 | 6362 | **6784** (+6.6%) |
| 0.25 | 78 | **30** | 29.8 | 16.2 | 4653 | **4770** (+2.5%) |
| 0.35 | 62 | **29** | 39.4 | 16.1 | 1854 | **2165** (+16.8%) |
| 0.45 | **121** | **30** | 55.1 | 15.9 | 599 | **696** (+16.2%) |

![Labyrinth, no crowding relief](docs/figures/fix3-labyrinth-collapsed.png)

*Relief off (`FORMICA_CROWD_RELIEF=0`): a smeared blob under the nest; the trails are short stubs.*

![Labyrinth, crowding relief on](docs/figures/fix3-labyrinth-spread.png)

*Relief on, same seed and parameters: the ants spread out and a trail runs from the nest to the bottom-right food pile.*

**An honest boundary**: at 45% density the passable fraction is 55%, while the square-lattice site percolation threshold is **0.593** — the terrain itself **does not percolate**: the open space is broken into finite pockets and the ants cannot get out at all. Crowding relief lets them spread within their pockets (peak 121 → 30, yield +16%) but cannot break them out. That part is the terrain, not the crowding rule — which is exactly what the table's bands show: which stretch relief can save, and which it cannot.
