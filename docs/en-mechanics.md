<!-- English counterpart of docs/zh-mechanics.md; see README.md for the overview. -->

# Mechanics

### Why "two" pheromones

This is the easiest thing in the whole project to get wrong. With a single pheromone field, one fatal thing happens: **the scent laid by returning ants points at the nest, so searching ants that follow it walk straight back home.**

Two independent fields turn "scent" into a directional signal:

| State | Behaviour | Lays | Follows |
|---|---|---|---|
| **Searching** | random walk + biased toward food pheromone | home pheromone | food pheromone |
| **Returning** | head straight for the nest | food pheromone | home pheromone* |

\* Returning ants by default **walk straight toward the nest** and do not follow scent (an explicit requirement of the brief). The `home_pheromone_weight` slider can blend home pheromone into the steering; the default 0.0 is the brief's behaviour, kept as a Phase 4 experiment knob.

The key property: **no ant ever writes the field it is currently following.** Food pheromone is laid only by ants carrying food home, so it inherently means "food is that way"; home pheromone is laid only by ants still searching, so it inherently means "the nest is that way". The directionality of the information is not designed in — it falls out of the **correspondence between state and action**.

### Why it converges

Nothing in a single ant's behaviour converges: it merely "leans a little toward the stronger smell". Convergence comes from positive feedback:

1. One ant **happens** to find food, turns home, and lays food pheromone along the way.
2. More searching ants smell the trail, bias toward it → a higher chance of also reaching the food.
3. They too haul food back along the same path, laying the trail on thicker.
4. A strong trail attracts more ants — the path is "locked in".

The other half of that feedback is **evaporation**. Evaporation erases trails nobody walks, so the path to an abandoned food pile decays on its own. Without evaporation every cell saturates to its ceiling, the gradient disappears, and the ants go blind. **Deposition and evaporation are a balanced pair**; neither works alone.

### Why the ants are not identical

A single ant's rules contain nothing beyond randomness — but **the amount of randomness differs per ant**. `AntVigor` draws a "boldness" from a log-normal distribution that multiplies both wander strength and speed.

This is not cosmetic. A population of ants running **identical rules** is statistically radially symmetric: they leave the nest together, at the same speed, facing away from it, so the population is geometrically an expanding ring. This is independent of parameter values — it is a theorem about ensemble averages. The only way to break it is **differences between individuals**.

Real colonies work this way: a few scouts run far and fast on nearly straight lines while most workers dawdle near the nest. At the defaults roughly 2% are the former and 2% the latter. The cost is yield in sparse colonies (see [Finding 4](#4-individuality-sweep-arrives-faster-hauls-less-when-sparse)); the benefit is finding the first food ~28% sooner — and a diffusion pattern that is no longer a circle.

### Three antennae, not a gradient

An ant does not "climb toward the highest concentration". It samples three points ahead (left `-sensor_angle`, straight ahead, right `+sensor_angle`) and **turns toward the strongest reading**.

The distinction matters: gradient ascent only tells an ant "go where it's stronger", but a uniformly dense trail has no gradient. Three-point sampling performs **trail alignment** — an ant already on the trail reads equal values on both sides and keeps going straight; one hitting the trail obliquely turns to align with it. This is exactly what real ants do with their antennae.

Each antenna samples a `(2r+1)²` **box average** rather than a single cell. An ant moves 2 cells per tick; point sampling would let it step over its own one-cell trail, and the readings would be pure noise.
