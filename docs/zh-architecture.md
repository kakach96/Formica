<!-- Extracted from the Chinese README (README.zh-CN.md). Full context lives there. -->

# 架构

### 数据导向设计

任务书要求"蚂蚁数据用 Component 存储，不用单独的 struct Vec"。所以蚂蚁不是 `Vec<Ant>`，而是**八个独立 Component**：

| Component | 含义 | 谁写 |
|---|---|---|
| `AntMarker` | 标记 | — |
| `AntColony(usize)` | 属于哪一窝：决定读写哪套信息素场、回哪个巢 | 生成时固定 |
| `AntPos { x, y }` | 连续坐标 | `move_ants`, `relieve_crowding` |
| `AntHeading { angle }` | 朝向（弧度） | `decide_heading`, `move_ants`, `relieve_crowding` |
| `AntState` | 状态机：`Searching` / `Returning` | `pickup_food`, `deliver_food` |
| `Carrying { amount }` | 携带量 | `pickup_food`, `deliver_food` |
| `PheromoneSensors { left, center, right }` | 三个触角读数 | `sense_pheromones` |
| `AntVigor { wander, speed }` | **个体差异**：这只蚂蚁自己的游走强度与速度倍率 | 生成时固定，永不改变 |

`AntVigor` 是整份代码里唯一的"个体性"，也是唯一一个**只在生成时写一次、之后再不改动**的 Component。
它存在的理由见[三处观感优化 #1](#1-初始状态蚂蚁直接圆形扩散不合理)：没有它，
一群规则相同的蚂蚁在数学上必然是径向对称的。

捕食者同样拆成四个 Component（`PredatorMarker` / `PredatorPos` / `PredatorHeading` / `PredatorAlert`）。

资源（`Resource`）：

| Resource | 内容 |
|---|---|
| `SimConfig` | 全部可调参数 |
| `PheromoneGrid` | **每窝两个** `f32` 场，装在一个扁平 `Vec` 里（`256×256×2 窝×2 通道`） |
| `Obstacles` | 每格是否墙 + 形状版本号 |
| `FoodField` | 每格食物量 + 堆信息 + 总量账目（含玩家删除量） |
| `Nests` | 每窝巢穴的位置与半径 |
| `CrowdGrid` | 4×4 粗粒度蚂蚁计数 + 峰值 + 被占用的方块数（诊断与拥挤缓解共用） |
| `SimRng` | SplitMix64 确定性随机数 |
| `SimStats` | 统计、**分窝账目**、运回曲线、浓度读数 |

两个设计细节值得单独说：

- **`PheromoneGrid` 永远按最大窝数分配**。窝数是可以运行时改的，如果按当前窝数分配，
  玩家一开第二窝，1 号窝的索引就会被夹到 0 号窝的场上——两窝开始**共享同一套气味**，
  而且什么都不会报错。多出来的场每张 256 KiB，代价是一条稍长的蒸发扫描。
- **`Carrying.amount` 是 `f32`，累计账目是 `f64`**。前者读一次就丢，后者要跨几万 tick 累加。

### System 拆分

任务书要求 sense / decide / move / deposit / evaporate 分开写。实际拆成 **16 个 system**，
串在一个**独立的 `SimulationTick` schedule** 里：

```
sync_nests              窝数变了就重建巢穴（否则两窝会挤在同一个巢）
      ↓
sync_obstacles          按预设重新铺墙（几何参数写在预设里，所以拖滑块会自动重建）
      ↓
scrub_obstacle_pheromones  墙下的信息素清零
      ↓
sync_predator_count     捕食者数量跟上配置
      ↓
evaporate_pheromones    c *= (1 - rate)，所有窝一次扫完
      ↓
sense_pheromones        三个触角 → PheromoneSensors（只读自己那窝）
      ↓
decide_heading          sensors + 个体噪声 + 回自己的巢 → AntHeading
      ↓
move_ants               AntHeading × 个体速度 → AntPos（边界反射 + 绕墙滑行）
      ↓
move_predators          追最近的蚂蚁，追不到就游走
      ↓
tally_crowding          蚂蚁坐标 → 4×4 粗粒度密度网格
      ↓
relieve_crowding        站在过密格子里的蚂蚁朝最空的邻格挪一小步
      ↓
deposit_pheromones      AntPos + state → 自己那窝的场（撒放核也要绕开墙）
      ↓
pickup_food             踩到食物 → Carrying, state := Returning
      ↓
deliver_food            回到**自己的**巢穴 → 记在自己那窝账上, state := Searching
      ↓
predators_catch_ants    吃掉够近的蚂蚁（巢内免疫）
      ↓
sync_ant_count          把被吃的补回来——放在捕食之后，所以 tick 边界上数量精确
      ↓
update_stats            计数 + 记录曲线点
```

两个拥挤 system 夹在移动和撒放之间，位置是刻意的：`tally_crowding` 必须看到本 tick 的最终位置，
而 `relieve_crowding` 必须在撒放之前落地，这样被挤开的蚂蚁才会按**它实际站的位置**留下气味。

**为什么用独立 schedule 而不是 `Update` / `FixedUpdate`**：这个 schedule 引擎**永远不会自己跑**。
命令行版直接 `world.run_schedule(SimulationTick)` × N，零引擎开销、与真实时间完全无关；
窗口版在 `Update` 里跑 N 次，所以"快进"只是一个计数器，而不是另一套物理。
两条路径跑的是**同一批 system、同一个顺序**——Phase 1 测出来的数字就是对 Phase 2 画面的预测。

### 依赖

`formica-core` 只依赖 `bevy_ecs` + `bevy_app`，**不依赖渲染**。所以 Phase 1 的验收运行不需要 GPU、
不需要窗口、可以跑在 CI 里。`formica-app` 才引入完整 Bevy（只开 `2d` + `ui` feature，不含 3D 与音频）。

版本：**Bevy 0.19.1** + **bevy_egui 0.42.0**（egui 0.36）。

---
