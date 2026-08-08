//! Layer 2 of the terrain vertical: iterative **stream-power fluvial erosion**
//! plus **thermal relaxation** — a small landscape-evolution model (LEM) carved
//! on top of the Perlin base shape ([`super::heightmap`]).
//!
//! ## Why this model (and not droplets)
//!
//! The branching, tree-like ("dendritic") valley networks that make eroded
//! mountains read as *real* come from **flow accumulation**: water from every
//! cell is routed to its lowest neighbor, and *drainage area* `A` accumulates
//! down that network so trunk rivers carry far more water than their tributaries.
//! Erosion driven by that area cuts deep shared valleys while the low-area ridges
//! between them are barely touched — which is exactly the look. Independent
//! water *droplets* can't reproduce it: they each carve in isolation and never
//! pool into a connected network.
//!
//! Both of the project's terrain references are built on this — Cordonnier et al.
//! 2016 (`reference/2016_cordonnier.pdf`) and the analytical Tzathas et al. 2024
//! (`reference/Analytical_Terrains_EG.pdf`). The grid pipe-model of Mei et al.
//! 2007 (`reference/download.pdf`) is an alternative *hydraulic* scheme (rainfall
//! on an existing terrain); it is great for surface detail but does not build
//! ranges, so it isn't what we use here.
//!
//! ## The algorithm (one timestep)
//!
//! 1. **Flow routing** — a Priority-Flood (Barnes 2014) raises every depression to
//!    its spill level, each cell then takes its steepest downhill neighbor, and the
//!    level ground left inside the filled basins is given a direction by the
//!    flat-resolution of Barnes, Lehman & Mulla 2014. Nothing dead-ends, and the
//!    receiver forest yields a downstream-first processing order.
//! 2. **Drainage area** — accumulate cell areas up the receiver tree (each cell
//!    adds its area to its receiver), processing the order in reverse.
//! 3. **Stream-power incision** — pull each cell toward its receiver by the stream
//!    power `K·Aᵐ`, using the unconditionally stable *implicit* update of
//!    Braun & Willett 2013 (FastScape). Processing downstream-first means a cell's
//!    receiver is already updated, so the linear-time implicit solve is exact.
//! 4. **Thermal relaxation** — shed any slope above the talus angle to its lower
//!    neighbors (Musgrave 1989), rounding the spiky divides into natural ridges.
//!
//! Iterate. More passes = a more deeply incised, mature landscape.
//!
//! ## A pass is a unit of time, not a parameter
//!
//! This module used to expose one batch call — "erode this by N passes" — with N
//! sitting in [`ErosionParams`] beside the erodibility, as though *how eroded* were
//! a property of the model. It isn't. It is a position on a **time axis**, and the
//! difference only became visible when the demo started animating it: the landscape
//! at pass 60 is not configured differently from the one at pass 30, it is the same
//! landscape thirty passes later.
//!
//! So the batch call is gone and [`step`] is public in its place. The caller runs
//! passes one at a time, keeps the ones it wants, and treats the sequence as what
//! it is — a history. [`water_of`] reads the water off any state without advancing
//! it, which is what lets a caller show a pass it did not just compute.
//!
//! ## The water
//!
//! Steps 1 and 2 compute, every single pass, exactly the two fields you need to
//! *draw* water — and this module used to throw both away when the pass ended:
//!
//! - **Lakes** are the Priority-Flood's own fill. `filled - z` is how far the
//!   flood had to raise a cell to make it drain, which is zero wherever the
//!   terrain already drains and positive inside every depression. That is a lake,
//!   and its depth, for free.
//! - **Rivers** are drainage area. `area` is the number of cells draining through
//!   this one, so thresholding it *is* the river network — the same quantity the
//!   incision is proportional to, which is why the rivers you see are exactly the
//!   ones doing the carving.
//!
//! Both now come back out of [`step`] as a [`Water`], describing the terrain it
//! was handed rather than being dropped on the floor.
//!
//! Drawing them needed one change to the model itself: **lakes have to survive the
//! pass that finds them**. The implicit update raises a pit toward the rim it was
//! breached from, so a single pass used to pack every depression with rock — the
//! reason this demo had no lakes to draw. Skipping submerged cells fixes that and
//! is the honest reading anyway (no stream over a lake bed, so no incision), but
//! on its own it stalls the whole model: a fifth of the map ends up underwater and
//! inert. [`ErosionParams::deposition`] is the term that resolves it.
//!
//! Like the rest of the demo this lives **entirely in the consumer** (roadmap
//! principle 3): the engine never sees a heightmap, only the mesh built from one.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};

/// Tunable erosion parameters — the knobs the UI exposes.
#[derive(Debug, Clone, Copy)]
pub struct ErosionParams {
    /// Fluvial erodibility `K` (folds in the timestep): how strongly rivers pull
    /// the terrain down toward their outlet each step.
    pub erodibility: f32,
    /// Drainage-area exponent `m` in the stream power `K·Aᵐ` (geomorphology uses
    /// ~0.4–0.6). Higher values concentrate erosion into the big rivers.
    pub m: f32,

    /// Fraction of a lake's depth filled with sediment each pass — **siltation**.
    ///
    /// This term was added because leaving it out was measurably wrong, not for
    /// looks. Preserving depressions (so lakes exist at all) means submerged cells
    /// never incise — and with a fifth of the map underwater and inert, the whole
    /// model nearly stops: sixty passes moved the mean height 2.5% and the terrain
    /// read as flooded rather than eroded.
    ///
    /// Rivers carry sediment and drop it where they slow down, which is exactly
    /// where they meet standing water. So lakes silt up, spill over, and their
    /// basins join the network — *drainage integration*, the mechanism Cordonnier
    /// et al. build their whole model around, and the reason a mature landscape has
    /// long dendritic trunks rather than a thousand ponds. Turning this up buries
    /// the lakes fast; turning it to zero freezes them and stalls the erosion,
    /// which is worth doing once just to see the difference.
    pub deposition: f32,

    /// Whether the thermal (talus relaxation) pass runs each timestep.
    pub thermal: bool,
    /// Critical slope (talus angle, rise/run) above which material slides.
    pub talus: f32,
    /// Fraction of the over-talus excess moved per sweep (kept ≤ 0.5 for
    /// stability).
    pub thermal_rate: f32,
}

impl Default for ErosionParams {
    fn default() -> Self {
        Self {
            erodibility: 0.004,
            m: 0.5,
            deposition: 0.05,
            // Off by default: a strong thermal pass rounds the dendritic detail
            // back into blobs. It's available as a finishing touch.
            thermal: false,
            talus: 1.5,
            thermal_rate: 0.3,
        }
    }
}

/// The standing water and river network riding on the terrain — the byproduct of
/// flow routing that the model has always computed and now hands back to be drawn.
///
/// Both fields are `n × n`, row-major, indexed exactly like the heightmap.
#[derive(Debug, Clone, Default)]
pub struct Water {
    /// Standing-water depth per cell: `filled - z`, how far the Priority-Flood had
    /// to raise this cell to give it somewhere to drain. Zero wherever the terrain
    /// drains freely; positive inside every depression, where it *is* the lake.
    pub depth: Vec<f32>,
    /// Drainage area per cell: how many cells' water passes through this one,
    /// itself included. `1.0` on a ridge top, thousands in a trunk river — so a
    /// threshold on this is the river network.
    pub area: Vec<f32>,
    /// Downstream neighbour of each cell; outlets and boundary cells receive
    /// themselves.
    ///
    /// This is the river network as a *graph* rather than as a per-cell mask, and
    /// it is here because drawing a river well needs the links, not the cells. A
    /// threshold on [`area`](Self::area) says which cells are wet and nothing
    /// about which way the water goes, so a mask can only ever be rendered as the
    /// grid-aligned staircase it is. Each `c -> receiver[c]` link is a *segment*,
    /// and a segment can be given a direction, a width, and a bank.
    pub receiver: Vec<usize>,
}

impl Water {
    /// A dry `count`-cell grid — what a degenerate (`n < 3`) grid reports.
    fn empty(count: usize) -> Self {
        Self {
            depth: vec![0.0; count],
            area: vec![1.0; count],
            receiver: (0..count).collect(),
        }
    }
}

/// The water standing on `heights` right now, without advancing anything.
///
/// [`step`] hands back the water it was *driven* by, which covers a caller walking
/// the timeline forward. This covers the one that isn't walking: showing a pass it
/// did not just compute — a scrub backwards into a stored state, or the raw base
/// heightmap at pass zero. Raw Perlin noise is full of depressions, and they hold
/// just as much water for never having been eroded.
///
/// It is the expensive half of a pass (the Priority-Flood) and none of the cheap
/// half, so a caller that can get its water from [`step`] should.
pub fn water_of(heights: &[f32], n: usize) -> Water {
    if n < 3 {
        return Water::empty(heights.len());
    }
    analyze(heights, n).1
}

/// Advance `heights` by **one** timestep of flow-routed stream-power incision,
/// siltation, and optional thermal relaxation, returning the [`Water`] that drove
/// it. See the module docs for the model.
///
/// **The returned water belongs to the heights that went *in*, not the ones coming
/// out** — it is what the pass read to decide where to cut. That pairing is the
/// useful one for a caller keeping a history: stepping a stored pass `k` yields
/// both the water to draw *at* `k` and the heights at `k + 1`, so walking the
/// timeline forward costs exactly one flow routing per pass and never re-analyzes
/// a state it has already seen.
pub fn step(heights: &mut [f32], n: usize, params: &ErosionParams) -> Water {
    if n < 3 {
        return Water::empty(heights.len());
    }
    let (flow, water) = analyze(heights, n);

    // Implicit stream-power incision, downstream-first so each receiver is
    // already at its new height when we solve the cell above it:
    //   z'[c] = (z[c] + f·z'[r]) / (1 + f),   f = K·Aᵐ / L.
    for &c in &flow.order {
        let r = water.receiver[c];
        if r == c {
            continue; // outlet / fixed base level
        }
        // Two cells are left alone, and the reason is the same for both: they are
        // under water, or they are being asked to flow *uphill* through a breach.
        //
        // `receiver` is downhill on the **filled** surface, which inside a basin
        // means uphill in the real terrain — the flood reaches a pit from the rim
        // above it. Applying the implicit update there does not incise the pit, it
        // *raises* it toward the rim: one pass and every depression on the map has
        // been packed with rock and can never hold water again. That is why this
        // demo had no lakes to draw. Skipping those cells keeps depressions, and
        // "no fluvial incision under standing water" is the honest reading anyway
        // — a lake bed has no stream running over it to cut with.
        //
        // What still erodes is the lake's *outlet*, which is dry and drains
        // downhill like anything else — so spillways cut down over the run and
        // lakes partly drain themselves, without that being put in by hand.
        if water.depth[c] > MIN_POND || heights[r] >= heights[c] {
            continue;
        }
        let f = params.erodibility * water.area[c].powf(params.m) / flow.dist[c];
        let z = (heights[c] + f * heights[r]) / (1.0 + f);
        // Never incise below the receiver (keeps slopes downhill).
        heights[c] = z.max(heights[r]);
    }

    // Siltation: rivers drop their load where they meet standing water, so every
    // lake loses a fraction of its depth to sediment each pass. Lakes shrink from
    // the shallows inward, spill, and hand their basin to the drainage network.
    // See [`ErosionParams::deposition`] for why this term is load-bearing.
    let fill = params.deposition.clamp(0.0, 1.0);
    if fill > 0.0 {
        for (h, &d) in heights.iter_mut().zip(&water.depth) {
            if d > MIN_POND {
                *h += fill * d;
            }
        }
    }

    if params.thermal {
        thermal_sweep(heights, n, params);
    }

    water
}

/// The shared first half of a pass: route the flow, accumulate drainage area, and
/// read the standing water off the filled surface.
fn analyze(z: &[f32], n: usize) -> (Flow, Water) {
    let mut flow = flow_route(z, n);
    // The receiver tree moves into the `Water`, which is what gets handed to the
    // caller: it is as much a description of where the water *is* as the depths
    // are, and the renderer needs it to draw a river as a channel rather than as
    // a set of squares. `Flow` keeps the parts only the solver uses.
    let receiver = std::mem::take(&mut flow.receiver);

    // Drainage area: every cell contributes one unit of area to itself, then
    // pushes its total down to its receiver (reverse topological order).
    let mut area = vec![1.0f32; z.len()];
    for &c in flow.order.iter().rev() {
        let r = receiver[c];
        if r != c {
            area[r] += area[c];
        }
    }

    // How far the flood had to lift each cell to drain it — zero almost
    // everywhere, and a lake wherever it isn't.
    let depth = flow
        .filled
        .iter()
        .zip(z)
        .map(|(f, h)| (f - h).max(0.0))
        .collect();

    (
        flow,
        Water {
            depth,
            area,
            receiver,
        },
    )
}

/// Depth below which standing water is too shallow to be worth calling a pond.
///
/// Heights are normalized to `[0, 1]`, so this is a thousandth of the terrain's
/// full relief.
///
/// It used to carry a second, larger job: the flood raised each cell a hair above
/// the one it was reached from, so a long flat run accumulated ε into a "depth"
/// that was an artifact of the algorithm and not water, and this threshold was
/// what discarded it. [`fill_depressions`] no longer adds that ε — a cell outside
/// a depression now comes out at exactly its own height — so the artifact is gone
/// at the source and this is back to meaning what it says.
pub const MIN_POND: f32 = 1.0e-3;

// --- Thermal erosion -------------------------------------------------------

/// One talus-relaxation sweep: move a fraction of each over-critical-slope excess
/// from a cell to its lower 4-neighbors. Double-buffered through a delta grid so
/// the whole sweep reads one consistent state.
fn thermal_sweep(h: &mut [f32], n: usize, p: &ErosionParams) {
    let dx = 1.0 / n as f32;
    let talus = (p.talus * dx).max(0.0);
    let rate = p.thermal_rate.clamp(0.0, 0.5);
    let mut delta = vec![0.0f32; h.len()];
    const NEIGHBORS: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

    for y in 0..n {
        for x in 0..n {
            let c = y * n + x;
            let hc = h[c];
            for (ox, oy) in NEIGHBORS {
                let nx = x as isize + ox;
                let ny = y as isize + oy;
                if nx < 0 || ny < 0 || nx >= n as isize || ny >= n as isize {
                    continue;
                }
                let nb = ny as usize * n + nx as usize;
                let diff = hc - h[nb];
                if diff > talus {
                    // Only the higher cell of a pair sees `diff > talus`, so each
                    // transfer is counted once; the 0.5 keeps it gentle.
                    let m = 0.5 * rate * (diff - talus);
                    delta[c] -= m;
                    delta[nb] += m;
                }
            }
        }
    }
    for (hi, d) in h.iter_mut().zip(&delta) {
        *hi += *d;
    }
}

// --- Flow routing (Priority-Flood) -----------------------------------------

/// Per-cell downstream receiver, a downstream-first processing order, and the
/// distance from each cell to its receiver.
struct Flow {
    /// Downstream receiver per cell; boundary/outlet cells receive themselves.
    receiver: Vec<usize>,
    /// Cells by increasing filled elevation (outlets first) — a valid topological
    /// order over the receiver forest.
    order: Vec<usize>,
    /// Distance from each cell to its receiver (1 orthogonal, √2 diagonal).
    dist: Vec<f32>,
    /// The **filled** surface: the terrain with every depression flooded to its
    /// spill point. This used to be a local the flood dropped on the way out; it
    /// is kept now because `filled - z` is the standing water (see [`Water`]).
    filled: Vec<f32>,
}

/// Distance marker for "this BFS has not reached the cell".
const UNREACHED: u32 = u32::MAX;

/// A min-heap node ordered by (filled) elevation, with a deterministic index
/// tie-break so the flood is reproducible.
#[derive(PartialEq)]
struct HeapNode {
    elev: f32,
    idx: u32,
}
impl Eq for HeapNode {}
impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reversed so `BinaryHeap` (a max-heap) pops the *lowest* elevation.
        other
            .elev
            .total_cmp(&self.elev)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}
impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// D8 neighbor offsets paired with their step distance.
const D8: [(isize, isize, f32); 8] = [
    (-1, 0, 1.0),
    (1, 0, 1.0),
    (0, -1, 1.0),
    (0, 1, 1.0),
    (-1, -1, std::f32::consts::SQRT_2),
    (1, -1, std::f32::consts::SQRT_2),
    (-1, 1, std::f32::consts::SQRT_2),
    (1, 1, std::f32::consts::SQRT_2),
];

/// Raise every depression to its spill elevation — Priority-Flood (Barnes 2014).
///
/// Grow inward from the boundary in elevation order. A cell reached from `c`
/// cannot drain lower than `c` did, so it comes out at `max(z, filled[c])`:
/// outside a depression that is just `z`, and inside one it is the level of the
/// rim the water spills over — the lake surface.
///
/// **No ε, and that is the fix for the water rails.** The classic variant lifts
/// each cell a hair above the one it was reached from so that everything has a
/// strictly lower neighbour and the receiver can simply be "whoever got here
/// first". It is cheap and it is wrong in a way that shows: on flat ground the
/// flood is a breadth-first wave, so "whoever got here first" is a BFS parent
/// tree, whose branches are *dead-straight rays* from the point the wave entered.
/// Drainage area then piles onto those rays, and the demo drew them as rivers —
/// the long parallel diagonal lines across every drained basin. Filling honestly
/// leaves the depression exactly level and hands the direction problem to
/// [`resolve_flats`], which is where it belongs.
fn fill_depressions(z: &[f32], n: usize) -> Vec<f32> {
    let count = n * n;
    let mut filled = vec![0.0f32; count];
    let mut visited = vec![false; count];
    let mut heap = BinaryHeap::with_capacity(4 * n);

    // Seed every boundary cell at its own height: the grid drains off its edges.
    for y in 0..n {
        for x in 0..n {
            if x == 0 || y == 0 || x == n - 1 || y == n - 1 {
                let c = y * n + x;
                filled[c] = z[c];
                visited[c] = true;
                heap.push(HeapNode {
                    elev: z[c],
                    idx: c as u32,
                });
            }
        }
    }

    while let Some(node) = heap.pop() {
        let c = node.idx as usize;
        for (nb, _) in neighbors(c, n) {
            if visited[nb] {
                continue;
            }
            visited[nb] = true;
            filled[nb] = z[nb].max(filled[c]);
            heap.push(HeapNode {
                elev: filled[nb],
                idx: nb as u32,
            });
        }
    }

    filled
}

/// The D8 neighbors of `c` on an `n × n` grid, each with its step distance.
fn neighbors(c: usize, n: usize) -> impl Iterator<Item = (usize, f32)> {
    let (cx, cy) = ((c % n) as isize, (c / n) as isize);
    D8.into_iter().filter_map(move |(ox, oy, step)| {
        let (nx, ny) = (cx + ox, cy + oy);
        (nx >= 0 && ny >= 0 && nx < n as isize && ny < n as isize)
            .then(|| (ny as usize * n + nx as usize, step))
    })
}

/// Route flow over the filled surface: fill the depressions, point every cell at
/// a downstream neighbour, and hand back a downstream-first order.
///
/// Three steps, because a filled surface has two quite different kinds of cell on
/// it. Anywhere with a lower neighbour takes the **steepest** one, which is the
/// ordinary D8 answer. Everywhere else is *level* — the interior of a filled
/// depression — and has no steepest anything; [`resolve_flats`] gives those a
/// direction. [`downstream_order`] then walks the resulting forest.
fn flow_route(z: &[f32], n: usize) -> Flow {
    let count = n * n;
    let filled = fill_depressions(z, n);
    let mut receiver = vec![usize::MAX; count];
    let mut dist = vec![1.0f32; count];

    for y in 0..n {
        for x in 0..n {
            let c = y * n + x;
            // The boundary is base level: it drains off the map, to itself.
            if x == 0 || y == 0 || x == n - 1 || y == n - 1 {
                receiver[c] = c;
                continue;
            }
            let mut steepest = 0.0f32;
            for (nb, step) in neighbors(c, n) {
                let slope = (filled[c] - filled[nb]) / step;
                if slope > steepest {
                    steepest = slope;
                    receiver[c] = nb;
                    dist[c] = step;
                }
            }
        }
    }

    // Whatever is still unassigned has no lower neighbour at all: it is level
    // ground, and level ground is the whole of every filled basin.
    resolve_flats(&filled, n, &mut receiver, &mut dist);

    let order = downstream_order(&receiver);
    Flow {
        receiver,
        order,
        dist,
        filled,
    }
}

/// Give every cell on level ground a downstream neighbour — the flat-resolution
/// of Barnes, Lehman & Mulla 2014 ("An efficient assignment of drainage direction
/// over flat surfaces in raster digital elevation models").
///
/// A filled depression is exactly level, so "downhill" is not defined on it and
/// something has to invent a direction. Doing that badly is what produced the
/// rails: any rule that picks the neighbour nearest the exit alone leaves every
/// cell sprinting for the door in a straight line, and a hundred cells doing that
/// in parallel is a hundred parallel lines.
///
/// The fix is to steer by two distances at once, measured over the level ground
/// with a pair of breadth-first sweeps:
///
/// - `to_exit` — steps to the nearest cell that *does* drain off the flat. Flow
///   must strictly descend this, which is what makes the result acyclic and what
///   guarantees it reaches the spill point.
/// - `from_rim` — steps from the higher ground pouring in around the edge. Among
///   the neighbours that get closer to the exit, the one **furthest from the rim**
///   wins.
///
/// That second term is the whole point. It pulls flow lines off the shoreline and
/// onto the middle of the basin before they run for the outlet, so they *merge*
/// into a channel instead of racing side by side — a drained lake gets one river
/// down its axis with tributaries joining it, which is what a drained lake looks
/// like.
fn resolve_flats(filled: &[f32], n: usize, receiver: &mut [usize], dist: &mut [f32]) {
    let count = n * n;
    // Snapshot before assigning anything, or a cell resolved early would stop
    // counting as level ground for its neighbours and split the flat in two.
    let level: Vec<bool> = (0..count).map(|c| receiver[c] == usize::MAX).collect();
    if !level.iter().any(|l| *l) {
        return;
    }

    // Sweep 1, outward from the exits. A flat's exits are the cells on it that
    // already have a receiver — the spill point, and any shore the flood reached
    // without having to raise it.
    let mut to_exit = vec![UNREACHED; count];
    let mut queue = VecDeque::new();
    for c in 0..count {
        if !level[c] {
            continue;
        }
        if neighbors(c, n).any(|(nb, _)| !level[nb] && filled[nb] == filled[c]) {
            to_exit[c] = 1;
            queue.push_back(c);
        }
    }
    bfs_over_flat(&mut to_exit, &mut queue, filled, n, &level);

    // Sweep 2, inward from the rim: the higher ground that drains into the flat.
    let mut from_rim = vec![UNREACHED; count];
    let mut queue = VecDeque::new();
    for c in 0..count {
        if !level[c] {
            continue;
        }
        if neighbors(c, n).any(|(nb, _)| filled[nb] > filled[c]) {
            from_rim[c] = 0;
            queue.push_back(c);
        }
    }
    bfs_over_flat(&mut from_rim, &mut queue, filled, n, &level);

    for c in 0..count {
        if !level[c] {
            continue;
        }
        if to_exit[c] == UNREACHED {
            // No way off this flat. A correct fill cannot produce one — every
            // cell has a non-ascending path to the boundary — so this is a guard
            // against a future change rather than a case that fires: leave the
            // cell as its own outlet, which is inert but never a cycle.
            receiver[c] = c;
            continue;
        }
        // Closer to the exit, then as far from the rim as possible.
        let mut best: Option<(u32, usize)> = None;
        for (nb, step) in neighbors(c, n) {
            if filled[nb] != filled[c] {
                continue;
            }
            // An exit is rank zero: leaving the flat always beats crossing it.
            let rank = if level[nb] { to_exit[nb] } else { 0 };
            if rank >= to_exit[c] {
                continue;
            }
            let depth = if level[nb] { from_rim[nb] } else { 0 };
            if best.map_or(true, |(b, _)| depth > b) {
                best = Some((depth, nb));
                receiver[c] = nb;
                dist[c] = step;
            }
        }
        if best.is_none() {
            receiver[c] = c;
        }
    }
}

/// Breadth-first over one level surface: spread `field` from the cells already
/// seeded in `queue` to every level neighbour at the same elevation.
fn bfs_over_flat(
    field: &mut [u32],
    queue: &mut VecDeque<usize>,
    filled: &[f32],
    n: usize,
    level: &[bool],
) {
    while let Some(c) = queue.pop_front() {
        for (nb, _) in neighbors(c, n) {
            if level[nb] && filled[nb] == filled[c] && field[nb] == UNREACHED {
                field[nb] = field[c] + 1;
                queue.push_back(nb);
            }
        }
    }
}

/// Cells ordered so that every receiver comes before the cells draining into it.
///
/// The flood used to hand this over for free — popping by elevation *is* a
/// downstream-first order — but the flood no longer decides who drains where, so
/// the order has to come off the receiver forest itself. Breadth-first from the
/// outlets down the donor lists, which is O(n) and needs no sorting.
fn downstream_order(receiver: &[usize]) -> Vec<usize> {
    let count = receiver.len();
    // Donor lists, packed: `donors[first[c]..first[c + 1]]` drains into `c`.
    let mut first = vec![0usize; count + 1];
    for (c, &r) in receiver.iter().enumerate() {
        if r != c {
            first[r + 1] += 1;
        }
    }
    for i in 0..count {
        first[i + 1] += first[i];
    }
    let mut next = first.clone();
    let mut donors = vec![0usize; first[count]];
    for (c, &r) in receiver.iter().enumerate() {
        if r != c {
            donors[next[r]] = c;
            next[r] += 1;
        }
    }

    let mut order: Vec<usize> = (0..count).filter(|&c| receiver[c] == c).collect();
    let mut i = 0;
    while i < order.len() {
        let c = order[i];
        i += 1;
        order.extend_from_slice(&donors[first[c]..first[c + 1]]);
    }
    order
}
